use super::*;

#[test]
fn a_deleted_note_is_in_the_trash_and_can_be_restored() {
    let (state, _d, ids) = state_with(&[("Lecture 4", "cs130")]);
    assert_eq!(
        run(delete_note(State(state.clone()), Path(ids[0].clone()))),
        StatusCode::NO_CONTENT
    );
    let Json(trash) = run(list_trash(State(state.clone()))).unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].title, "Lecture 4");
    assert_eq!(trash[0].directory, "cs130");

    let Json(back) = run(restore_note(State(state.clone()), Path(ids[0].clone()))).unwrap();
    assert_eq!(back.directory, "cs130");
    assert!(state.fresh().find_note(&ids[0]).is_some());
    let Json(trash) = run(list_trash(State(state.clone()))).unwrap();
    assert!(trash.is_empty());
    assert_eq!(
        run(restore_note(State(state), Path(ids[0].clone()))).unwrap_err(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn chosen_notes_and_folders_go_to_the_trash_and_the_root_is_refused() {
    let (state, _d, ids) = state_with(&[
        ("Keep", ""),
        ("Loose", ""),
        ("Graphs", "cs130"),
        ("Deep", "cs130/week1"),
        ("Other", "math"),
    ]);
    let moved = run(move_to_trash(
        State(state.clone()),
        Json(TrashMove {
            notes: vec![ids[1].clone(), "missing".into()],
            dirs: vec!["cs130".into()],
        }),
    ))
    .unwrap();
    assert_eq!(moved.0["notes"], 3);
    assert_eq!(moved.0["folders"], 1);
    let store = state.fresh();
    let left: Vec<&str> = store.notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert!(left.contains(&"Keep") && left.contains(&"Other"));
    assert!(!store.dir_exists("cs130"));
    assert_eq!(store.trashed().len(), 3);
    drop(store);
    for bad in ["", "/", "../outside"] {
        let refused = run(move_to_trash(
            State(state.clone()),
            Json(TrashMove {
                notes: vec![],
                dirs: vec![bad.into()],
            }),
        ));
        assert_eq!(refused.unwrap_err(), StatusCode::BAD_REQUEST, "{bad:?}");
    }
}

#[test]
fn undo_brings_back_every_note_and_folder_a_move_to_the_trash_took() {
    let (state, _d, ids) = state_with(&[
        ("Loose", ""),
        ("Graphs", "cs130"),
        ("Deep", "cs130/week1"),
        ("Other", "math"),
    ]);
    {
        let mut store = state.fresh();
        assert!(store.create_dir("cs130/empty"));
        store.save().unwrap();
    }
    let moved = run(move_to_trash(
        State(state.clone()),
        Json(TrashMove {
            notes: vec![ids[0].clone()],
            dirs: vec!["cs130".into()],
        }),
    ))
    .unwrap()
    .0;
    let mut taken: Vec<String> = serde_json::from_value(moved["ids"].clone()).unwrap();
    taken.sort();
    let mut expected = ids[..3].to_vec();
    expected.sort();
    assert_eq!(taken, expected);
    let dirs: Vec<String> = serde_json::from_value(moved["dirs"].clone()).unwrap();
    assert!(dirs.contains(&"cs130/empty".to_string()), "{dirs:?}");

    let undone = run(restore_many(
        State(state.clone()),
        Json(TrashChoice {
            ids: taken,
            all: false,
            dirs,
        }),
    ))
    .unwrap();
    assert_eq!(undone.0["restored"], 3);
    let store = state.fresh();
    assert_eq!(store.notes.len(), 4);
    assert!(store.dir_exists("cs130/empty"));
    assert!(store.dir_exists("cs130/week1"));
    assert!(store.trashed().is_empty());
    drop(store);

    let refused = run(restore_many(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![],
            all: false,
            dirs: vec!["../outside".into()],
        }),
    ));
    assert_eq!(refused.unwrap_err(), StatusCode::BAD_REQUEST);
}

