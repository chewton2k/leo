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
