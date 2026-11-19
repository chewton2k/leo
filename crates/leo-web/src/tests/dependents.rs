use std::time::{Duration, Instant};

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

fn picture_library(n: usize, trashed: usize) -> (tempfile::TempDir, Store, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes");
    let pictures = notes.join("attachments");
    std::fs::create_dir_all(&pictures).unwrap();
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let a = format!("20261117-101500-p{i}.png");
        let b = format!("20261117-101500-q{i}.png");
        if i < 20_000 {
            std::fs::write(pictures.join(&a), b"png").unwrap();
            std::fs::write(pictures.join(&b), b"png").unwrap();
        }
        let body = format!(
            "Lecture {i} words about the subject.\n\n![one](attachments/{a})\n\nMore words.\n\n![[{b}]]\n"
        );
        let note = leo_core::notes::Note::new(
            format!("Note {i}"),
            body,
            vec![],
            format!("Course {}", i % 40),
        );
        let root = if i < trashed {
            notes.join(".trash")
        } else {
            notes.clone()
        };
        let folder = root.join(&note.directory);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(format!("{}.md", note.title)),
            format!(
                "---\nid: {}\ntitle: {}\ncreated_at: {}\nupdated_at: {}\n---\n\n{}",
                note.id,
                note.title,
                note.created_at.to_rfc3339(),
                note.updated_at.to_rfc3339(),
                note.body,
            ),
        )
        .unwrap();
        ids.push(note.id);
    }
    let store = Store::load_from(&notes).unwrap();
    (dir, store, ids)
}

#[test]
#[ignore]
fn p_c03_benchmark_picture_owners() {
    let n: usize = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let trashed = (n / 10).max(50);
    let (_dir, mut store, ids) = picture_library(n, trashed);
    let notes = store.notes_dir.clone();
    let mut loads = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        let loaded = Store::load_from(&notes).unwrap();
        loads.push(start.elapsed());
        drop(loaded);
    }
    let start = Instant::now();
    assert!(!store.changed_on_disk());
    let checked = start.elapsed();
    let start = Instant::now();
    store.observe_generations();
    let observed = start.elapsed();
    eprintln!(
        "p_c03 notes {n}: notes folder check {:.1} ms, generations {:.1} ms (both under the lock once per removal pass)",
        checked.as_secs_f64() * 1000.0,
        observed.as_secs_f64() * 1000.0
    );
    let live = ids[n / 2].clone();
    let mut saves = Vec::new();
    for round in 0..7 {
        let note = store.find_note_mut(&live).unwrap();
        note.body.push_str(&format!("\nedit {round}"));
        let start = Instant::now();
        store.save().unwrap();
        saves.push(start.elapsed());
    }
    let start = Instant::now();
    let emptied = store.delete_from_trash(&ids[..50]);
    let deleted = start.elapsed();
    assert_eq!(emptied.forgotten.len(), 50);
    let names: Vec<String> = std::fs::read_dir(notes.join("attachments"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let start = Instant::now();
    let owed = store.trash_reads_owed();
    let read: Vec<_> = owed
        .into_iter()
        .filter_map(|owed| owed.read().map(|pictures| (owed, pictures)))
        .collect();
    store.settle_owed(read);
    let first = start.elapsed();
    let start = Instant::now();
    let due = store
        .picture_pass(&names, chrono::Utc::now(), Some(chrono::Duration::days(30)))
        .unwrap();
    let passed = start.elapsed();
    assert!(due.len() >= 100);
    eprintln!(
        "p_c03 notes {n} trashed {trashed}: load {:.1} ms, autosave {:.2} ms, delete 50 for good {:.2} ms, first trash read {:.1} ms, picture pass {:.2} ms over {} pictures",
        median(loads),
        median(saves),
        deleted.as_secs_f64() * 1000.0,
        first.as_secs_f64() * 1000.0,
        passed.as_secs_f64() * 1000.0,
        names.len()
    );
    let attachments = notes.join("attachments");
    let ledger = notes.join(".leo").join(leo_core::attachments::MADE);
    std::fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    let mut made = String::new();
    for entry in std::fs::read_dir(&attachments).unwrap().flatten() {
        let meta = entry.metadata().unwrap();
        let modified = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        made.push_str(&format!(
            "{}\t{}\t{modified}\t{}\n",
            entry.file_name().to_string_lossy(),
            meta.len(),
            leo_core::paths::digest_text(&leo_core::paths::digest(b"png"))
        ));
    }
    for i in 0..(n / 10) {
        let name = format!("20261117-101500-r{i}.png");
        std::fs::write(attachments.join(&name), b"png").unwrap();
        let meta = std::fs::metadata(attachments.join(&name)).unwrap();
        let modified = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        made.push_str(&format!(
            "{name}\t{}\t{modified}\t{}\n",
            meta.len(),
            leo_core::paths::digest_text(&leo_core::paths::digest(b"png"))
        ));
    }
    std::fs::write(&ledger, made).unwrap();
    let (mut state, _other, _) = state_with(&[]);
    state.chats = _dir.path().join("chats");
    state.notes_dir = notes.clone();
    state.store = std::sync::Arc::new(std::sync::Mutex::new(crate::Storage {
        changes: None,
        store,
        reload: false,
    }));
    crate::dependents::TIMED
        .lock()
        .unwrap()
        .retain(|(notes, _)| notes != &state.notes_dir);
    crate::dependents::pictures(&state, chrono::Utc::now());
    state
        .fresh()
        .observe(chrono::Utc::now(), Some(chrono::Duration::days(31)));
    crate::dependents::pictures(&state, chrono::Utc::now());
    crate::dependents::pictures(&state, chrono::Utc::now());
    let timed: Vec<_> = crate::dependents::TIMED
        .lock()
        .unwrap()
        .iter()
        .filter(|(notes, _)| notes == &state.notes_dir)
        .map(|(_, timed)| *timed)
        .collect();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "p_c03 picture pass notes {n}, {} unlinked pictures: releasing {:.1} ms outside / {:.1} ms under the lock; first move batch {:.1} / {:.1}; follow-up batch {:.1} / {:.1}",
        n / 10,
        ms(timed[0].0), ms(timed[0].1), ms(timed[1].0), ms(timed[1].1), ms(timed[2].0), ms(timed[2].1)
    );
    assert!(notes
        .join(".trash/attachments/20261117-101500-r0.png")
        .exists());
}

use super::*;

const X: &str = "20261117-101500-slide.png";
const Y: &str = "20261117-101500-board.png";

fn picture(state: &AppState, name: &str) -> std::path::PathBuf {
    let folder = state.notes_dir.join("attachments");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    std::fs::write(&path, b"png").unwrap();
    let leo = state.notes_dir.join(".leo");
    std::fs::create_dir_all(&leo).unwrap();
    let mut made = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(leo.join(leo_core::attachments::MADE))
        .unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    let modified = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::io::Write::write_all(
        &mut made,
        format!(
            "{name}\t{}\t{modified}\t{}\n",
            meta.len(),
            leo_core::paths::digest_text(&leo_core::paths::digest(b"png"))
        )
        .as_bytes(),
    )
    .unwrap();
    path
}

fn binned(state: &AppState, name: &str) -> bool {
    state
        .notes_dir
        .join(".trash/attachments")
        .join(name)
        .exists()
}

fn let_days_pass(state: &AppState, days: i64) {
    state
        .fresh()
        .observe(chrono::Utc::now(), Some(chrono::Duration::days(days)));
}

fn held_folder(state: &AppState, kind: &str, id: &str) -> std::path::PathBuf {
    state
        .notes_dir
        .parent()
        .unwrap()
        .join("held")
        .join(kind)
        .join(id)
}

fn linking(name: &str) -> String {
    format!("Words\n\n![a picture](attachments/{name})\n")
}

fn write_body(state: &AppState, id: &str, body: &str) {
    let mut store = state.fresh();
    store.find_note_mut(id).unwrap().body = body.to_string();
    store.save().unwrap();
}

fn trash(state: &AppState, id: &str) {
    assert_eq!(
        run(delete_note(State(state.clone()), Path(id.to_string()))),
        StatusCode::NO_CONTENT
    );
}

fn delete_for_good(state: &AppState, ids: &[&str], all: bool) -> serde_json::Value {
    run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: ids.iter().map(|id| id.to_string()).collect(),
            all,
            dirs: vec![],
            revisions: shown(state),
        }),
    ))
    .unwrap()
    .0
}

fn refused_for_good(state: &AppState, ids: &[&str]) -> StatusCode {
    run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: ids.iter().map(|id| id.to_string()).collect(),
            revisions: shown(state),
            ..Default::default()
        }),
    ))
    .unwrap_err()
    .0
}

fn restore(state: &AppState, ids: &[&str]) -> serde_json::Value {
    run(restore_many(
        State(state.clone()),
        Json(TrashChoice {
            ids: ids.iter().map(|id| id.to_string()).collect(),
            all: false,
            dirs: vec![],
            revisions: Default::default(),
        }),
    ))
    .unwrap()
    .0
}

fn later(days: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() + chrono::Duration::days(days)
}

fn pass(state: &AppState, days: i64) {
    let_days_pass(state, days.max(0));
    crate::dependents::pictures(state, chrono::Utc::now());
}

fn rows(state: &AppState, sql: &str, id: &str) -> i64 {
    crate::db::for_chats(&state.chats)
        .with(|c| c.query_row(sql, [id], |r| r.get(0)))
        .unwrap()
}

struct Hanging {
    originals: std::path::PathBuf,
    sources: std::path::PathBuf,
}