#[test]
fn a_trashed_folder_leaves_no_empty_directory_to_block_its_name() {
    let (state, _d, _ids) = state_with(&[("Notes", "Week 1"), ("Later", "Week 01")]);
    let root = state.fresh().notes_dir.clone();
    assert!(root.join("Week 1").is_dir());
    let _ = run(move_to_trash(
        State(state.clone()),
        Json(TrashMove {
            notes: vec![],
            dirs: vec!["Week 1".into()],
        }),
    ))
    .unwrap();
    assert!(!root.join("Week 1").exists());
    let renamed = run(crate::routes::notes::rename_dir(
        State(state.clone()),
        Json(crate::routes::notes::RenameDirBody {
            from: "Week 01".into(),
            name: "Week 1".into(),
        }),
    ))
    .unwrap();
    assert_eq!(renamed.0["path"], "Week 1");
}

#[test]
fn a_trashed_file_leo_cannot_read_is_listed_and_cannot_be_restored() {
    let (state, _d, _ids) = state_with(&[]);
    let trash = state.fresh().notes_dir.join(".trash/cs130");
    std::fs::create_dir_all(&trash).unwrap();
    std::fs::write(trash.join("Broken.md"), "---\ntitle: [\n---\nx").unwrap();
    let Json(listed) = run(list_trash(State(state.clone()))).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            listed[0].title.as_str(),
            listed[0].directory.as_str(),
            listed[0].readable
        ),
        ("Broken", "cs130", false)
    );
    let restored = run(restore_many(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![listed[0].id.clone()],
            all: false,
            dirs: vec![],
        }),
    ))
    .unwrap();
    assert_eq!(restored.0["restored"], 0);
    assert_eq!(restored.0["unreadable"], 1);
    assert!(trash.join("Broken.md").exists());
}

#[cfg(unix)]
#[test]
fn p02_empty_trash_with_one_locked_folder_reports_what_failed() {
    use std::os::unix::fs::PermissionsExt;
    let dirs = ["a", "b", "c", "d", "e", "f"];
    let spec: Vec<(String, String)> = dirs
        .iter()
        .flat_map(|d| (0..2).map(move |n| (format!("{d}{n}"), d.to_string())))
        .collect();
    let refs: Vec<(&str, &str)> = spec.iter().map(|(t, d)| (t.as_str(), d.as_str())).collect();
    let (state, _d, _ids) = state_with(&refs);
    let moved = run(move_to_trash(
        State(state.clone()),
        Json(TrashMove {
            notes: vec![],
            dirs: dirs.iter().map(|d| d.to_string()).collect(),
        }),
    ))
    .unwrap();
    assert_eq!(moved.0["notes"], 12);
    let trash = state.fresh().notes_dir.join(".trash");
    let locked: Vec<_> = ["b", "d", "f"].iter().map(|d| trash.join(d)).collect();
    for dir in &locked {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    let result = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![],
            all: true,
            dirs: vec![],
        }),
    ));
    let left = state.fresh().trashed().len();
    for dir in &locked {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let result = result.unwrap();
    assert_eq!(result.0["deleted"], 6);
    assert_eq!(result.0["failed"], 6);
    assert_eq!(left, 6);
}

