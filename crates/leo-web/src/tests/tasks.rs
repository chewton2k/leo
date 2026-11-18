use super::*;
use crate::routes::tasks::{
    change_task, create_task, delete_task, import_tasks, list_tasks, promote_task, task_ics,
};

fn change(state: &AppState, id: &str, body: serde_json::Value) -> Response {
    run(change_task(
        State(state.clone()),
        Path(id.to_string()),
        Json(serde_json::from_value(body).unwrap()),
    ))
}

fn listed(state: &AppState) -> Vec<serde_json::Value> {
    json_of(run(list_tasks(State(state.clone()))))["tasks"]
        .as_array()
        .unwrap()
        .clone()
}

fn body_of(state: &AppState, id: &str) -> String {
    state
        .fresh()
        .notes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .body
        .clone()
}

#[test]
fn note_action_items_are_ticked_by_desired_state_and_dated_in_the_note() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "## Action items\n- [ ] Submit HW 4\n- [ ] Essay due Oct 16\n## Check yourself\n- [ ] What is a cell?".into();
        store.save().unwrap();
    }
    let tasks = listed(&state);
    assert_eq!(tasks.len(), 2, "{tasks:?}");
    let hw = tasks.iter().find(|t| t["title"] == "Submit HW 4").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..2 {
        assert_eq!(
            change(&state, &hw, serde_json::json!({"done": true})).status(),
            200
        );
    }
    assert!(
        body_of(&state, &ids[0]).contains("- [x] Submit HW 4"),
        "a repeated tick stays ticked"
    );
    let dated = json_of(change(
        &state,
        &hw,
        serde_json::json!({"date": "2026-10-16", "time": "23:59"}),
    ));
    assert_eq!(dated["date"], "2026-10-16");
    assert_eq!(
        dated["id"],
        hw.as_str(),
        "the task keeps its id after its text changes"
    );
    assert!(body_of(&state, &ids[0])
        .contains("- [x] Submit HW 4 — due Friday, October 16, 2026 at 11:59 pm."));
    let essay = listed(&state)
        .iter()
        .find(|t| t["title"] == "Essay due Oct 16")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let unsure = change(&state, &essay, serde_json::json!({"date": "2026-10-16"}));
    assert_eq!(unsure.status(), 409);
    let preview = json_of(unsure)["preview"].clone();
    assert_eq!(
        preview["after"],
        "Essay due Oct 16 — due Friday, October 16, 2026."
    );
    assert!(
        body_of(&state, &ids[0]).contains("- [ ] Essay due Oct 16\n"),
        "nothing changes before confirming"
    );
    assert_eq!(
        change(
            &state,
            &essay,
            serde_json::json!({"date": "2026-10-16", "confirm": true})
        )
        .status(),
        200
    );
    assert!(body_of(&state, &ids[0]).contains("Essay due Oct 16 — due Friday, October 16, 2026."));
    assert_eq!(
        run(delete_task(State(state.clone()), Path(hw.clone()))).status(),
        409
    );
}

#[test]
fn a_promoted_checkbox_and_a_standalone_task_live_and_leave_cleanly() {
    let (state, _d, ids) = state_with(&[("Plans", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "Ideas\n- [ ] Ask tutor about the exam\nEnd".into();
        store.save().unwrap();
    }
    assert!(listed(&state).is_empty(), "plain checkboxes are not tasks");
    let promoted = run(promote_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"note": ids[0], "line": 1})).unwrap()),
    ));
    assert_eq!(promoted.status(), 201);
    let promoted = json_of(promoted);
    assert_eq!(promoted["kind"], "exam");
    assert_eq!(listed(&state).len(), 1);
    let id = promoted["id"].as_str().unwrap().to_string();
    assert_eq!(
        run(delete_task(State(state.clone()), Path(id))).status(),
        204
    );
    assert!(listed(&state).is_empty());
    assert_eq!(
        body_of(&state, &ids[0]),
        "Ideas\n- [ ] Ask tutor about the exam\nEnd",
        "unpromoting leaves the note alone"
    );

    let made = run(create_task(State(state.clone()), Json(serde_json::from_value(serde_json::json!({"title": "  Book dentist  ", "date": "2026-11-02", "time": "9:30"})).unwrap())));
    assert_eq!(made.status(), 201);
    let made = json_of(made);
    assert_eq!(
        (made["title"].as_str(), made["time"].as_str()),
        (Some("Book dentist"), Some("09:30:00"))
    );
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(
        change(&state, &id, serde_json::json!({"date": null})).status(),
        200
    );
    assert_eq!(listed(&state)[0]["date"], serde_json::Value::Null);
    assert_eq!(
        run(task_ics(State(state.clone()), Path(id.clone()))).status(),
        400,
        "an undated task asks for a date first"
    );
    change(&state, &id, serde_json::json!({"date": "2026-11-02"}));
    let ics = run(task_ics(State(state.clone()), Path(id.clone())));
    let text = String::from_utf8(
        run(axum::body::to_bytes(ics.into_body(), usize::MAX))
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("SUMMARY:Book dentist") && text.contains("DTSTART;VALUE=DATE:20261102"));
    let removed = json_of(run(delete_task(State(state.clone()), Path(id.clone()))));
    assert!(listed(&state).is_empty());
    let again = run(import_tasks(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({"tasks": [removed.clone(), removed]}))
                .unwrap(),
        ),
    ));
    assert_eq!(
        json_of(again)["added"],
        1,
        "importing the same task twice adds it once"
    );
    assert_eq!(listed(&state).len(), 1);
    assert_eq!(
        run(create_task(
            State(state.clone()),
            Json(serde_json::from_value(serde_json::json!({"title": " "})).unwrap())
        ))
        .status(),
        400
    );
}