fn hang_everything_on(state: &AppState, id: &str) -> Hanging {
    let originals = crate::routes::uploads::originals_dir(&state.notes_dir, id).unwrap();
    std::fs::create_dir_all(&originals).unwrap();
    std::fs::write(originals.join("slides.pdf"), "%PDF").unwrap();
    let sources = leo_core::recording::root(&state.notes_dir).join(id);
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(sources.join("passages.json"), "[]").unwrap();
    crate::margin::put(
        &state.chats,
        &crate::margin::new_thread(
            id,
            "read",
            crate::margin::Place {
                prefix: String::new(),
                suffix: String::new(),
            },
            "words",
            Vec::new(),
        ),
        crate::margin::MOST_OPEN,
    );
    crate::tasks::put_meta(
        &state.chats,
        &crate::tasks::Meta {
            id: format!("task-{id}"),
            note: id.into(),
            text: "Read".into(),
            promoted: true,
            ..Default::default()
        },
    )
    .unwrap();
    crate::db::for_chats(&state.chats)
        .with(|c| {
            c.execute(
                "INSERT INTO reading (note, anchor, at) VALUES (?1, 'a', 't')",
                [id],
            )?;
            c.execute(
                "INSERT INTO vectors (note, at, hash, v) VALUES (?1, 0, 'h', x'00000000')",
                [id],
            )?;
            c.execute("INSERT INTO graph_notes (id, data) VALUES (?1, '{}')", [id])
        })
        .unwrap();
    Hanging { originals, sources }
}

fn held(state: &AppState, id: &str) -> [bool; 7] {
    [
        crate::routes::uploads::originals_dir(&state.notes_dir, id)
            .unwrap()
            .exists(),
        leo_core::recording::root(&state.notes_dir)
            .join(id)
            .exists(),
        crate::margin::every(&state.chats)
            .iter()
            .any(|t| t.note == id),
        crate::tasks::metas(&state.chats)
            .iter()
            .any(|m| m.note == id),
        rows(state, "SELECT count(*) FROM reading WHERE note = ?1", id) > 0,
        rows(state, "SELECT count(*) FROM vectors WHERE note = ?1", id) > 0,
        rows(state, "SELECT count(*) FROM graph_notes WHERE id = ?1", id) > 0,
    ]
}

fn trash_files(state: &AppState) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![state.notes_dir.join(".trash")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    found
}

fn keep_trash(state: &AppState, days: Option<u32>) {
    leo_core::keep::save(
        &state.notes_dir,
        &leo_core::keep::Keep {
            trash_days: days,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn a_note_that_expires_from_the_trash_takes_everything_with_it() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = ids[0].as_str();
    let path = picture(&state, X);
    write_body(&state, id, &linking(X));
    let hanging = hang_everything_on(&state, id);
    trash(&state, id);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert!(state.fresh().trashed().is_empty());
    assert!(!hanging.originals.exists() && !hanging.sources.exists());
    assert!(held_folder(&state, "attachments", id)
        .join("slides.pdf")
        .exists());
    assert!(held_folder(&state, "recording-sources", id).exists());
    assert_eq!(held(&state, id)[2..], [true; 5], "rows wait too");
    assert!(
        !path.exists() && binned(&state, X),
        "its picture waits in the trash"
    );
    let listed = run(list_trash(State(state.clone()))).unwrap().0;
    assert!(listed.iter().any(|row| row.kind == "held"));
    assert!(listed.iter().any(|row| row.kind == "picture"));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert_eq!(held(&state, id), [false; 7]);
    assert!(!held_folder(&state, "attachments", id).exists());
    assert!(
        !binned(&state, X),
        "gone a trash period after it moved there"
    );
}

#[cfg(unix)]
#[test]
fn a_note_deleted_outside_leo_loses_its_dependents_after_the_sweep_grace() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = ids[0].clone();
    hang_everything_on(&state, &id);
    std::fs::remove_file(state.notes_dir.join("Lecture.md")).unwrap();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    assert!(crate::margin::sweep_store(&state.chats, kept, &[], &data).is_empty());
    let mut missing: std::collections::BTreeMap<String, crate::margin::Missing> =
        crate::margin::state_of(&state.chats, "missing").unwrap();
    assert!(missing.contains_key(&id));
    missing.insert(id.clone(), crate::margin::Missing::for_days(8));
    crate::margin::set_state(&state.chats, "missing", &missing);
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    let gone = crate::margin::sweep_store(&state.chats, kept, &[], &data);
    assert_eq!(gone, std::slice::from_ref(&id));
    crate::dependents::clear(&state, gone, crate::dependents::Why::Swept);
    assert!(held_folder(&state, "attachments", &id).exists());
    assert!(held(&state, &id)[2]);
    let_days_pass(&state, 31);
    crate::dependents::clear(&state, Vec::new(), crate::dependents::Why::Swept);
    assert_eq!(held(&state, &id), [false; 7]);
}

#[cfg(unix)]
#[test]
fn a_note_holding_kept_audio_is_tracked_by_the_sweep() {
    let (state, _d, ids) = state_with(&[("Gone", ""), ("Other", "")]);
    let id = ids[0].clone();
    left_outside(&state, "Gone.md");
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let audio = vec![id.clone(), "never-a-note".to_string()];
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    crate::margin::sweep_store(&state.chats, kept, &audio, &data);
    let missing: std::collections::BTreeMap<String, crate::margin::Missing> =
        crate::margin::state_of(&state.chats, "missing").unwrap();
    assert!(missing.contains_key(&id));
    assert!(!missing.contains_key("never-a-note"));
}

fn left_outside(state: &AppState, file: &str) {
    std::fs::remove_file(state.notes_dir.join(file)).unwrap();
    drop(state.fresh());
}

#[test]
fn restore_brings_every_dependent_back() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = ids[0].as_str();
    let path = picture(&state, X);
    write_body(&state, id, &linking(X));
    hang_everything_on(&state, id);
    trash(&state, id);
    pass(&state, 400);
    assert_eq!(held(&state, id), [true; 7]);
    assert!(path.exists());
    assert_eq!(restore(&state, &[id])["restored"], 1);
    pass(&state, 400);
    assert_eq!(held(&state, id), [true; 7]);
    assert!(path.exists());
    assert_eq!(state.fresh().picture_owners(X), 1);
}

#[test]
fn a_picture_a_card_links_is_kept() {
    for deleted in [false, true] {
        let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
        let path = picture(&state, X);
        write_body(&state, &ids[0], &linking(X));
        let now = chrono::Utc::now();
        crate::boards::put_card(
            &state.chats,
            &crate::boards::Card {
                id: "cpicture1".into(),
                board: "bboard1".into(),
                column: "c1".into(),
                rank: 1024.0,
                title: "Card".into(),
                subheading: String::new(),
                description: format!("See ![x](attachments/{X})"),
                priority: String::new(),
                created_at: now,
                updated_at: now,
                deleted,
            },
        )
        .unwrap();
        trash(&state, &ids[0]);
        delete_for_good(&state, &[&ids[0]], false);
        pass(&state, 400);
        assert!(path.exists(), "deleted card awaiting undo: {deleted}");
        assert!(state.fresh().picture_rows().is_empty());
    }
}

#[test]
fn a_picture_a_chat_links_is_kept() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    crate::chats::save(
        &state.chats,
        "chat-picture-1",
        crate::chats::Saving {
            title: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({
                "role": "assistant",
                "text": format!("Here it is ![x](attachments/{X})"),
            })],
            existed: false,
            from: None,
        },
        chrono::Utc::now(),
    )
    .unwrap();
    trash(&state, &ids[0]);
    delete_for_good(&state, &[&ids[0]], false);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn no_picture_is_deleted_while_a_note_file_cannot_be_read() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = picture(&state, X);
    pass(&state, 0);
    std::fs::write(
        state.notes_dir.join("Broken.md"),
        "---\ntitle: [unclosed\n---\nbody\n",
    )
    .unwrap();
    drop(state.fresh());
    pass(&state, 400);
    assert!(path.exists(), "a live file leo cannot read");
    std::fs::remove_file(state.notes_dir.join("Broken.md")).unwrap();
    std::fs::create_dir_all(state.notes_dir.join(".trash")).unwrap();
    std::fs::write(
        state.notes_dir.join(".trash/Broken.md"),
        "---\ntitle: [unclosed\n---\nbody\n",
    )
    .unwrap();
    drop(state.fresh());
    pass(&state, 400);
    assert!(path.exists(), "a trashed file leo cannot read");
    std::fs::remove_file(state.notes_dir.join(".trash/Broken.md")).unwrap();
    drop(state.fresh());
    pass(&state, 400);
    assert!(!path.exists());
}

#[test]
fn a_damaged_trashed_note_keeps_every_dependent() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = ids[0].clone();
    let path = picture(&state, X);
    write_body(&state, &id, &linking(X));
    hang_everything_on(&state, &id);
    trash(&state, &id);
    for file in trash_files(&state) {
        std::fs::write(&file, "---\ntitle: [unclosed\n---\nbody\n").unwrap();
    }
    keep_trash(&state, None);
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(id.clone(), crate::margin::Missing::for_days(8));
    crate::margin::set_state(&state.chats, "missing", &missing);
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    assert!(
        crate::margin::sweep_store(&state.chats, kept, std::slice::from_ref(&id), &data).is_empty()
    );
    crate::dependents::clear(&state, vec![id.clone()], crate::dependents::Why::Asked);
    run(crate::keeping::expire_now(&state));
    pass(&state, 400);
    assert_eq!(held(&state, &id), [true; 7]);
    assert!(path.exists());
}

#[test]
fn a_guarded_sweep_resets_the_missing_clock() {
    let (state, _d, ids) = state_with(&[("Gone", ""), ("Other", "")]);
    let id = ids[0].clone();
    hang_everything_on(&state, &id);
    left_outside(&state, "Gone.md");
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(id.clone(), crate::margin::Missing::for_days(6));
    crate::margin::set_state(&state.chats, "missing", &missing);
    std::fs::create_dir_all(state.notes_dir.join(".trash")).unwrap();
    let broken = state.notes_dir.join(".trash/Broken.md");
    std::fs::write(&broken, "---\ntitle: [unclosed\n---\nbody\n").unwrap();
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    assert!(crate::margin::sweep_store(&state.chats, kept, &[], &data).is_empty());
    let after: std::collections::BTreeMap<String, crate::margin::Missing> =
        crate::margin::state_of(&state.chats, "missing").unwrap();
    assert!(after.is_empty());
    std::fs::remove_file(&broken).unwrap();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    assert!(crate::margin::sweep_store(&state.chats, kept, &[], &data).is_empty());
    let after: std::collections::BTreeMap<String, crate::margin::Missing> =
        crate::margin::state_of(&state.chats, "missing").unwrap();
    assert!(after[&id].observed < 3600, "the seven days start again");
}