#[test]
#[ignore]
fn p02_benchmark_trash_requests() {
    let size: usize = std::env::var("LEO_BENCH_TRASH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let big = std::env::var("LEO_BENCH_BIG").is_ok_and(|v| v == "1");
    let body = "word ".repeat(1024);
    let (state, _d, _) = state_with(&[]);
    let trash = state.fresh().notes_dir.join(".trash");
    std::fs::create_dir_all(&trash).unwrap();
    let ids: Vec<String> = (0..size)
        .map(|_| uuid::Uuid::new_v4().to_string())
        .collect();
    for (n, id) in ids.iter().enumerate() {
        std::fs::write(
            trash.join(format!("Note {n}.md")),
            format!("---\nid: {id}\ntitle: Note {n}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n{body}"),
        )
        .unwrap();
    }
    if big {
        std::fs::write(
            trash.join("Big.md"),
            format!(
                "---\nid: {}\ntitle: Big\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n{}",
                uuid::Uuid::new_v4(),
                "word ".repeat(4 * 1024 * 1024)
            ),
        )
        .unwrap();
    }
    let total = size + usize::from(big);
    drop(state.fresh());
    let measure = |label: &str, f: &dyn Fn()| {
        let started = std::time::Instant::now();
        f();
        println!(
            "trash {total:>6}: {label:<22} {:>10.1} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    };
    measure("list", &|| {
        let Json(listed) = run(list_trash(State(state.clone()))).unwrap();
        assert_eq!(listed.len(), total);
    });
    measure("restore 50", &|| {
        let out = run(restore_many(
            State(state.clone()),
            Json(TrashChoice {
                ids: ids[..50].to_vec(),
                all: false,
                dirs: vec![],
            }),
        ))
        .unwrap();
        assert_eq!(out.0["restored"], 50);
    });
    measure("delete 50 for good", &|| {
        let out = run(crate::routes::trash::delete_from_trash(
            State(state.clone()),
            Json(TrashChoice {
                ids: ids[50..100].to_vec(),
                all: false,
                dirs: vec![],
            }),
        ))
        .unwrap();
        assert_eq!(out.0["deleted"], 50);
    });
    measure("empty trash", &|| {
        let out = run(crate::routes::trash::delete_from_trash(
            State(state.clone()),
            Json(TrashChoice {
                ids: vec![],
                all: true,
                dirs: vec![],
            }),
        ))
        .unwrap();
        assert_eq!(out.0["deleted"], total - 100);
    });
}

fn p02_count(state: &AppState, sql: &str, note: &str) -> i64 {
    crate::tasks::db_of(&state.chats)
        .unwrap()
        .with(|c| c.query_row(sql, [note], |r| r.get(0)))
        .unwrap()
}

#[test]
fn p02_deleting_a_note_for_good_leaves_nothing_behind_for_its_id() {
    let (state, _d, ids) = state_with(&[("Lecture 4", "")]);
    let id = ids[0].clone();
    let notes = state.fresh().notes_dir.clone();
    let data = notes.parent().unwrap().to_path_buf();
    let sources = leo_core::recording::root(&notes).join(&id);
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(sources.join("passages.json"), "[]").unwrap();
    let originals = crate::routes::uploads::originals_dir(&notes, &id).unwrap();
    std::fs::create_dir_all(&originals).unwrap();
    std::fs::write(originals.join("slides.pdf"), "%PDF").unwrap();
    crate::tasks::put_meta(
        &state.chats,
        &crate::tasks::Meta {
            id: "task-p02-0001".into(),
            note: id.clone(),
            text: "Read chapter 3".into(),
            description: "before Friday".into(),
            promoted: true,
            ..Default::default()
        },
    )
    .unwrap();
    crate::record::set_writeup_pending(&state.chats, &id, true);

    assert_eq!(
        run(delete_note(State(state.clone()), Path(id.clone()))),
        StatusCode::NO_CONTENT
    );
    let gone = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![id.clone()],
            all: false,
            dirs: vec![],
        }),
    ))
    .unwrap();
    assert_eq!(gone.0["deleted"], 1);

    let mut left = Vec::new();
    for (name, path) in [
        ("recording-sources/<id>", &sources),
        ("<data>/attachments/<id>", &originals),
    ] {
        if path.exists() {
            let files: Vec<String> = std::fs::read_dir(path)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            left.push(format!("{name} {files:?}"));
        }
    }
    let tasks = p02_count(
        &state,
        "SELECT COUNT(*) FROM note_tasks WHERE note = ?1",
        &id,
    );
    let writeups = p02_count(
        &state,
        "SELECT COUNT(*) FROM writeup_pending WHERE note = ?1",
        &id,
    );
    if tasks > 0 {
        left.push(format!("leo.db note_tasks rows: {tasks}"));
    }
    if writeups > 0 {
        left.push(format!("leo.db writeup_pending rows: {writeups}"));
    }
    println!(
        "data dir {}: left for the deleted note: {left:#?}",
        data.display()
    );
    assert!(left.is_empty(), "left behind: {left:#?}");
}

