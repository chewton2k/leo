use super::*;

#[test]
fn a_failed_read_does_not_reload_the_store_and_a_failed_write_does() {
    let (state, _dir, _ids) = state_with(&[("One", "")]);
    let missed: Result<(), StatusCode> = run(state.read_store(|_| Err(StatusCode::NOT_FOUND)));
    assert_eq!(missed, Err(StatusCode::NOT_FOUND));
    assert!(!state.store.lock().unwrap().reload);
    let failed: Result<(), StatusCode> =
        run(state.with_store(|_| Err(StatusCode::INTERNAL_SERVER_ERROR)));
    assert_eq!(failed, Err(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(state.store.lock().unwrap().reload);
}

#[test]
fn a_panic_inside_the_store_reloads_it_from_disk_instead_of_failing_every_request() {
    let (state, _dir, ids) = state_with(&[("One", "")]);
    let id = ids[0].clone();
    let panicked: Result<(), StatusCode> = run(state.with_store(move |store| {
        store.find_note_mut(&id).unwrap().body = "Never saved".into();
        panic!("simulated failure inside the store");
    }));
    assert_eq!(panicked, Err(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(state.store.is_poisoned());
    let bodies = run(state.read_store(|store| {
        Ok(store
            .notes
            .iter()
            .map(|note| note.body.clone())
            .collect::<Vec<_>>())
    }))
    .unwrap();
    assert_eq!(bodies, vec![String::new()]);
    assert!(!state.store.is_poisoned());
    let id = ids[0].clone();
    let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), StatusCode> = store_now(&state, move |store| {
            store.find_note_mut(&id).unwrap().body = "Never saved".into();
            panic!("simulated failure inside the store");
        });
    }));
    assert!(again.is_err() && state.store.is_poisoned());
    let body = store_now(&state, |store| Ok(store.notes[0].body.clone())).unwrap();
    assert_eq!(body, "");
}

#[test]
fn saving_another_note_preserves_external_creates_and_edits_before_watch_delivery() {
    for creating in [false, true] {
        let (state, _dir, ids) = state_with(&[("Local", ""), ("External", "")]);
        let root = state.fresh().notes_dir.clone();
        state.store.lock().unwrap().changes = Some(crate::store_refresh::Changes::delayed());
        if creating {
            std::fs::write(root.join("Added.md"), "Outside edit").unwrap();
        } else {
            let path = root.join("External.md");
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.push_str("Outside edit");
            std::fs::write(path, text).unwrap();
        }
        let local = ids[0].clone();
        run(state.with_store(move |store| {
            store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
            store.save().unwrap();
            Ok(())
        }))
        .unwrap();
        let outside = if creating {
            root.join("Added.md")
        } else {
            root.join("External.md")
        };
        assert!(std::fs::read_to_string(&outside)
            .unwrap()
            .contains("Outside edit"));
        assert!(state.store.lock().unwrap().changed_on_disk());
        state.store.lock().unwrap().changes = None;
        let seen = run(state.read_store(|store| {
            Ok(store
                .notes
                .iter()
                .any(|note| note.body.contains("Outside edit")))
        }))
        .unwrap();
        assert!(
            seen,
            "external create={creating} was absorbed into an unread fingerprint"
        );
    }
}

/// An ID prefix shared by several notes must not delete all of them.
#[test]
fn deleting_by_an_ambiguous_prefix_deletes_nothing() {
    let (state, _d, _ids) = state_with(&[("A", ""), ("B", "")]);
    let prefix = {
        let mut store = state.fresh();
        let first = store.notes[0].id.clone();
        store.notes[1].id = format!("{first}x");
        store.touch_all();
        store.save().unwrap();
        first[..first.len() - 1].to_string()
    };
    let status = run(delete_note(State(state.clone()), Path(prefix)));
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(state.fresh().notes.len(), 2);
}

#[test]
fn creating_a_note_in_a_new_directory_registers_it() {
    let (state, _d, _ids) = state_with(&[]);
    let body = CreateBody {
        title: "Lecture".to_string(),
        body: None,
        directory: Some("cs162".to_string()),
        request: None,
    };
    run(create_note(State(state.clone()), Json(body)))
        .ok()
        .unwrap();
    assert!(state.fresh().dir_exists("cs162"));
}

#[test]
fn moving_to_a_missing_directory_is_refused() {
    let (state, _d, ids) = state_with(&[("A", "")]);
    let body = MoveBody {
        directory: "nowhere".to_string(),
    };
    let out = run(move_note(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(body),
    ));
    assert_eq!(out.err(), Some(StatusCode::NOT_FOUND));
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .find_note(&ids[0])
            .unwrap()
            .directory,
        ""
    );
}

#[test]
fn a_note_can_be_pinned_from_the_phone() {
    let (state, _d, ids) = state_with(&[("Syllabus", "")]);
    let body = UpdateBody {
        title: None,
        body: None,
        tags: None,
        pinned: Some(true),
        base: None,
        written: None,
    };
    let Json(note) = run(update_note(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(body),
    ))
    .unwrap();
    assert!(note.pinned);
    assert!(state.fresh().find_note(&ids[0]).unwrap().pinned);
}

#[test]
fn folders_come_with_how_many_notes_they_hold() {
    let (state, _d, _ids) = state_with(&[("A", "cs130"), ("B", "cs130/lec"), ("C", "")]);
    let Json(dirs) = run(list_folders(State(state))).unwrap();
    assert_eq!(dirs.len(), 2);
    assert_eq!(dirs[0].name, "cs130");
    assert_eq!(dirs[0].notes, 2);
}

#[test]
fn every_folder_is_listed_for_moving_a_note() {
    let (state, _d, _ids) = state_with(&[("A", "cs130"), ("B", "cs130/lec"), ("C", "Ideas")]);
    let Json(all) = run(list_folders(State(state))).unwrap();
    let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["cs130", "cs130/lec", "Ideas"]);
}

