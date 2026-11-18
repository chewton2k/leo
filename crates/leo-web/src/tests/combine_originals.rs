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