#[test]
fn a_trashed_note_edited_outside_to_link_a_picture_keeps_it() {
    let (state, _d, ids) = state_with(&[("A", ""), ("B", ""), ("Other", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[1], &linking(X));
    trash(&state, &ids[0]);
    trash(&state, &ids[1]);
    for file in trash_files(&state) {
        let text = std::fs::read_to_string(&file).unwrap();
        if text.contains(&ids[0]) {
            std::fs::write(&file, format!("{text}\n{}", linking(X))).unwrap();
        }
    }
    state.fresh().trash_may_have_changed();
    delete_for_good(&state, &[&ids[1]], false);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn an_unreadable_trashed_body_suspends_picture_deletion() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    trash(&state, &ids[0]);
    let mut odd = b"---\nid: odd\ntitle: Odd\n---\n".to_vec();
    odd.extend_from_slice(&[0xff, 0xfe, 0xfd]);
    std::fs::write(state.notes_dir.join(".trash/Odd.md"), odd).unwrap();
    state.fresh().trash_may_have_changed();
    delete_for_good(&state, &[&ids[0]], false);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn the_final_picture_check_holds_every_owner_writer() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    trash(&state, &ids[0]);
    *crate::dependents::WATCH.lock().unwrap() = Some(state.notes_dir.clone());
    delete_for_good(&state, &[&ids[0]], false);
    crate::dependents::WATCH.lock().unwrap().take();
    assert!(!path.exists());
    let held = crate::dependents::HELD_LOCKS.lock().unwrap().clone();
    assert!(!held.is_empty());
    assert!(held.iter().all(|h| *h == [true; 3]), "{held:?}");
}

#[test]
fn a_fresh_unlinked_picture_waits_the_trash_period_then_the_trash() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = picture(&state, X);
    pass(&state, 0);
    pass(&state, 29);
    assert!(path.exists());
    pass(&state, 1);
    assert!(
        !path.exists() && binned(&state, X),
        "moved to the trash first"
    );
    pass(&state, 29);
    assert!(binned(&state, X));
    pass(&state, 1);
    assert!(!binned(&state, X));
}

#[test]
fn a_picture_another_program_named_is_never_touched() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = picture(&state, "photo.png");
    pass(&state, 0);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn a_link_in_another_case_keeps_the_picture() {
    let (state, _d, ids) = state_with(&[("A", "")]);
    let path = picture(&state, X);
    write_body(
        &state,
        &ids[0],
        &format!("![x](Attachments/{})", X.to_uppercase()),
    );
    pass(&state, 0);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn with_trash_kept_forever_an_unlinked_picture_stays_and_a_deleted_note_takes_its_picture() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    keep_trash(&state, None);
    let unlinked = picture(&state, Y);
    let carried = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    pass(&state, 0);
    trash(&state, &ids[0]);
    delete_for_good(&state, &[&ids[0]], false);
    pass(&state, 4000);
    assert!(unlinked.exists());
    assert!(!carried.exists());
}

#[test]
fn a_picture_removed_from_a_note_is_a_trash_row_with_restore_and_delete() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    let x = picture(&state, X);
    let y = picture(&state, Y);
    write_body(&state, &ids[0], &format!("{}{}", linking(X), linking(Y)));
    write_body(&state, &ids[1], &linking(X));
    trash(&state, &ids[1]);
    let listed = run(list_trash(State(state.clone()))).unwrap().0;
    let note = listed.iter().find(|row| row.id == ids[1]).unwrap();
    assert_eq!((note.pictures, note.kind), (1, "note"));
    write_body(&state, &ids[0], "Words");
    pass(&state, 0);
    pass(&state, 31);
    let listed = run(list_trash(State(state.clone()))).unwrap().0;
    let pictures: Vec<&str> = listed
        .iter()
        .filter(|row| row.kind == "picture")
        .map(|row| row.title.as_str())
        .collect();
    assert_eq!(pictures, [Y], "X still rides in the trashed note");
    assert_eq!(restore(&state, &[&format!("picture:{Y}")])["restored"], 1);
    assert!(y.exists());
    let listed = run(list_trash(State(state.clone()))).unwrap().0;
    assert!(listed.iter().all(|row| row.kind == "note"));
    pass(&state, 400);
    assert!(y.exists(), "a restored picture is not trashed again");
    assert_eq!(restore(&state, &[&ids[1]])["restored"], 1);
    write_body(&state, &ids[1], "Words");
    pass(&state, 31);
    assert!(binned(&state, X));
    let answer = delete_for_good(&state, &[&format!("picture:{X}")], false);
    assert_eq!(
        (answer["deleted"].as_u64(), answer["failed"].as_u64()),
        (Some(1), Some(0))
    );
    assert!(!x.exists() && !binned(&state, X));
    assert_eq!(
        delete_for_good(&state, &["picture:../secret.png"], false)["deleted"],
        0
    );
}

#[test]
fn empty_trash_removes_notes_their_pictures_and_picture_rows() {
    let (state, _d, ids) = state_with(&[("A", ""), ("B", ""), ("Other", "")]);
    let x = picture(&state, X);
    picture(&state, Y);
    write_body(&state, &ids[0], &linking(X));
    write_body(&state, &ids[1], &linking(Y));
    write_body(&state, &ids[1], "Words");
    pass(&state, 0);
    pass(&state, 31);
    assert!(binned(&state, Y));
    trash(&state, &ids[0]);
    let answer = delete_for_good(&state, &[], true);
    assert_eq!(answer["deleted"], 2);
    assert!(!x.exists() && !binned(&state, X) && !binned(&state, Y));
    assert!(run(list_trash(State(state.clone()))).unwrap().0.is_empty());
}

#[test]
fn deleting_a_note_for_good_deletes_its_vectors_and_summary() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    hang_everything_on(&state, &ids[0]);
    trash(&state, &ids[0]);
    assert_eq!(held(&state, &ids[0]), [true; 7]);
    delete_for_good(&state, &[&ids[0]], false);
    assert_eq!(held(&state, &ids[0]), [false; 7]);
}

#[derive(Default)]
struct Audio {
    owners: Mutex<Vec<String>>,
    discarded: Mutex<Vec<String>>,
    refused: Mutex<Vec<String>>,
    locked: std::sync::atomic::AtomicBool,
    retargeted: Mutex<Vec<(String, String)>>,
    restored: Mutex<Vec<crate::record::Moved>>,
    after_discard: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl crate::record::Housekeeper for Audio {
    fn kept_sessions(&self, note: &str) -> anyhow::Result<Vec<crate::record::KeptSession>> {
        Ok(self
            .owners
            .lock()
            .unwrap()
            .contains(&note.to_string())
            .then(|| crate::record::KeptSession {
                digest: Some(leo_core::paths::digest(note.as_bytes())),
                folder: "20261117-101500".into(),
                session: note.into(),
                started: chrono::DateTime::UNIX_EPOCH,
            })
            .into_iter()
            .collect())
    }
    fn discard_held_audio(
        &self,
        note: &str,
        sessions: &[crate::record::KeptSession],
        _verified: &[Option<leo_core::paths::FolderStamps>],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self
                .refused
                .lock()
                .unwrap()
                .iter()
                .any(|owner| owner == note),
            "in use"
        );
        let mut owners = self.owners.lock().unwrap();
        if sessions.iter().any(|each| each.session == note) && owners.iter().any(|o| o == note) {
            owners.retain(|owner| owner != note);
            self.discarded.lock().unwrap().push(note.into());
            if let Some(hook) = self.after_discard.lock().unwrap().take() {
                hook();
            }
        }
        Ok(())
    }
    fn kept_for_notes(&self) -> Vec<String> {
        self.owners.lock().unwrap().clone()
    }
    fn plan_retarget(&self, from: &str, target: &str) -> anyhow::Result<Vec<crate::record::Moved>> {
        anyhow::ensure!(
            !self.locked.load(std::sync::atomic::Ordering::Acquire),
            "in use"
        );
        Ok(self
            .owners
            .lock()
            .unwrap()
            .iter()
            .any(|owner| owner == from)
            .then(|| crate::record::Moved {
                session: "20261117-101500".into(),
                identity: None,
                previous: Some(from.to_string()),
                target: target.to_string(),
            })
            .into_iter()
            .collect())
    }
    fn apply_retarget(
        &self,
        from: &str,
        target: &str,
        moved: &[crate::record::Moved],
    ) -> anyhow::Result<()> {
        self.retargeted
            .lock()
            .unwrap()
            .push((from.to_string(), target.to_string()));
        if !moved.is_empty() {
            for owner in self
                .owners
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|o| *o == from)
            {
                *owner = target.to_string();
            }
        }
        Ok(())
    }
    fn restore_kept(&self, moved: &[crate::record::Moved]) -> anyhow::Result<()> {
        let mut owners = self.owners.lock().unwrap();
        for each in moved {
            for owner in owners.iter_mut().filter(|o| **o == each.target) {
                *owner = each.previous.clone().unwrap();
            }
        }
        self.restored.lock().unwrap().extend(moved.iter().cloned());
        Ok(())
    }
}

fn with_audio(notes: &[(&str, &str)]) -> (AppState, tempfile::TempDir, Vec<String>, Arc<Audio>) {
    let (mut state, dir, ids) = state_with(notes);
    let audio = Arc::new(Audio::default());
    state.powers.housekeeper = Some(audio.clone());
    (state, dir, ids, audio)
}

fn deleting(state: &AppState) -> std::collections::BTreeSet<String> {
    crate::margin::state_of(&state.chats, "deleting").unwrap_or_default()
}

fn combine(state: &AppState, into: &str, from: &str) -> Response {
    use crate::routes::notes::version_of;
    let (base, with_base) = {
        let store = state.fresh();
        (
            version_of(store.find_note(into).unwrap()),
            version_of(store.find_note(from).unwrap()),
        )
    };
    run(crate::routes::combine::keep(
        State(state.clone()),
        Path(into.to_string()),
        Json(
            serde_json::from_value(serde_json::json!({
                "with": from, "body": "Both notes, combined.", "base": base, "with_base": with_base
            }))
            .unwrap(),
        ),
    ))
}