#[test]
fn descriptions_are_optional_preserved_clearable_and_restored_with_tasks() {
    let (state, _dir, _) = state_with(&[]);
    let make = |body| {
        json_of(run(create_task(
            State(state.clone()),
            Json(serde_json::from_value(body).unwrap()),
        )))
    };
    let legacy = make(serde_json::json!({"title": "No details"}));
    assert_eq!(legacy["description"], "");
    let details = "Read chapter 3\nBring café notes; <b>not HTML</b>";
    let task = make(
        serde_json::json!({"title": "Study", "description": format!("  {details}  "), "date": "2026-10-16", "repeat": [0, 2]}),
    );
    let id = task["id"].as_str().unwrap();
    assert_eq!(task["description"], details);
    let raw = crate::tasks::load(&state.chats, id).unwrap();
    assert_eq!(raw.description, details);
    assert_eq!(
        json_of(change(
            &state,
            id,
            serde_json::json!({"done": true, "on": "2026-10-19"})
        ))["description"],
        details
    );
    let ics = run(task_ics(State(state.clone()), Path(id.into())));
    let bytes = run(axum::body::to_bytes(ics.into_body(), usize::MAX)).unwrap();
    let ics = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        ics.contains("Read chapter 3\\nBring café notes\\; <b>not HTML</b>"),
        "{ics}"
    );
    let removed = json_of(run(delete_task(State(state.clone()), Path(id.into()))));
    assert_eq!(removed["description"], details);
    let imported = run(import_tasks(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"tasks": [removed]})).unwrap()),
    ));
    assert_eq!(json_of(imported)["added"], 1);
    assert_eq!(
        crate::tasks::load(&state.chats, id).unwrap().description,
        details
    );
    assert_eq!(
        json_of(change(
            &state,
            id,
            serde_json::json!({"description": "Revised ✨"})
        ))["description"],
        "Revised ✨"
    );
    assert_eq!(
        json_of(change(&state, id, serde_json::json!({"description": "  "})))["description"],
        ""
    );
    let long = "界".repeat(crate::tasks::MOST_DESCRIPTION + 1);
    assert_eq!(
        change(&state, id, serde_json::json!({"description": long})).status(),
        400
    );
    assert_eq!(
        crate::tasks::load(&state.chats, id).unwrap().description,
        ""
    );
    assert_eq!(
        run(create_task(
            State(state.clone()),
            Json(
                serde_json::from_value(
                    serde_json::json!({"title": "Too much", "description": long})
                )
                .unwrap()
            )
        ))
        .status(),
        400
    );
    let at_limit = "界".repeat(crate::tasks::MOST_DESCRIPTION);
    assert_eq!(
        json_of(change(
            &state,
            id,
            serde_json::json!({"description": at_limit})
        ))["description"],
        at_limit
    );
}

