use super::*;

fn hearing() -> record::Listener {
    Arc::new(
        |listening: record::Listening, heard: &mut dyn FnMut(record::Heard)| {
            use std::sync::atomic::Ordering;
            while !listening.stop.load(Ordering::Relaxed) {
                let kept = listening.points.lock().unwrap().len();
                listening.saved_points.store(kept, Ordering::Release);
                heard(record::Heard::Clock {
                    secs: 1,
                    paused: listening.pause.load(Ordering::Relaxed),
                    level: 0.25,
                });
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            heard(record::Heard::Step {
                label: "Writing the notes".into(),
                steps: Some((1, 2)),
            });
            let points: Vec<String> = listening
                .points
                .lock()
                .unwrap()
                .iter()
                .map(|(_, text)| text.clone())
                .collect();
            Ok(record::Recorded {
                title: "Lecture".into(),
                body: format!("points: {}", points.join(", ")),
                ..Default::default()
            })
        },
    )
}

fn start_recording(state: &AppState, source: record::Source) -> Response {
    run(record::start(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({
                "directory": "cs130",
                "source": source,
            }))
            .unwrap(),
        ),
    ))
}

fn recording_until(state: &AppState, done: impl Fn(&str) -> bool) -> record::RecordView {
    let started = std::time::Instant::now();
    loop {
        let view = state
            .recording
            .lock()
            .unwrap()
            .as_ref()
            .map(|j| j.view())
            .unwrap();
        if done(view.state) {
            return view;
        }
        assert!(started.elapsed().as_secs() < 10, "stuck at {view:?}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_recording_keeps_its_points_pauses_and_becomes_a_note() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    let started = start_recording(&state, record::Source::Microphone);
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let id = json_of(started)["id"].as_str().unwrap().to_string();
    recording_until(&state, |s| s == "recording");

    let jotted = run(record::point(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({ "text": "exam is on BFS" })).unwrap()),
    ));
    assert_eq!(json_of(jotted)["points"][0][1], "exam is on BFS");
    let paused = run(record::pause(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({ "paused": true })).unwrap()),
    ));
    assert_eq!(json_of(paused)["state"], "paused");
    recording_until(&state, |s| s == "paused");

    let busy = start_recording(&state, record::Source::Microphone);
    assert_eq!(busy.status(), StatusCode::CONFLICT);

    let stopped = run(record::stop(State(state.clone()), Path(id.clone())));
    assert_eq!(json_of(stopped)["state"], "writing");

    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done", "{view:?}");
    let note = state
        .fresh()
        .find_note(view.note.as_deref().unwrap())
        .unwrap()
        .clone();
    assert_eq!(note.title, "Lecture");
    assert_eq!(note.directory, "cs130");
    assert_eq!(note.body, "points: exam is on BFS");
    assert!(!view.levels.is_empty());
    assert!(
        view.levels.len() <= record::LEVELS_KEPT,
        "only the last few seconds are kept"
    );
    assert!(view.levels.iter().all(|l| *l == 0.25));

    let gone = run(record::status(State(state.clone()), Path("nope".into())));
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    let again = start_recording(&state, record::Source::Microphone);
    assert_eq!(again.status(), StatusCode::ACCEPTED);
    let id = json_of(again)["id"].as_str().unwrap().to_string();
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn the_computers_own_microphone_and_sound_can_be_recorded() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(Arc::new(
        |_: record::Listening, _: &mut dyn FnMut(record::Heard)| {
            Err(anyhow::anyhow!("No sound was recorded."))
        },
    ));
    let allowed = start_recording(&state, record::Source::Screen);
    assert_eq!(allowed.status(), StatusCode::ACCEPTED);
    let view = recording_until(&state, |s| s == "failed" || s == "done");
    assert_eq!(view.state, "failed");
    assert_eq!(view.error.as_deref(), Some("No sound was recorded."));
    assert!(state.fresh().notes.is_empty());
}

#[test]
fn screen_recordings_are_marked_as_screen_audio() {
    let (mut state, _d, _ids) = state_with(&[]);
    let seen: Arc<Mutex<Option<bool>>> = Default::default();
    let saw = Arc::clone(&seen);
    state.listener = Some(Arc::new(
        move |listening: record::Listening, _: &mut dyn FnMut(record::Heard)| {
            *saw.lock().unwrap() = Some(listening.screen);
            Ok(record::Recorded {
                title: "Lecture video".into(),
                body: "notes".into(),
                ..Default::default()
            })
        },
    ));
    let started = start_recording(&state, record::Source::Screen);
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done", "{view:?}");
    assert_eq!(*seen.lock().unwrap(), Some(true));
}

#[test]
fn speaker_labels_ride_the_start_request_for_every_source() {
    for (source, expected) in [("microphone", true), ("screen", true), ("nativecall", true)] {
        let (mut state, _d, _ids) = state_with(&[]);
        let seen: Arc<Mutex<Option<bool>>> = Default::default();
        let saw = Arc::clone(&seen);
        state.listener = Some(Arc::new(
            move |listening: record::Listening, _: &mut dyn FnMut(record::Heard)| {
                *saw.lock().unwrap() = Some(listening.profile.speakers);
                Ok(record::Recorded {
                    title: "Standup".into(),
                    body: "notes".into(),
                    ..Default::default()
                })
            },
        ));
        let started = run(record::start(
            State(state.clone()),
            Json(
                serde_json::from_value(serde_json::json!({
                    "source": source,
                    "profile": { "speakers": true },
                }))
                .unwrap(),
            ),
        ));
        assert_eq!(started.status(), StatusCode::ACCEPTED);
        recording_until(&state, |s| s == "done" || s == "failed");
        assert_eq!(*seen.lock().unwrap(), Some(expected), "{source}");
    }
}

#[test]
fn recording_is_refused_without_a_recorder_or_into_a_folder_outside_the_notes() {
    let (mut state, _d, _ids) = state_with(&[]);
    let none = start_recording(&state, record::Source::Microphone);
    assert_eq!(none.status(), StatusCode::SERVICE_UNAVAILABLE);
    state.listener = Some(hearing());
    let outside = run(record::start(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({
                "directory": "../outside",
                "source": "microphone",
            }))
            .unwrap(),
        ),
    ));
    assert_eq!(outside.status(), StatusCode::BAD_REQUEST);
    assert!(state.recording.lock().unwrap().is_none());
}