#[cfg(unix)]
#[test]
fn kept_audio_leaves_with_its_note() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", ""), ("Gone", ""), ("Other", "")]);
    let gone_id = ids[2].clone();
    *audio.owners.lock().unwrap() = vec![ids[0].clone(), ids[1].clone(), gone_id.clone()];
    left_outside(&state, "Gone.md");
    trash(&state, &ids[0]);
    delete_for_good(&state, &[&ids[0]], false);
    assert_eq!(*audio.discarded.lock().unwrap(), [ids[0].clone()]);
    trash(&state, &ids[1]);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert!(
        !audio.discarded.lock().unwrap().contains(&ids[1]),
        "held first"
    );
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert!(
        audio.discarded.lock().unwrap().contains(&ids[1]),
        "then gone"
    );
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(gone_id.clone(), crate::margin::Missing::for_days(8));
    crate::margin::set_state(&state.chats, "missing", &missing);
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    let gone = crate::margin::sweep_store(
        &state.chats,
        kept,
        &crate::record::Housekeeper::kept_for_notes(&*audio),
        &data,
    );
    assert_eq!(gone, std::slice::from_ref(&gone_id));
    crate::dependents::clear(&state, gone, crate::dependents::Why::Swept);
    assert!(!audio.discarded.lock().unwrap().contains(&gone_id));
    let_days_pass(&state, 8);
    *audio.refused.lock().unwrap() = vec![gone_id.clone()];
    crate::dependents::clear(&state, Vec::new(), crate::dependents::Why::Swept);
    assert!(
        !audio.discarded.lock().unwrap().contains(&gone_id),
        "handed back, so tried again"
    );
    audio.refused.lock().unwrap().clear();
    crate::dependents::clear(&state, Vec::new(), crate::dependents::Why::Swept);
    assert!(audio.discarded.lock().unwrap().contains(&gone_id));
}

#[test]
fn combining_moves_kept_audio_to_the_remaining_note() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", "")]);
    *audio.owners.lock().unwrap() = vec![ids[1].clone()];
    assert_eq!(combine(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(
        *audio.retargeted.lock().unwrap(),
        [(ids[1].clone(), ids[0].clone())]
    );
    assert_eq!(*audio.owners.lock().unwrap(), [ids[0].clone()]);
    let undone = run(crate::routes::combine::uncombine(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({ "from": ids[1] })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    assert_eq!(audio.restored.lock().unwrap().len(), 1);
    assert_eq!(*audio.owners.lock().unwrap(), [ids[1].clone()]);
}

#[test]
fn a_failed_retarget_is_retried_before_the_absorbed_notes_audio_can_go() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", ""), ("Other", "")]);
    *audio.owners.lock().unwrap() = vec![ids[1].clone()];
    audio
        .locked
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(combine(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    delete_for_good(&state, &[&ids[1]], false);
    assert!(
        audio.discarded.lock().unwrap().is_empty(),
        "it has not followed A yet"
    );
    assert!(deleting(&state).contains(&ids[1]));
    audio
        .locked
        .store(false, std::sync::atomic::Ordering::Release);
    run(crate::keeping::expire_now(&state));
    assert_eq!(*audio.owners.lock().unwrap(), [ids[0].clone()]);
    assert!(audio.discarded.lock().unwrap().is_empty());
    trash(&state, &ids[0]);
    delete_for_good(&state, &[&ids[0]], false);
    assert_eq!(*audio.discarded.lock().unwrap(), [ids[0].clone()]);
}

fn unrecorded(state: &AppState, name: &str) -> std::path::PathBuf {
    let folder = state.notes_dir.join("attachments");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(name), b"png").unwrap();
    folder.join(name)
}

fn orphan(state: &AppState, name: &str) -> std::path::PathBuf {
    let path = picture(state, name);
    pass(state, 0);
    path
}

#[test]
fn a_picture_named_by_any_card_board_or_task_field_is_kept() {
    for (folder, text) in [
        (
            "cards",
            "{\"id\":\"cone1\",\"title\":\"see attachments/20261117%2D101500%2Dslide.png\"}"
                .to_string(),
        ),
        ("boards", format!("{{\"id\":\"bone1\",\"name\":\"{X}\"}}")),
        (
            "tasks",
            format!("{{\"id\":\"tone1\",\"description\":\"<img src=attachments/{X}>\"}}"),
        ),
    ] {
        let (state, _d, _ids) = state_with(&[("Other", "")]);
        let path = orphan(&state, X);
        let dir = state.notes_dir.join(".leo").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("one1.json"), text).unwrap();
        pass(&state, 400);
        assert!(path.exists(), "{folder}");
    }
}

#[test]
fn a_picture_a_chat_names_percent_encoded_is_kept() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    crate::chats::save(
        &state.chats,
        "chat-picture-2",
        crate::chats::Saving {
            title: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({
                "role": "assistant",
                "text": "![x](attachments/20261117%2D101500%2Dslide.png)",
            })],
            existed: false,
            from: None,
        },
        chrono::Utc::now(),
    )
    .unwrap();
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn a_picture_any_recording_file_names_is_kept() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, Y);
    let data = state.notes_dir.parent().unwrap();
    let session = data.join("recordings").join("20261117-101500");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("points.jsonl"),
        format!("{{\"text\":\"![s](attachments/{Y})\"}}\n"),
    )
    .unwrap();
    pass(&state, 400);
    assert!(path.exists());
    let sources = data.join("recording-sources").join("note-1");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::rename(session.join("points.jsonl"), sources.join("points.jsonl")).unwrap();
    pass(&state, 800);
    assert!(path.exists());
}

#[test]
fn a_picture_named_in_any_file_of_the_notes_folder_is_kept() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    let hidden = state.notes_dir.join(".obsidian");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(
        hidden.join("canvas.canvas"),
        format!("{{\"file\":\"attachments/{X}\"}}"),
    )
    .unwrap();
    pass(&state, 400);
    assert!(path.exists());
}

#[cfg(unix)]
#[test]
fn a_file_leo_cannot_read_stops_picture_deletion() {
    use std::os::unix::fs::PermissionsExt;
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    let cards = state.notes_dir.join(".leo/cards");
    std::fs::create_dir_all(&cards).unwrap();
    let card = cards.join("cone2.json");
    std::fs::write(&card, "{}").unwrap();
    std::fs::set_permissions(&card, std::fs::Permissions::from_mode(0o000)).unwrap();
    pass(&state, 400);
    std::fs::set_permissions(&card, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(path.exists());
    let session = state
        .notes_dir
        .parent()
        .unwrap()
        .join("recordings/20261117-101500");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o000)).unwrap();
    pass(&state, 800);
    std::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(path.exists());
}

#[cfg(unix)]
#[test]
fn an_unreadable_trash_folder_stops_every_deletion() {
    use std::os::unix::fs::PermissionsExt;
    let (state, _d, ids) = state_with(&[("Lecture", "course"), ("Other", "")]);
    let id = ids[0].clone();
    let path = picture(&state, X);
    write_body(&state, &id, &linking(X));
    hang_everything_on(&state, &id);
    trash(&state, &id);
    let folder = state.notes_dir.join(".trash/course");
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000)).unwrap();
    state.fresh().trash_may_have_changed();
    crate::dependents::clear(&state, vec![id.clone()], crate::dependents::Why::Asked);
    pass(&state, 400);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(held(&state, &id), [true; 7]);
    assert!(path.exists());
}

#[test]
fn a_picture_of_the_same_name_elsewhere_keeps_leos_and_is_never_touched() {
    let (state, _d, ids) = state_with(&[("A", "cs130")]);
    let root = orphan(&state, X);
    let nested = state.notes_dir.join("cs130/attachments");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join(X), b"png").unwrap();
    write_body(&state, &ids[0], &format!("![a](attachments/{X})"));
    pass(&state, 400);
    assert!(
        root.exists(),
        "the name is linked, so leo keeps its own copy too"
    );
    assert!(nested.join(X).exists());
    write_body(&state, &ids[0], "Words");
    pass(&state, 800);
    pass(&state, 1200);
    assert!(!root.exists());
    assert!(
        nested.join(X).exists(),
        "only the picture leo saved is ever removed"
    );
}

#[test]
fn a_clock_that_jumps_forward_deletes_no_picture() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    state.fresh().observe(later(0), None);
    state.fresh().observe(later(400), None);
    crate::dependents::pictures(&state, later(400));
    assert!(path.exists());
}

#[test]
fn a_keeping_file_leo_cannot_read_deletes_no_picture() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    std::fs::write(leo_core::keep::path_for(&state.notes_dir), "{broken").unwrap();
    trash(&state, &ids[0]);
    delete_for_good(&state, &[&ids[0]], false);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn a_picture_leo_did_not_record_saving_is_never_deleted() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = unrecorded(&state, X);
    pass(&state, 0);
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn a_note_whose_folder_went_away_keeps_its_dependents() {
    let (state, _d, ids) = state_with(&[("Lecture", "drive"), ("Other", "")]);
    let id = ids[0].clone();
    hang_everything_on(&state, &id);
    std::fs::rename(state.notes_dir.join("drive"), state.notes_dir.join(".away")).unwrap();
    drop(state.fresh());
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(id.clone(), crate::margin::Missing::for_days(8));
    crate::margin::set_state(&state.chats, "missing", &missing);
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    assert!(crate::margin::sweep_store(&state.chats, kept, &[], &data).is_empty());
    assert_eq!(held(&state, &id), [true; 7]);
}

#[test]
fn a_renamed_headerless_note_keeps_its_dependents() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    std::fs::write(state.notes_dir.join("Plain.md"), "no header\n").unwrap();
    let id = state
        .fresh()
        .notes
        .iter()
        .find(|n| n.title == "Plain")
        .unwrap()
        .id
        .clone();
    hang_everything_on(&state, &id);
    std::fs::rename(
        state.notes_dir.join("Plain.md"),
        state.notes_dir.join("Renamed.md"),
    )
    .unwrap();
    drop(state.fresh());
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(id.clone(), crate::margin::Missing::for_days(8));
    crate::margin::set_state(&state.chats, "missing", &missing);
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    assert!(crate::margin::sweep_store(&state.chats, kept, &[], &data).is_empty());
    assert_eq!(held(&state, &id), [true; 7]);
}

