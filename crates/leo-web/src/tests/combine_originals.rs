use super::*;
use crate::routes::combine;
use crate::routes::notes::version_of;
use crate::routes::uploads::originals_dir;

fn original(state: &AppState, note: &str, name: &str, text: &str) {
    let folder = originals_dir(&state.notes_dir, note).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(name), text).unwrap();
}

fn names(state: &AppState, note: &str) -> Vec<String> {
    crate::originals::files(&state.notes_dir, note)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn combined(state: &AppState, into: &str, from: &str) -> Response {
    let (base, with_base) = {
        let store = state.fresh();
        (
            version_of(store.find_note(into).unwrap()),
            version_of(store.find_note(from).unwrap()),
        )
    };
    run(combine::keep(
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

fn undo(state: &AppState, into: &str, from: &str) -> Response {
    run(combine::uncombine(
        State(state.clone()),
        Path(into.to_string()),
        Json(serde_json::from_value(serde_json::json!({ "from": from })).unwrap()),
    ))
}

#[test]
fn combining_carries_the_absorbed_notes_original_files() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "%PDF slides");
    let saved = combined(&state, &ids[0], &ids[1]);
    assert_eq!(saved.status(), StatusCode::OK);
    let answer = json_of(saved);
    assert!(answer.get("originals").is_none(), "{answer}");
    assert_eq!(names(&state, &ids[0]), ["slides.pdf"]);
    assert_eq!(names(&state, &ids[1]), ["slides.pdf"]);
    let kept = originals_dir(&state.notes_dir, &ids[0])
        .unwrap()
        .join("slides.pdf");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "%PDF slides");
}

#[test]
fn a_carried_original_whose_name_is_taken_gets_a_number() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[0], "slides.pdf", "mine");
    original(&state, &ids[1], "slides.pdf", "theirs");
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(names(&state, &ids[0]), ["slides (2).pdf", "slides.pdf"]);
    let folder = originals_dir(&state.notes_dir, &ids[0]).unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join("slides.pdf")).unwrap(),
        "mine"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("slides (2).pdf")).unwrap(),
        "theirs"
    );
}

#[test]
fn undoing_a_combine_takes_the_original_files_back() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", ""), ("Other", "")]);
    original(&state, &ids[0], "slides.pdf", "mine");
    original(&state, &ids[1], "slides.pdf", "theirs");
    original(&state, &ids[1], "handout.pdf", "handout");
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(
        undo(&state, &ids[0], &ids[2]).status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        names(&state, &ids[0]).len(),
        3,
        "a record for another note is left alone"
    );
    assert_eq!(
        undo(&state, &ids[0], &ids[1]).status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(names(&state, &ids[0]), ["slides.pdf"]);
    assert_eq!(names(&state, &ids[1]), ["handout.pdf", "slides.pdf"]);
    let folder = originals_dir(&state.notes_dir, &ids[0]).unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join("slides.pdf")).unwrap(),
        "mine"
    );
}

#[test]
fn undoing_a_combine_of_1001_originals_works() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    for n in 0..1001 {
        original(&state, &ids[1], &format!("page {n}.txt"), "page");
    }
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(names(&state, &ids[0]).len(), 1001);
    assert_eq!(
        undo(&state, &ids[0], &ids[1]).status(),
        StatusCode::NO_CONTENT
    );
    assert!(names(&state, &ids[0]).is_empty());
    assert_eq!(names(&state, &ids[1]).len(), 1001);
}

#[cfg(unix)]
#[test]
fn a_failed_combine_save_leaves_no_carried_original() {
    use std::os::unix::fs::PermissionsExt;
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "theirs");
    let notes = state.notes_dir.clone();
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = combined(&state, &ids[0], &ids[1]);
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(names(&state, &ids[0]).is_empty());
    assert_eq!(names(&state, &ids[1]), ["slides.pdf"]);
}

#[test]
fn the_read_copy_folder_is_not_carried() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "theirs");
    let read = originals_dir(&state.notes_dir, &ids[1])
        .unwrap()
        .join(".read");
    std::fs::create_dir_all(&read).unwrap();
    std::fs::write(read.join("slides.pdf.txt"), "read text").unwrap();
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    let folder = originals_dir(&state.notes_dir, &ids[0]).unwrap();
    assert!(!folder.join(".read").exists());
    assert!(read.join("slides.pdf.txt").exists());
}

#[test]
fn keeping_drops_a_combine_record_older_than_a_day() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", ""), ("Handout", "")]);
    original(&state, &ids[1], "slides.pdf", "slides");
    original(&state, &ids[2], "handout.pdf", "handout");
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    combine::forget_old_combines(&state.chats, chrono::Utc::now() - chrono::Duration::days(1));
    assert_eq!(
        undo(&state, &ids[0], &ids[1]).status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        names(&state, &ids[0]).is_empty(),
        "a record from today stays"
    );
    assert_eq!(combined(&state, &ids[0], &ids[2]).status(), StatusCode::OK);
    combine::forget_old_combines(&state.chats, chrono::Utc::now() + chrono::Duration::days(1));
    assert_eq!(
        undo(&state, &ids[0], &ids[2]).status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        names(&state, &ids[0]),
        ["handout.pdf"],
        "an expired record undoes nothing"
    );
}