#[test]
fn a_point_or_stop_during_a_save_waits_for_it_instead_of_failing() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let saving = state.recording.lock().unwrap().as_ref().unwrap().writing();
    let busy_until_told = || {
        saving.store(true, std::sync::atomic::Ordering::Release);
        let saving = saving.clone();
        let (tell, told) = std::sync::mpsc::channel::<()>();
        let held = std::thread::spawn(move || {
            let _ = told.recv();
            saving.store(false, std::sync::atomic::Ordering::Release);
        });
        (tell, held)
    };
    let waits = |work: &std::thread::JoinHandle<axum::response::Response>| {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < until {
            assert!(!work.is_finished());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    let finishes = |work: &std::thread::JoinHandle<axum::response::Response>| {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !work.is_finished() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(work.is_finished());
    };
    let (tell, held) = busy_until_told();
    let point = {
        let (state, id) = (state.clone(), id.clone());
        std::thread::spawn(move || {
            run(record::point(
                State(state),
                Path(id),
                Json(serde_json::from_value(serde_json::json!({"text": "heaps"})).unwrap()),
            ))
        })
    };
    waits(&point);
    tell.send(()).unwrap();
    held.join().unwrap();
    finishes(&point);
    assert_eq!(point.join().unwrap().status(), StatusCode::OK);
    let (tell, held) = busy_until_told();
    let stopped = {
        let state = state.clone();
        std::thread::spawn(move || run(record::stop(State(state), Path(id))))
    };
    waits(&stopped);
    tell.send(()).unwrap();
    held.join().unwrap();
    finishes(&stopped);
    assert_eq!(stopped.join().unwrap().status(), StatusCode::OK);
    let view = recording_until(&state, |s| s == "done");
    assert_eq!(view.points.len(), 1);
}

#[test]
fn a_panicking_listener_fails_the_recording() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(Arc::new(|_, _| panic!("simulated failure")));
    start_recording(&state, record::Source::Microphone);
    let view = recording_until(&state, |s| s == "failed");
    assert!(view.error.is_some());
}

#[test]
fn source_commit_runs_after_the_note_is_visible_on_disk_and_recovery_uses_the_same_id() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (mut state, _d, _ids) = state_with(&[]);
    let committed = Arc::new(AtomicBool::new(false));
    let seen = committed.clone();
    state.listener = Some(Arc::new(move |input, _| {
        let archive = leo_core::recording::Archive {
            id: input.id,
            started: chrono::Utc::now(),
            passages: vec![],
            points: vec![],
            context: String::new(),
            wants: String::new(),
            warnings: vec![],
            trace: vec![],
            from: String::new(),
        };
        let seen = seen.clone();
        Ok(record::Recorded {
            title: "Saved once".into(),
            body: "body".into(),
            source: Some(archive),
            commit: Some(Arc::new(move |notes, id| {
                let store = leo_core::store::Store::load_from(notes)?;
                assert!(store.find_note(id).is_some());
                seen.store(true, Ordering::Relaxed);
                Ok(())
            })),
            into: None,
            writer_failed: false,
        })
    }));
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let view = recording_until(&state, |s| s == "done");
    assert_eq!(view.note.as_deref(), Some(id.as_str()));
    assert!(committed.load(Ordering::Relaxed));
}

#[test]
fn a_recording_whose_notes_were_not_written_says_so_until_they_are() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.regenerator = Some(Arc::new(|_, _| Ok(("Written".into(), "Notes".into()))));
    state.listener = Some(Arc::new(|input, _| {
        Ok(record::Recorded {
            title: "Recording".into(),
            body: "## Transcript\n\nwhat was said".into(),
            source: Some(leo_core::recording::Archive {
                id: input.id,
                started: chrono::Utc::now(),
                passages: vec![leo_core::recording::Passage {
                    start_secs: 0,
                    end_secs: 5,
                    text: "what was said".into(),
                    speaker: String::new(),
                    echo: false,
                    turns: Vec::new(),
                }],
                points: vec![],
                context: String::new(),
                wants: String::new(),
                warnings: vec![],
                trace: vec![],
                from: String::new(),
            }),
            writer_failed: true,
            ..Default::default()
        })
    }));
    start_recording(&state, record::Source::Microphone);
    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done", "{view:?}");
    assert!(view.writer_failed);
    let note = view.note.unwrap();
    let pending = || {
        run(crate::routes::notes::get_note(
            State(state.clone()),
            Path(note.clone()),
        ))
        .unwrap()
        .writeup_pending
    };
    assert!(pending());
    let written = run(crate::routes::sources::regenerate(
        State(state.clone()),
        Path(note.clone()),
        Json(serde_json::from_value(serde_json::json!({})).unwrap()),
    ));
    assert_eq!(written.status(), StatusCode::OK);
    assert!(pending());
    let _ = run(crate::routes::notes::update_note(
        State(state.clone()),
        Path(note.clone()),
        Json(
            serde_json::from_value(serde_json::json!({"body": "Written notes", "written": true}))
                .unwrap(),
        ),
    ))
    .unwrap();
    assert!(!pending());
    let _ = run(crate::routes::notes::update_note(
        State(state.clone()),
        Path(note.clone()),
        Json(serde_json::from_value(serde_json::json!({"written": false})).unwrap()),
    ))
    .unwrap();
    assert!(pending());
}

#[test]
fn recovering_a_recording_nothing_kept_is_not_found() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    let response = run(record::recover(
        State(state.clone()),
        Path("gone-0001".into()),
    ));
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(state.recording.lock().unwrap().is_none());
}

#[test]
fn a_failed_recording_can_be_deleted_without_deleting_notes() {
    let (mut state, _d, ids) = state_with(&[("Keep this note", "")]);
    state.listener = Some(Arc::new(|_, _| anyhow::bail!("No sound was recorded.")));
    let id = json_of(start_recording(&state, record::Source::Nativecall))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "failed");
    let saving = state.recording.lock().unwrap().as_ref().unwrap().writing();
    saving.store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(
        run(record::discard(State(state.clone()), Path(id.clone()))).status(),
        StatusCode::CONFLICT
    );
    saving.store(false, std::sync::atomic::Ordering::Release);
    let response = run(record::discard(State(state.clone()), Path(id.clone())));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response)["deleted"], true);
    assert!(state.recording.lock().unwrap().is_none());
    assert!(state.discarded_recordings.lock().unwrap().contains(&id));
    assert!(state.store.lock().unwrap().find_note(&ids[0]).is_some());
    assert_eq!(
        run(record::discard(State(state), Path(id))).status(),
        StatusCode::OK
    );
}

