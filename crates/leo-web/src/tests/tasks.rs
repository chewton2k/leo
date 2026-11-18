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
    json_of(run(list_tasks(
        State(state.clone()),
        Query(Default::default()),
    )))["tasks"]
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
    let none = Arc::new(Vec::new());
    let index = state.listed.index(&state.generation, &store, &metas, &none);
    let id = index.all[300].id.clone();
    let (at, held) = (Arc::as_ptr(&index), Arc::strong_count(&index));
    let found = state
        .listed
        .find(&state.generation, &store, &metas, &none, &id)
        .unwrap();
    assert_eq!(found.id, id);
    assert_eq!(Arc::strong_count(&index), held);
    let again = state.listed.index(&state.generation, &store, &metas, &none);
    assert_eq!(Arc::as_ptr(&again), at);
    assert_eq!(state.listed.builds(), 1);
    assert!(state
        .listed
        .find(&state.generation, &store, &metas, &none, "nothing")
        .is_none());
}

#[test]
fn editing_a_note_task_reads_only_its_note() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    let lines: String = (0..600)
        .map(|i| format!("- [ ] Task number {i}\n"))
        .collect();
    with_action_items(&state, &ids[0], &format!("## Action items\n{lines}"));
    let first = listed(&state);
    assert_eq!(state.listed.builds(), 1);
    let (id, cloned) = (
        first[250]["id"].as_str().unwrap().to_string(),
        state.listed.cloned(),
    );
    let ticked = change(&state, &id, serde_json::json!({"done": true}));
    assert_eq!(ticked.status(), StatusCode::OK);
    assert_eq!(json_of(ticked)["done"], true);
    assert_eq!(state.listed.builds(), 1);
    assert_eq!(state.listed.cloned(), cloned + 1);
    assert_eq!(
        body_of(&state, &ids[0])
            .lines()
            .filter(|l| l.starts_with("- [x]"))
            .count(),
        1
    );
}

#[test]
fn a_task_past_two_thousand_can_be_edited() {
    let (state, _d, _ids) = state_with(&[]);
    let folder = state.notes_dir.join("bio");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..2_100 {
        std::fs::write(
            folder.join(format!("Note {i}.md")),
            "## Action items\n- [ ] Hand it in\n",
        )
        .unwrap();
    }
    state
        .refresh_requested
        .store(true, std::sync::atomic::Ordering::Release);
    let last = {
        let store = state.fresh();
        assert_eq!(store.notes.len(), 2_100);
        store.notes[2_050].id.clone()
    };
    let task = crate::tasks::note_task_id(&last, "Hand it in");
    let ticked = change(&state, &task, serde_json::json!({"done": true}));
    assert_eq!(ticked.status(), StatusCode::OK);
    assert_eq!(json_of(ticked)["done"], true);
    assert!(body_of(&state, &last).contains("- [x] Hand it in"));
}

fn page_of(state: &AppState, query: serde_json::Value) -> serde_json::Value {
    json_of(run(list_tasks(
        State(state.clone()),
        Query(serde_json::from_value(query).unwrap()),
    )))
}

