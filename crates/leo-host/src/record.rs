use std::cell::RefCell;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use leo_services::ai::chat::Jotted;
use leo_services::session::capture::Source;
use leo_services::session::recorder::{self, Controls, Event, Request};
use leo_web::record::{Heard, Listener, Listening, Recorded};

pub fn listener() -> Listener {
    Arc::new(|listening, heard| listen(listening, heard))
}

fn listen(listening: Listening, heard: &mut dyn FnMut(Heard)) -> Result<Recorded> {
    if listening.recover.is_some() {
        return recover(listening, heard);
    }
    if listening.call {
        return listen_call(listening, heard);
    }
    let Listening {
        preview,
        recover: _,
        id,
        profile,
        call: _,
        finish_now,
        screen,
        directory,
        title,
        stop,
        pause,
        points,
        wants,
    } = listening;
    let controls = Controls {
        preview: Some(preview),
        finish_now,
        stop,
        pause,
        points: Default::default(),
        wants,
    };
    let input = Source::from_env(screen);
    let heard = RefCell::new(heard);
    let failed = RefCell::new(None);
    let finished = RefCell::new(None);
    let mirror = || {
        let (Ok(from), Ok(mut to)) = (points.lock(), controls.points.lock()) else {
            return;
        };
        if from.len() != to.len() {
            *to = from
                .iter()
                .map(|(at_secs, text)| Jotted {
                    at_secs: *at_secs,
                    text: text.clone(),
                })
                .collect();
        }
    };
    recorder::record(
        Request {
            id: Some(id),
            profile: Some(profile),
            title,
            append_to: None,
            dir: directory,
            screen,
            input,
            draft: draft_fn(),
        },
        &controls,
        &|event| {
            mirror();
            let mut tell = heard.borrow_mut();
            match event {
                Event::Started(_) => {}
                Event::Clock {
                    secs,
                    paused,
                    level,
                } => tell(Heard::Clock {
                    secs,
                    paused,
                    level,
                }),
                Event::Progress { label, steps } => tell(Heard::Step { label, steps }),
                Event::Transcript(text) => tell(Heard::Transcript(text)),
                Event::Fallback { from, to } => tell(Heard::Warning(format!("{from}: {to}"))),
                Event::Warning(text) => tell(Heard::Warning(text)),
                Event::Failed(text) => *failed.borrow_mut() = Some(text),
                Event::Finished { session, .. } => *finished.borrow_mut() = Some(session),
            }
        },
    );
    let heard = heard.into_inner();
    if let Some(problem) = failed.into_inner() {
        return Err(anyhow!(problem));
    }
    let dir = finished
        .into_inner()
        .ok_or_else(|| anyhow!("The recording ended without any audio."))?;
    heard(Heard::Step {
        label: "Writing the notes".into(),
        steps: None,
    });
    let (tx, rx) = std::sync::mpsc::channel::<(usize, usize)>();
    let work_dir = dir.clone();
    let written = std::thread::scope(|scope| {
        let work = scope.spawn(move || {
            recorder::write_up(&work_dir, None, &move |done, total| {
                let _ = tx.send((done, total));
            })
        });
        for (done, total) in rx {
            heard(Heard::Step {
                label: "Writing the notes".into(),
                steps: (total > 1).then_some((done, total)),
            });
        }
        work.join()
            .map_err(|_| anyhow!("leo hit an internal error while writing the notes."))?
    })?;
    for problem in written.problems {
        heard(Heard::Warning(problem));
    }
    let source = leo_services::session::Session::open(&dir)?.archive();
    Ok(Recorded {
        title: written.title,
        body: written.body,
        writer_failed: written.writer_failed,
        source: Some(source),
        commit: Some(Arc::new(move |notes, note| {
            leo_services::session::Session::open(&dir)?.commit(notes, note)
        })),
    })
}

pub fn regenerator() -> leo_web::record::Regenerator {
    Arc::new(|sources, profile| {
        let written = write_sources(sources, &profile);
        if !written.problems.is_empty() {
            anyhow::bail!(written.problems.join("; "));
        }
        Ok((written.title, written.body))
    })
}