fn edit(
    state: &AppState,
    id: &str,
    body: &str,
    base: Option<String>,
) -> Result<NoteResponse, StatusCode> {
    run(update_note(
        State(state.clone()),
        Path(id.to_string()),
        Json(UpdateBody {
            title: None,
            body: Some(body.to_string()),
            tags: None,
            pinned: None,
            base,
            written: None,
        }),
    ))
    .map(|Json(n)| n)
}

#[test]
fn an_edit_based_on_the_current_version_is_saved_and_returns_the_next_version() {
    let (state, _d, ids) = state_with(&[("Shared", "")]);
    let current = run(get_note(State(state.clone()), Path(ids[0].clone())))
        .unwrap()
        .0
        .version;
    let saved = edit(&state, &ids[0], "from the phone", Some(current.clone())).unwrap();
    assert_eq!(saved.body, "from the phone");
    assert_ne!(saved.version, current);
}

#[test]
fn an_edit_based_on_an_old_version_is_refused_and_changes_nothing() {
    let (state, dir, ids) = state_with(&[("Shared", "")]);
    let seen = run(get_note(State(state.clone()), Path(ids[0].clone())))
        .unwrap()
        .0;
    let file = dir.path().join("notes").join("Shared.md");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("{text}typed in Obsidian\n")).unwrap();

    let refused = edit(&state, &ids[0], "from the phone", Some(seen.version));
    assert_eq!(refused.unwrap_err(), StatusCode::CONFLICT);
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("typed in Obsidian"));
}
#[test]
fn a_new_folder_answers_with_json_so_the_page_can_read_it() {
    let (state, _dir, _) = state_with(&[]);
    let (status, Json(body)) = run(create_dir(
        State(state.clone()),
        Json(CreateDirBody {
            path: "cs130/week1".into(),
        }),
    ))
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, serde_json::json!({ "path": "cs130/week1" }));
    assert_eq!(
        run(create_dir(
            State(state),
            Json(CreateDirBody {
                path: "cs130/week1".into(),
            }),
        ))
        .err(),
        Some(StatusCode::CONFLICT)
    );
}