#[test]
fn the_task_list_is_read_a_page_at_a_time() {
    let (state, _d, ids) = state_with(&[("Big", "bio"), ("Small", "bio")]);
    let lines: String = (0..620)
        .map(|i| format!("- [ ] Task number {i}\n"))
        .collect();
    with_action_items(&state, &ids[0], &format!("## Action items\n{lines}"));
    with_action_items(
        &state,
        &ids[1],
        "## Action items\n- [ ] Other one\n- [ ] Other two",
    );
    let whole = page_of(&state, serde_json::json!({"limit": 10_000}));
    assert_eq!(whole["total"], 622);
    let first = page_of(&state, serde_json::json!({}));
    assert_eq!(first["tasks"].as_array().unwrap().len(), 500);
    assert_eq!(first["total"], 622);
    let rest = page_of(&state, serde_json::json!({"offset": 500}));
    assert_eq!(rest["tasks"].as_array().unwrap().len(), 122);
    let joined: Vec<&serde_json::Value> = first["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .chain(rest["tasks"].as_array().unwrap())
        .collect();
    assert_eq!(
        joined,
        whole["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .collect::<Vec<_>>()
    );
    let big = page_of(&state, serde_json::json!({"note": ids[0]}));
    assert_eq!(
        (big["tasks"].as_array().unwrap().len(), big["total"].clone()),
        (500, 620.into())
    );
    let big_rest = page_of(&state, serde_json::json!({"note": ids[0], "offset": 500}));
    assert_eq!(big_rest["tasks"].as_array().unwrap().len(), 120);
    assert_eq!(
        page_of(&state, serde_json::json!({"note": ids[1]}))["total"],
        2
    );
    let word = page_of(&state, serde_json::json!({"q": "NUMBER 61"}));
    assert_eq!(word["total"], 11);
    assert!(word["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["title"].as_str().unwrap().contains("number 61")));
    assert_eq!(
        page_of(&state, serde_json::json!({"offset": 9_999}))["tasks"],
        serde_json::json!([])
    );
}

fn rev_of(state: &AppState, query: serde_json::Value) -> String {
    let reply = run(crate::routes::notes::get_rev(
        State(state.clone()),
        Query(serde_json::from_value(query).unwrap()),
    ))
    .unwrap();
    reply.0["rev"].as_str().unwrap().to_string()
}

#[test]
fn the_revision_moves_when_a_note_or_task_changes() {
    let (state, dir, ids) = state_with(&[("Lecture 5", "bio")]);
    let notes_dir = state.notes_dir.clone();
    state.store.lock().unwrap().changes = Some(crate::store_refresh::Changes::new(&notes_dir));
    let none = serde_json::json!({});
    let first = rev_of(&state, none.clone());
    assert_eq!(first.len(), 16);
    for _ in 0..3 {
        assert_eq!(rev_of(&state, none.clone()), first);
    }

    let id = ids[0].clone();
    run(state.with_store(move |store| {
        store.find_note_mut(&id).unwrap().body = "Something new.".into();
        store.save().unwrap();
        Ok(())
    }))
    .unwrap();
    let saved = rev_of(&state, none.clone());
    assert_ne!(saved, first, "a save moves it");
    assert_eq!(rev_of(&state, none.clone()), saved);

    let folder: crate::routes::notes::CreateDirBody =
        serde_json::from_value(serde_json::json!({"path": "chem"})).unwrap();
    let _ = run(crate::routes::notes::create_dir(
        State(state.clone()),
        Json(folder),
    ))
    .unwrap();
    let folded = rev_of(&state, none.clone());
    assert_ne!(folded, saved, "a new folder moves it");

    let made = run(create_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"title": "Pay rent"})).unwrap()),
    ));
    assert!(made.status().is_success());
    let tasked = rev_of(&state, none.clone());
    assert_ne!(tasked, folded, "a task created through the route moves it");

    let board = json_of(run(crate::routes::boards::create_board(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"name": "Plan"})).unwrap()),
    )));
    let boarded = rev_of(&state, none.clone());
    assert_ne!(boarded, tasked);
    let card = json_of(run(crate::routes::boards::create_card(
        State(state.clone()),
        Path(board["id"].as_str().unwrap().to_string()),
        Json(serde_json::from_value(serde_json::json!({"title": "Aaaa"})).unwrap()),
    )));
    let carded = rev_of(&state, none.clone());
    assert_ne!(carded, boarded);
    assert_eq!(rev_of(&state, none.clone()), carded);

    let path = dir
        .path()
        .join("notes/.leo/cards")
        .join(format!("{}.json", card["id"].as_str().unwrap()));
    let before = std::fs::metadata(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Aaaa"));
    std::fs::write(&path, text.replace("Aaaa", "Bbbb")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(before.modified().unwrap() + std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), before.len());
    let started = std::time::Instant::now();
    let rewritten = loop {
        let now = rev_of(&state, none.clone());
        if now != carded || started.elapsed() > std::time::Duration::from_secs(10) {
            break now;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_ne!(
        rewritten, carded,
        "a card file rewritten in place with the same length and a new time moves it"
    );

    std::fs::write(
        notes_dir.join("outside.md"),
        "---\nid: 11111111-2222-4333-8444-555555555555\ntitle: Outside\n---\nWritten by another program.\n",
    )
    .unwrap();
    let seen = rev_of(&state, serde_json::json!({"foreground": "1"}));
    assert_ne!(seen, rewritten, "foreground=1 reads the disk and moves it");
    assert_eq!(rev_of(&state, none), seen);
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
            crate::tasks::in_order(&mut all);
            all.truncate(crate::tasks::LISTED_PAGE);
            all.len()
        };
        let first = median_ms(1, || {
            rt.block_on(crate::routes::tasks::listed_index(&state))
                .unwrap();
        });
        let repeat = median_ms(11, || {
            rt.block_on(crate::routes::tasks::listed_index(&state))
                .unwrap();
        });
        let mut paired = [Vec::new(), Vec::new()];
        for _ in 0..7 {
            paired[0].push(median_ms(1, || {
                before(&state);
            }));
            paired[1].push(median_ms(1, || {
                rt.block_on(crate::routes::tasks::listed_index(&state))
                    .unwrap();
            }));
        }
        paired.iter_mut().for_each(|v| v.sort_by(f64::total_cmp));
        println!(
            "notes {n}: old way {:.2} ms, cached way {:.2} ms (median of 7, alternating)",
            paired[0][3], paired[1][3]
        );
        let metas = crate::tasks::metas(&state.chats);
        let stored = crate::tasks::snapshot(&state.chats);
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

#[test]
#[ignore]
fn audit_bench_p11_lock_work() {
    let sizes: Vec<usize> = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_else(|| vec![5_000, 50_000]);
    for n in sizes {
        let (state, _dir, _other) = bench_state(n);
        let metas = crate::tasks::metas(&state.chats);
        let stored = crate::tasks::snapshot(&state.chats);
        let store = crate::lock_storage(&state.store);
        let id = {
            let all = crate::tasks::note_tasks(&store, &metas);
            all[all.len() / 2].id.clone()
        };
        let (listed, generation) = (state.listed.clone(), state.generation.clone());
        let warm = |store: &Store| {
            crate::tasks::find_listed(&listed, &generation, store, &metas, &stored, &id).unwrap();
        };
        warm(&store);
        let mut paired = [Vec::new(), Vec::new(), Vec::new()];
        for _ in 0..9 {
            paired[0].push(median_ms(1, || {
                crate::tasks::note_tasks(&store, &metas)
                    .into_iter()
                    .find(|t| t.id == id)
                    .unwrap();
            }));
            paired[1].push(median_ms(1, || warm(&store)));
            generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            paired[2].push(median_ms(1, || warm(&store)));
        }
        paired.iter_mut().for_each(|v| v.sort_by(f64::total_cmp));
        println!(
            "notes {n}: old scan {:.3} ms, find_listed warm {:.4} ms, find_listed after a change {:.3} ms (median of 9, alternating)",
            paired[0][4], paired[1][4], paired[2][4]
        );
    }
}

#[test]
#[ignore]
fn audit_bench_p11_combine_lock() {
    use crate::routes::notes::version_of;
    let megabytes: usize = std::env::var("LEO_BENCH_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let (state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    let notes = state.fresh().notes_dir.clone();
    let big = leo_core::recording::Archive {
        id: "session-bfs01".into(),
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 5,
            text: "word ".repeat(megabytes * 1024 * 1024 / 5),
            speaker: String::new(),
        }],
        points: vec![],
        context: String::new(),
        wants: String::new(),
        warnings: vec![],
        trace: vec![],
        from: String::new(),
    };
    let held = |state: &AppState, work: &mut dyn FnMut()| {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let longest = Arc::new(Mutex::new(0f64));
        let probe = {
            let (state, stop, longest) = (state.clone(), stop.clone(), longest.clone());
            std::thread::spawn(move || {
                let mut since: Option<std::time::Instant> = None;
                while !stop.load(std::sync::atomic::Ordering::Acquire) {
                    let free = state.store.try_lock().is_ok();
                    match (free, since) {
                        (false, None) => since = Some(std::time::Instant::now()),
                        (true, Some(at)) => {
                            let mut longest = longest.lock().unwrap();
                            *longest = longest.max(at.elapsed().as_secs_f64() * 1000.0);
                            since = None;
                        }
                        _ => {}
                    }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            })
        };
        work();
        stop.store(true, std::sync::atomic::Ordering::Release);
        probe.join().unwrap();
        let held = *longest.lock().unwrap();
        held
    };
    leo_core::recording::save(&notes, &ids[1], &big).unwrap();
    let (from, target) = (ids[1].clone(), ids[0].clone());
    let old = held(&state, &mut || {
        let (notes, from, target) = (notes.clone(), from.clone(), target.clone());
        run(state.with_store(move |_| {
            let copied = leo_core::recording::copy_to(&notes, &from, &target, "BFS").unwrap();
            leo_core::recording::remove(&notes, &target, &copied).unwrap();
            Ok(())
        }))
        .unwrap();
    });
    let (with_base, base) = {
        let store = state.fresh();
        (
            version_of(store.find_note(&ids[1]).unwrap()),
            version_of(store.find_note(&ids[0]).unwrap()),
        )
    };
    let new = held(&state, &mut || {
        let body = serde_json::json!({ "with": ids[1], "body": "combined", "base": base, "with_base": with_base });
        let saved = crate::routes::combine::keep(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(serde_json::from_value(body).unwrap()),
        );
        assert!(run(saved).status().is_success());
    });
    println!(
        "{megabytes} MB transcript: lock held {old:.2} ms with the copy inside (old way), {new:.2} ms by keep (new way)"
    );
}

#[test]
#[ignore]
fn audit_bench_rev_probe() {
    let sizes: Vec<usize> = std::env::var("LEO_BENCH_CARDS")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_else(|| vec![5_000, 50_000]);
    let notes: usize = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, dir, _other) = bench_state(notes);
    let cards = dir.path().join("notes/.leo/cards");
    std::fs::create_dir_all(&cards).unwrap();
    let mut written = 0;
    for target in sizes {
        while written < target {
            std::fs::write(cards.join(format!("c{written:08}.json")), "{}").unwrap();
            written += 1;
        }
        let probe = median_ms(11, || {
            let _ = rt
                .block_on(crate::routes::notes::get_rev(
                    State(state.clone()),
                    Query(serde_json::from_value(serde_json::json!({})).unwrap()),
                ))
                .unwrap();
        });
        println!("notes {notes}, card files {written}: probe {probe:.3} ms");
    }
}

fn stored_task(n: usize) -> crate::tasks::Task {
    crate::tasks::Task {
        id: format!("taudit{n:08}"),
        title: format!("Standalone {n}"),
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
    }
}

fn write_task_files(dir: &std::path::Path, from: usize, to: usize) {
    let folder = dir.join("notes/.leo/tasks");
    std::fs::create_dir_all(&folder).unwrap();
    for n in from..to {
        let task = stored_task(n);
        std::fs::write(
            folder.join(format!("{}.json", task.id)),
            serde_json::to_string(&task).unwrap() + "\n",
        )
        .unwrap();
    }
}

#[test]
fn tasks_past_ten_thousand_are_made_listed_and_counted() {
    let (state, dir, _) = state_with(&[]);
    write_task_files(dir.path(), 0, 10_050);
    let created = run(create_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"title": "One more"})).unwrap()),
    ));
    assert_eq!(created.status(), 201);
    assert_eq!(crate::tasks::count(&state.chats), 10_051);
    let page = page_of(&state, serde_json::json!({"offset": 10_000, "limit": 100}));
    assert_eq!(page["total"], 10_051);
    assert_eq!(page["tasks"].as_array().unwrap().len(), 51);
}