#[test]
fn undo_is_refused_once_the_absorbed_note_is_deleted_for_good() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "%PDF slides");
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    let gone = run(crate::routes::trash::delete_from_trash(
        State(state.clone()),
        Json(TrashChoice {
            ids: vec![ids[1].clone()],
            all: false,
            dirs: vec![],
            revisions: shown(&state),
        }),
    ));
    assert!(gone.is_ok());
    assert_eq!(
        undo(&state, &ids[0], &ids[1]).status(),
        StatusCode::CONFLICT
    );
    assert_eq!(names(&state, &ids[0]), ["slides.pdf"]);
    let kept = originals_dir(&state.notes_dir, &ids[0])
        .unwrap()
        .join("slides.pdf");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "%PDF slides");
}

#[test]
fn keeping_never_drops_a_combine_record_replaced_while_it_looked() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", ""), ("Handout", "")]);
    original(&state, &ids[1], "slides.pdf", "slides");
    original(&state, &ids[2], "handout.pdf", "handout");
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    let chats = state.chats.clone();
    let replace = state.clone();
    let (into, with) = (ids[0].clone(), ids[2].clone());
    combine::AFTER_COPY.lock().unwrap().push((
        format!("forget:{}", chats.display()),
        Box::new(move || {
            assert_eq!(combined(&replace, &into, &with).status(), StatusCode::OK);
        }),
    ));
    combine::forget_old_combines(&chats, chrono::Utc::now() + chrono::Duration::hours(1));
    assert_eq!(
        undo(&state, &ids[0], &ids[2]).status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        !names(&state, &ids[0]).contains(&"handout.pdf".to_string()),
        "the replacement record still undoes its combine"
    );
}

fn archive(id: &str) -> leo_core::recording::Archive {
    leo_core::recording::Archive {
        id: id.into(),
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 5,
            text: format!("said in {id}"),
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
    }
}

#[test]
fn undo_keeps_copies_that_changed_since_the_combine() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "%PDF slides");
    original(&state, &ids[1], "handout.pdf", "%PDF handout");
    leo_core::recording::save(&state.notes_dir, &ids[1], &archive("session-one01")).unwrap();
    leo_core::recording::save(&state.notes_dir, &ids[1], &archive("session-two02")).unwrap();
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    let folder = originals_dir(&state.notes_dir, &ids[0]).unwrap();
    std::fs::remove_file(folder.join("slides.pdf")).unwrap();
    std::fs::write(folder.join("slides.pdf"), "%PDF synced replacement").unwrap();
    let sources = leo_core::recording::folder(&state.notes_dir, &ids[0]).unwrap();
    let corrected = std::fs::read_dir(&sources)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("session-one01.") && name.ends_with(".jsonl"))
        })
        .unwrap();
    let mut text = std::fs::read_to_string(&corrected).unwrap();
    text = text.replace("said in session-one01", "corrected by hand");
    std::fs::write(&corrected, &text).unwrap();
    assert_eq!(
        undo(&state, &ids[0], &ids[1]).status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(names(&state, &ids[0]), ["slides.pdf"]);
    assert_eq!(
        std::fs::read_to_string(folder.join("slides.pdf")).unwrap(),
        "%PDF synced replacement"
    );
    let left: Vec<String> = leo_core::recording::list(&state.notes_dir, &ids[0])
        .unwrap()
        .into_iter()
        .map(|head| head.id)
        .collect();
    assert_eq!(left, ["session-one01"]);
    assert_eq!(std::fs::read_to_string(corrected).unwrap(), text);
}

#[test]
fn undo_keeps_a_copy_corrected_while_the_combine_was_still_copying() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "%PDF slides");
    leo_core::recording::save(&state.notes_dir, &ids[1], &archive("session-one01")).unwrap();
    leo_core::recording::save(&state.notes_dir, &ids[1], &archive("session-two02")).unwrap();
    let sources = leo_core::recording::folder(&state.notes_dir, &ids[0]).unwrap();
    let corrected = std::sync::Arc::new(std::sync::Mutex::new(None));
    let noted = corrected.clone();
    let folder = sources.clone();
    combine::AFTER_COPY.lock().unwrap().push((
        format!("published:{}", ids[0]),
        Box::new(move || {
            let path = std::fs::read_dir(&folder)
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("session-one01.") && name.ends_with(".jsonl")
                        })
                })
                .unwrap();
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("said in session-one01", "corrected meanwhile");
            std::fs::write(&path, &text).unwrap();
            *noted.lock().unwrap() = Some((path, text));
        }),
    ));
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    assert_eq!(
        undo(&state, &ids[0], &ids[1]).status(),
        StatusCode::NO_CONTENT
    );
    let left: Vec<String> = leo_core::recording::list(&state.notes_dir, &ids[0])
        .unwrap()
        .into_iter()
        .map(|head| head.id)
        .collect();
    assert_eq!(left, ["session-one01"]);
    let (path, text) = corrected.lock().unwrap().clone().unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

#[test]
fn undo_waits_when_a_transcript_correction_is_being_written() {
    let (state, _d, ids) = state_with(&[("Notes", ""), ("Slides", "")]);
    original(&state, &ids[1], "slides.pdf", "%PDF slides");
    assert_eq!(combined(&state, &ids[0], &ids[1]).status(), StatusCode::OK);
    run(async {
        let correction = state.source_writing.lock().await;
        let mut undo = Box::pin(combine::uncombine(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(serde_json::from_value(serde_json::json!({ "from": ids[1] })).unwrap()),
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(500), &mut undo)
                .await
                .is_err()
        );
        assert_eq!(names(&state, &ids[0]), ["slides.pdf"]);
        drop(correction);
        assert_eq!(undo.await.status(), StatusCode::NO_CONTENT);
    });
    assert!(names(&state, &ids[0]).is_empty());
}