#[test]
fn p02_task_metadata_of_a_live_note_survives_ten_thousand_orphan_rows() {
    let (state, _d, ids) = state_with(&[("Live", "")]);
    crate::tasks::db_of(&state.chats)
        .unwrap()
        .with(|c| {
            c.execute(
                "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 9999) INSERT INTO note_tasks (id, note, text, before, after, kind, promoted, sent_at, description) SELECT printf('orphan-%05d', i), 'gone-note', 'old', '', '', NULL, 1, NULL, '' FROM n",
                [],
            )
        })
        .unwrap();
    crate::tasks::put_meta(
        &state.chats,
        &crate::tasks::Meta {
            id: "live-task".into(),
            note: ids[0].clone(),
            text: "Live task".into(),
            description: "keep me".into(),
            promoted: true,
            ..Default::default()
        },
    )
    .unwrap();
    let metas = crate::tasks::metas(&state.chats);
    let found = metas.iter().any(|m| m.id == "live-task");
    println!(
        "metas returned {} rows; live row present: {found}",
        metas.len()
    );
    assert!(
        found,
        "the live note's task metadata fell outside the LIMIT"
    );
}

fn deleting(state: &AppState) -> std::collections::BTreeSet<String> {
    crate::margin::state_of(&state.chats, "deleting").unwrap_or_default()
}

fn sources_of(state: &AppState, id: &str) -> std::path::PathBuf {
    let folder = leo_core::recording::root(&state.notes_dir).join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("passages.json"), "[]").unwrap();
    folder
}

fn delete_for_good(state: &AppState, id: &str) {
    assert_eq!(
        run(delete_note(State(state.clone()), Path(id.to_string()))),
        StatusCode::NO_CONTENT
    );
    let gone = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![id.to_string()],
            all: false,
            dirs: vec![],
        }),
    ))
    .unwrap();
    assert_eq!(gone.0["deleted"], 1);
}

fn clear_now(state: &AppState, ids: Vec<String>, active: Option<String>) {
    let owners = crate::dependents::Owners::of(&state.fresh());
    crate::dependents::clear(&state.chats, &state.notes_dir, ids, active, &owners);
}

#[cfg(unix)]
#[test]
fn a_locked_recording_folder_is_removed_on_the_next_try() {
    use std::os::unix::fs::PermissionsExt;
    let (state, _d, ids) = state_with(&[("Lecture 4", "")]);
    let id = ids[0].clone();
    let folder = sources_of(&state, &id);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).unwrap();
    delete_for_good(&state, &id);
    assert!(folder.join("passages.json").exists());
    assert!(deleting(&state).contains(&id));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    clear_now(&state, Vec::new(), None);
    assert!(!folder.exists());
    assert!(deleting(&state).is_empty());
}

#[test]
fn a_running_recordings_folder_is_kept() {
    let (state, _d, _) = state_with(&[]);
    let id = "a1b2c3d4-0000-4000-8000-00000000abcd".to_string();
    let folder = sources_of(&state, &id);
    clear_now(&state, vec![id.clone()], Some(id.clone()));
    assert!(folder.join("passages.json").exists());
    assert!(deleting(&state).contains(&id));
    clear_now(&state, Vec::new(), None);
    assert!(!folder.exists());
    assert!(deleting(&state).is_empty());
}

#[test]
fn a_pending_note_that_is_back_keeps_its_files() {
    let (state, _d, ids) = state_with(&[("Back again", ""), ("Trashed", "")]);
    let live = sources_of(&state, &ids[0]);
    let trashed = sources_of(&state, &ids[1]);
    assert_eq!(
        run(delete_note(State(state.clone()), Path(ids[1].clone()))),
        StatusCode::NO_CONTENT
    );
    clear_now(&state, vec![ids[0].clone(), ids[1].clone()], None);
    assert!(live.join("passages.json").exists());
    assert!(trashed.join("passages.json").exists());
    assert!(deleting(&state).is_empty());
}