fn write_sources(
    sources: Vec<leo_core::recording::Archive>,
    profile: &leo_core::recording::Profile,
) -> leo_services::ai::long::Structured {
    let speakers: std::collections::HashSet<&str> = sources
        .iter()
        .flat_map(|s| s.passages.iter().map(|p| p.speaker.as_str()))
        .collect();
    let named = speakers.len() > 1;
    let mut parts = Vec::new();
    let mut points = Vec::new();
    let mut offset = 0;
    for source in sources {
        let end = source
            .passages
            .iter()
            .map(|p| p.end_secs)
            .max()
            .unwrap_or(0);
        for p in source.passages {
            parts.push(leo_services::session::Part {
                index: parts.len() as u32,
                start_secs: offset + p.start_secs,
                end_secs: offset + p.end_secs,
                text: if named && !p.speaker.is_empty() {
                    format!("[{}] {}", p.speaker, p.text)
                } else {
                    p.text
                },
            });
        }
        for p in source.points {
            points.push(Jotted {
                at_secs: offset + p.at_secs,
                text: p.text,
            });
        }
        offset += end;
    }
    leo_services::ai::long::structure_recording(
        &parts,
        &points,
        None,
        None,
        "Recording",
        &|prompt, max| {
            leo_services::ai::chat_outcome(
                leo_services::ai::long::with_profile(prompt, profile),
                max,
            )
            .map(|o| o.value)
        },
        &|_, _| {},
        leo_services::ai::writing_budget(),
    )
}

fn listen_call(listening: Listening, heard: &mut dyn FnMut(Heard)) -> Result<Recorded> {
    let Listening {
        preview,
        id,
        profile,
        directory,
        title,
        stop,
        pause,
        finish_now,
        points,
        wants,
        ..
    } = listening;
    let tracks = ["You", "Others"];
    let (tx, rx) = std::sync::mpsc::channel();
    let mut dirs = Vec::new();
    let mut warnings = Vec::new();
    std::thread::scope(|scope| {
        for speaker in tracks {
            let input = Source::from_env(speaker == "Others");
            let tx = tx.clone();
            let id = id.clone();
            let profile = leo_core::recording::Profile {
                speakers: false,
                ..profile.clone()
            };
            let title = title.clone();
            let directory = directory.clone();
            let controls = Controls {
                preview: Some(preview.clone()),
                finish_now: finish_now.clone(),
                stop: stop.clone(),
                pause: pause.clone(),
                points: Default::default(),
                wants: wants.clone(),
            };
            let points = points.clone();
            scope.spawn(move || {
                recorder::record(
                    Request {
                        id: Some(format!("{id}-{}", speaker.to_lowercase())),
                        profile: Some(profile),
                        title,
                        append_to: None,
                        dir: directory,
                        screen: speaker == "Others",
                        input,
                        draft: None,
                    },
                    &controls,
                    &|event| {
                        if speaker == "You" {
                            if let (Ok(from), Ok(mut to)) = (points.lock(), controls.points.lock())
                            {
                                *to = from
                                    .iter()
                                    .map(|(at_secs, text)| Jotted {
                                        at_secs: *at_secs,
                                        text: text.clone(),
                                    })
                                    .collect();
                            }
                        }
                        let _ = tx.send((speaker, event));
                    },
                );
            });
        }
        drop(tx);
        for (speaker, event) in rx {
            match event {
                Event::Clock {
                    secs,
                    paused,
                    level,
                } => heard(Heard::Clock {
                    secs,
                    paused,
                    level,
                }),
                Event::Transcript(text) => heard(Heard::Transcript(format!("{speaker}: {text}"))),
                Event::Warning(text) | Event::Failed(text) => {
                    warnings.push(format!("{speaker}: {text}"))
                }
                Event::Progress { label, steps } => heard(Heard::Step {
                    label: format!("{speaker}: {label}"),
                    steps,
                }),
                Event::Finished { session, .. } => dirs.push((speaker, session)),
                _ => {}
            }
        }
    });
    finish_tracks(dirs, warnings, id, profile, wants, heard)
}