#[cfg(unix)]
#[test]
fn a_missing_clock_needs_time_leo_saw_pass() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = ids[0].clone();
    hang_everything_on(&state, &id);
    std::fs::remove_file(state.notes_dir.join("Lecture.md")).unwrap();
    drop(state.fresh());
    let jumped = crate::margin::Missing {
        since: later(-30),
        observed: 0,
        last: later(-30),
    };
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(id.clone(), jumped);
    crate::margin::set_state(&state.chats, "missing", &missing);
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    assert!(crate::margin::sweep_store(&state.chats, kept, &[], &data).is_empty());
}

#[test]
fn a_folder_leo_never_made_for_a_note_is_never_removed() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let photos = data.join("attachments").join("family-photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("beach.jpg"), b"jpg").unwrap();
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(
        "family-photos".to_string(),
        crate::margin::Missing::for_days(8),
    );
    crate::margin::set_state(&state.chats, "missing", &missing);
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    let gone = crate::margin::sweep_store(&state.chats, kept, &[], &data);
    assert!(gone.is_empty());
    crate::dependents::clear(&state, gone, crate::dependents::Why::Swept);
    assert!(photos.join("beach.jpg").exists());
}

#[test]
fn a_note_that_is_live_again_keeps_its_dependents_however_its_id_is_cased() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = ids[0].clone();
    hang_everything_on(&state, &id);
    crate::dependents::clear(&state, vec![id.clone()], crate::dependents::Why::Asked);
    crate::dependents::clear(
        &state,
        vec![id.to_uppercase()],
        crate::dependents::Why::Asked,
    );
    assert_eq!(held(&state, &id), [true; 7]);
}

#[cfg(unix)]
#[test]
fn a_linked_originals_folder_is_never_followed() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.pdf"), "mine").unwrap();
    let folder = crate::routes::uploads::originals_dir(&state.notes_dir, "gone-note").unwrap();
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside.path(), &folder).unwrap();
    crate::dependents::clear(
        &state,
        vec!["gone-note".into()],
        crate::dependents::Why::Asked,
    );
    assert!(outside.path().join("keep.pdf").exists());
}

#[test]
fn an_undone_combine_never_has_its_audio_moved_later() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", ""), ("Other", "")]);
    *audio.owners.lock().unwrap() = vec![ids[1].clone()];
    audio
        .locked
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(combine(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    let undone = run(crate::routes::combine::uncombine(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({ "from": ids[1] })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    audio
        .locked
        .store(false, std::sync::atomic::Ordering::Release);
    run(crate::keeping::expire_now(&state));
    assert!(audio.retargeted.lock().unwrap().is_empty());
    assert_eq!(*audio.owners.lock().unwrap(), [ids[1].clone()]);
}

#[test]
fn the_move_of_kept_audio_is_noted_before_the_source_is_trashed() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", "")]);
    *audio.owners.lock().unwrap() = vec![ids[1].clone()];
    let seen = Arc::new(Mutex::new(None));
    {
        let (seen, chats, pair) = (
            seen.clone(),
            state.chats.clone(),
            (ids[1].clone(), ids[0].clone()),
        );
        crate::routes::combine::AFTER_COPY.lock().unwrap().push((
            ids[0].clone(),
            Box::new(move || {
                *seen.lock().unwrap() =
                    Some(crate::routes::combine::pending_retargets(&chats).contains(&pair));
            }),
        ));
    }
    assert_eq!(combine(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(*seen.lock().unwrap(), Some(true));
    assert!(crate::routes::combine::pending_retargets(&state.chats).is_empty());
}

#[test]
fn a_link_written_outside_leo_brings_a_trashed_picture_back() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    pass(&state, 31);
    assert!(binned(&state, X));
    std::fs::write(
        state.notes_dir.join("Synced.md"),
        format!("---\nid: synced\ntitle: Synced\n---\n{}", linking(X)),
    )
    .unwrap();
    drop(state.fresh());
    assert!(path.exists() && !binned(&state, X));
}

#[test]
fn a_card_naming_a_trashed_picture_brings_it_back() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    pass(&state, 31);
    assert!(binned(&state, X));
    let cards = state.notes_dir.join(".leo/cards");
    std::fs::create_dir_all(&cards).unwrap();
    std::fs::write(
        cards.join("cone9.json"),
        format!("{{\"description\":\"{X}\"}}"),
    )
    .unwrap();
    pass(&state, 1);
    assert!(path.exists() && !binned(&state, X));
}

#[test]
fn a_chat_link_with_encoded_date_digits_keeps_the_picture() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    crate::chats::save(
        &state.chats,
        "chat-picture-3",
        crate::chats::Saving {
            title: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({
                "role": "assistant",
                "text": "![x](attachments/%32%30261117-101500-slide.png)",
            })],
            existed: false,
            from: None,
        },
        chrono::Utc::now(),
    )
    .unwrap();
    pass(&state, 400);
    assert!(path.exists());
}

#[test]
fn a_picture_replaced_by_another_file_is_never_leos() {
    let (state, _d, _ids) = state_with(&[("Other", "")]);
    let path = orphan(&state, X);
    std::fs::write(&path, b"a different picture someone put here").unwrap();
    pass(&state, 400);
    pass(&state, 400);
    assert!(path.exists() && !binned(&state, X));
}