#[test]
fn a_restore_of_more_than_one_piece_keeps_them_all() {
    let (state, _dir, _) = state_with(&[]);
    let tasks: Vec<_> = (0..1_200).map(stored_task).collect();
    let answer = json_of(run(import_tasks(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({ "tasks": tasks })).unwrap()),
    )));
    assert_eq!(answer["added"], 1_200);
    assert_eq!(crate::tasks::count(&state.chats), 1_200);
}

#[test]
fn an_unchanged_task_file_is_not_read_again() {
    let (state, dir, _) = state_with(&[]);
    write_task_files(dir.path(), 0, 50);
    let reads = || crate::jsonline::READS.with(|reads| reads.get());
    let first = crate::tasks::load_all(&state.chats);
    assert_eq!(first.len(), 50);
    let after_first = reads();
    assert_eq!(crate::tasks::load_all(&state.chats).len(), 50);
    assert_eq!(reads(), after_first, "an unchanged folder reads no file");
    let mut changed = stored_task(7);
    changed.title = "Renamed".into();
    changed.updated_at += chrono::Duration::seconds(5);
    crate::tasks::put(&state.chats, &changed).unwrap();
    let again = crate::tasks::load_all(&state.chats);
    assert_eq!(reads(), after_first + 1, "one written file is read once");
    assert!(again.iter().any(|t| t.title == "Renamed"));
}