#[test]
fn checkbox_descriptions_are_separate_from_the_note_and_survive_date_and_kind_changes() {
    let (state, _dir, ids) = state_with(&[("Plan", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "## Action items\n- [ ] Read chapter 3\n".into();
        store.save().unwrap();
    }
    let id = listed(&state)[0]["id"].as_str().unwrap().to_string();
    let before = body_of(&state, &ids[0]);
    assert_eq!(listed(&state)[0]["description"], "");
    assert_eq!(
        json_of(change(
            &state,
            &id,
            serde_json::json!({"description": "Focus on heaps"})
        ))["description"],
        "Focus on heaps"
    );
    assert_eq!(body_of(&state, &ids[0]), before);
    change(
        &state,
        &id,
        serde_json::json!({"kind": "assignment", "date": "2026-10-16"}),
    );
    assert_eq!(listed(&state)[0]["description"], "Focus on heaps");
    change(&state, &id, serde_json::json!({"description": ""}));
    assert_eq!(listed(&state)[0]["description"], "");
    assert_eq!(listed(&state)[0]["id"], id);
}

#[test]
fn one_day_of_a_repeating_task_moves_without_moving_the_rest() {
    let (state, _d, _ids) = state_with(&[]);
    let made = json_of(run(create_task(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({"title": "Gym", "date": "2026-11-02", "time": "09:00", "repeat": [0, 2]}))
                .unwrap(),
        ),
    )));
    let id = made["id"].as_str().unwrap();
    let moved = json_of(change(
        &state,
        id,
        serde_json::json!({"occurrence": "2026-11-04", "date": "2026-11-05", "time": "15:00", "end": "16:00"}),
    ));
    assert_eq!(moved["date"], "2026-11-02", "the series keeps its start");
    assert_eq!(moved["moved"]["2026-11-04"]["date"], "2026-11-05");
    assert_eq!(moved["moved"]["2026-11-04"]["end"], "16:00:00");
    let done = json_of(change(
        &state,
        id,
        serde_json::json!({"done": true, "on": "2026-11-04"}),
    ));
    assert_eq!(
        done["done_on"],
        serde_json::json!(["2026-11-04"]),
        "done keeps the original day"
    );
    assert_eq!(
        change(
            &state,
            id,
            serde_json::json!({"occurrence": "2026-11-03", "date": "2026-11-05"})
        )
        .status(),
        400,
        "a Tuesday is not one of its days"
    );
    assert_eq!(
        change(&state, id, serde_json::json!({"occurrence": "2026-11-04"})).status(),
        400
    );
    let back = json_of(change(
        &state,
        id,
        serde_json::json!({"occurrence": "2026-11-04", "date": "2026-11-04", "time": "09:00"}),
    ));
    assert!(back.get("moved").is_none(), "{back}");
}

#[cfg(unix)]
#[test]
fn a_task_that_cannot_be_deleted_says_so_instead_of_vanishing_until_the_next_load() {
    use std::os::unix::fs::PermissionsExt;
    let (state, dir, _) = state_with(&[]);
    let made = json_of(run(create_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"title": "Book dentist"})).unwrap()),
    )));
    let id = made["id"].as_str().unwrap().to_string();
    let folder = dir.path().join("notes/.leo/tasks");
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let locked = std::fs::write(folder.join("probe"), "x").is_err();
    let reply = run(delete_task(State(state.clone()), Path(id.clone())));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    if locked {
        assert_eq!(reply.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(crate::tasks::load(&state.chats, &id).is_some());
    }
}

#[test]
fn a_repeated_request_id_creates_one_task_and_answers_with_the_first() {
    let (state, _dir, _) = state_with(&[]);
    let request = uuid::Uuid::new_v4().to_string();
    let make = |title: &str| {
        json_of(run(create_task(
            State(state.clone()),
            Json(
                serde_json::from_value(serde_json::json!({"title": title, "request": request}))
                    .unwrap(),
            ),
        )))
    };
    let first = make("Book dentist");
    let again = make("Book dentist");
    assert_eq!(first["id"], again["id"]);
    assert_eq!(listed(&state).len(), 1);
    let other = json_of(run(create_task(
        State(state.clone()),
        Json(
            serde_json::from_value(
                serde_json::json!({"title": "Book dentist", "request": uuid::Uuid::new_v4().to_string()}),
            )
            .unwrap(),
        ),
    )));
    assert_ne!(first["id"], other["id"]);
    assert_eq!(listed(&state).len(), 2);
}

fn with_action_items(state: &AppState, id: &str, body: &str) {
    let mut store = state.fresh();
    store.find_note_mut(id).unwrap().body = body.into();
    store.save().unwrap();
}