#[test]
fn nothing_is_removed_while_a_note_cannot_be_read() {
    let (state, _d, _) = state_with(&[("Readable", "")]);
    let id = "a1b2c3d4-0000-4000-8000-00000000beef".to_string();
    let folder = sources_of(&state, &id);
    std::fs::write(state.notes_dir.join("Broken.md"), b"---\ntitle: [\n---\nx").unwrap();
    assert!(!state.fresh().unreadable.is_empty());
    clear_now(&state, vec![id.clone()], None);
    assert!(folder.join("passages.json").exists());
    assert!(deleting(&state).contains(&id));
    std::fs::remove_file(state.notes_dir.join("Broken.md")).unwrap();
    clear_now(&state, Vec::new(), None);
    assert!(!folder.exists());
}

#[test]
fn nothing_is_removed_while_a_trashed_file_cannot_be_read() {
    let (state, _d, _) = state_with(&[("Readable", "")]);
    let id = "a1b2c3d4-0000-4000-8000-00000000cafe".to_string();
    let folder = sources_of(&state, &id);
    let trash = state.notes_dir.join(".trash");
    std::fs::create_dir_all(&trash).unwrap();
    std::fs::write(trash.join("Broken.md"), b"---\ntitle: [\n---\nx").unwrap();
    clear_now(&state, vec![id.clone()], None);
    assert!(folder.join("passages.json").exists());
    assert!(deleting(&state).contains(&id));
}

#[test]
fn the_sweep_removes_task_details_of_a_note_gone_for_a_week() {
    let (state, _d, ids) = state_with(&[("Live", "")]);
    for (task, note) in [("task-live", ids[0].as_str()), ("task-gone", "gone-note")] {
        crate::tasks::put_meta(
            &state.chats,
            &crate::tasks::Meta {
                id: task.into(),
                note: note.into(),
                text: "Read".into(),
                promoted: true,
                ..Default::default()
            },
        )
        .unwrap();
    }
    let kept: std::collections::HashSet<String> = ids.iter().cloned().collect();
    let now = chrono::Utc::now();
    crate::margin::sweep(&state.chats, &kept, now);
    crate::margin::sweep(
        &state.chats,
        &kept,
        now + chrono::Duration::days(crate::margin::SWEEP_AFTER_DAYS + 1),
    );
    let left: Vec<String> = crate::tasks::metas(&state.chats)
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(left, ["task-live"]);
}

#[test]
fn deleting_the_absorbed_note_for_good_keeps_the_remaining_notes_original() {
    use crate::routes::notes::version_of;
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    let folder =
        |note: &str| crate::routes::uploads::originals_dir(&state.notes_dir, note).unwrap();
    std::fs::create_dir_all(folder(&ids[1])).unwrap();
    std::fs::write(folder(&ids[1]).join("slides.pdf"), "%PDF slides").unwrap();
    let (base, with_base) = {
        let store = state.fresh();
        (
            version_of(store.find_note(&ids[0]).unwrap()),
            version_of(store.find_note(&ids[1]).unwrap()),
        )
    };
    let saved = run(crate::routes::combine::keep(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(
            serde_json::from_value(serde_json::json!({
                "with": ids[1], "body": "Both notes, combined.", "base": base, "with_base": with_base
            }))
            .unwrap(),
        ),
    ));
    assert_eq!(saved.status(), StatusCode::OK);
    let gone = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![ids[1].clone()],
            all: false,
            dirs: vec![],
        }),
    ))
    .unwrap();
    assert_eq!(gone.0["deleted"], 1);
    assert!(!folder(&ids[1]).exists());
    assert_eq!(
        std::fs::read_to_string(folder(&ids[0]).join("slides.pdf")).unwrap(),
        "%PDF slides"
    );
    assert!(crate::margin::state_of::<serde_json::Value>(
        &state.chats,
        &format!("combined:{}", ids[0])
    )
    .is_some());
    delete_for_good(&state, &ids[0]);
    assert!(!folder(&ids[0]).exists());
    assert!(crate::margin::state_of::<serde_json::Value>(
        &state.chats,
        &format!("combined:{}", ids[0])
    )
    .is_none());
}