fn finish_tracks(
    dirs: Vec<(&'static str, std::path::PathBuf)>,
    warnings: Vec<String>,
    id: String,
    mut profile: leo_core::recording::Profile,
    wants: Arc<std::sync::Mutex<Option<String>>>,
    heard: &mut dyn FnMut(Heard),
) -> Result<Recorded> {
    if dirs.is_empty() {
        anyhow::bail!(warnings.join("; "));
    }
    let mut source = leo_services::session::Session::open(&dirs[0].1)?.archive();
    source.id = id;
    source.passages.clear();
    source.points.clear();
    for (speaker, dir) in &dirs {
        let session = leo_services::session::Session::open(dir)?;
        let archive = session.archive();
        source
            .passages
            .extend(archive.passages.into_iter().map(|p| labelled(speaker, p)));
        source.points.extend(archive.points);
        source.warnings.extend(archive.warnings);
    }
    source.passages.sort_by_key(|p| p.start_secs);
    source.warnings.extend(warnings);
    for warning in &source.warnings {
        heard(Heard::Warning(warning.clone()));
    }
    heard(Heard::Step {
        label: "Writing the call notes".into(),
        steps: None,
    });
    if let Some(latest) = wants.lock().ok().and_then(|w| w.clone()) {
        profile.wants = latest;
    }
    source.wants = profile.wants.clone();
    let written = write_sources(vec![source.clone()], &profile);
    for problem in &written.problems {
        heard(Heard::Warning(problem.clone()));
    }
    let (title, body) = (written.title, written.body);
    let committed = source.clone();
    Ok(Recorded {
        title,
        body,
        writer_failed: written.writer_failed,
        source: Some(source),
        commit: Some(Arc::new(move |notes, note| {
            leo_core::recording::save_merged(notes, note, &committed)?;
            for (_, dir) in &dirs {
                leo_services::session::Session::open(dir)?.finish()?;
            }
            Ok(())
        })),
    })
}

fn labelled(
    speaker: &str,
    mut passage: leo_core::recording::Passage,
) -> leo_core::recording::Passage {
    if !speaker.is_empty() {
        passage.speaker = speaker.into();
    }
    passage
}

fn saved_sessions() -> Result<Vec<(String, std::path::PathBuf)>> {
    let root = leo_services::session::root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    anyhow::ensure!(
        std::fs::symlink_metadata(&root)?.is_dir(),
        "Recording recovery cannot use a symbolic link."
    );
    Ok(leo_services::session::Session::unfinished(&root)
        .into_iter()
        .filter_map(|dir| {
            let session = leo_services::session::Session::open(&dir).ok()?;
            let id = session.manifest.id;
            Some((id, dir))
        })
        .collect())
}

fn recording_id(id: &str) -> &str {
    id.strip_suffix("-you")
        .or_else(|| id.strip_suffix("-others"))
        .unwrap_or(id)
}

pub fn pending() -> Result<Vec<leo_web::record::Pending>> {
    let mut grouped = std::collections::BTreeMap::new();
    for (id, dir) in saved_sessions()? {
        let session = leo_services::session::Session::open(&dir)?;
        let call = id.ends_with("-you") || id.ends_with("-others");
        let id = recording_id(&id).to_owned();
        grouped
            .entry(id.clone())
            .or_insert(leo_web::record::Pending {
                id,
                title: session.manifest.title,
                directory: session.manifest.dir,
                profile: session.manifest.profile,
                started: Some(session.manifest.started),
                source: if call {
                    leo_web::record::Source::Nativecall
                } else if session.manifest.screen {
                    leo_web::record::Source::Screen
                } else {
                    leo_web::record::Source::Microphone
                },
            });
    }
    Ok(grouped.into_values().collect())
}

fn recover(listening: Listening, heard: &mut dyn FnMut(Heard)) -> Result<Recorded> {
    let id = listening
        .recover
        .as_deref()
        .ok_or_else(|| anyhow!("Choose an interrupted recording."))?;
    let selected: Vec<_> = saved_sessions()?
        .into_iter()
        .filter(|(saved, _)| recording_id(saved) == id)
        .collect();
    anyhow::ensure!(
        !selected.is_empty() && selected.len() <= 2,
        "This recording cannot be recovered."
    );
    let mut dirs = Vec::new();
    let warnings = Vec::new();
    for (saved, dir) in selected {
        let speaker = if saved.ends_with("-you") {
            "You"
        } else if saved.ends_with("-others") {
            "Others"
        } else {
            ""
        };
        let controls = Controls {
            preview: Some(listening.preview.clone()),
            finish_now: listening.finish_now.clone(),
            stop: listening.stop.clone(),
            pause: listening.pause.clone(),
            points: Default::default(),
            wants: listening.wants.clone(),
        };
        resume_retained(&dir, &controls, heard)?;
        dirs.push((speaker, dir));
    }
    finish_tracks(
        dirs,
        warnings,
        id.to_owned(),
        listening.profile,
        listening.wants,
        heard,
    )
}

fn resume_retained(
    dir: &std::path::Path,
    controls: &Controls,
    heard: &mut dyn FnMut(Heard),
) -> Result<()> {
    let failed = RefCell::new(None);
    let finished = std::cell::Cell::new(false);
    let heard = RefCell::new(heard);
    leo_services::session::recorder::resume_with_controls(dir, controls, &|event| match event {
        Event::Progress { label, steps } => heard.borrow_mut()(Heard::Step { label, steps }),
        Event::Transcript(text) => heard.borrow_mut()(Heard::Transcript(text)),
        Event::Warning(text) => heard.borrow_mut()(Heard::Warning(text)),
        Event::Failed(text) => *failed.borrow_mut() = Some(text),
        Event::Finished { .. } => finished.set(true),
        _ => {}
    });
    if let Some(problem) = failed.into_inner() {
        anyhow::bail!("This recording could not be recovered: {problem}. Its available audio is kept until Settings → Keeping says it can go.");
    }
    anyhow::ensure!(
        finished.get(),
        "This recording is still incomplete. Try recovering it again."
    );
    Ok(())
}

fn draft_fn() -> Option<recorder::DraftFn> {
    leo_services::ai::writing_ready().then(|| -> recorder::DraftFn {
        Arc::new(|prompt, max, stopped| leo_services::ai::chat_outcome_until(prompt, max, stopped))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_single_recording_keeps_its_own_speaker_and_a_call_track_takes_its_side() {
        let passage = leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 5,
            text: "hello".into(),
            speaker: "Microphone".into(),
        };
        assert_eq!(super::labelled("", passage.clone()).speaker, "Microphone");
        assert_eq!(super::labelled("You", passage).speaker, "You");
    }

    #[test]
    fn unreadable_recovery_audio_is_an_error_before_any_note_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let result = super::resume_retained(
            dir.path(),
            &leo_services::session::recorder::Controls::default(),
            &mut |_| {},
        );
        assert!(result.is_err());
    }

    #[test]
    fn the_call_commit_keeps_both_sides() {
        use leo_core::recording::{load, save, Archive, Passage, Point};
        use leo_services::session::{Manifest, Session};
        if let Some(order) = std::env::var_os("LEO_CALL_COMMIT_CHILD") {
            let (first, second) = if order == "you" {
                ("You", "Others")
            } else {
                ("Others", "You")
            };
            let home = std::path::PathBuf::from(std::env::var_os("LEO_HOME").unwrap());
            let notes = home.join("notes");
            let point = Point {
                at_secs: 2,
                text: "ask about the budget".into(),
            };
            save(
                &notes,
                "call-0001",
                &Archive {
                    id: "call-0001".into(),
                    started: chrono::Utc::now(),
                    passages: vec![Passage {
                        start_secs: 0,
                        end_secs: 5,
                        text: "the first side".into(),
                        speaker: first.into(),
                    }],
                    points: vec![point.clone()],
                    context: String::new(),
                    wants: String::new(),
                    warnings: vec![],
                    trace: vec![],
                    from: String::new(),
                },
            )
            .unwrap();
            let commit_once = || {
                let mut manifest = Manifest::new(None, None, "", second == "Others");
                manifest.id = format!("call-0001-{}", second.to_lowercase());
                let dir = Session::create(&home.join("recordings"), manifest)
                    .unwrap()
                    .dir;
                std::fs::write(dir.join("seg-00000.txt"), "the second side").unwrap();
                let recorded = super::finish_tracks(
                    vec![(second, dir)],
                    vec![],
                    "call-0001".into(),
                    Default::default(),
                    Default::default(),
                    &mut |_| {},
                )
                .unwrap();
                (recorded.commit.unwrap())(&notes, "call-0001").unwrap();
                let saved = load(&notes, "call-0001").unwrap();
                assert_eq!(saved.len(), 1);
                (saved[0].passages.clone(), saved[0].points.clone())
            };
            let (passages, points) = commit_once();
            assert_eq!(
                passages
                    .iter()
                    .map(|p| (p.speaker.as_str(), p.text.as_str()))
                    .collect::<Vec<_>>(),
                [(first, "the first side"), (second, "the second side")]
            );
            assert_eq!(points, [point]);
            assert_eq!(
                commit_once(),
                (passages, points),
                "recovering the same side again changes nothing"
            );
            return;
        }
        for order in ["you", "others"] {
            let home = tempfile::tempdir().unwrap();
            let missing = home.path().join("missing");
            std::fs::write(
                home.path().join("config.toml"),
                format!(
                    "[chat]\nchain = [\"ollama\"]\n[providers.codex]\nbin = {0}\n[providers.claude_code]\nbin = {0}\n[providers.ollama]\nbase_url = \"\"\n",
                    serde_json::to_string(&missing).unwrap()
                ),
            )
            .unwrap();
            let output = leo_core::children::quiet_command(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "record::tests::the_call_commit_keeps_both_sides",
                    "--nocapture",
                ])
                .env("LEO_CALL_COMMIT_CHILD", order)
                .env("LEO_HOME", home.path())
                .env("LEO_NO_MICROPHONE", "1")
                .env_remove("LEO_USE_KEYCHAIN")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{order} first: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