#[test]
fn a_second_task_listing_with_nothing_changed_does_not_scan_the_notes_again() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    with_action_items(&state, &ids[0], "## Action items\n- [ ] Submit HW 4");
    let first = listed(&state);
    let again = listed(&state);
    assert_eq!(first, again);
    assert_eq!(first.len(), 1);
    assert_eq!(state.listed.builds(), 1);
}

#[test]
fn a_saved_note_makes_the_next_listing_scan_again() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    with_action_items(&state, &ids[0], "## Action items\n- [ ] Submit HW 4");
    assert_eq!(listed(&state).len(), 1);
    let id = ids[0].clone();
    run(state.with_store(move |store| {
        store.find_note_mut(&id).unwrap().body =
            "## Action items\n- [ ] Submit HW 4\n- [ ] Email Maya".into();
        store.save().unwrap();
        Ok(())
    }))
    .unwrap();
    assert_eq!(listed(&state).len(), 2);
    assert_eq!(state.listed.builds(), 2);
}

#[test]
fn a_changed_task_record_makes_the_next_listing_scan_again() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    with_action_items(&state, &ids[0], "## Action items\n- [ ] Submit HW 4");
    let before = listed(&state);
    let meta = crate::tasks::Meta {
        id: before[0]["id"].as_str().unwrap().into(),
        note: ids[0].clone(),
        text: "Submit HW 4".into(),
        description: "Chapter four".into(),
        ..Default::default()
    };
    crate::tasks::put_meta(&state.chats, &meta).unwrap();
    let after = listed(&state);
    assert_eq!(after[0]["description"], "Chapter four");
    assert_eq!(state.listed.builds(), 2);
}

#[test]
fn a_standalone_task_saved_makes_the_next_listing_scan_again() {
    let (state, _d, _ids) = state_with(&[]);
    assert!(listed(&state).is_empty());
    let created = run(create_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"title": "Pay rent"})).unwrap()),
    ));
    assert!(created.status().is_success());
    assert_eq!(listed(&state)[0]["title"], "Pay rent");
}

#[test]
fn a_note_edited_outside_leo_makes_the_next_listing_scan_again() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    with_action_items(&state, &ids[0], "## Action items\n- [ ] Submit HW 4");
    assert_eq!(listed(&state).len(), 1);
    let path = std::fs::read_dir(state.notes_dir.join("bio"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "md"))
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{text}\n- [ ] Email Maya, a longer line\n")).unwrap();
    state
        .refresh_requested
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(listed(&state).len(), 2);
    assert_eq!(state.listed.builds(), 2);
}

#[test]
fn a_listing_queued_before_a_save_and_run_after_it_shows_the_saved_note() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    with_action_items(&state, &ids[0], "## Action items\n- [ ] Submit HW 4");
    assert_eq!(listed(&state).len(), 1);
    let mut held = crate::lock_storage(&state.store);
    let waiting = {
        let state = state.clone();
        std::thread::spawn(move || listed(&state))
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    held.find_note_mut(&ids[0]).unwrap().body =
        "## Action items\n- [ ] Submit HW 4\n- [ ] Email Maya".into();
    held.save().unwrap();
    state
        .generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    drop(held);
    assert_eq!(waiting.join().unwrap().len(), 2);
}

#[test]
fn finding_one_task_copies_one_task_and_not_the_list() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    let lines: String = (0..600)
        .map(|i| format!("- [ ] Task number {i}\n"))
        .collect();
    with_action_items(&state, &ids[0], &format!("## Action items\n{lines}"));
    let metas = crate::tasks::metas(&state.chats);
    let store = state.fresh();
    let index = state.listed.index(&state.generation, &store, &metas, &[]);
    let id = index.all[300].id.clone();
    let (at, held) = (Arc::as_ptr(&index), Arc::strong_count(&index));
    let found = state
        .listed
        .find(&state.generation, &store, &metas, &[], &id)
        .unwrap();
    assert_eq!(found.id, id);
    assert_eq!(Arc::strong_count(&index), held);
    let again = state.listed.index(&state.generation, &store, &metas, &[]);
    assert_eq!(Arc::as_ptr(&again), at);
    assert_eq!(state.listed.builds(), 1);
    assert!(state
        .listed
        .find(&state.generation, &store, &metas, &[], "nothing")
        .is_none());
}