#[test]
fn an_active_recording_cannot_be_deleted_from_its_error_panel() {
    let (mut state, _d, _) = state_with(&[]);
    state.listener = Some(hearing());
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    assert_eq!(
        run(record::discard(State(state.clone()), Path(id.clone()))).status(),
        StatusCode::CONFLICT
    );
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn the_overview_says_what_would_stop_a_recording_from_being_written_up() {
    struct Gaps;
    impl crate::record::Housekeeper for Gaps {
        fn checks(&self) -> record::Checks {
            record::Checks {
                speech: false,
                writing: false,
                headset: true,
            }
        }
    }
    let (mut state, _dir, _) = state_with(&[]);
    let ready = json_of(run(record::overview(
        State(state.clone()),
        Default::default(),
    )));
    assert_eq!(ready["checks"]["speech"], true);
    assert_eq!(ready["checks"]["writing"], true);
    assert_eq!(ready["checks"]["headset"], false);
    state.housekeeper = Some(Arc::new(Gaps));
    let gaps = json_of(run(record::overview(State(state), Default::default())));
    assert_eq!(gaps["checks"]["speech"], false);
    assert_eq!(gaps["checks"]["writing"], false);
    assert_eq!(gaps["checks"]["headset"], true);
}

#[test]
fn a_bad_cursor_is_refused() {
    struct Paged(std::sync::Mutex<Vec<(Option<String>, usize)>>);
    impl crate::record::Housekeeper for Paged {
        fn pending_page(
            &self,
            after: Option<&str>,
            limit: usize,
        ) -> anyhow::Result<(Vec<record::Pending>, Option<String>)> {
            self.0
                .lock()
                .unwrap()
                .push((after.map(str::to_owned), limit));
            Ok((Vec::new(), Some("20260101-090000-2".into())))
        }
    }
    let asked = Arc::new(Paged(Default::default()));
    let (mut state, _dir, _) = state_with(&[]);
    state.housekeeper = Some(asked.clone());
    let page = |after: &str| {
        run(record::overview(
            State(state.clone()),
            axum::extract::Query(record::Page {
                after: Some(after.into()),
            }),
        ))
    };
    for bad in ["../x", "20260101-090000/..", "20260101-090000-", "x", ""] {
        assert_eq!(page(bad).status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    let next = json_of(page("20260101-090000-3"));
    assert_eq!(next["more"], true);
    assert_eq!(next["after"], "20260101-090000-2");
    assert_eq!(
        *asked.0.lock().unwrap(),
        vec![(Some("20260101-090000-3".to_string()), record::RECORD_PAGE)]
    );
    assert_eq!(record::RECORD_PAGE, 50);
}

#[test]
fn native_recordings_can_be_recovered_without_reopening_capture() {
    struct Kept;
    impl crate::record::Housekeeper for Kept {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok(vec![record::Pending {
                id: "native-saved".into(),
                title: Some("Kept lecture".into()),
                directory: "cs130".into(),
                source: record::Source::Microphone,
                profile: Default::default(),
                started: None,
                note: None,
            }])
        }
    }
    let (mut state, _dir, _) = state_with(&[]);
    state.housekeeper = Some(Arc::new(Kept));
    state.listener = Some(Arc::new(|listening, _| {
        assert_eq!(listening.recover.as_deref(), Some("native-saved"));
        Ok(record::Recorded {
            title: "Kept lecture".into(),
            body: "Saved speech".into(),
            ..Default::default()
        })
    }));
    let overview = json_of(run(record::overview(
        State(state.clone()),
        Default::default(),
    )));
    assert_eq!(overview["pending"][0]["id"], "native-saved");
    let recovered = run(record::recover(
        State(state.clone()),
        Path("native-saved".into()),
    ));
    assert_eq!(recovered.status(), StatusCode::ACCEPTED);
    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done");
    assert_eq!(state.fresh().notes[0].body, "Saved speech");
}

const KEPT_SAVED: &str = "kept-saved-01";

fn recover_saved_into(
    state: &mut AppState,
    into: &str,
) -> (record::RecordView, Arc<Mutex<Vec<String>>>) {
    struct Kept;
    impl crate::record::Housekeeper for Kept {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok(vec![record::Pending {
                id: KEPT_SAVED.into(),
                title: None,
                directory: String::new(),
                source: record::Source::Microphone,
                profile: Default::default(),
                started: None,
                note: None,
            }])
        }
    }
    let committed = Arc::new(Mutex::new(Vec::new()));
    let seen = committed.clone();
    let into = into.to_owned();
    state.housekeeper = Some(Arc::new(Kept));
    state.listener = Some(Arc::new(move |_, _| {
        let seen = seen.clone();
        Ok(record::Recorded {
            title: "Recovered".into(),
            body: "Words that must not replace the note".into(),
            source: Some(leo_core::recording::Archive {
                id: KEPT_SAVED.into(),
                started: chrono::Utc::now(),
                passages: vec![leo_core::recording::Passage {
                    start_secs: 300,
                    end_secs: 600,
                    text: "the part that failed before".into(),
                    speaker: String::new(),
                    echo: false,
                    turns: Vec::new(),
                }],
                points: vec![],
                context: String::new(),
                wants: String::new(),
                warnings: vec![],
                trace: vec![],
                from: String::new(),
            }),
            into: Some(into.clone()),
            commit: Some(Arc::new(move |_, note| {
                seen.lock().unwrap().push(note.to_owned());
                Ok(())
            })),
            writer_failed: false,
        })
    }));
    let recovered = run(record::recover(
        State(state.clone()),
        Path(KEPT_SAVED.into()),
    ));
    assert_eq!(recovered.status(), StatusCode::ACCEPTED);
    (
        recording_until(state, |s| s == "done" || s == "failed"),
        committed,
    )
}

fn joined_the_note(
    state: &AppState,
    view: &record::RecordView,
    committed: &Mutex<Vec<String>>,
    into: &str,
    count: usize,
) {
    assert_eq!(view.state, "done", "{:?}", view.error);
    assert_eq!(view.note.as_deref(), Some(into));
    let store = state.fresh();
    assert_eq!(store.notes.len(), count);
    assert_eq!(store.find_note(into).unwrap().body, "");
    assert!(store.find_note(KEPT_SAVED).is_none());
    drop(store);
    let sources = kept(&state.notes_dir, into);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, KEPT_SAVED);
    assert_eq!(sources[0].passages[0].text, "the part that failed before");
    assert_eq!(*committed.lock().unwrap(), [into]);
}

fn trash(state: &AppState, id: &str, for_good: bool) {
    let mut store = leo_core::store::Store::load_from(&state.notes_dir).unwrap();
    assert_eq!(store.delete_notes(&[id.to_owned()]), 1);
    store.save().unwrap();
    if for_good {
        store.delete_from_trash(&[id.to_owned()]);
    }
}