#[test]
fn deleting_the_last_note_for_good_still_takes_its_picture() {
    let (state, _d, ids) = state_with(&[("Only", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    trash(&state, &ids[0]);
    assert!(state.fresh().notes.is_empty());
    delete_for_good(&state, &[&ids[0]], false);
    assert!(!path.exists() && !binned(&state, X));
}

#[test]
fn an_older_picture_no_note_uses_is_listed_and_goes_only_when_asked() {
    let (state, _d, ids) = state_with(&[("A", "")]);
    let old = unrecorded(&state, Y);
    let linked = unrecorded(&state, X);
    write_body(&state, &ids[0], &linking(X));
    pass(&state, 0);
    pass(&state, 4000);
    assert!(old.exists() && linked.exists(), "never automatically");
    let listed = run(list_trash(State(state.clone()))).unwrap().0;
    let unused: Vec<&str> = listed
        .iter()
        .filter(|row| row.kind == "unused")
        .map(|row| row.title.as_str())
        .collect();
    assert_eq!(unused, [Y]);
    let answer = delete_for_good(
        &state,
        &[&format!("unused:{Y}"), &format!("unused:{X}")],
        false,
    );
    assert_eq!(answer["deleted"], 1);
    assert!(!old.exists() && linked.exists());
}

#[test]
fn an_expired_notes_files_come_back_with_it_or_when_restored_from_the_trash() {
    let (state, _d, ids) = state_with(&[("Lecture", ""), ("Second", ""), ("Other", "")]);
    for id in &ids[..2] {
        hang_everything_on(&state, id);
    }
    let text = std::fs::read_to_string(state.notes_dir.join("Lecture.md")).unwrap();
    trash(&state, &ids[0]);
    trash(&state, &ids[1]);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert!(held_folder(&state, "attachments", &ids[0]).exists());
    std::fs::write(state.notes_dir.join("Lecture.md"), text).unwrap();
    drop(state.fresh());
    crate::dependents::clear(&state, Vec::new(), crate::dependents::Why::Expired);
    assert_eq!(
        held(&state, &ids[0]),
        [true; 7],
        "the note came back, so did its files"
    );
    assert_eq!(
        restore(&state, &[&format!("held:{}", ids[1])])["restored"],
        1
    );
    let_days_pass(&state, 400);
    run(crate::keeping::expire_now(&state));
    assert!(
        crate::routes::uploads::originals_dir(&state.notes_dir, &ids[1])
            .unwrap()
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn a_swept_note_is_checked_again_before_its_files_move() {
    let (state, _d, ids) = state_with(&[("A", "course"), ("B", "course"), ("Other", "")]);
    let id = ids[0].clone();
    hang_everything_on(&state, &id);
    left_outside(&state, "course/A.md");
    let mut missing = std::collections::BTreeMap::new();
    missing.insert(id.clone(), crate::margin::Missing::for_days(8));
    crate::margin::set_state(&state.chats, "missing", &missing);
    let data = state.notes_dir.parent().unwrap().to_path_buf();
    let kept = crate::margin::sweep_kept(&state.fresh()).unwrap();
    let gone = crate::margin::sweep_store(&state.chats, kept, &[], &data);
    assert_eq!(gone, std::slice::from_ref(&id));
    left_outside(&state, "course/B.md");
    crate::dependents::clear(&state, gone, crate::dependents::Why::Swept);
    assert_eq!(held(&state, &id), [true; 7]);
}

#[test]
fn keeping_never_moves_audio_for_a_combine_that_has_not_saved() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", "")]);
    *audio.owners.lock().unwrap() = vec![ids[1].clone()];
    let before_save = Arc::new(Mutex::new(None));
    {
        let (early, seen, moved) = (state.clone(), before_save.clone(), audio.clone());
        crate::routes::combine::AFTER_COPY.lock().unwrap().push((
            ids[0].clone(),
            Box::new(move || {
                crate::routes::combine::retry_retargets(&early);
                *seen.lock().unwrap() = Some(moved.retargeted.lock().unwrap().len());
            }),
        ));
    }
    assert_eq!(combine(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(
        *before_save.lock().unwrap(),
        Some(0),
        "nothing moved before the save"
    );
    assert_eq!(
        audio.retargeted.lock().unwrap().len(),
        1,
        "moved once, after the save"
    );
}

#[test]
fn moved_sessions_are_kept_before_the_noted_move_is_dropped() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", ""), ("Other", "")]);
    *audio.owners.lock().unwrap() = vec![ids[1].clone()];
    audio
        .locked
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(combine(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    audio
        .locked
        .store(false, std::sync::atomic::Ordering::Release);
    crate::routes::combine::retry_retargets(&state);
    assert_eq!(*audio.owners.lock().unwrap(), [ids[0].clone()]);
    let undone = run(crate::routes::combine::uncombine(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({ "from": ids[1] })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    assert_eq!(*audio.owners.lock().unwrap(), [ids[1].clone()]);
}

#[test]
fn a_replaced_binned_picture_keeps_its_bytes_for_both_delete_paths() {
    for asked in [false, true] {
        let (state, _dir, _) = state_with(&[]);
        picture(&state, X);
        pass(&state, 0);
        pass(&state, 31);
        assert!(binned(&state, X));
        let path = state.notes_dir.join(".trash/attachments").join(X);
        std::fs::write(&path, "replacement").unwrap();
        if asked {
            assert_eq!(
                refused_for_good(&state, &[&format!("picture:{X}")]),
                StatusCode::CONFLICT
            );
            assert_eq!(
                delete_for_good(&state, &[&format!("picture:{X}")], false)["failed"],
                1
            );
            delete_for_good(&state, &[], true);
        } else {
            pass(&state, 31);
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), "replacement");
    }
}

#[test]
fn note_task_descriptions_and_held_transcripts_own_pictures() {
    let (state, _dir, ids) = state_with(&[("A", "")]);
    let x = picture(&state, X);
    let y = picture(&state, Y);
    crate::db::for_chats(&state.chats).with(|c| {
        c.execute(
            "INSERT INTO note_tasks (id, note, text, before, after, promoted, description) VALUES ('task', ?1, '', '', '', 1, ?2)",
            rusqlite::params![ids[0], linking(X)],
        )
    }).unwrap();
    let held = held_folder(&state, "recording-sources", &ids[0]);
    std::fs::create_dir_all(&held).unwrap();
    std::fs::write(held.join("transcript.txt"), linking(Y)).unwrap();
    pass(&state, 0);
    pass(&state, 31);
    assert!(x.exists() && y.exists());
}

#[test]
fn a_held_note_never_purges_fresh_home_folders() {
    for live in [false, true] {
        let (state, _dir, ids) = state_with(&[("A", ""), ("Other", "")]);
        let id = &ids[0];
        hang_everything_on(&state, id);
        let note = state.notes_dir.join("A.md");
        let text = std::fs::read_to_string(&note).unwrap();
        trash(&state, id);
        keep_trash(&state, Some(7));
        let_days_pass(&state, 8);
        run(crate::keeping::expire_now(&state));
        let home = crate::routes::uploads::originals_dir(&state.notes_dir, id).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let fresh = home.join("fresh.pdf");
        std::fs::write(&fresh, "fresh original").unwrap();
        if live {
            std::fs::write(note, text).unwrap();
        }
        let_days_pass(&state, 8);
        run(crate::keeping::expire_now(&state));
        assert_eq!(std::fs::read_to_string(fresh).unwrap(), "fresh original");
        if live {
            assert!(
                !held_folder(&state, "attachments", id).exists(),
                "the old files find a conflict-safe home"
            );
        }
    }
}

#[test]
fn a_saved_combine_reconciles_audio_even_if_committed_flag_was_not_written() {
    let (state, _d, ids, audio) = with_audio(&[("A", ""), ("B", "")]);
    let (target, source) = (&ids[0], &ids[1]);
    trash(&state, source);
    *audio.owners.lock().unwrap() = vec![target.clone()];
    let moved = serde_json::json!([{
        "session": "20261117-101500",
        "previous": source,
        "target": target,
    }]);
    let archive = serde_json::json!({
        "from": source,
        "originals": [],
        "at": chrono::Utc::now(),
        "kept": [],
    });
    assert!(crate::margin::set_state(
        &state.chats,
        &format!("combined:{target}"),
        &archive
    ));
    assert!(crate::margin::set_state(
        &state.chats,
        "retarget",
        &serde_json::json!([{
            "from": source, "target": target, "committed": false, "moved": moved
        }])
    ));
    crate::routes::combine::retry_retargets(&state);
    assert!(crate::routes::combine::pending_retargets(&state.chats).is_empty());
    let undone = run(crate::routes::combine::uncombine(
        State(state.clone()),
        Path(target.clone()),
        Json(serde_json::from_value(serde_json::json!({ "from": source })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        audio.owners.lock().unwrap().as_slice(),
        std::slice::from_ref(source)
    );
}

#[test]
fn after_an_interrupted_hold_an_unrecorded_held_folder_stays_visible() {
    let (state, _d, _) = state_with(&[("Other", "")]);
    let id = "removed-note";
    let folder = held_folder(&state, "attachments", id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("original.pdf"), "original").unwrap();
    crate::dependents::recover_holds(&state);
    let listed = run(list_trash(State(state.clone()))).unwrap().0;
    assert!(listed.iter().any(|row| row.id == format!("held:{id}")));
}

#[test]
fn a_failed_held_restore_reports_the_remaining_item() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    hang_everything_on(&state, &ids[0]);
    trash(&state, &ids[0]);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    let path = held_folder(&state, "recording-sources", &ids[0]);
    std::fs::remove_dir_all(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("replacement.txt"), "keep").unwrap();
    let answer = restore(&state, &[&format!("held:{}", ids[0])]);
    assert_eq!(answer["restored"], 0);
    assert_eq!(answer["files_failed"], 1);
    assert!(path.join("replacement.txt").exists());
}

#[test]
fn a_reused_note_id_does_not_restore_a_previous_notes_hold() {
    let (state, _d, ids) = state_with(&[("A", ""), ("Other", "")]);
    hang_everything_on(&state, &ids[0]);
    let note = state.notes_dir.join("A.md");
    let old = std::fs::read_to_string(&note).unwrap();
    trash(&state, &ids[0]);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    let fresh: String = old
        .lines()
        .map(|line| {
            if line.starts_with("created_at:") {
                "created_at: 2020-01-01T00:00:00Z"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(note, fresh).unwrap();
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert!(held_folder(&state, "attachments", &ids[0]).exists());
    assert!(
        !crate::routes::uploads::originals_dir(&state.notes_dir, &ids[0])
            .unwrap()
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_cannot_block_a_picture_scan() {
    let (state, _d, _) = state_with(&[]);
    let path = picture(&state, X);
    pass(&state, 0);
    let fifo = state.notes_dir.join("external");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let (done, received) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        pass(&state, 31);
        done.send(()).unwrap();
    });
    let checked = received.recv_timeout(Duration::from_secs(3));
    if checked.is_err() {
        let _ = std::fs::OpenOptions::new().write(true).open(&fifo);
    }
    worker.join().unwrap();
    checked.unwrap();
    assert!(path.exists());
}

#[test]
fn a_picture_named_after_the_scan_survives_each_destructive_step() {
    for owner in ["card", "task", "chat", "recording"] {
        for step in ["bin", "expire", "delete"] {
            let (state, _d, ids) = state_with(&[("A", "")]);
            let path = picture(&state, X);
            pass(&state, 0);
            let reference = match owner {
                "card" => state.notes_dir.join(".leo/cards/changed.json"),
                "recording" => state
                    .notes_dir
                    .parent()
                    .unwrap()
                    .join("recordings/changed.txt"),
                _ => state.notes_dir.join(".leo/tasks/changed.json"),
            };
            std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
            std::fs::write(&reference, "no picture").unwrap();
            if step != "bin" {
                pass(&state, 31);
                assert!(binned(&state, X));
            }
            crate::dependents::AFTER_SCAN.lock().unwrap().push((
                state.notes_dir.clone(),
                Box::new(move |state| {
                    match owner {
                        "chat" => {
                            chats::save(
                                &state.chats,
                                "chat-after-scan",
                                chats::Saving {
                                    existed: false,
                                    from: None,
                                    title: String::new(),
                                    refs: vec![],
                                    messages: vec![serde_json::json!({"role": "user", "text": linking(X)})],
                                },
                                chrono::Utc::now(),
                            ).unwrap();
                        }
                        "task" => {
                            crate::db::for_chats(&state.chats).with(|c| c.execute(
                                "INSERT INTO note_tasks (id, note, text, before, after, promoted, description) VALUES ('raced', ?1, '', '', '', 1, ?2)",
                                rusqlite::params![ids[0], linking(X)],
                            )).unwrap();
                        }
                        _ => std::fs::write(&reference, linking(X)).unwrap(),
                    }
                }),
            ));
            if step == "delete" {
                let answer = delete_for_good(&state, &[&format!("picture:{X}")], false);
                assert_eq!(answer["failed"], 1, "{owner}");
            } else {
                pass(&state, 31);
            }
            assert!(path.exists() || binned(&state, X), "{owner}, {step}");
            if step == "bin" {
                assert!(path.exists(), "{owner} kept it at home");
            }
        }
    }
}

#[test]
fn a_replacement_at_home_is_rechecked_at_the_move() {
    let (state, _d, _) = state_with(&[]);
    let path = picture(&state, X);
    pass(&state, 0);
    let replacement = path.clone();
    crate::dependents::AFTER_SCAN.lock().unwrap().push((
        state.notes_dir.clone(),
        Box::new(move |_| {
            std::fs::write(replacement, "replacement").unwrap();
        }),
    ));
    pass(&state, 31);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "replacement");
    assert!(!binned(&state, X));
}

#[test]
fn automatic_cleanup_waits_for_idle_and_a_capture_to_finish() {
    let (mut state, _dir, _) = state_with(&[]);
    assert!(!crate::cleanup_idle(&state));
    Arc::get_mut(&mut state.activity).unwrap().born -= crate::DEPENDENTS_IDLE;
    assert!(crate::cleanup_idle(&state));
    let capture = state.capture_activity.begin().unwrap();
    assert!(!crate::cleanup_idle(&state));
    drop(capture);
    assert!(crate::cleanup_idle(&state));
    state.activity.touch();
    assert!(!crate::cleanup_idle(&state));
}

#[test]
fn picture_restore_keeps_a_case_only_conflict() {
    let (state, _d, _) = state_with(&[]);
    let original = picture(&state, X);
    let bytes = std::fs::read(&original).unwrap();
    pass(&state, 0);
    pass(&state, 31);
    let conflict = original.with_file_name(X.to_ascii_uppercase());
    std::fs::write(&conflict, "other picture").unwrap();
    let answer = restore(&state, &[&format!("picture:{X}")]);
    assert_eq!(answer["restored"], 1);
    assert_eq!(std::fs::read(conflict).unwrap(), b"other picture");
    assert!(std::fs::read_dir(original.parent().unwrap())
        .unwrap()
        .flatten()
        .any(|entry| {
            entry.file_name().to_string_lossy().contains("-restored-")
                && std::fs::read(entry.path()).unwrap() == bytes
        }));
}

#[test]
fn a_hold_preserves_an_in_place_edit_even_with_its_timestamp_restored() {
    let (state, _dir, ids) = state_with(&[("A", "")]);
    let id = &ids[0];
    hang_everything_on(&state, id);
    trash(&state, id);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    let file = held_folder(&state, "recording-sources", id).join("passages.json");
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::fs::write(&file, "{}").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "{}");
}

#[test]
fn a_hold_cannot_forget_rows_written_by_a_replacement_note() {
    let (state, _dir, ids) = state_with(&[("A", ""), ("Other", "")]);
    let id = &ids[0];
    hang_everything_on(&state, id);
    trash(&state, id);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));

    let replacement = state.notes_dir.join("Replacement.md");
    std::fs::write(
        &replacement,
        format!("---\nid: {id}\ncreated_at: 2024-01-01T00:00:00Z\n---\nnew note\n"),
    )
    .unwrap();
    assert!(state.fresh().find_note(id).is_some());
    let thread = crate::margin::new_thread(
        id,
        "read",
        crate::margin::Place {
            prefix: String::new(),
            suffix: String::new(),
        },
        "replacement",
        Vec::new(),
    );
    assert!(crate::margin::put(
        &state.chats,
        &thread,
        crate::margin::MOST_OPEN
    ));
    crate::tasks::put_meta(
        &state.chats,
        &crate::tasks::Meta {
            id: format!("task-{id}"),
            note: id.clone(),
            text: "replacement task".into(),
            promoted: true,
            ..Default::default()
        },
    )
    .unwrap();
    std::fs::remove_file(replacement).unwrap();
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));

    assert_eq!(
        rows(
            &state,
            "SELECT count(*) FROM margin_threads WHERE note = ?1",
            id
        ),
        1
    );
    assert_eq!(
        rows(&state, "SELECT count(*) FROM graph_notes WHERE id = ?1", id),
        0
    );
    assert!(crate::margin::every(&state.chats)
        .iter()
        .any(|row| row.id == thread.id));
    assert!(crate::tasks::metas(&state.chats)
        .iter()
        .any(|row| row.text == "replacement task"));
}

#[test]
fn a_binned_picture_replacement_cannot_inherit_age_by_copying_its_stamp() {
    for asked in [false, true] {
        let (state, _dir, _) = state_with(&[]);
        picture(&state, X);
        pass(&state, 0);
        pass(&state, 31);
        let path = state.notes_dir.join(".trash/attachments").join(X);
        let meta = std::fs::metadata(&path).unwrap();
        let bytes = vec![b'z'; meta.len() as usize];
        std::fs::write(&path, &bytes).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(meta.modified().unwrap())
            .unwrap();
        if asked {
            assert_eq!(
                refused_for_good(&state, &[&format!("picture:{X}")]),
                StatusCode::CONFLICT
            );
        } else {
            pass(&state, 31);
        }
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn a_saved_margin_reply_and_note_task_text_keep_their_pictures() {
    let (state, _dir, ids) = state_with(&[("A", "")]);
    let x = picture(&state, X);
    let y = picture(&state, Y);
    let thread = crate::margin::new_thread(
        &ids[0],
        "read",
        crate::margin::Place {
            prefix: String::new(),
            suffix: String::new(),
        },
        "margin",
        Vec::new(),
    );
    assert!(crate::margin::put(
        &state.chats,
        &thread,
        crate::margin::MOST_OPEN
    ));
    crate::db::for_chats(&state.chats)
        .with(|c| {
            c.execute(
                "UPDATE margin_threads SET messages = ?1 WHERE id = ?2",
                rusqlite::params![
                    serde_json::json!([{"role": "assistant", "text": linking(X)}]).to_string(),
                    thread.id
                ],
            )
        })
        .unwrap();
    crate::tasks::put_meta(
        &state.chats,
        &crate::tasks::Meta {
            id: "a-task".into(),
            note: ids[0].clone(),
            text: linking(Y),
            promoted: true,
            ..Default::default()
        },
    )
    .unwrap();
    pass(&state, 0);
    pass(&state, 31);
    assert!(x.exists() && y.exists());
}

#[test]
fn an_edited_combine_target_keeps_the_ambiguous_intent_and_its_audio() {
    let (state, _dir, ids, audio) = with_audio(&[("A", ""), ("B", "")]);
    let (target, source) = (&ids[0], &ids[1]);
    *audio.owners.lock().unwrap() = vec![source.clone()];
    trash(&state, source);
    write_body(&state, target, "An outside edit");
    let record =
        serde_json::json!({"from": source, "originals": [], "at": chrono::Utc::now(), "kept": []});
    assert!(crate::margin::set_state(
        &state.chats,
        "retarget",
        &serde_json::json!([{
            "from": source, "target": target, "committed": false,
            "record": record, "version": "saved-version"
        }])
    ));
    crate::routes::combine::reconcile_retargets(&state, true);
    delete_for_good(&state, &[source], false);
    assert!(crate::routes::combine::pending_retargets(&state.chats)
        .contains(&(source.clone(), target.clone())));
    assert!(audio.discarded.lock().unwrap().is_empty());
}

#[test]
fn the_recording_inventory_for_a_hold_runs_without_the_notes_lock() {
    struct Checked(Arc<Mutex<crate::Storage>>);
    impl crate::record::Housekeeper for Checked {
        fn kept_sessions(&self, _id: &str) -> anyhow::Result<Vec<crate::record::KeptSession>> {
            assert!(self.0.try_lock().is_ok());
            Ok(Vec::new())
        }
    }
    let (mut state, _dir, ids) = state_with(&[("A", "")]);
    state.housekeeper = Some(Arc::new(Checked(state.store.clone())));
    trash(&state, &ids[0]);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    run(crate::keeping::expire_now(&state));
    assert!(crate::dependents::held_rows(&state.chats).contains_key(&ids[0]));
}

#[test]
fn another_librarys_audio_work_does_not_defer_a_hold() {
    let (busy, _busy_dir, _) = state_with(&[]);
    let (state, _dir, ids) = state_with(&[("A", "")]);
    hang_everything_on(&state, &ids[0]);
    trash(&state, &ids[0]);
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    let _working = crate::routes::combine::audio(&busy);
    crate::keeping::expire_dependents(&state);
    assert!(held_folder(&state, "attachments", &ids[0]).exists());
}

#[test]
fn old_trash_expiry_cannot_enrol_a_seen_replacement() {
    let (state, _dir, ids) = state_with(&[("A", ""), ("Other", "")]);
    let id = &ids[0];
    trash(&state, id);
    let replacement = state.notes_dir.join("Replacement.md");
    std::fs::write(
        &replacement,
        format!("---\nid: {id}\ncreated_at: 2024-01-01T00:00:00Z\n---\nnew note\n"),
    )
    .unwrap();
    assert!(state.fresh().find_note(id).is_some());
    let owned = hang_everything_on(&state, id);
    std::fs::remove_file(&replacement).unwrap();
    drop(state.fresh());
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    crate::keeping::expire_dependents(&state);
    assert!(owned.originals.exists() && owned.sources.exists());
    assert!(!held_folder(&state, "attachments", id).exists());
}

#[test]
fn a_same_stamp_home_replacement_is_not_the_picture_leo_saved() {
    let (state, _dir, _) = state_with(&[]);
    let path = picture(&state, X);
    pass(&state, 0);
    let meta = std::fs::metadata(&path).unwrap();
    let replacement = vec![b'z'; meta.len() as usize];
    std::fs::write(&path, &replacement).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(meta.modified().unwrap())
        .unwrap();
    pass(&state, 31);
    assert_eq!(std::fs::read(path).unwrap(), replacement);
    assert!(!binned(&state, X));
}

#[test]
fn the_picture_pass_reads_live_markdown_even_when_its_fingerprint_matches() {
    let (state, _dir, ids) = state_with(&[("A", "")]);
    let picture = picture(&state, X);
    let link = linking(X);
    let padding = "x".repeat(link.len());
    write_body(&state, &ids[0], &padding);
    pass(&state, 0);
    let note = state.notes_dir.join("A.md");
    let original = std::fs::read_to_string(&note).unwrap();
    let modified = std::fs::metadata(&note).unwrap().modified().unwrap();
    let replaced = original.replace(&padding, &link);
    assert_ne!(original, replaced);
    assert_eq!(original.len(), replaced.len());
    std::fs::write(&note, replaced).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&note)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    pass(&state, 31);
    assert!(picture.exists());
}

#[test]
fn an_ambiguous_combine_protects_a_held_targets_audio_with_forever_audio_retention() {
    let (state, _dir, ids, audio) = with_audio(&[("A", ""), ("B", "")]);
    let (target, source) = (&ids[0], &ids[1]);
    *audio.owners.lock().unwrap() = vec![target.clone()];
    trash(&state, target);
    trash(&state, source);
    assert!(crate::margin::set_state(
        &state.chats,
        "retarget",
        &serde_json::json!([{
            "from": source, "target": target, "committed": false, "version": "ambiguous"
        }])
    ));
    delete_for_good(&state, &[target], false);
    assert!(audio.discarded.lock().unwrap().is_empty());
}

fn replace_keeping_stamp(path: &std::path::Path) -> Vec<u8> {
    let meta = std::fs::metadata(path).unwrap();
    let bytes = vec![b'q'; meta.len() as usize];
    std::fs::write(path, &bytes).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(meta.modified().unwrap())
        .unwrap();
    bytes
}

#[test]
fn an_unused_picture_replaced_after_it_was_shown_is_kept() {
    let (state, _d, _) = state_with(&[("A", "")]);
    let old = unrecorded(&state, Y);
    pass(&state, 0);
    let displayed = shown(&state);
    assert!(displayed.contains_key(&format!("unused:{Y}")));
    let bytes = replace_keeping_stamp(&old);
    let refused = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![format!("unused:{Y}")],
            revisions: displayed,
            ..Default::default()
        }),
    ))
    .unwrap_err();
    assert_eq!(refused.0, StatusCode::CONFLICT);
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    assert_eq!(
        delete_for_good(&state, &[&format!("unused:{Y}")], false)["deleted"],
        1
    );
    assert!(!old.exists());
}

#[test]
fn a_released_picture_whose_bytes_changed_is_kept() {
    let (state, _d, ids) = state_with(&[("Only", "")]);
    let path = picture(&state, X);
    write_body(&state, &ids[0], &linking(X));
    trash(&state, &ids[0]);
    let bytes = replace_keeping_stamp(&path);
    delete_for_good(&state, &[&ids[0]], false);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

fn held_and_due(state: &AppState, id: &str) -> std::path::PathBuf {
    hang_everything_on(state, id);
    trash(state, id);
    keep_trash(state, Some(7));
    let_days_pass(state, 8);
    crate::keeping::expire_dependents(state);
    let held = held_folder(state, "attachments", id);
    assert!(held.join("slides.pdf").exists());
    let_days_pass(state, 8);
    held
}

fn before_removing(state: &AppState, hook: impl FnOnce(&AppState) + Send + 'static) {
    crate::dependents::BEFORE_REMOVING
        .lock()
        .unwrap()
        .push((state.notes_dir.clone(), Box::new(hook)));
}

#[test]
fn a_note_synced_back_while_its_hold_is_checked_keeps_its_files() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let text = std::fs::read_to_string(state.notes_dir.join("Lecture.md")).unwrap();
    let held = held_and_due(&state, &ids[0]);
    before_removing(&state, move |state| {
        std::fs::write(state.notes_dir.join("Lecture.md"), text).unwrap();
    });
    crate::keeping::expire_dependents(&state);
    assert!(held.join("slides.pdf").exists());
}

#[test]
fn a_held_file_replaced_after_it_was_hashed_is_kept() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let held = held_and_due(&state, &ids[0]);
    let file = held.join("slides.pdf");
    let replaced = file.clone();
    before_removing(&state, move |_| {
        std::fs::write(&replaced, "%PDX").unwrap();
    });
    crate::keeping::expire_dependents(&state);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "%PDX");
}

#[test]
fn an_unchanged_due_hold_still_goes() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let held = held_and_due(&state, &ids[0]);
    crate::keeping::expire_dependents(&state);
    assert!(!held.exists());
}

#[test]
fn another_librarys_task_writer_does_not_hold_up_the_picture_pass() {
    let (busy, _busy_dir, _) = state_with(&[]);
    let (state, _dir, _) = state_with(&[]);
    let _boards = crate::boards::writing(&busy.chats);
    let _tasks = crate::tasks::writing(&busy.chats);
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        crate::dependents::pictures(&state, chrono::Utc::now());
        let _ = done.send(());
    });
    assert!(finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .is_ok());
}