#[test]
fn web_note_creation_directory_creation_and_moves_reject_traversal() {
    let (state, dir, ids) = state_with(&[("A", "")]);
    for path in [
        "../outside",
        "/tmp/outside",
        "nested/../../outside",
        "C:\\outside",
        ".trash",
    ] {
        let created = run(create_note(
            State(state.clone()),
            Json(CreateBody {
                title: "Escape".into(),
                body: None,
                directory: Some(path.into()),
                request: None,
            }),
        ));
        assert_eq!(created.err(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(
            run(create_dir(
                State(state.clone()),
                Json(CreateDirBody { path: path.into() })
            ))
            .err(),
            Some(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            run(move_note(
                State(state.clone()),
                Path(ids[0].clone()),
                Json(MoveBody {
                    directory: path.into()
                })
            ))
            .err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }
    assert!(!dir.path().join("outside").exists());
    assert_eq!(state.fresh().notes.len(), 1);
}

#[test]
fn a_reload_failure_is_reported_instead_of_serving_a_stale_store() {
    let (state, dir, _) = state_with(&[("A", "")]);
    let folders = dir.path().join("notes").join("directories.json");
    let _ = std::fs::remove_file(&folders);
    std::fs::create_dir(&folders).unwrap();
    let result = run(list_notes(
        State(state),
        Query(ListParams {
            limit: None,
            dir: None,
            offset: 0,
            brief: false,
            sort: None,
            show: None,
        }),
    ));
    assert_eq!(result.err(), Some(StatusCode::INTERNAL_SERVER_ERROR));
}

#[test]
fn an_empty_folder_created_by_another_app_is_visible_without_restarting() {
    let (state, dir, _) = state_with(&[]);
    let mut other = Store::load_from(&dir.path().join("notes")).unwrap();
    other.create_dir("new-folder");
    other.save().unwrap();
    let Json(dirs) = run(list_folders(State(state))).unwrap();
    assert_eq!(dirs[0].name, "new-folder");
}
#[test]
fn failed_operations_do_not_leave_unsaved_changes_in_the_cached_store() {
    let (state, _dir, ids) = state_with(&[("Original", "")]);
    let id = ids[0].clone();
    let failed: Result<(), StatusCode> = run(state.with_store(move |store| {
        store.find_note_mut(&id).unwrap().body = "Never saved".into();
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }));
    assert!(failed.is_err());
    let Json(note) = run(get_note(State(state), Path(ids[0].clone()))).unwrap();
    assert_eq!(note.body, "");
}

#[test]
fn a_large_folder_comes_in_pages_with_its_total_and_short_cards() {
    let names: Vec<String> = (0..250).map(|i| format!("Note {i:03}")).collect();
    let notes: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "big")).collect();
    let (state, _d, ids) = state_with(&notes);
    {
        let mut store = state.fresh();
        let body = format!("- [x] read\n- [ ] write\n{}", "long text ".repeat(200));
        store.find_note_mut(&ids[0]).unwrap().body = body;
        store.save().unwrap();
    }
    let page = |offset: usize, limit: usize| {
        let response = run(list_notes(
            State(state.clone()),
            Query(ListParams {
                limit: Some(limit),
                dir: Some("big".into()),
                offset,
                brief: true,
                sort: None,
                show: None,
            }),
        ))
        .unwrap();
        let total: usize = response.headers()["x-total"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        (total, json_of(response))
    };
    let (total, first) = page(0, 200);
    assert_eq!(total, 250);
    assert_eq!(first.as_array().unwrap().len(), 200);
    let (_, rest) = page(200, 200);
    assert_eq!(rest.as_array().unwrap().len(), 50);
    let mut seen: Vec<String> = first
        .as_array()
        .unwrap()
        .iter()
        .chain(rest.as_array().unwrap())
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        250,
        "no note is skipped or repeated across pages"
    );
    let long = first
        .as_array()
        .unwrap()
        .iter()
        .chain(rest.as_array().unwrap())
        .find(|n| n["id"] == ids[0].as_str())
        .unwrap();
    assert_eq!(long["tasks"], serde_json::json!([1, 2]));
    assert_eq!(
        long["body"].as_str().unwrap().chars().count(),
        EXCERPT_CHARS
    );
}

#[test]
fn an_excerpt_starts_near_the_first_match_and_counts_only_real_tasks() {
    let body = format!("{}needle in the text", "é".repeat(500));
    let cut = excerpt(&body, &["needle".into()]);
    assert!(
        cut.starts_with('…') && cut.contains("needle in the text"),
        "{cut}"
    );
    assert_eq!(excerpt("short", &["absent".into()]), "short");
    let words = format!(
        "{} the queue holds work",
        "breadth first search ".repeat(10)
    );
    let cut = excerpt(&words, &["queue".into()]);
    assert!(
        ["…breadth ", "…first ", "…search "]
            .iter()
            .any(|w| cut.starts_with(w)),
        "{cut}"
    );
    assert_eq!(
        excerpt("İstanbul needle", &["needle".into()]),
        "İstanbul needle"
    );
    assert_eq!(
        tasks_in("- [x] a\n  - [X] b\n- [ ] c\n- [y] no\n-[ ] no\n* [ ] other bullet"),
        [2, 3]
    );
}

#[test]
fn a_folder_moves_into_another_with_its_notes_and_leaves_no_empty_folder_behind() {
    let (state, dir, ids) = state_with(&[("Lecture", "cs130/week1"), ("Other", "cs162")]);
    let moved = |from: &str, into: &str| {
        run(move_dir(
            State(state.clone()),
            Json(MoveDirBody {
                from: from.into(),
                into: into.into(),
            }),
        ))
    };
    let out = moved("cs130/week1", "cs162").unwrap();
    assert_eq!(out.0["path"], "cs162/week1");
    let store = state.fresh();
    assert_eq!(store.find_note(&ids[0]).unwrap().directory, "cs162/week1");
    assert!(!store.dir_exists("cs130/week1"));
    assert!(dir.path().join("notes/cs162/week1/Lecture.md").exists());
    assert!(!dir.path().join("notes/cs130/week1").exists());
    drop(store);

    assert_eq!(
        moved("cs162", "cs162/week1").err(),
        Some(StatusCode::CONFLICT)
    );
    assert_eq!(moved("nowhere", "cs162").err(), Some(StatusCode::NOT_FOUND));
    assert_eq!(moved("../outside", "").err(), Some(StatusCode::BAD_REQUEST));
    assert_eq!(
        moved("cs162", "../outside").err(),
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(moved("cs162/week1", "").unwrap().0["path"], "week1");
}

#[test]
fn folders_can_be_renamed_in_place_without_changing_note_identity() {
    let (state, dir, ids) = state_with(&[("Lecture", "cs130/week1"), ("Other", "cs130/week10")]);
    let rename = |from: &str, name: &str| {
        run(crate::routes::notes::rename_dir(
            State(state.clone()),
            Json(crate::routes::notes::RenameDirBody {
                from: from.into(),
                name: name.into(),
            }),
        ))
    };
    let out = rename("cs130/week1", "Week one").unwrap();
    assert_eq!(out.0["path"], "cs130/Week one");
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().directory,
        "cs130/Week one"
    );
    assert_eq!(store.find_note(&ids[1]).unwrap().directory, "cs130/week10");
    assert!(dir.path().join("notes/cs130/Week one/Lecture.md").exists());
    assert!(!dir.path().join("notes/cs130/week1").exists());
    drop(store);
    assert_eq!(
        rename("cs130/Week one", "week10").err(),
        Some(StatusCode::CONFLICT)
    );
    for bad in ["../outside", "a/b", "a\\b", "", ".."] {
        assert_eq!(
            rename("cs130/Week one", bad).err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }
    assert_eq!(rename("", "root").err(), Some(StatusCode::BAD_REQUEST));
    assert_eq!(rename("missing", "New").err(), Some(StatusCode::NOT_FOUND));
}

#[test]
fn library_sorting_happens_before_pagination_and_brief_results_include_visuals() {
    let (state, _dir, ids) = state_with(&[("Zebra", "B"), ("Alpha", "A"), ("Pinned", "")]);
    {
        let mut store = state.store.lock().unwrap();
        store.find_note_mut(&ids[2]).unwrap().pinned = true;
        store.find_note_mut(&ids[1]).unwrap().body =
            "# Lecture\nA useful description of this lecture.\n```mermaid\ngraph LR\n A --> B\n```"
                .into();
        store.save().unwrap();
    }
    let page = |sort: &str, offset: usize| {
        run(list_notes(State(state.clone()), Query(serde_json::from_value(serde_json::json!({ "sort": sort, "offset": offset, "limit": 1, "brief": true })).unwrap())))
    };
    for sort in ["title", "updated"] {
        let out = page(sort, 0).unwrap();
        assert_eq!(out.headers()["x-total"], "3");
        assert_eq!(json_of(out)[0]["id"], ids[2]);
    }
    let alpha = json_of(page("title", 1).unwrap());
    assert_eq!(alpha[0]["id"], ids[1]);
    assert_eq!(alpha[0]["summary"], "A useful description of this lecture.");
    assert_eq!(alpha[0]["preview"], "```mermaid\ngraph LR\n A --> B\n```");
    for unknown in ["unknown", "opened", "folder"] {
        assert_eq!(page(unknown, 0).err(), Some(StatusCode::BAD_REQUEST));
    }
}

#[test]
fn routine_client_errors_do_not_reload_every_note() {
    let (state, _dir, _ids) = state_with(&[("One", "")]);
    for code in [
        StatusCode::NOT_FOUND,
        StatusCode::CONFLICT,
        StatusCode::BAD_REQUEST,
    ] {
        let failed: Result<(), StatusCode> = run(state.with_store(move |_| Err(code)));
        assert_eq!(failed, Err(code));
        assert!(!state.store.lock().unwrap().reload, "{code}");
        let now: Result<(), StatusCode> = store_now(&state, |_| Err(code));
        assert_eq!(now, Err(code));
        assert!(!state.store.lock().unwrap().reload, "{code}");
    }
}

#[test]
fn a_new_folder_that_windows_cannot_hold_is_refused() {
    let (state, _dir, _ids) = state_with(&[]);
    for bad in ["Q&A?", "Week 1.", "CON", "Notes/CON", "Notes/What?"] {
        let made = run(create_note(
            State(state.clone()),
            Json(CreateBody {
                title: "Idea".into(),
                body: None,
                directory: Some(bad.into()),
                request: None,
            }),
        ));
        assert_eq!(made.err(), Some(StatusCode::BAD_REQUEST), "{bad}");
        assert!(!state.fresh().dir_exists(bad), "{bad}");
    }
    assert!(state.fresh().notes.is_empty());
    let fine = run(create_note(
        State(state.clone()),
        Json(CreateBody {
            title: "Idea".into(),
            body: None,
            directory: Some("Week 1".into()),
            request: None,
        }),
    ));
    assert!(fine.is_ok());
}

#[test]
fn a_stale_save_whose_content_already_reached_leo_is_not_a_conflict() {
    let (state, _dir, ids) = state_with(&[("Draft", "")]);
    let id = ids[0].clone();
    let send = |value: serde_json::Value| {
        run(update_note(
            State(state.clone()),
            Path(id.clone()),
            Json(serde_json::from_value::<UpdateBody>(value).unwrap()),
        ))
    };
    let first = send(serde_json::json!({ "body": "typed words" })).unwrap();
    let before = first.0.updated_at.clone();
    let again = send(serde_json::json!({ "body": "typed words", "base": "stale" })).unwrap();
    assert_eq!(again.0.body, "typed words");
    assert_eq!(again.0.version, first.0.version);
    assert_eq!(again.0.updated_at, before);
    let differing = send(serde_json::json!({ "body": "other words", "base": "stale" }));
    assert_eq!(differing.err(), Some(StatusCode::CONFLICT));
    assert_eq!(state.fresh().notes.len(), 1);
}

#[test]
fn the_unused_checkbox_toggle_route_is_gone() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let id = store
            .create_note("Boxes", "- [ ] one", vec![], "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let running = start_in(
            Places {
                store,
                sessions: dir.path().join("sessions.json"),
            },
            StartOptions {
                port: 0,
                token: "0123456789abcdef0123456789abcdef".into(),
            },
            Powers::default(),
        )
        .await
        .unwrap();
        let port = running.port();
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let request = format!(
            "POST /api/notes/{id}/toggle?checkbox=1&token=0123456789abcdef0123456789abcdef HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
            .await
            .unwrap();
        let mut reply = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut reply)
            .await
            .unwrap();
        assert!(reply.starts_with("HTTP/1.1 404"), "{reply}");
        running.stop().await;
    });
}