#[test]
fn a_trashed_notes_audio_is_not_offered() {
    struct Owned(Vec<(&'static str, Option<String>)>);
    impl crate::record::Housekeeper for Owned {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok(self
                .0
                .iter()
                .map(|(id, note)| record::Pending {
                    id: (*id).into(),
                    title: None,
                    directory: String::new(),
                    source: record::Source::Microphone,
                    profile: Default::default(),
                    started: None,
                    note: note.clone(),
                })
                .collect())
        }
    }
    let (mut state, _dir, ids) = state_with(&[("Live", ""), ("Trashed", "")]);
    trash(&state, &ids[1], false);
    state.housekeeper = Some(Arc::new(Owned(vec![
        ("rec-live-0001", Some(ids[0].clone())),
        ("rec-trashed-01", Some(ids[1].clone())),
        ("rec-gone-00001", Some("note-gone-0001".into())),
        ("rec-unsaved-01", None),
    ])));
    let overview = json_of(run(record::overview(State(state), Default::default())));
    let listed: Vec<&str> = overview["pending"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ["rec-live-0001", "rec-unsaved-01"]);
}

#[test]
fn a_recovered_failed_part_joins_its_notes_transcript() {
    let (mut state, _dir, ids) = state_with(&[("Lecture", "")]);
    let (view, committed) = recover_saved_into(&mut state, &ids[0]);
    joined_the_note(&state, &view, &committed, &ids[0], 1);
}

#[test]
fn recovery_after_combine_adds_to_the_remaining_note() {
    let (mut state, _dir, ids) = state_with(&[("Absorbed", ""), ("Remaining", "")]);
    trash(&state, &ids[0], false);
    let (view, committed) = recover_saved_into(&mut state, &ids[1]);
    joined_the_note(&state, &view, &committed, &ids[1], 1);
    assert!(state.fresh().find_note(&ids[0]).is_none());
}

#[test]
fn recovery_after_the_absorbed_note_is_deleted_still_reaches_the_remaining_note() {
    let (mut state, _dir, ids) = state_with(&[("Absorbed", ""), ("Remaining", "")]);
    trash(&state, &ids[0], true);
    let (view, committed) = recover_saved_into(&mut state, &ids[1]);
    joined_the_note(&state, &view, &committed, &ids[1], 1);
    assert!(state.fresh().find_note(&ids[0]).is_none());
}

#[test]
fn recovery_into_a_note_that_is_gone_keeps_the_audio() {
    let (mut state, _dir, ids) = state_with(&[("Kept", "")]);
    trash(&state, &ids[0], false);
    let (view, committed) = recover_saved_into(&mut state, &ids[0]);
    assert_eq!(view.state, "failed");
    assert_eq!(
        view.error.as_deref(),
        Some("The note this recording belongs to is not in your notes; restore it from the trash, then recover again.")
    );
    assert!(committed.lock().unwrap().is_empty());
    assert!(state.fresh().notes.is_empty());
    assert!(kept(&state.notes_dir, &ids[0]).is_empty());
}

#[test]
fn deleting_after_a_failed_recovery_or_from_the_waiting_list_removes_that_session() {
    #[derive(Default)]
    struct Kept(std::sync::Mutex<Vec<String>>);
    impl crate::record::Housekeeper for Kept {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok(["failed-native", "waiting-native"]
                .into_iter()
                .filter(|id| !self.0.lock().unwrap().iter().any(|gone| gone == id))
                .map(|id| record::Pending {
                    id: id.into(),
                    title: None,
                    directory: String::new(),
                    source: record::Source::Microphone,
                    profile: Default::default(),
                    started: None,
                    note: None,
                })
                .collect())
        }
        fn discard_recording(&self, id: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(id.into());
            Ok(())
        }
    }
    let (mut state, _dir, _) = state_with(&[]);
    let kept = Arc::new(Kept::default());
    state.housekeeper = Some(kept.clone());
    state.listener = Some(Arc::new(|_, _| anyhow::bail!("The model is missing.")));
    let recovered = run(record::recover(
        State(state.clone()),
        Path("failed-native".into()),
    ));
    let job = json_of(recovered)["id"].as_str().unwrap().to_string();
    assert_ne!(job, "failed-native");
    recording_until(&state, |s| s == "failed");
    assert_eq!(
        run(record::discard(
            State(state.clone()),
            Path("failed-native".into())
        ))
        .status(),
        StatusCode::CONFLICT,
        "a session being recovered is not deleted from the waiting list"
    );
    assert_eq!(
        run(record::discard(State(state.clone()), Path(job))).status(),
        StatusCode::OK
    );
    assert_eq!(*kept.0.lock().unwrap(), ["failed-native"]);
    assert_eq!(
        run(record::discard(
            State(state.clone()),
            Path("waiting-native".into())
        ))
        .status(),
        StatusCode::OK
    );
    assert_eq!(*kept.0.lock().unwrap(), ["failed-native", "waiting-native"]);
    assert_eq!(
        run(record::discard(State(state), Path("unknown-0001".into()))).status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn recovering_native_audio_reports_writing_and_can_finish_while_waiting_for_a_model() {
    struct Kept;
    impl crate::record::Housekeeper for Kept {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok(vec![record::Pending {
                id: "waiting-native".into(),
                title: None,
                directory: "cs130".into(),
                source: record::Source::Microphone,
                profile: Default::default(),
                started: None,
                note: None,
            }])
        }
    }
    let (mut state, _dir, _) = state_with(&[]);
    state.housekeeper = Some(Arc::new(Kept));
    state.listener = Some(Arc::new(|listening, _| {
        let start = std::time::Instant::now();
        while !listening
            .finish_now
            .load(std::sync::atomic::Ordering::Acquire)
        {
            if start.elapsed() > std::time::Duration::from_secs(2) {
                return Err(anyhow::anyhow!("Finish was not offered"));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(record::Recorded {
            title: "Kept lecture".into(),
            body: "Available speech".into(),
            ..Default::default()
        })
    }));
    let recovered = run(record::recover(
        State(state.clone()),
        Path("waiting-native".into()),
    ));
    let id = json_of(recovered)["id"].as_str().unwrap().to_owned();
    let held = recording_until(&state, |_| true);
    assert_eq!(held.session, "waiting-native");
    assert_eq!(held.id, id);
    assert_ne!(held.id, held.session);
    let response = run(record::finish_available(State(state.clone()), Path(id)));
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        recording_until(&state, |s| s == "done" || s == "failed").state,
        "done"
    );
}

#[test]
fn stopping_waits_until_a_pasted_screenshot_is_kept_with_its_recording() {
    let (mut state, _dir, _) = state_with(&[]);
    state.listener = Some(hearing());
    let (entered, started) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let wait = Arc::new(Mutex::new(Some(wait)));
    state.reader = Some(Arc::new(move |_, _| {
        let wait = wait.lock().unwrap().take().unwrap();
        entered.send(()).unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        Ok("A diagram of a queue".into())
    }));
    let started_recording = start_recording(&state, record::Source::Microphone);
    let id = json_of(started_recording)["id"]
        .as_str()
        .unwrap()
        .to_owned();
    recording_until(&state, |s| s == "recording");
    let copied = state.clone();
    let saved_id = id.clone();
    let snapshot = std::thread::spawn(move || {
        run(record::snapshot(
            State(copied),
            Path(saved_id),
            Json(
                serde_json::from_value(serde_json::json!({ "at":0, "data":"iVBORw0KGgo=" }))
                    .unwrap(),
            ),
        ))
    });
    started
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let stopped_early = run(async {
        tokio::time::timeout(
            std::time::Duration::from_millis(50),
            record::stop(State(state.clone()), Path(id.clone())),
        )
        .await
    });
    assert!(stopped_early.is_err());
    release.send(()).unwrap();
    assert_eq!(snapshot.join().unwrap().status(), StatusCode::OK);
    assert_eq!(
        run(record::stop(State(state.clone()), Path(id))).status(),
        StatusCode::OK
    );
    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done");
    let notes = state.fresh();
    assert!(notes.notes[0].body.contains("Captured slide"));
    assert!(notes.notes[0].body.contains("diagram of a queue"));
}

#[test]
fn an_installation_fence_refuses_recording_before_creating_a_job() {
    let (mut state, _dir, _) = state_with(&[("Test", "cs130")]);
    state.listener = Some(Arc::new(|_, _| {
        panic!("capture must not start during installation")
    }));
    let installing = state.capture_activity.install().unwrap();
    let response = start_recording(&state, record::Source::Microphone);
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(state.recording.lock().unwrap().is_none());
    drop(installing);
}

struct Watching {
    recording: record::Recordings,
    locked_during: std::sync::Mutex<Vec<bool>>,
    ids: Vec<&'static str>,
}

impl record::Housekeeper for Watching {
    fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
        Ok(self
            .ids
            .iter()
            .map(|id| record::Pending {
                id: (*id).into(),
                title: None,
                directory: String::new(),
                source: record::Source::Microphone,
                profile: Default::default(),
                started: Some(chrono::Utc::now() - chrono::Duration::days(90)),
                note: None,
            })
            .collect())
    }
    fn discard_recording(&self, _id: &str) -> anyhow::Result<()> {
        self.locked_during
            .lock()
            .unwrap()
            .push(self.recording.try_lock().is_err());
        Ok(())
    }
}

#[test]
fn deleting_and_expiring_unfinished_audio_hold_the_recording_lock_while_they_discard() {
    let (mut state, _d, _ids) = state_with(&[]);
    let watching = Arc::new(Watching {
        recording: state.recording.clone(),
        locked_during: Default::default(),
        ids: vec!["old-one", "old-two"],
    });
    state.housekeeper = Some(watching.clone());
    let deleted = run(record::discard(
        State(state.clone()),
        Path("old-one".into()),
    ));
    assert_eq!(deleted.status(), StatusCode::OK);
    let expired = crate::keeping::expire_audio(
        &*watching,
        Some(30),
        chrono::Utc::now(),
        &state.recording,
        &Default::default(),
    );
    assert_eq!(expired, 2);
    assert_eq!(*watching.locked_during.lock().unwrap(), [true, true, true]);
}

#[test]
fn expiry_deletes_with_one_batch_call() {
    struct Batched {
        recording: record::Recordings,
        calls: std::sync::Mutex<Vec<(Vec<String>, bool)>>,
    }
    impl record::Housekeeper for Batched {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok([("old-one", 90), ("old-two", 40), ("new-one", 2)]
                .into_iter()
                .map(|(id, days)| record::Pending {
                    id: id.into(),
                    title: None,
                    directory: String::new(),
                    source: record::Source::Microphone,
                    profile: Default::default(),
                    started: Some(chrono::Utc::now() - chrono::Duration::days(days)),
                    note: None,
                })
                .collect())
        }
        fn discard_recording(&self, _id: &str) -> anyhow::Result<()> {
            panic!("expiry discards in one batch")
        }
        fn discard_recordings(&self, ids: &[String]) -> usize {
            self.calls
                .lock()
                .unwrap()
                .push((ids.to_vec(), self.recording.try_lock().is_err()));
            ids.len()
        }
    }
    let (state, _d, _ids) = state_with(&[]);
    let batched = Batched {
        recording: state.recording.clone(),
        calls: Default::default(),
    };
    let expired = crate::keeping::expire_audio(
        &batched,
        Some(30),
        chrono::Utc::now(),
        &state.recording,
        &Default::default(),
    );
    assert_eq!(expired, 2);
    assert_eq!(
        *batched.calls.lock().unwrap(),
        [(vec!["old-one".to_string(), "old-two".to_string()], true)]
    );
}