#[test]
fn a_hold_that_appears_after_the_trash_was_shown_is_kept_by_empty_trash() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let displayed = shown(&state);
    let held = held_and_due(&state, &ids[0]);
    let done = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            all: true,
            revisions: displayed,
            ..Default::default()
        }),
    ))
    .unwrap()
    .0;
    assert_eq!(done["deleted"], 0);
    assert!(held.join("slides.pdf").exists());
    delete_for_good(&state, &[], true);
    assert!(!held.exists());
}

#[test]
fn a_hold_without_its_shown_revision_answers_conflict() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let held = held_and_due(&state, &ids[0]);
    let mut displayed = shown(&state);
    let key = format!("held:{}", ids[0]);
    assert!(displayed.contains_key(&key));
    displayed.insert(key.clone(), "00".repeat(32));
    let refused = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![key],
            revisions: displayed,
            ..Default::default()
        }),
    ))
    .unwrap_err();
    assert_eq!(refused.0, StatusCode::CONFLICT);
    assert!(held.join("slides.pdf").exists());
}

#[test]
fn a_held_row_changed_behind_its_trigger_is_kept() {
    let (state, _dir, ids) = state_with(&[("A", "")]);
    let id = &ids[0];
    hang_everything_on(&state, id);
    let ticket = crate::margin::record_held_rows(&state.chats, id).unwrap().0;
    crate::db::for_chats(&state.chats)
        .with(|c| {
            c.execute_batch("DROP TRIGGER held_reading_UPDATE")?;
            c.execute("UPDATE reading SET anchor = 'b' WHERE note = ?1", [id])
        })
        .unwrap();
    assert!(crate::margin::finish_held_rows(
        &state.chats,
        id,
        &ticket,
        true
    ));
    assert_eq!(
        rows(&state, "SELECT count(*) FROM reading WHERE note = ?1", id),
        1
    );
    assert_eq!(
        rows(&state, "SELECT count(*) FROM graph_notes WHERE id = ?1", id),
        0
    );
}