#[test]
fn folder_note_counts_include_everything_below_and_ignore_lookalike_names() {
    let (state, _dir, _ids) = state_with(&[
        ("A", "cs130"),
        ("B", "cs130/week1"),
        ("C", "cs130/week1"),
        ("D", "cs13"),
        ("E", ""),
    ]);
    let Json(all) = run(list_folders(State(state.clone()))).unwrap();
    let store = state.fresh();
    assert!(!all.is_empty());
    for dir in &all {
        let inside = store
            .notes
            .iter()
            .filter(|n| {
                n.directory == dir.name || n.directory.starts_with(&format!("{}/", dir.name))
            })
            .count();
        assert_eq!(dir.notes, inside, "{}", dir.name);
    }
    let count = |name: &str| all.iter().find(|d| d.name == name).unwrap().notes;
    assert_eq!(
        (count("cs130"), count("cs130/week1"), count("cs13")),
        (3, 2, 1)
    );
}

#[test]
fn renaming_or_moving_a_folder_takes_its_pictures_and_other_files_along() {
    let (state, _dir, ids) = state_with(&[("Cells", "Bio"), ("Other", "Misc")]);
    let root = state.fresh().notes_dir.clone();
    std::fs::write(root.join("Bio/cell.png"), b"png").unwrap();
    std::fs::create_dir_all(root.join("Bio/img")).unwrap();
    std::fs::write(root.join("Bio/img/deep.png"), b"deep").unwrap();
    let renamed = run(crate::routes::notes::rename_dir(
        State(state.clone()),
        Json(crate::routes::notes::RenameDirBody {
            from: "Bio".into(),
            name: "Biology".into(),
        }),
    ))
    .unwrap();
    assert_eq!(renamed.0["path"], "Biology");
    assert_eq!(
        std::fs::read(root.join("Biology/cell.png")).unwrap(),
        b"png"
    );
    assert_eq!(
        std::fs::read(root.join("Biology/img/deep.png")).unwrap(),
        b"deep"
    );
    assert!(!root.join("Bio").exists());
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "Biology"
    );

    let _ = run(move_dir(
        State(state.clone()),
        Json(MoveDirBody {
            from: "Biology".into(),
            into: "Misc".into(),
        }),
    ))
    .unwrap();
    assert_eq!(
        std::fs::read(root.join("Misc/Biology/cell.png")).unwrap(),
        b"png"
    );
    assert!(!root.join("Biology").exists());
}

fn organize_call(name: &str, args: serde_json::Value) -> crate::tools::Call {
    crate::tools::Call {
        name: name.into(),
        args,
    }
}

#[test]
fn a_tool_name_organize_does_not_know_is_an_error_not_a_deleted_note() {
    let (state, _dir, _ids) = state_with(&[("Lecture 3", "")]);
    run(async {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let mut desk = crate::tools::Desk::new(vec![], 8000);
            let done = crate::routes::organize::run(
                &state,
                &mut desk,
                crate::tools::Access::Auto,
                &organize_call("archive_note", serde_json::json!({ "note": "Lecture 3" })),
            );
            assert!(done.proposal.is_none(), "{}", done.result);
            assert!(done.result.contains("archive_note"), "{}", done.result);
            assert!(!done.step.contains("delete"), "{}", done.step);
            let asked = crate::routes::organize::run(
                &state,
                &mut desk,
                crate::tools::Access::Auto,
                &organize_call("delete_note", serde_json::json!({ "note": "Lecture 3" })),
            );
            assert!(matches!(
                asked.proposal,
                Some(crate::tools::Proposal::DeleteNote { .. })
            ));
        })
        .await
        .unwrap();
    });
}