#[test]
fn expiry_and_delete_leave_the_recording_being_recovered_alone() {
    let (mut state, _d, _ids) = state_with(&[]);
    let watching = Arc::new(Watching {
        recording: state.recording.clone(),
        locked_during: Default::default(),
        ids: vec!["recovering-one"],
    });
    state.housekeeper = Some(watching.clone());
    state.listener = Some(hearing());
    let recovered = run(record::recover(
        State(state.clone()),
        Path("recovering-one".into()),
    ));
    assert_eq!(recovered.status(), StatusCode::ACCEPTED);
    let expired = crate::keeping::expire_audio(
        &*watching,
        Some(30),
        chrono::Utc::now(),
        &state.recording,
        &Default::default(),
    );
    assert_eq!(expired, 0);
    let deleted = run(record::discard(
        State(state.clone()),
        Path("recovering-one".into()),
    ));
    assert_eq!(deleted.status(), StatusCode::CONFLICT);
    assert!(watching.locked_during.lock().unwrap().is_empty());
    let id = state.recording.lock().unwrap().as_ref().unwrap().view().id;
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done" || s == "failed");
}

fn one_side(id: String, speaker: &str, text: &str) -> leo_core::recording::Archive {
    leo_core::recording::Archive {
        id,
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 5,
            text: text.into(),
            speaker: speaker.into(),
            echo: false,
            turns: Vec::new(),
        }],
        points: vec![],
        context: String::new(),
        wants: String::new(),
        warnings: vec![],
        trace: vec![],
        from: String::new(),
    }
}

