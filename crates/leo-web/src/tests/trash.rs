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