#[test]
fn moving_a_task_in_felix_keeps_its_end_time_and_the_undo_does_too() {
    let (state, _dir, _ids) = state_with(&[]);
    let made = run(crate::routes::tasks::create_task(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({
                "title": "Lab",
                "date": "2026-11-03",
                "time": "14:00",
                "end": "15:00"
            }))
            .unwrap(),
        ),
    ));
    let made = json_of(made);
    let id = made["id"].as_str().unwrap().to_string();
    run(async {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let moved = crate::tools::Proposal::UpdateTask {
                summary: "Move the lab".into(),
                id: id.clone(),
                description: None,
                done: None,
                date: Some("2026-11-05".into()),
                clear_date: false,
                time: None,
                task_kind: None,
                on: None,
            };
            let shown = crate::routes::organize::act_now(&state, &moved).unwrap();
            let stored = crate::tasks::load(&state.chats, &id).unwrap();
            assert_eq!(stored.date, chrono::NaiveDate::from_ymd_opt(2026, 11, 5));
            assert_eq!(stored.end, chrono::NaiveTime::from_hms_opt(15, 0, 0));
            let undo: crate::tools::Proposal =
                serde_json::from_value(shown["undo"].clone()).unwrap();
            crate::routes::organize::act_now(&state, &undo).unwrap();
            let back = crate::tasks::load(&state.chats, &id).unwrap();
            assert_eq!(back.date, chrono::NaiveDate::from_ymd_opt(2026, 11, 3));
            assert_eq!(back.end, chrono::NaiveTime::from_hms_opt(15, 0, 0));
        })
        .await
        .unwrap();
    });
}

