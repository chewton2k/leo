use super::*;

#[test]
fn keeping_choices_are_saved_and_applied_right_away() {
    let (state, _d, _ids) = state_with(&[]);
    let old = chrono::Utc::now() - chrono::Duration::days(45);
    chats::save(
        &state.chats,
        "chat-old-0001",
        chats::Saving {
            title: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({"role": "user", "text": "hi"})],
        },
        old,
    )
    .unwrap();
    let page = run(get_keep(State(state.clone()))).unwrap().0;
    assert_eq!(page["trash_days"], 30);
    assert!(
        page["chat_days"].is_null(),
        "chats are kept forever unless asked"
    );
    assert_eq!(page["choices"].as_array().unwrap().len(), 5);

    let bad = run(set_keep(
        State(state.clone()),
        Json(leo_core::keep::Keep {
            trash_days: Some(3),
            ..Default::default()
        }),
    ));
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert_eq!(chats::list(&state.chats).len(), 1);

    let preview = |field: &str, days: Option<u32>| {
        let body = serde_json::from_value(serde_json::json!({ "field": field, "days": days }));
        run(crate::routes::housekeeping::preview_keep(
            State(state.clone()),
            Json(body.unwrap()),
        ))
    };
    assert_eq!(json_of(preview("chat_days", Some(30)))["count"], 1);
    assert_eq!(json_of(preview("chat_days", Some(90)))["count"], 0);
    assert_eq!(json_of(preview("chat_days", None))["count"], 0);
    assert_eq!(json_of(preview("upload_days", Some(7)))["count"], 0);
    assert_eq!(
        preview("chat_days", Some(3)).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        chats::list(&state.chats).len(),
        1,
        "a preview deletes nothing"
    );

    let set = run(set_keep(
        State(state.clone()),
        Json(leo_core::keep::Keep {
            trash_days: None,
            chat_days: Some(30),
            ..Default::default()
        }),
    ));
    assert_eq!(json_of(set)["chat_days"], 30);
    assert!(
        chats::list(&state.chats).is_empty(),
        "a 45-day-old chat goes at once"
    );
    let notes_dir = state.fresh().notes_dir.clone();
    assert_eq!(leo_core::keep::load(&notes_dir).trash_days, None);
}

#[test]
fn a_shorter_upload_period_deletes_old_uploads_at_once_and_forever_deletes_nothing() {
    let (state, _d, _ids) = state_with(&[]);
    let notes_dir = state.fresh().notes_dir.clone();
    let folder = crate::keeping::data_dir(&notes_dir)
        .join("attachments")
        .join("note-one");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("old.pdf");
    std::fs::write(&file, "x").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 86_400);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(old)
        .unwrap();

    let forever = run(set_keep(State(state.clone()), Json(Default::default())));
    assert_eq!(forever.status(), StatusCode::OK);
    assert!(file.exists());

    let shorter = run(set_keep(
        State(state.clone()),
        Json(leo_core::keep::Keep {
            upload_days: Some(30),
            ..Default::default()
        }),
    ));
    assert_eq!(json_of(shorter)["upload_days"], 30);
    assert!(!file.exists());
}

#[test]
fn the_daily_expiry_removes_an_old_chat_and_its_documents() {
    let (state, dir, _) = state_with(&[]);
    let old = chrono::Utc::now() - chrono::Duration::days(45);
    chats::save(
        &state.chats,
        "chat-old-audit1",
        chats::Saving {
            title: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({"role": "user", "text": "photosynthesis in leaves"})],
        },
        old,
    )
    .unwrap();
    chat_files::add(
        &state.chats,
        "chat-old-audit1",
        "notes.pdf",
        "chlorophyll",
        old,
    )
    .unwrap();
    chat_files::add(
        &state.chats,
        "chat-never-saved",
        "orphan.pdf",
        "an orphan",
        chrono::Utc::now() - chrono::Duration::days(2),
    )
    .unwrap();
    leo_core::keep::save(
        &dir.path().join("notes"),
        &leo_core::keep::Keep {
            trash_days: Some(30),
            chat_days: Some(30),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(chats::search(&state.chats, "photosynthesis").len(), 1);
    assert_eq!(chat_files::list(&state.chats, "chat-old-audit1").len(), 1);
    run(keeping::expire_now(&state));
    assert!(
        chats::load(&state.chats, "chat-old-audit1").is_none(),
        "the daily expiry left a 45-day-old chat with chat_days = 30"
    );
    assert_eq!(chats::search(&state.chats, "photosynthesis").len(), 0);
    assert_eq!(
        chat_files::list(&state.chats, "chat-old-audit1").len(),
        0,
        "its document was left"
    );
    assert_eq!(
        chat_files::list(&state.chats, "chat-never-saved").len(),
        0,
        "the orphan document was left"
    );
}