fn bench_state(n: usize) -> (AppState, tempfile::TempDir, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let notes_dir = dir.path().join("notes");
    let mut store = Store::load_from(&notes_dir).unwrap();
    let filler =
        "the graph search queue stack heap tree node edge visit order breadth. ".repeat(70);
    let mut ids = Vec::new();
    for i in 0..n {
        let body = if i % 5 == 0 {
            format!("{filler}\n\n## Action items\n- [ ] Read chapter {i}\n- [ ] Submit lab {i} Oct 16\n")
        } else {
            filler.clone()
        };
        let id = store
            .create_note(
                format!("Lecture {i}"),
                body,
                vec![],
                &format!("Course {}", i % 20),
            )
            .unwrap()
            .id
            .clone();
        ids.push(id);
    }
    store.save().unwrap();
    let store = Store::load_from(&notes_dir).unwrap();
    let (mut state, other, _) = state_with(&[]);
    state.chats = dir.path().join("chats");
    state.store = Arc::new(Mutex::new(Storage {
        changes: Some(store_refresh::Changes::new(&notes_dir)),
        store,
        reload: false,
    }));
    for (i, id) in ids.iter().enumerate().take(1_000) {
        let meta = crate::tasks::Meta {
            id: format!("m{i:08}"),
            note: id.clone(),
            text: format!("Read chapter {i}"),
            ..Default::default()
        };
        crate::tasks::put_meta(&state.chats, &meta).unwrap();
        let task = crate::tasks::Task {
            id: format!("t{i:08}"),
            title: format!("Standalone {i}"),
            description: String::new(),
            kind: "other".into(),
            done: false,
            date: None,
            time: None,
            end: None,
            note: None,
            sent_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            repeat: Vec::new(),
            done_on: Vec::new(),
            moved: Default::default(),
            deleted: false,
        };
        crate::tasks::put(&state.chats, &task).unwrap();
    }
    (state, dir, other)
}

fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = std::time::Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[runs / 2]
}

#[test]
#[ignore]
fn audit_bench_all_listed() {
    let sizes: Vec<usize> = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_else(|| vec![5_000, 50_000]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    for n in sizes {
        let (state, _dir, _other) = bench_state(n);
        let before = |state: &AppState| {
            let (metas, stored) = (
                crate::tasks::metas(&state.chats),
                crate::tasks::load_all(&state.chats),
            );
            let store = crate::lock_storage(&state.store);
            let mut all = crate::tasks::note_tasks(&store, &metas);
            all.extend(
                stored
                    .iter()
                    .map(|t| crate::tasks::standalone(t, Some(&store))),
            );
            crate::tasks::sorted(all).len()
        };
        let first = median_ms(1, || {
            rt.block_on(crate::routes::tasks::all_listed(&state))
                .unwrap();
        });
        let repeat = median_ms(11, || {
            rt.block_on(crate::routes::tasks::all_listed(&state))
                .unwrap();
        });
        let mut paired = [Vec::new(), Vec::new()];
        for _ in 0..7 {
            paired[0].push(median_ms(1, || {
                before(&state);
            }));
            paired[1].push(median_ms(1, || {
                rt.block_on(crate::routes::tasks::all_listed(&state))
                    .unwrap();
            }));
        }
        paired.iter_mut().for_each(|v| v.sort_by(f64::total_cmp));
        println!(
            "notes {n}: old way {:.2} ms, cached way {:.2} ms (median of 7, alternating)",
            paired[0][3], paired[1][3]
        );
        let metas = crate::tasks::metas(&state.chats);
        let stored = crate::tasks::load_all(&state.chats);
        let (all, generation) = (state.listed.clone(), state.generation.clone());
        let found = {
            let store = crate::lock_storage(&state.store);
            let id = all.index(&generation, &store, &metas, &stored).all[0]
                .id
                .clone();
            median_ms(11, || {
                all.find(&generation, &store, &metas, &stored, &id).unwrap();
            })
        };
        let reading_metas = median_ms(11, || {
            crate::tasks::metas(&state.chats);
        });
        let reading_tasks = median_ms(11, || {
            crate::tasks::load_all(&state.chats);
        });
        println!("task records read {reading_metas:.2} ms, standalone task files read {reading_tasks:.2} ms");
        println!(
            "notes {n}: first {first:.2} ms, repeat (median of 11) {repeat:.2} ms, warm find {found:.4} ms"
        );
    }
}