fn searched(state: &AppState, params: serde_json::Value) -> (usize, Vec<serde_json::Value>) {
    let response = run(crate::routes::notes::search_notes(
        State(state.clone()),
        Query(serde_json::from_value(params).unwrap()),
    ))
    .unwrap();
    let total = response.headers()["x-total"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let found = json_of(response).as_array().unwrap().clone();
    (total, found)
}

fn ids_of(found: &[serde_json::Value]) -> Vec<String> {
    found
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_very_long_search_uses_every_word_and_does_not_hold_the_notes_lock() {
    let filler: Vec<String> = (0..3000).map(|i| format!("filler{i}")).collect();
    let body = filler.join(" ");
    let mut notes: Vec<(String, &str)> = (0..40).map(|i| (format!("Note {i}"), "")).collect();
    notes.push(("zebra".into(), ""));
    let shaped: Vec<(&str, &str)> = notes.iter().map(|(t, d)| (t.as_str(), *d)).collect();
    let (state, _dir, ids) = state_with(&shaped);
    {
        let mut store = state.fresh();
        for id in &ids {
            store.find_note_mut(id).unwrap().body = body.clone();
        }
        store.save().unwrap();
    }
    let query = filler
        .iter()
        .cycle()
        .take(9499)
        .cloned()
        .chain(["zebra".to_string()])
        .collect::<Vec<_>>()
        .join(" ");
    state.graphs.load();
    searched(&state, serde_json::json!({ "q": "warm" }));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let searching = {
        let state = state.clone();
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let found = searched(&state, serde_json::json!({ "q": query, "brief": true }));
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            found
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(20));
    let started = std::time::Instant::now();
    run(state.read_store(|s| Ok(s.notes.len()))).unwrap();
    let waited = started.elapsed();
    let still_searching = !done.load(std::sync::atomic::Ordering::SeqCst);
    let (total, found) = searching.join().unwrap();
    assert!(
        still_searching,
        "the search ended before the read: nothing was measured"
    );
    assert!(
        waited < std::time::Duration::from_millis(200),
        "a read waited {waited:?} behind a long search"
    );
    assert_eq!(total, 1);
    assert_eq!(found[0]["title"], "zebra");
}

#[test]
fn search_results_are_read_a_page_at_a_time() {
    let titles: Vec<String> = (0..350).map(|i| format!("Match {i}")).collect();
    let notes: Vec<(&str, &str)> = titles.iter().map(|t| (t.as_str(), "")).collect();
    let (state, _dir, _) = state_with(&notes);
    let (total, first) = searched(&state, serde_json::json!({ "q": "match", "limit": 61 }));
    assert_eq!((total, first.len()), (350, 61));
    let (total, late) = searched(
        &state,
        serde_json::json!({ "q": "match", "offset": 300, "limit": 61 }),
    );
    assert_eq!((total, late.len()), (350, 50));
    let (total, all) = searched(&state, serde_json::json!({ "q": "match", "limit": 1000 }));
    assert_eq!((total, all.len()), (350, 350));
    let (total, plain) = searched(&state, serde_json::json!({ "q": "match" }));
    assert_eq!((total, plain.len()), (350, 300));
    let mut joined = Vec::new();
    for offset in (0..350).step_by(61) {
        let (_, page) = searched(
            &state,
            serde_json::json!({ "q": "match", "offset": offset, "limit": 61 }),
        );
        joined.extend(ids_of(&page));
    }
    assert_eq!(joined, ids_of(&all));
}

#[test]
fn sixty_notes_with_one_title_are_all_reachable_and_none_repeats() {
    let folders: Vec<String> = (0..60).map(|i| format!("F{i:02}")).collect();
    let notes: Vec<(&str, &str)> = folders.iter().map(|f| ("Same", f.as_str())).collect();
    let (state, _dir, ids) = state_with(&notes);
    {
        let mut store = state.fresh();
        let at = chrono::Utc::now();
        for id in &ids {
            let note = store.find_note_mut(id).unwrap();
            note.body = "the same text".into();
            note.updated_at = at;
        }
        store.save().unwrap();
    }
    let pages = |state: &AppState| {
        let (_, first) = searched(state, serde_json::json!({ "q": "same", "limit": 50 }));
        let (_, second) = searched(
            state,
            serde_json::json!({ "q": "same", "offset": 50, "limit": 50 }),
        );
        (first, second)
    };
    let (first, second) = pages(&state);
    assert_eq!((first.len(), second.len()), (50, 10));
    let order: Vec<String> = ids_of(&first).into_iter().chain(ids_of(&second)).collect();
    let distinct: std::collections::HashSet<&String> = order.iter().collect();
    assert_eq!(distinct.len(), 60);
    let places: std::collections::HashSet<&str> = first
        .iter()
        .chain(&second)
        .map(|n| n["directory"].as_str().unwrap())
        .collect();
    assert_eq!(places.len(), 60);
    state.store.lock().unwrap().reload = true;
    *state.searched.lock().unwrap() = None;
    let (first, second) = pages(&state);
    let again: Vec<String> = ids_of(&first).into_iter().chain(ids_of(&second)).collect();
    assert_eq!(again, order);
}

#[test]
fn a_later_page_does_not_search_again() {
    let (mut state, _dir, ids) = state_with(&[("Queues one", ""), ("Queues two", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "Take the oldest item first.".into();
        store.find_note_mut(&ids[1]).unwrap().body = "A line of people.".into();
        store.save().unwrap();
    }
    let inner = crate::vectors::fake_meaning();
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let meaning: crate::vectors::Meaning = {
        let asked = Arc::clone(&asked);
        Arc::new(move |texts: &[String], query: bool| {
            if query {
                asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            inner(texts, query)
        })
    };
    crate::vectors::catch_up(&state.fresh(), &state.vectors, &meaning, 100);
    state.meaning = Some(meaning);
    let (total, first) = searched(&state, serde_json::json!({ "q": "queues", "limit": 1 }));
    let (_, second) = searched(
        &state,
        serde_json::json!({ "q": "queues", "offset": 1, "limit": 1 }),
    );
    assert_eq!(total, 2);
    assert_ne!(ids_of(&first), ids_of(&second));
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[1]).unwrap().body = "A line of people, waiting.".into();
        store.save().unwrap();
    }
    searched(
        &state,
        serde_json::json!({ "q": "queues", "offset": 1, "limit": 1 }),
    );
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn a_changed_library_drops_the_kept_ranking() {
    let (state, _dir, ids) = state_with(&[("Match one", ""), ("Match two", "")]);
    let (total, first) = searched(&state, serde_json::json!({ "q": "match", "limit": 1 }));
    assert_eq!((total, first.len()), (2, 1));
    let leaving = ids
        .iter()
        .find(|id| **id != ids_of(&first)[0])
        .unwrap()
        .clone();
    {
        let mut store = state.fresh();
        store.find_note_mut(&leaving).unwrap().title = "Other".into();
        store.save().unwrap();
    }
    let (total, next) = searched(
        &state,
        serde_json::json!({ "q": "match", "offset": 1, "limit": 1 }),
    );
    assert_eq!(total, 1);
    assert!(next.is_empty(), "{next:?}");
}

#[test]
fn a_retried_create_makes_one_note_and_one_board() {
    let (state, _dir, _) = state_with(&[]);
    let (note_request, board_request) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let mut notes_made = Vec::new();
    for _ in 0..2 {
        let made = run(create_note(
            State(state.clone()),
            Json(
                serde_json::from_value(serde_json::json!({
                    "title": "Same",
                    "body": "typed",
                    "request": note_request.to_string()
                }))
                .unwrap(),
            ),
        ))
        .unwrap();
        notes_made.push(axum::response::IntoResponse::into_response(made).status());
    }
    assert_eq!(notes_made, [StatusCode::CREATED, StatusCode::OK]);
    let mut boards_made = Vec::new();
    for _ in 0..2 {
        let made = run(crate::routes::boards::create_board(
            State(state.clone()),
            Json(
                serde_json::from_value(serde_json::json!({
                    "name": "Plan",
                    "request": board_request.to_string()
                }))
                .unwrap(),
            ),
        ));
        boards_made.push(made.status());
    }
    assert_eq!(boards_made, [StatusCode::CREATED, StatusCode::OK]);
    let store = state.fresh();
    let files = std::fs::read_dir(&store.notes_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
        .count();
    let notes = store.notes.iter().filter(|n| n.title == "Same").count();
    drop(store);
    let boards = crate::boards::boards(&state.chats)
        .iter()
        .filter(|b| b.name == "Plan")
        .count();
    assert_eq!((notes, files, boards), (1, 1, 1));
}

const BENCH_WORDS: &[&str] = &[
    "the", "graph", "search", "queue", "stack", "heap", "tree", "node", "edge", "visit", "order",
    "breadth", "depth", "first", "lecture", "exam", "proof", "lemma", "theorem", "plant", "light",
    "sugar", "cell", "energy", "market", "price", "supply", "demand", "history", "empire", "river",
    "trade", "language", "grammar", "verb", "noun", "memory", "cache", "process", "thread", "lock",
    "network", "packet", "route", "signal", "matrix", "vector", "integral", "limit", "series",
    "function", "variable", "loop", "array", "list", "hash", "table", "sort", "merge", "split",
];

fn bench_body(seed: usize) -> String {
    let mut at = (seed as u64).wrapping_mul(6_364_136_223_846_793_005) ^ 0x9e37_79b9;
    let mut body = String::new();
    while body.len() < 5000 {
        at = at
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        body.push_str(BENCH_WORDS[(at >> 33) as usize % BENCH_WORDS.len()]);
        body.push(if body.len() % 480 < 8 { '\n' } else { ' ' });
    }
    body
}

fn bench_state(n: usize) -> (AppState, tempfile::TempDir, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let notes_dir = dir.path().join("notes");
    let mut store = Store::load_from(&notes_dir).unwrap();
    for i in 0..n {
        store
            .create_note(
                format!("Lecture {i} {}", BENCH_WORDS[i % BENCH_WORDS.len()]),
                bench_body(i),
                vec![],
                &format!("Course {}", i % 20),
            )
            .unwrap();
    }
    store.save().unwrap();
    let store = Store::load_from(&notes_dir).unwrap();
    let (mut state, other, _) = state_with(&[]);
    state.graphs = Arc::new(graph::Graphs::for_notes(&notes_dir, None));
    state.notes_dir = notes_dir;
    state.store = Arc::new(Mutex::new(Storage {
        changes: Some(store_refresh::Changes::new(&state.notes_dir)),
        store,
        reload: false,
    }));
    while state.store.lock().unwrap().changes.as_mut().unwrap().due() {}
    (state, dir, other)
}

fn bench_ms(started: std::time::Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "benchmark"]
fn search_and_review_marks_benchmark() {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let n: usize = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let built = std::time::Instant::now();
    let (state, _dir, _other) = bench_state(n);
    println!("P-C06 notes={n} library_built_ms={:.0}", bench_ms(built));
    let search = |q: &str, extra: serde_json::Value| {
        let mut params = serde_json::json!({ "q": q, "brief": true });
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let started = std::time::Instant::now();
        let found = rt.block_on(crate::routes::notes::search_notes(
            State(state.clone()),
            Query(serde_json::from_value(params).unwrap()),
        ));
        assert!(found.is_ok());
        bench_ms(started)
    };
    let mut one: Vec<f64> = (0..5)
        .map(|_| {
            *state.searched.lock().unwrap() = None;
            search("queue", serde_json::json!({}))
        })
        .collect();
    one.sort_by(f64::total_cmp);
    println!(
        "P-C06 notes={n} search_1_word_ms={:.1} profile={profile}",
        one[2]
    );
    let long = BENCH_WORDS
        .iter()
        .cycle()
        .take(9500)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let long_ms = search(&long, serde_json::json!({}));
    let find_ms = {
        let store = state.store.lock().unwrap();
        let started = std::time::Instant::now();
        std::hint::black_box(store.find(&long).len());
        let long_find = bench_ms(started);
        let started = std::time::Instant::now();
        std::hint::black_box(store.find("queue").len());
        (long_find, bench_ms(started))
    };
    println!(
        "P-C06 notes={n} store_find_9500_words_ms={:.1} store_find_1_word_ms={:.1} profile={profile}",
        find_ms.0, find_ms.1
    );
    println!("P-C06 notes={n} search_9500_words_ms={long_ms:.1} profile={profile}");
    *state.searched.lock().unwrap() = None;
    let waiting = {
        let other = state.clone();
        let long = long.clone();
        let searching = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(crate::routes::notes::search_notes(
                    State(other),
                    Query(serde_json::from_value(serde_json::json!({ "q": long })).unwrap()),
                ))
                .is_ok()
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut longest = 0f64;
        while !searching.is_finished() {
            let started = std::time::Instant::now();
            rt.block_on(state.read_store(|s| Ok(s.notes.len())))
                .unwrap();
            longest = longest.max(bench_ms(started));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(searching.join().unwrap());
        longest
    };
    search("queue", serde_json::json!({ "limit": 61 }));
    let later = search("queue", serde_json::json!({ "offset": 61, "limit": 61 }));
    println!(
        "P-C06 notes={n} longest_read_behind_long_search_ms={waiting:.1} second_page_ms={later:.3} profile={profile}"
    );
    let chat = "chat-00000001";
    crate::chats::save(
        &state.chats,
        chat,
        crate::chats::Saving {
            existed: false,
            from: None,
            title: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({ "role": "user", "text": "quiz me" })],
        },
        chrono::Utc::now(),
    )
    .unwrap();
    for k in [10_000usize, 40_000] {
        crate::review::reset(&state.chats);
        let keys: Vec<String> = (0..k).map(|i| format!("{chat}:{i:016x}")).collect();
        let started = std::time::Instant::now();
        let added = crate::review::mark_reviewed(&state.chats, &keys).unwrap();
        println!(
            "P-C06 mark_reviewed keys={k} added={added} ms={:.1} profile={profile}",
            bench_ms(started)
        );
    }
}

fn median_ms<R>(runs: usize, mut f: impl FnMut() -> R) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let started = std::time::Instant::now();
            std::hint::black_box(f());
            bench_ms(started)
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

#[test]
#[ignore = "benchmark"]
fn audit_p14_title_and_matching_work() {
    let n: usize = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let (state, _dir, _other) = bench_state(n);
    let store = state.store.lock().unwrap();
    let store = &store.store;
    let hit = store.notes[n / 2].title.clone();
    let titles = crate::titles::Titles::default();
    let lookup = |title: &str| titles.exact(store, title).map(|x| x.id.len());
    lookup(&hit);
    let lookup_miss = median_ms(51, || lookup("no such note title"));
    let lookup_hit = median_ms(51, || lookup(&hit));
    let sort = median_ms(11, || {
        let mut notes: Vec<&leo_core::notes::Note> = store.notes.iter().collect();
        crate::browse::order(&mut notes, Some("title")).unwrap();
        notes.len()
    });
    let page: Vec<&leo_core::notes::Note> = store.notes.iter().take(1000).collect();
    let excerpts = median_ms(101, || {
        page.iter()
            .map(|note| excerpt(&note.body, &[]).len())
            .sum::<usize>()
    });
    println!(
        "P-C12 notes={n} title_lookup_miss_ms={lookup_miss:.4} title_lookup_hit_ms={lookup_hit:.4} title_sort_ms={sort:.2} excerpt_1000_no_words_ms={excerpts:.3}"
    );
}

fn held_back(state: &AppState) {
    state.store.lock().unwrap().changes = Some(crate::store_refresh::Changes::delayed());
}

fn loaded(state: &AppState, text: &'static str) -> bool {
    run(state.read_store(move |store| Ok(store.notes.iter().any(|note| note.body.contains(text)))))
        .unwrap()
}

#[test]
fn a_save_does_not_walk_while_the_watcher_is_quiet() {
    let walks = |watcher: Option<bool>, forced: bool| {
        let (state, _dir, ids) = state_with(&[("Local", ""), ("External", "")]);
        let root = state.fresh().notes_dir.clone();
        match watcher {
            Some(_) => held_back(&state),
            None => state.store.lock().unwrap().changes = None,
        }
        let path = root.join("External.md");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("Outside edit");
        std::fs::write(&path, text).unwrap();
        if forced {
            state
                .refresh_requested
                .store(true, std::sync::atomic::Ordering::Release);
        }
        let local = ids[0].clone();
        let read_first = run(state.with_store(move |store| {
            let read = store.notes.iter().any(|n| n.body.contains("Outside edit"));
            store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
            store.save().unwrap();
            Ok(read)
        }))
        .unwrap();
        usize::from(read_first)
    };
    let quiet = if cfg!(windows) { 1 } else { 0 };
    assert_eq!(walks(Some(true), false), quiet);
    assert_eq!(walks(None, false), 1);
    assert_eq!(walks(Some(true), true), 1);
}

#[test]
fn an_outside_edit_to_the_saved_note_just_before_the_save_becomes_a_conflict_copy() {
    let (state, _dir, ids) = state_with(&[("Local", "")]);
    let root = state.fresh().notes_dir.clone();
    held_back(&state);
    let path = root.join("Local.md");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\nOutside edit");
    std::fs::write(&path, &text).unwrap();
    let local = ids[0].clone();
    run(state.with_store(move |store| {
        store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
        store.save().unwrap();
        Ok(())
    }))
    .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    let copy = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("(conflict from leo)")
        })
        .expect("a conflict copy");
    assert!(std::fs::read_to_string(copy.path())
        .unwrap()
        .contains("Saved in Leo"));
    state.store.lock().unwrap().changes = None;
    assert!(loaded(&state, "Outside edit"));
    assert!(loaded(&state, "Saved in Leo"));
}

fn folders_on_disk(root: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("directories.json")).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

fn folder_outside(root: &std::path::Path, change: impl FnOnce(&mut Vec<String>)) {
    let mut list = folders_on_disk(root);
    change(&mut list);
    std::fs::write(
        root.join("directories.json"),
        serde_json::to_string_pretty(&list).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_folder_added_outside_just_before_a_save_is_kept() {
    for leo_first in [false, true] {
        let (state, _dir, ids) = state_with(&[("Local", "")]);
        let root = state.fresh().notes_dir.clone();
        held_back(&state);
        let local = ids[0].clone();
        let outside_root = root.clone();
        let add_outside = move || {
            let root = &outside_root;
            std::fs::create_dir_all(root.join("outside")).unwrap();
            folder_outside(root, |list| list.push("outside".into()));
        };
        if !leo_first {
            add_outside();
        }
        let add_outside = add_outside.clone();
        run(state.with_store(move |store| {
            store.create_dir("mine");
            if leo_first {
                add_outside();
            }
            store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
            store.save().unwrap();
            Ok(())
        }))
        .unwrap();
        let disk = folders_on_disk(&root);
        assert!(
            disk.contains(&"outside".to_string()),
            "leo first={leo_first}: {disk:?}"
        );
        assert!(
            disk.contains(&"mine".to_string()),
            "leo first={leo_first}: {disk:?}"
        );
        state.store.lock().unwrap().changes = None;
        let shown = run(state.read_store(|store| Ok(store.dir_exists("outside")))).unwrap();
        assert!(shown, "leo first={leo_first}");
    }
}

#[test]
fn a_folder_removed_outside_just_before_a_save_stays_removed() {
    for leo_first in [false, true] {
        let (state, _dir, ids) = state_with(&[("Local", "")]);
        let root = state.fresh().notes_dir.clone();
        run(state.with_store(|store| {
            store.create_dir("gone");
            store.save().unwrap();
            Ok(())
        }))
        .unwrap();
        assert!(folders_on_disk(&root).contains(&"gone".to_string()));
        held_back(&state);
        let local = ids[0].clone();
        let outside_root = root.clone();
        let remove_outside = move || {
            let root = &outside_root;
            let _ = std::fs::remove_dir_all(root.join("gone"));
            folder_outside(root, |list| list.retain(|f| f != "gone"));
        };
        if !leo_first {
            remove_outside();
        }
        let remove_outside = remove_outside.clone();
        run(state.with_store(move |store| {
            store.create_dir("mine");
            if leo_first {
                remove_outside();
            }
            store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
            store.save().unwrap();
            Ok(())
        }))
        .unwrap();
        let disk = folders_on_disk(&root);
        assert!(
            !disk.contains(&"gone".to_string()),
            "leo first={leo_first}: {disk:?}"
        );
        assert!(
            disk.contains(&"mine".to_string()),
            "leo first={leo_first}: {disk:?}"
        );
        state.store.lock().unwrap().changes = None;
        let shown = run(state.read_store(|store| Ok(store.dir_exists("gone")))).unwrap();
        assert!(!shown, "leo first={leo_first}");
    }
}

#[test]
#[ignore = "run on the Windows CI runner before the walk is skipped there"]
fn a_burst_of_outside_edits_is_never_missed_by_the_watcher() {
    let (state, _dir, ids) = state_with(&[("Local", "")]);
    let root = state.fresh().notes_dir.clone();
    state.store.lock().unwrap().changes = Some(crate::store_refresh::Changes::new(&root));
    run(state.read_store(|_| Ok(()))).unwrap();
    for i in 0..2_000 {
        std::fs::write(root.join(format!("Burst {i}.md")), format!("burst {i}")).unwrap();
    }
    let local = ids[0].clone();
    run(state.with_store(move |store| {
        store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
        store.save().unwrap();
        Ok(())
    }))
    .unwrap();
    let waited = std::time::Instant::now();
    loop {
        let count = run(state.read_store(|store| {
            Ok(store
                .notes
                .iter()
                .filter(|n| n.body.starts_with("burst "))
                .count())
        }))
        .unwrap();
        if count == 2_000 {
            break;
        }
        assert!(
            waited.elapsed() < std::time::Duration::from_secs(30),
            "{count} of 2000 loaded"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
