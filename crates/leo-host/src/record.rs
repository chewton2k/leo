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
            turns: false,
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
                        turns: true,
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
        let mut last_others = 0.0;
        for (speaker, event) in rx {
            if let Some(clock) = clock_of(speaker, &event, &mut last_others) {
                heard(clock);
                continue;
            }
            match event {
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

fn clock_of(speaker: &str, event: &Event, last_others: &mut f32) -> Option<Heard> {
    let Event::Clock {
        secs,
        paused,
        level,
    } = *event
    else {
        return None;
    };
    if speaker == "Others" {
        *last_others = level;
        return None;
    }
    Some(Heard::Clock {
        secs,
        paused,
        level: level.max(*last_others),
    })
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
    let mut source = merged(&dirs)?;
    source.id = id;
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

fn merged(dirs: &[(&str, std::path::PathBuf)]) -> Result<leo_core::recording::Archive> {
    let sessions = dirs
        .iter()
        .map(|(speaker, dir)| Ok((*speaker, leo_services::session::Session::open(dir)?)))
        .collect::<Result<Vec<_>>>()?;
    let first = sessions
        .first()
        .ok_or_else(|| anyhow!("This recording cannot be recovered."))?;
    let earliest = sessions
        .iter()
        .map(|(_, session)| session.manifest.started)
        .min()
        .unwrap_or(first.1.manifest.started);
    let mut source = first.1.archive();
    source.passages.clear();
    source.points.clear();
    for (speaker, session) in &sessions {
        let offset = (session.manifest.started - earliest).num_seconds().max(0) as u64;
        let archive = session.archive();
        source
            .passages
            .extend(archive.passages.into_iter().map(|p| {
                let mut p = labelled(speaker, p);
                p.start_secs += offset;
                p.end_secs += offset;
                p
            }));
        source.points.extend(archive.points);
        source.warnings.extend(archive.warnings);
    }
    source.passages.sort_by_key(|p| p.start_secs);
    if sessions.iter().all(|(_, session)| session.manifest.turns) {
        let mut joined: Vec<leo_core::recording::Passage> = Vec::new();
        for passage in std::mem::take(&mut source.passages) {
            match joined.last_mut() {
                Some(last)
                    if last.speaker == passage.speaker
                        && passage.end_secs.saturating_sub(last.start_secs)
                            <= leo_services::session::SEGMENT_SECS =>
                {
                    last.end_secs = last.end_secs.max(passage.end_secs);
                    last.text = leo_services::speech::chunking::join_transcripts(&[
                        &last.text,
                        &passage.text,
                    ]);
                }
                _ => joined.push(passage),
            }
        }
        source.passages = joined;
    }
    Ok(source)
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

fn saved_sessions() -> Result<Vec<leo_services::session::Session>> {
    let root = leo_services::session::root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    anyhow::ensure!(
        std::fs::symlink_metadata(&root)?.is_dir(),
        "Recording recovery cannot use a symbolic link."
    );
    Ok(leo_services::session::Session::unfinished(&root))
}

fn recording_id(id: &str) -> &str {
    id.strip_suffix("-you")
        .or_else(|| id.strip_suffix("-others"))
        .unwrap_or(id)
}

pub fn pending() -> Result<Vec<leo_web::record::Pending>> {
    let mut grouped = std::collections::BTreeMap::new();
    for session in saved_sessions()? {
        let id = &session.manifest.id;
        let call = id.ends_with("-you") || id.ends_with("-others");
        let id = recording_id(id).to_owned();
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
        .filter(|session| recording_id(&session.manifest.id) == id)
        .map(|session| (session.manifest.id, session.dir))
        .collect();
    anyhow::ensure!(
        !selected.is_empty() && selected.len() <= 2,
        "This recording cannot be recovered."
    );
    let mut dirs = Vec::new();
    let mut warnings = Vec::new();
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
        match resume_retained(&dir, &controls, heard) {
            Ok(()) => dirs.push((speaker, dir)),
            Err(e) if !speaker.is_empty() => warnings.push(format!("{speaker}: {e}")),
            Err(e) => return Err(e),
        }
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
        let kept = if dir.exists() {
            " Its available audio is kept until Settings → Keeping says it can go."
        } else {
            ""
        };
        anyhow::bail!(
            "This recording could not be recovered: {}.{kept}",
            problem.trim_end_matches('.')
        );
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
    fn the_call_meter_keeps_one_reading() {
        use leo_services::session::recorder::Event;
        let clock = |secs, level| Event::Clock {
            secs,
            paused: false,
            level,
        };
        let mut last_others = 0.0;
        let mut shown = Vec::new();
        for (speaker, event) in [
            ("You", clock(1, 0.2)),
            ("Others", clock(1, 0.7)),
            ("You", clock(2, 0.1)),
            ("Others", clock(2, 0.05)),
            ("You", clock(3, 0.4)),
            ("You", Event::Transcript("hello".into())),
        ] {
            if let Some(leo_web::record::Heard::Clock { secs, level, .. }) =
                super::clock_of(speaker, &event, &mut last_others)
            {
                shown.push((secs, level));
            }
        }
        assert_eq!(shown, [(1, 0.2), (2, 0.7), (3, 0.4)]);
    }

    #[test]
    fn a_call_keeps_who_spoke_when() {
        use leo_services::session::{turns_name, Manifest, Session};
        let root = tempfile::tempdir().unwrap();
        let mut dirs = Vec::new();
        for (speaker, screen, first) in [("You", false, 0u64), ("Others", true, 20)] {
            let mut manifest = Manifest::new(None, None, "", screen);
            manifest.duration_secs = Some(120);
            manifest.turns = true;
            let session = Session::create(&root.path().join(speaker), manifest).unwrap();
            let turns: Vec<(u64, String)> = (0..3)
                .map(|n| {
                    let at = first + n * 40;
                    (at * 1000, format!("{speaker} at {at}s."))
                })
                .collect();
            let text: Vec<&str> = turns.iter().map(|(_, text)| text.as_str()).collect();
            std::fs::write(session.dir.join("seg-00000.txt"), text.join(" ")).unwrap();
            std::fs::write(
                session.dir.join(turns_name(0)),
                serde_json::to_string(&turns).unwrap(),
            )
            .unwrap();
            dirs.push((speaker, session.dir.clone()));
        }
        let source = super::merged(&dirs).unwrap();
        let order: Vec<String> = source
            .passages
            .iter()
            .map(|p| format!("{}@{}-{}s", p.speaker, p.start_secs, p.end_secs))
            .collect();
        assert_eq!(
            order,
            [
                "You@0-40s",
                "Others@20-60s",
                "You@40-80s",
                "Others@60-100s",
                "You@80-120s",
                "Others@100-120s"
            ]
        );
        assert_eq!(source.passages[3].text, "Others at 60s.");

        let mut manifest = Manifest::new(None, None, "", false);
        manifest.duration_secs = Some(600);
        let single = Session::create(&root.path().join("single"), manifest).unwrap();
        std::fs::write(single.dir.join("seg-00000.txt"), "First part.").unwrap();
        std::fs::write(single.dir.join("seg-00001.txt"), "Second part.").unwrap();
        let kept = super::merged(&[("", single.dir.clone())]).unwrap();
        assert_eq!(kept.passages.len(), 2, "a single recording keeps its parts");
    }

    #[test]
    fn one_side_talking_through_a_call_is_not_one_passage() {
        use leo_services::session::{turns_name, Manifest, Session, SEGMENT_SECS};
        let root = tempfile::tempdir().unwrap();
        let mut dirs = Vec::new();
        for (speaker, screen, parts) in [("You", false, 3u32), ("Others", true, 0)] {
            let mut manifest = Manifest::new(None, None, "", screen);
            manifest.duration_secs = Some(900);
            manifest.turns = true;
            let session = Session::create(&root.path().join(speaker), manifest).unwrap();
            for part in 0..parts {
                let turns: Vec<(u64, String)> = (0..3)
                    .map(|n| (n * 100_000, format!("Part {part} turn {n}.")))
                    .collect();
                let text: Vec<&str> = turns.iter().map(|(_, text)| text.as_str()).collect();
                std::fs::write(
                    session.dir.join(format!("seg-{part:05}.txt")),
                    text.join(" "),
                )
                .unwrap();
                std::fs::write(
                    session.dir.join(turns_name(part)),
                    serde_json::to_string(&turns).unwrap(),
                )
                .unwrap();
            }
            dirs.push((speaker, session.dir.clone()));
        }
        let source = super::merged(&dirs).unwrap();
        assert!(source.passages.len() >= 3, "{:?}", source.passages.len());
        assert!(source
            .passages
            .iter()
            .all(|p| p.end_secs - p.start_secs <= SEGMENT_SECS));
        let said: Vec<&str> = source.passages.iter().map(|p| p.text.as_str()).collect();
        assert!(said.join(" ").contains("Part 2 turn 2."));
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

    fn in_scratch_home(test: &str, check: impl FnOnce()) {
        if std::env::var_os("LEO_SCRATCH_HOME_CHILD").is_some() {
            check();
            return;
        }
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
            .args(["--exact", test, "--nocapture"])
            .env("LEO_SCRATCH_HOME_CHILD", "1")
            .env("LEO_HOME", home.path())
            .env("LEO_NO_MICROPHONE", "1")
            .env_remove("LEO_USE_KEYCHAIN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn listening(recover: &str) -> leo_web::record::Listening {
        leo_web::record::Listening {
            preview: Default::default(),
            recover: Some(recover.into()),
            id: recover.into(),
            profile: Default::default(),
            call: false,
            screen: false,
            directory: String::new(),
            title: None,
            stop: Default::default(),
            finish_now: Default::default(),
            pause: Default::default(),
            points: Default::default(),
            wants: Default::default(),
        }
    }

    fn track(id: &str, screen: bool) -> std::path::PathBuf {
        use leo_services::session::{Manifest, Session};
        let mut manifest = Manifest::new(None, None, "", screen);
        manifest.id = id.into();
        Session::create(&leo_services::session::root().unwrap(), manifest)
            .unwrap()
            .dir
    }

    fn silent_part(dir: &std::path::Path) {
        leo_services::session::wav::write(&dir.join("seg-00000.wav"), &vec![0i16; 16_000 * 2])
            .unwrap();
    }

    #[test]
    fn a_call_with_a_silent_side_recovers_the_other_side_at_once() {
        in_scratch_home(
            "record::tests::a_call_with_a_silent_side_recovers_the_other_side_at_once",
            || {
                let you = track("x-you", false);
                std::fs::write(you.join("seg-00000.txt"), "hello from my side").unwrap();
                silent_part(&track("x-others", true));
                let mut warnings = Vec::new();
                let first = super::recover(listening("x"), &mut |heard| {
                    if let leo_web::record::Heard::Warning(w) = heard {
                        warnings.push(w)
                    }
                });
                let source = first.unwrap().source.unwrap();
                assert_eq!(source.passages.len(), 1);
                assert_eq!(source.passages[0].speaker, "You");
                let others: Vec<&String> = warnings
                    .iter()
                    .filter(|w| w.starts_with("Others: "))
                    .collect();
                assert_eq!(others.len(), 1, "{warnings:?}");
                assert!(others[0].contains("No sound was recorded from the computer"));
            },
        );
    }

    #[test]
    fn a_failed_recovery_says_audio_is_kept_only_when_it_is() {
        in_scratch_home(
            "record::tests::a_failed_recovery_says_audio_is_kept_only_when_it_is",
            || {
                let dir = track("y", false);
                silent_part(&dir);
                let error = super::recover(listening("y"), &mut |_| {})
                    .err()
                    .unwrap()
                    .to_string();
                assert!(!dir.exists());
                assert!(
                    error.starts_with(
                        "This recording could not be recovered: No sound was recorded."
                    ),
                    "{error}"
                );
                assert!(!error.contains("kept"), "{error}");
                assert!(!error.contains(".."), "{error}");
                assert!(error.ends_with('.'), "{error}");
            },
        );
    }
}