#[test]
fn recovering_a_leftover_call_track_keeps_the_other_sides_passages() {
    struct Kept(Arc<Mutex<String>>);
    impl crate::record::Housekeeper for Kept {
        fn pending_recordings(&self) -> anyhow::Result<Vec<record::Pending>> {
            Ok(vec![record::Pending {
                id: self.0.lock().unwrap().clone(),
                title: None,
                directory: String::new(),
                source: record::Source::Nativecall,
                profile: Default::default(),
                started: None,
                note: None,
            }])
        }
    }
    let (mut state, dir, _) = state_with(&[]);
    let leftover: Arc<Mutex<String>> = Default::default();
    state.housekeeper = Some(Arc::new(Kept(leftover.clone())));
    state.listener = Some(Arc::new(|input, _| {
        let source = match input.recover {
            Some(id) => one_side(id, "Others", "the other side"),
            None => leo_core::recording::Archive {
                points: vec![leo_core::recording::Point {
                    at_secs: 2,
                    text: "ask about the budget".into(),
                }],
                ..one_side(input.id, "You", "my side")
            },
        };
        Ok(record::Recorded {
            title: "Call".into(),
            body: "notes".into(),
            source: Some(source),
            ..Default::default()
        })
    }));
    start_recording(&state, record::Source::Microphone);
    let note = recording_until(&state, |s| s == "done" || s == "failed")
        .note
        .unwrap();
    *leftover.lock().unwrap() = note.clone();
    let notes = dir.path().join("notes");
    let recover = || {
        let recovered = run(record::recover(State(state.clone()), Path(note.clone())));
        assert_eq!(recovered.status(), StatusCode::ACCEPTED);
        assert_eq!(
            recording_until(&state, |s| s == "done" || s == "failed").state,
            "done"
        );
        serde_json::to_value(kept(&notes, &note)).unwrap()
    };
    let first = recover();
    let sources = kept(&notes, &note);
    assert_eq!(
        sources
            .iter()
            .flat_map(|a| a.passages.iter().map(|p| p.speaker.as_str()))
            .collect::<Vec<_>>(),
        ["You", "Others"]
    );
    assert_eq!(sources[0].points[0].text, "ask about the budget");
    assert_eq!(
        recover(),
        first,
        "recovering the same side again changes nothing"
    );
}

fn saying(said: Vec<record::Heard>) -> record::Listener {
    let said = std::sync::Mutex::new(said);
    Arc::new(
        move |listening: record::Listening, heard: &mut dyn FnMut(record::Heard)| {
            use std::sync::atomic::Ordering;
            for said in said.lock().unwrap().drain(..) {
                heard(said);
            }
            heard(record::Heard::Clock {
                secs: 1,
                paused: false,
                level: 0.25,
            });
            while !listening.stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(record::Recorded {
                title: "Call".into(),
                body: "notes".into(),
                ..Default::default()
            })
        },
    )
}

#[test]
fn a_call_shows_both_sides_in_turns_and_a_microphone_recording_stays_plain() {
    let said = |speaker, text: &str| record::Heard::Said {
        speaker,
        text: text.into(),
    };
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(saying(vec![
        said("You", "Shall we start?"),
        said("Others", "Yes."),
        said("You", "Shall we start? Great."),
    ]));
    let id = json_of(start_recording(&state, record::Source::Nativecall))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let view = json_of(run(record::status(State(state.clone()), Path(id.clone()))));
    assert_eq!(
        view["transcript"],
        "You: Shall we start?\nOthers: Yes.\nYou: Great."
    );
    assert_eq!(
        view["turns"],
        serde_json::json!([
            { "n": 0, "speaker": "You", "text": "Shall we start?" },
            { "n": 1, "speaker": "Others", "text": "Yes." },
            { "n": 2, "speaker": "You", "text": "Great." },
        ])
    );
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done" || s == "failed");

    state.listener = Some(saying(vec![record::Heard::Transcript(
        "You: is only a word here.".into(),
    )]));
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let view = json_of(run(record::status(State(state.clone()), Path(id.clone()))));
    assert_eq!(view["transcript"], "You: is only a word here.");
    assert!(view.get("turns").is_none());
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done" || s == "failed");
}

fn saving_quickly() -> record::Listener {
    Arc::new(
        |listening: record::Listening, heard: &mut dyn FnMut(record::Heard)| {
            use std::sync::atomic::Ordering;
            while !listening.stop.load(Ordering::Relaxed) {
                let kept = listening.points.lock().unwrap().len();
                listening.saved_points.store(kept, Ordering::Release);
                heard(record::Heard::Clock {
                    secs: 28_800,
                    paused: false,
                    level: 0.25,
                });
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(record::Recorded {
                title: "Lecture".into(),
                body: "points".into(),
                ..Default::default()
            })
        },
    )
}

#[test]
#[ignore]
fn benchmark_many_points() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let text = "x".repeat(1000);
    for total in [200usize, 2_000, 10_000] {
        let (mut state, _d, _ids) = state_with(&[]);
        state.listener = Some(saving_quickly());
        let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
            .as_str()
            .unwrap()
            .to_string();
        recording_until(&state, |s| s == "recording");
        let mut last = std::time::Duration::ZERO;
        let mut refused = None;
        for n in 0..total {
            let started = std::time::Instant::now();
            let answer = runtime.block_on(record::point(
                State(state.clone()),
                Path(id.clone()),
                Json(serde_json::from_value(serde_json::json!({ "text": text })).unwrap()),
            ));
            last = started.elapsed();
            if answer.status() != StatusCode::OK {
                refused = Some((n + 1, answer.status()));
                break;
            }
        }
        let status = runtime.block_on(record::status(State(state.clone()), Path(id.clone())));
        let bytes = runtime
            .block_on(axum::body::to_bytes(status.into_body(), usize::MAX))
            .unwrap()
            .len();
        println!("{total} points: status {bytes} bytes, last add {last:?}, refused {refused:?}");
        run(record::stop(State(state.clone()), Path(id)));
        recording_until(&state, |s| s == "done" || s == "failed");
    }
}

fn add_point(state: &AppState, id: &str, text: &str) -> Response {
    run(record::point(
        State(state.clone()),
        Path(id.to_string()),
        Json(serde_json::from_value(serde_json::json!({ "text": text })).unwrap()),
    ))
}