#[test]
fn a_task_file_edited_outside_leo_is_seen() {
    let (state, dir, _) = state_with(&[]);
    write_task_files(dir.path(), 0, 3);
    assert_eq!(crate::tasks::load_all(&state.chats).len(), 3);
    let mut edited = stored_task(1);
    edited.title = "Edited by hand, longer".into();
    edited.updated_at += chrono::Duration::seconds(5);
    std::fs::write(
        dir.path().join("notes/.leo/tasks/taudit00000001.json"),
        serde_json::to_string(&edited).unwrap() + "\n",
    )
    .unwrap();
    std::fs::remove_file(dir.path().join("notes/.leo/tasks/taudit00000002.json")).unwrap();
    let seen = crate::tasks::load_all(&state.chats);
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().any(|t| t.title == "Edited by hand, longer"));
}

#[test]
#[ignore]
fn audit_bench_task_files() {
    let sizes: Vec<usize> = std::env::var("LEO_BENCH_TASKS")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_else(|| vec![5_000, 20_000]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (state, dir, _other) = state_with(&[]);
    crate::lock_storage(&state.store).changes = Some(store_refresh::Changes::new(&state.notes_dir));
    let task_dir = dir.path().join("notes/.leo/tasks");
    std::fs::create_dir_all(&task_dir).unwrap();
    let mut written = 0;
    for target in sizes {
        while written < target {
            let task = stored_task(written);
            std::fs::write(
                task_dir.join(format!("{}.json", task.id)),
                serde_json::to_string(&task).unwrap() + "\n",
            )
            .unwrap();
            written += 1;
        }
        let first = median_ms(1, || {
            rt.block_on(crate::routes::tasks::listed_index(&state))
                .unwrap();
        });
        let create = median_ms(5, || {
            let response = rt.block_on(create_task(
                State(state.clone()),
                Json(serde_json::from_value(serde_json::json!({"title": "Read"})).unwrap()),
            ));
            assert_eq!(response.status(), 201);
        });
        let repeat = median_ms(11, || {
            rt.block_on(crate::routes::tasks::listed_index(&state))
                .unwrap();
        });
        let probe = median_ms(11, || {
            let _ = rt
                .block_on(crate::routes::notes::get_rev(
                    State(state.clone()),
                    Query(serde_json::from_value(serde_json::json!({})).unwrap()),
                ))
                .unwrap();
        });
        println!(
            "task files {written}: first list {first:.2} ms, new task {create:.2} ms, repeat list {repeat:.3} ms, rev probe {probe:.3} ms"
        );
    }
}