#[test]
fn deleting_forty_notes_for_good_takes_all_their_files_at_once() {
    let titles: Vec<String> = (0..40).map(|n| format!("Note {n:02}")).collect();
    let notes: Vec<(&str, &str)> = titles.iter().map(|t| (t.as_str(), "")).collect();
    let (state, _dir, ids) = state_with(&notes);
    let folders: Vec<_> = ids
        .iter()
        .map(|id| {
            hang_everything_on(&state, id);
            trash(&state, id);
            crate::routes::uploads::originals_dir(&state.notes_dir, id).unwrap()
        })
        .collect();
    let chosen: Vec<&str> = ids.iter().map(String::as_str).collect();
    delete_for_good(&state, &chosen, false);
    assert!(folders.iter().all(|folder| !folder.exists()));
}

#[test]
fn a_trashed_copy_synced_back_while_its_hold_is_checked_keeps_its_files() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let text = std::fs::read_to_string(state.notes_dir.join("Lecture.md")).unwrap();
    let held = held_and_due(&state, &ids[0]);
    before_removing(&state, move |state| {
        std::fs::create_dir_all(state.notes_dir.join(".trash")).unwrap();
        std::fs::write(state.notes_dir.join(".trash/Lecture.md"), text).unwrap();
    });
    crate::keeping::expire_dependents(&state);
    assert!(held.join("slides.pdf").exists());
}

#[test]
fn a_deferred_delete_for_good_purges_only_what_was_there_when_asked() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = &ids[0];
    let owned = hang_everything_on(&state, id);
    trash(&state, id);
    let capture = state.capture_activity.begin().unwrap();
    delete_for_good(&state, &[id], false);
    assert!(
        owned.originals.join("slides.pdf").exists(),
        "deferred while recording"
    );
    std::fs::write(owned.originals.join("synced.pdf"), "%PDF arrived later").unwrap();
    drop(capture);
    crate::keeping::expire_dependents(&state);
    assert!(!owned.sources.exists(), "what was shown goes at once");
    let held = held_folder(&state, "attachments", id);
    assert!(
        held.join("synced.pdf").exists(),
        "a file that arrived later waits its own trash period"
    );
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    crate::keeping::expire_dependents(&state);
    assert!(!held.exists());
}

#[test]
fn deferred_delete_holds_audio_and_rows_that_arrived_after_confirmation() {
    let (mut state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = &ids[0];
    let audio = std::sync::Arc::new(Audio::default());
    state.housekeeper = Some(audio.clone());
    trash(&state, id);
    let capture = state.capture_activity.begin().unwrap();
    delete_for_good(&state, &[id], false);
    audio.owners.lock().unwrap().push(id.clone());
    crate::db::for_chats(&state.chats)
        .with(|c| {
            c.execute(
                "INSERT INTO reading (note, anchor, at) VALUES (?1, 'a', 't')",
                [id],
            )
        })
        .unwrap();
    drop(capture);
    crate::keeping::expire_dependents(&state);
    assert!(audio.discarded.lock().unwrap().is_empty());
    assert_eq!(
        rows(&state, "SELECT count(*) FROM reading WHERE note = ?1", id),
        1
    );
    keep_trash(&state, Some(7));
    let_days_pass(&state, 8);
    crate::keeping::expire_dependents(&state);
    assert_eq!(*audio.discarded.lock().unwrap(), [id.clone()]);
    assert_eq!(
        rows(&state, "SELECT count(*) FROM reading WHERE note = ?1", id),
        0
    );
}

#[test]
fn a_note_synced_back_during_an_earlier_purge_keeps_its_hold() {
    let (mut state, _dir, ids) = state_with(&[("First", ""), ("Second", "")]);
    let audio = std::sync::Arc::new(Audio::default());
    state.housekeeper = Some(audio.clone());
    *audio.owners.lock().unwrap() = ids.clone();
    let later = if ids[0] < ids[1] { 1 } else { 0 };
    let earlier = 1 - later;
    let live = state.notes_dir.join(["First.md", "Second.md"][later]);
    let original = std::fs::read(&live).unwrap();
    for id in &ids {
        hang_everything_on(&state, id);
        trash(&state, id);
    }
    *audio.after_discard.lock().unwrap() =
        Some(Box::new(move || std::fs::write(live, original).unwrap()));
    delete_for_good(&state, &[&ids[0], &ids[1]], false);
    assert_eq!(*audio.discarded.lock().unwrap(), [ids[earlier].clone()]);
    assert_eq!(held(&state, &ids[later])[2..], [true; 5]);
}

#[test]
fn a_row_rewritten_after_confirmation_gets_its_own_hold() {
    let (state, _dir, ids) = state_with(&[("Lecture", ""), ("Other", "")]);
    let id = &ids[0];
    hang_everything_on(&state, id);
    trash(&state, id);
    let confirmed = crate::dependents::confirm_removal(&state, [id.clone()].into());
    let removed = state.fresh().delete_from_trash(std::slice::from_ref(id));
    assert_eq!(removed.forgotten, [id.clone()]);
    crate::db::for_chats(&state.chats)
        .with(|c| c.execute("UPDATE reading SET anchor = 'later' WHERE note = ?1", [id]))
        .unwrap();
    crate::dependents::clear_shown(
        &state,
        removed.forgotten,
        crate::dependents::Why::Asked,
        None,
        &confirmed,
    );
    assert_eq!(
        rows(&state, "SELECT count(*) FROM reading WHERE note = ?1", id),
        1
    );
    assert!(crate::dependents::held_rows(&state.chats).contains_key(id));
}