fn two_hundred_and_two_points(state: &mut AppState) -> String {
    state.listener = Some(saving_quickly());
    let id = json_of(start_recording(state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(state, |s| s == "recording");
    for n in 0..201 {
        let answer = add_point(state, &id, &format!("point {n}"));
        assert_eq!(answer.status(), StatusCode::OK, "point {n}");
    }
    let pasted = run(record::snapshot(
        State(state.clone()),
        Path(id.clone()),
        Json(
            serde_json::from_value(serde_json::json!({ "at": 0, "data": "iVBORw0KGgo=" })).unwrap(),
        ),
    ));
    assert_eq!(pasted.status(), StatusCode::OK);
    id
}

#[test]
fn a_recording_takes_more_than_two_hundred_points() {
    let (mut state, _d, _ids) = state_with(&[]);
    let id = two_hundred_and_two_points(&mut state);
    let view = json_of(run(record::status(State(state.clone()), Path(id.clone()))));
    assert_eq!(view["points_total"], 202);
    let shown = view["points"].as_array().unwrap();
    assert_eq!(shown.len(), record::VIEW_POINTS);
    assert_eq!(shown[0][1], "point 152");
    assert!(shown[49][1].as_str().unwrap().starts_with("Slide at"));
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn a_long_point_is_kept() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(saving_quickly());
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let long = "界".repeat(20_000);
    assert_eq!(add_point(&state, &id, &long).status(), StatusCode::OK);
    let view = json_of(run(record::status(State(state.clone()), Path(id.clone()))));
    assert_eq!(view["points"][0][1], long.as_str());
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn what_you_want_can_be_any_length_while_recording() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(saving_quickly());
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let long = "界".repeat(20_000);
    let answer = run(record::wants(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({ "text": long })).unwrap()),
    ));
    assert_eq!(answer.status(), StatusCode::OK);
    assert_eq!(json_of(answer)["wants"], long.as_str());
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn earlier_points_come_fifty_at_a_time() {
    let (mut state, _d, _ids) = state_with(&[]);
    let id = two_hundred_and_two_points(&mut state);
    let page = |before: &str| {
        run(record::earlier_points(
            State(state.clone()),
            Path(id.clone()),
            axum::extract::Query(
                serde_json::from_value(serde_json::json!({ "before": before })).unwrap(),
            ),
        ))
    };
    let newest = json_of(page("202"));
    assert_eq!(newest["from"], 152);
    assert_eq!(newest["points"].as_array().unwrap().len(), 50);
    assert_eq!(newest["points"][0][1], "point 152");
    let first = json_of(page("2"));
    assert_eq!(first["from"], 0);
    assert_eq!(first["points"][0][1], "point 0");
    assert_eq!(first["points"][1][1], "point 1");
    assert_eq!(first["points"].as_array().unwrap().len(), 2);
    let bad = page("x");
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert!(json_of(bad)["error"].is_string());
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn a_point_is_answered_only_after_it_is_saved() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (mut state, _d, _ids) = state_with(&[]);
    let release = Arc::new(AtomicBool::new(false));
    let held = release.clone();
    state.listener = Some(Arc::new(
        move |listening: record::Listening, heard: &mut dyn FnMut(record::Heard)| {
            while !listening.stop.load(Ordering::Relaxed) {
                if held.load(Ordering::Acquire) {
                    let kept = listening.points.lock().unwrap().len();
                    listening.saved_points.store(kept, Ordering::Release);
                }
                heard(record::Heard::Clock {
                    secs: 1,
                    paused: false,
                    level: 0.25,
                });
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Ok(record::Recorded {
                title: "Lecture".into(),
                body: "points".into(),
                ..Default::default()
            })
        },
    ));
    let id = json_of(start_recording(&state, record::Source::Microphone))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let started = std::time::Instant::now();
    let waiting = add_point(&state, &id, "held back");
    assert!(started.elapsed() >= std::time::Duration::from_secs(4));
    assert_eq!(waiting.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(json_of(waiting)["error"]
        .as_str()
        .unwrap()
        .contains("still being saved"));
    let view = json_of(run(record::status(State(state.clone()), Path(id.clone()))));
    assert_eq!(view["points_total"], 1);
    assert_eq!(view["points"][0][1], "held back");
    release.store(true, Ordering::Release);
    let saved = add_point(&state, &id, "let through");
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(json_of(saved)["points_total"], 2);
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_the_engine_ends_the_native_recording_worker() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let stop_flag: Arc<Mutex<Option<Arc<AtomicBool>>>> = Default::default();
    let exited = Arc::new(AtomicBool::new(false));
    let (flag, done) = (stop_flag.clone(), exited.clone());
    let listener: record::Listener = Arc::new(move |listening, _| {
        *flag.lock().unwrap() = Some(listening.stop.clone());
        while !listening.stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        done.store(true, Ordering::Relaxed);
        anyhow::bail!("stopped")
    });
    let running = crate::start_in(
        crate::Places {
            store: leo_core::store::Store::load_from(&dir.path().join("notes")).unwrap(),
            sessions: dir.path().join("sessions.json"),
        },
        crate::StartOptions {
            port: 0,
            token: "0123456789abcdef0123456789abcdef".into(),
        },
        Powers {
            listener: Some(listener),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let started = record::start(
        State(running.state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({ "directory": "", "source": "microphone" }))
                .unwrap(),
        ),
    )
    .await;
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    while stop_flag.lock().unwrap().is_none() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let begun = std::time::Instant::now();
    running.stop().await;
    let stop_took = begun.elapsed();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let flag = stop_flag.lock().unwrap().clone().unwrap();
    let (signalled, gone) = (flag.load(Ordering::Relaxed), exited.load(Ordering::Relaxed));
    assert!(
        signalled && gone,
        "Running::stop left the recording worker running"
    );
    assert!(stop_took < crate::shutdown::GRACE + crate::shutdown::CLEANUP);
}

#[test]
fn a_recording_started_while_quitting_is_refused() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    assert!(record::stop_for_quit(&state).is_none());
    let started = run(record::start(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({ "directory": "", "source": "microphone" }))
                .unwrap(),
        ),
    ));
    assert_eq!(started.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_of(started)["error"], "leo is quitting.");
    assert!(state.recording.lock().unwrap().is_none());
}

fn kept(notes: &std::path::Path, note: &str) -> Vec<leo_core::recording::Archive> {
    let sources = leo_core::recording::Sources::of(notes, note).unwrap();
    sources
        .heads
        .iter()
        .map(|head| {
            let (mut passages, mut points) = (Vec::new(), Vec::new());
            sources
                .rows(
                    head,
                    |_, p| {
                        passages.push(p);
                        Ok(true)
                    },
                    |_, p| {
                        points.push(p);
                        Ok(true)
                    },
                )
                .unwrap();
            leo_core::recording::Archive {
                id: head.id.clone(),
                started: head.started,
                passages,
                points,
                context: head.context.clone(),
                wants: head.wants.clone(),
                warnings: head.warnings.clone(),
                trace: head.trace.clone(),
                from: head.from.clone(),
            }
        })
        .collect()
}

fn spoken(id: &str, text: &str) -> leo_core::recording::Archive {
    leo_core::recording::Archive {
        id: id.into(),
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 5,
            text: text.into(),
            speaker: String::new(),
            echo: false,
            turns: Vec::new(),
        }],
        points: vec![leo_core::recording::Point {
            at_secs: 2,
            text: format!("typed beside {text}"),
        }],
        context: String::new(),
        wants: String::new(),
        warnings: vec![],
        trace: vec![],
        from: String::new(),
    }
}

fn many(notes: &std::path::Path, note: &str, count: usize) {
    leo_core::recording::save(notes, note, &spoken("rec-00000", "the first one")).unwrap();
    let dir = leo_core::recording::folder(notes, note).unwrap();
    let mut head = leo_core::recording::head(notes, note, "rec-00000").unwrap();
    for n in 1..count {
        head.id = format!("rec-{n:05}");
        head.started += chrono::Duration::seconds(1);
        std::fs::write(
            dir.join(format!("{}.json", head.id)),
            serde_json::to_vec(&head).unwrap(),
        )
        .unwrap();
        for kind in ["passages", "points"] {
            std::fs::copy(
                dir.join(format!("rec-00000.1.{kind}.jsonl")),
                dir.join(format!("{}.1.{kind}.jsonl", head.id)),
            )
            .unwrap();
        }
    }
}

fn transcript_page(state: &AppState, id: &str, query: serde_json::Value) -> serde_json::Value {
    json_of(run(crate::routes::sources::sources(
        State(state.clone()),
        Path(id.to_string()),
        Query(serde_json::from_value(query).unwrap()),
    )))
}

#[test]
fn the_transcript_route_answers_the_list_a_page_and_matches_across_recordings() {
    let (state, _d, ids) = state_with(&[("Graphs", "")]);
    let notes = state.notes_dir.clone();
    let mut first = spoken("rec-first1", "graphs have nodes");
    first.wants = "the exam".into();
    first.trace = vec![leo_core::recording::Trace {
        at: chrono::Utc::now(),
        stage: "saved".into(),
        detail: "ok".into(),
    }];
    leo_core::recording::save(&notes, &ids[0], &first).unwrap();
    let mut second = spoken("rec-second", "nodes and edges");
    second.started += chrono::Duration::seconds(10);
    leo_core::recording::save(&notes, &ids[0], &second).unwrap();
    let page = transcript_page(&state, &ids[0], serde_json::json!({"q": "nodes"}));
    assert_eq!(page["list"].as_array().unwrap().len(), 2);
    assert!(page["list"][0].get("trace").is_none());
    assert_eq!(page["wants"], "the exam");
    assert_eq!(page["page"]["head"]["id"], "rec-first1");
    assert_eq!(page["page"]["head"]["trace"][0]["stage"], "saved");
    assert_eq!(page["page"]["passages"][0][1]["text"], "graphs have nodes");
    assert_eq!(page["page"]["next"], serde_json::Value::Null);
    let found: Vec<&str> = page["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["source"].as_str().unwrap())
        .collect();
    assert_eq!(found, ["rec-first1", "rec-second"]);
    assert_eq!(page["matches"][1]["seq"], 1);
    let later = transcript_page(
        &state,
        &ids[0],
        serde_json::json!({"at": "rec-second", "passages": "end", "points": "1"}),
    );
    assert_eq!(later["page"]["head"]["id"], "rec-second");
    assert!(later["page"]["passages"].as_array().unwrap().is_empty());
    assert_eq!(
        later["page"]["points"][0][1]["text"],
        "typed beside nodes and edges"
    );
    let refused = run(crate::routes::sources::sources(
        State(state.clone()),
        Path(ids[0].clone()),
        Query(serde_json::from_value(serde_json::json!({"passages": "soon"})).unwrap()),
    ));
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn a_fix_through_the_transcript_route_changes_only_the_rows_sent() {
    let (state, _d, ids) = state_with(&[("Graphs", "")]);
    let notes = state.notes_dir.clone();
    let mut source = spoken("rec-fixed1", "dykstra");
    source.passages.push(leo_core::recording::Passage {
        start_secs: 6,
        end_secs: 9,
        text: "untouched".into(),
        speaker: String::new(),
        echo: false,
        turns: Vec::new(),
    });
    leo_core::recording::save(&notes, &ids[0], &source).unwrap();
    let put = |body: serde_json::Value| {
        run(crate::routes::sources::edit_sources(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(serde_json::from_value(body).unwrap()),
        ))
    };
    let fixed = put(
        serde_json::json!({"source": "rec-fixed1", "base": 1, "changes": [{"kind": "passage", "seq": 1, "text": "Dijkstra"}]}),
    );
    assert_eq!(fixed.status(), StatusCode::OK);
    assert_eq!(json_of(fixed)["revision"], 2);
    let stale = put(
        serde_json::json!({"source": "rec-fixed1", "base": 1, "changes": [{"kind": "passage", "seq": 1, "text": "x"}]}),
    );
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let missing = put(
        serde_json::json!({"source": "rec-fixed1", "base": 2, "changes": [{"kind": "point", "seq": 7, "text": "x"}]}),
    );
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    let long = "x".repeat(300_000);
    let kept_whole = put(
        serde_json::json!({"source": "rec-fixed1", "base": 2, "changes": [{"kind": "point", "seq": 1, "text": long}]}),
    );
    assert_eq!(kept_whole.status(), StatusCode::OK);
    assert_eq!(kept(&notes, &ids[0])[0].points[0].text, long);
    let line = put(
        serde_json::json!({"source": "rec-fixed1", "base": 3, "changes": [{"kind": "passage", "seq": 1, "text": long}]}),
    );
    assert_eq!(line.status(), StatusCode::OK);
    assert_eq!(kept(&notes, &ids[0])[0].passages[0].text, long);
    let all = kept(&notes, &ids[0]);
    assert_eq!(
        all[0]
            .passages
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>(),
        [long.as_str(), "untouched"]
    );
    assert_eq!(all[0].points[0].text, long);
}

#[test]
fn regenerate_reads_a_note_with_1001_recordings() {
    let (mut state, _d, ids) = state_with(&[("Graphs", "")]);
    many(&state.notes_dir.clone(), &ids[0], 1001);
    let heard = Arc::new(Mutex::new(0));
    let seen = heard.clone();
    state.regenerator = Some(Arc::new(move |sources, _| {
        *seen.lock().unwrap() = sources.heads.len();
        Ok(("Written".into(), "Notes".into()))
    }));
    let answer = run(crate::routes::sources::regenerate(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({})).unwrap()),
    ));
    assert_eq!(answer.status(), StatusCode::OK);
    assert_eq!(*heard.lock().unwrap(), 1001);
}

#[test]
fn felix_reads_the_transcript_of_a_note_with_1001_recordings() {
    let (state, _d, ids) = state_with(&[("Graphs", "")]);
    many(&state.notes_dir.clone(), &ids[0], 1001);
    store_now(&state, |store| {
        let cache = crate::graph::Cache::default();
        let mut desk = crate::tools::Desk::new(vec![], crate::chat::ROOM)
            .with_access(crate::tools::Access::Read);
        let read = desk.run(
            store,
            &cache,
            &crate::tools::Call {
                name: "read_transcript".into(),
                args: serde_json::json!({"note": "Graphs", "find": "first"}),
            },
        );
        assert!(read.result.contains("the first one"), "{}", read.result);
        assert!(
            !read.result.starts_with("That did not work"),
            "{}",
            read.result
        );
        Ok(())
    })
    .unwrap();
}
