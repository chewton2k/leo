use super::*;
use crate::routes::uploads::{put_file, Named, SentFile};

fn making(state: &AppState, files: &[(&str, &str, &[u8])]) -> ImportBody {
    let upload = uuid::Uuid::new_v4().to_string();
    let files = files
        .iter()
        .map(|(name, mime, bytes)| {
            let item = uuid::Uuid::new_v4().to_string();
            let response = run(put_file(
                State(state.clone()),
                Path((upload.clone(), item.clone())),
                Query(Named {
                    name: name.to_string(),
                }),
                axum::body::Body::from(bytes.to_vec()),
            ));
            assert_eq!(response.status(), StatusCode::NO_CONTENT, "{name}");
            SentFile {
                item,
                name: name.to_string(),
                mime: mime.to_string(),
            }
        })
        .collect();
    ImportBody {
        upload,
        directory: String::new(),
        title: None,
        wants: String::new(),
        files,
        each: false,
        into: None,
    }
}

fn kept(figures: &[(&str, bool)]) -> (Vec<Figure>, crate::Scratch, std::path::PathBuf) {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let root = dir.path().to_path_buf();
    let list = figures
        .iter()
        .enumerate()
        .map(|(n, (place, photo))| {
            let path = root.join(format!("{n:05}.png"));
            std::fs::write(&path, PICTURE).unwrap();
            Figure {
                place: place.to_string(),
                path,
                photo: *photo,
            }
        })
        .collect();
    (list, crate::Scratch(dir), root)
}

fn bytes_of(file: &UploadFile) -> Vec<u8> {
    std::fs::read(file.path.as_ref().unwrap()).unwrap()
}

const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x01\0\0\0\0\xc8\x08\x02\0\0\0";

#[cfg(unix)]
#[test]
fn originals_refuse_symlinks_at_every_component_and_exports_omit_linked_files() {
    use std::os::unix::fs::symlink;
    for component in ["file", "note", "attachments"] {
        let (state, home, ids) = state_with(&[("Example", "")]);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("example.txt"), "FAKE PRIVATE CONTENT").unwrap();
        let attachments = home.path().join("attachments");
        let folder = attachments.join(&ids[0]);
        match component {
            "file" => {
                std::fs::create_dir_all(&folder).unwrap();
                symlink(
                    outside.path().join("example.txt"),
                    folder.join("example.txt"),
                )
                .unwrap();
            }
            "note" => {
                std::fs::create_dir_all(&attachments).unwrap();
                symlink(outside.path(), &folder).unwrap();
            }
            _ => {
                std::fs::create_dir_all(outside.path().join(&ids[0])).unwrap();
                std::fs::write(
                    outside.path().join(&ids[0]).join("example.txt"),
                    "FAKE PRIVATE CONTENT",
                )
                .unwrap();
                symlink(outside.path(), &attachments).unwrap();
            }
        }
        let notes = state.fresh().notes_dir.clone();
        assert!(
            originals::path_of(&notes, &ids[0], "example.txt").is_none(),
            "{component}"
        );
        assert!(originals::files(&notes, &ids[0]).is_empty(), "{component}");
        let response = run(get_original(
            State(state.clone()),
            Path((ids[0].clone(), "example.txt".into())),
            Query(OriginalView::default()),
        ));
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{component}");
        let listed = run(list_originals(State(state.clone()), Path(ids[0].clone())));
        if component == "file" {
            assert_eq!(json_of(listed), serde_json::json!([]));
        } else {
            assert_eq!(listed.status(), StatusCode::NOT_FOUND, "{component}");
        }
        let archive = run(routes::downloads::originals_zip(
            State(state),
            Path(ids[0].clone()),
        ));
        if component == "file" {
            assert_eq!(archive.status(), StatusCode::OK);
            let bytes = run(axum::body::to_bytes(archive.into_body(), 1024 * 1024)).unwrap();
            let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            assert!(zip.is_empty());
        } else {
            assert_eq!(archive.status(), StatusCode::NOT_FOUND, "{component}");
        }
    }
}

#[cfg(unix)]
#[test]
fn an_upload_cannot_write_originals_through_a_linked_attachment_root() {
    let (mut state, home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            Ok(Made {
                title: "Example".into(),
                body: "Example body".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("attachments")).unwrap();
    let made = json_of(run(start_import(
        State(state.clone()),
        Json(making(
            &state,
            &[("example.txt", "text/plain", b"EXAMPLE ORIGINAL")],
        )),
    )));
    let job = wait_for(&state, made["id"].as_str().unwrap());
    assert_eq!(job.state, "failed");
    assert!(job.error.unwrap().contains("originals"));
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn several_files_make_a_note_each_named_from_its_own_file_and_one_failure_spares_the_rest() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, wants: &str, progress: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(files.len(), 1, "each file is read on its own");
            assert_eq!(wants, "just the proofs");
            progress("Writing the note", 0, 1);
            let name = files[0].name.clone();
            if name == "broken.pdf" {
                anyhow::bail!("there is no text in broken.pdf");
            }
            Ok(Made {
                title: name.trim_end_matches(".pdf").to_string(),
                body: format!("## From {name}"),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let file = |name: &'static str| (name, "application/pdf", name.as_bytes());
    let body = ImportBody {
        directory: "ma161".into(),
        title: Some("Ignored for separate notes".into()),
        wants: "just the proofs".into(),
        each: true,
        ..making(
            &state,
            &[
                file("lecture 1.pdf"),
                file("broken.pdf"),
                file("lecture 2.pdf"),
            ],
        )
    };
    let made = json_of(run(start_import(State(state.clone()), Json(body))));
    let job = wait_for(&state, made["id"].as_str().unwrap());
    assert_eq!(job.state, "done", "{job:?}");
    assert_eq!(job.label, "Making 3 notes, one from each file");
    assert_eq!(job.notes.len(), 2);
    assert_eq!(job.note.as_ref(), job.notes.first());
    assert_eq!(
        job.error.as_deref(),
        Some("broken.pdf: there is no text in broken.pdf")
    );
    let store = state.fresh();
    let made: Vec<(String, String, String)> = job
        .notes
        .iter()
        .map(|id| {
            let n = store.find_note(id).unwrap();
            (n.title.clone(), n.directory.clone(), n.body.clone())
        })
        .collect();
    assert_eq!(made[0].0, "lecture 1");
    assert_eq!(made[1].0, "lecture 2");
    assert!(made.iter().all(|(_, dir, _)| dir == "ma161"));
    assert!(made[0].2.contains("*From lecture 1.pdf, uploaded"));
    assert!(made[1].2.contains("*From lecture 2.pdf, uploaded"));
    drop(store);
    let one = json_of(run(start_import(
        State(state.clone()),
        Json(ImportBody {
            each: true,
            ..making(&state, &[file("broken.pdf"), file("broken.pdf")])
        }),
    )));
    let failed = wait_for(&state, one["id"].as_str().unwrap());
    assert_eq!(failed.state, "failed");
    assert!(failed.notes.is_empty() && failed.note.is_none());
}

#[test]
fn an_upload_becomes_a_note_in_its_folder_and_keeps_the_original() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, wants: &str, progress: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(wants, "the problem, then the code");
            progress("Writing the note", 0, 1);
            assert_eq!(bytes_of(&files[0]), b"%PDF fake");
            assert!(files[0].bytes.is_empty(), "a file is read from its path");
            let (figures, scratch, _) = kept(&[("slide 2", false), ("board.jpg", true)]);
            Ok(Made {
                title: "Graph search".to_string(),
                body: "## BFS\n- uses a queue\n\n![The BFS tree](figure:1)\n\n![again](figure:1)\n![nothing](figure:9)".to_string(),
                figures,
                scratch: Some(scratch),
            })
        },
    ));
    let body = ImportBody {
        directory: "cs130".into(),
        title: None,
        wants: "the problem, then the code".into(),
        ..making(
            &state,
            &[("../../lecture 4.pdf", "application/pdf", b"%PDF fake")],
        )
    };
    let response = run(start_import(State(state.clone()), Json(body)));
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = wait_for(&state, &id);
    assert_eq!(job.state, "done", "{job:?}");
    let note_id = job.note.unwrap();
    let note = state.fresh().find_note(&note_id).unwrap().clone();
    assert_eq!(note.title, "Graph search");
    assert_eq!(note.directory, "cs130");
    let pictures: Vec<String> = note
        .body
        .lines()
        .flat_map(|l| {
            leo_core::attachments::pictures_in(l)
                .into_iter()
                .map(|(_, s)| s.target)
        })
        .collect();
    assert_eq!(pictures.len(), 2, "{}", note.body);
    assert!(
        note.body
            .starts_with("## BFS\n- uses a queue\n\n![The BFS tree](attachments/"),
        "{}",
        note.body
    );
    assert!(
        note.body
            .contains("\n\n## Photos\n\n![board.jpg](attachments/"),
        "{}",
        note.body
    );
    assert!(!note.body.contains("figure:"), "{}", note.body);
    assert!(
        note.body
            .contains("\n\n---\n*From lecture 4.pdf, uploaded "),
        "{}",
        note.body
    );
    let notes_dir = state.fresh().notes_dir.clone();
    for picture in &pictures {
        assert!(picture.ends_with(".png"), "{picture}");
        assert_eq!(std::fs::read(notes_dir.join(picture)).unwrap(), PICTURE);
    }
    let listed = run(list_originals(State(state.clone()), Path(note_id.clone())));
    let listed = run(axum::body::to_bytes(listed.into_body(), usize::MAX)).unwrap();
    assert!(String::from_utf8_lossy(&listed).contains("\"name\":\"lecture 4.pdf\""));
    let file = run(get_original(
        State(state.clone()),
        Path((note_id.clone(), "lecture 4.pdf".into())),
        Query(OriginalView::default()),
    ));
    assert_eq!(file.status(), StatusCode::OK);
    assert_eq!(file.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(file.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .starts_with("attachment"));
    assert!(file.headers().get("x-frame-options").is_none());
    let shown = run(get_original(
        State(state.clone()),
        Path((note_id.clone(), "lecture 4.pdf".into())),
        Query(OriginalView {
            view: Some("1".into()),
        }),
    ));
    assert!(shown.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .starts_with("inline"));
    assert_eq!(shown.headers()["x-frame-options"], "SAMEORIGIN");
    assert!(shown.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'self'"));
    assert!(viewable("board.JPG") && viewable("a.pdf"));
    assert!(!viewable("slides.pptx") && !viewable("page.html") && !viewable("x.svg"));
    for bad in ["../graph.json", "..", "a/b", ".hidden"] {
        let refused = run(get_original(
            State(state.clone()),
            Path((note_id.clone(), bad.into())),
            Query(OriginalView::default()),
        ));
        assert_eq!(refused.status(), StatusCode::NOT_FOUND, "{bad}");
    }
    let refused = run(list_originals(
        State(state.clone()),
        Path("../notes".into()),
    ));
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
}

#[test]
fn a_failed_upload_says_why_and_bad_requests_are_refused() {
    let (mut state, home, _ids) = state_with(&[]);
    let file = |state: &AppState| making(state, &[("board.jpg", "image/jpeg", &[1, 2, 3])]);
    let none = run(start_import(State(state.clone()), Json(file(&state))));
    assert_eq!(none.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(json_of(none)["error"]
        .as_str()
        .unwrap()
        .starts_with(leo_core::NO_WRITING_AI));
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            anyhow::bail!("qwen3:8b cannot read images")
        },
    ));
    let response = run(start_import(State(state.clone()), Json(file(&state))));
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = wait_for(&state, &id);
    assert_eq!(job.state, "failed");
    assert_eq!(job.error.as_deref(), Some("qwen3:8b cannot read images"));
    let outside = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: "../outside".into(),
            ..file(&state)
        }),
    ));
    assert_eq!(outside.status(), StatusCode::BAD_REQUEST);
    let empty = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            files: vec![],
            ..file(&state)
        }),
    ));
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    for item in ["never-sent", "../x", ""] {
        let mut body = file(&state);
        body.files[0].item = item.into();
        let missing = run(start_import(State(state.clone()), Json(body)));
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST, "{item:?}");
    }
    let mut twice = making(&state, &[("a.txt", "text/plain", b"a")]);
    twice.files.push(SentFile {
        item: twice.files[0].item.clone(),
        name: "b.txt".into(),
        mime: "text/plain".into(),
    });
    assert_eq!(
        run(start_import(State(state.clone()), Json(twice))).status(),
        StatusCode::BAD_REQUEST,
        "one sent file cannot stand for two"
    );
    let mut elsewhere = file(&state);
    let sent_to = home.path().join("incoming").join(&elsewhere.upload);
    elsewhere.upload = "../notes".into();
    assert_eq!(
        run(start_import(State(state.clone()), Json(elsewhere))).status(),
        StatusCode::BAD_REQUEST
    );
    std::fs::remove_dir_all(sent_to).unwrap();
    assert!(
        std::fs::read_dir(home.path().join("incoming"))
            .map(|mut folders| folders.next().is_none())
            .unwrap_or(true),
        "a refused or finished upload leaves nothing in incoming"
    );
    assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
    assert_eq!(safe_file_name("C:\\x\\notes?.pdf"), "notes-.pdf");
    assert_eq!(safe_file_name(".."), "upload");
    assert_eq!(safe_file_name("CON.pdf"), "CON-.pdf");
    assert_eq!(safe_file_name("aux"), "aux-");
    assert_eq!(safe_file_name("draft. "), "draft");
    assert_eq!(safe_file_name("Console.pdf"), "Console.pdf");
}

#[test]
fn a_download_name_works_in_every_browser_whatever_the_title() {
    assert_eq!(
        attachment_header("Lecture 4 originals.zip"),
        "attachment; filename=\"Lecture 4 originals.zip\"; filename*=UTF-8''Lecture%204%20originals.zip"
    );
    let accented = attachment_header("Café \"notes\".zip");
    assert!(
        accented.starts_with("attachment; filename=\"Caf_ _notes_.zip\""),
        "{accented}"
    );
    assert!(
        accented.ends_with("filename*=UTF-8''Caf%C3%A9%20%22notes%22.zip"),
        "{accented}"
    );
    assert!(HeaderValue::from_str(&accented).is_ok());
}

#[test]
fn background_work_is_listed_until_it_is_done() {
    let file = |name: &str| UploadFile {
        name: name.into(),
        mime: String::new(),
        bytes: vec![],
        path: None,
    };
    assert_eq!(
        upload_label(&[file("slides.pdf")]),
        "Making a note from slides.pdf"
    );
    assert_eq!(
        upload_label(&[file("a.jpg"), file("b.jpg"), file("c.jpg")]),
        "Making a note from a.jpg and 2 more"
    );
    let job = |state: &'static str, dir: &str| ImportJob {
        state,
        label: "Making a note from slides.pdf".into(),
        dir: dir.into(),
        step: "Writing the note".into(),
        done: 1,
        total: 3,
        note: None,
        notes: Vec::new(),
        error: None,
        section: None,
    };
    let mut imports = std::collections::HashMap::new();
    imports.insert("a".to_string(), job("working", "cs130"));
    imports.insert("b".to_string(), job("done", ""));
    imports.insert("c".to_string(), job("failed", ""));
    let writing = record::RecordView {
        id: "r".into(),
        session: "r".into(),
        source: record::Source::Microphone,
        state: "writing",
        secs: 60,
        step: "Writing the notes".into(),
        steps: Some((2, 4)),
        transcript: String::new(),
        warnings: vec![],
        points: vec![],
        points_total: 0,
        wants: String::new(),
        levels: vec![],
        levels_start: 0,
        note: None,
        error: None,
        writer_failed: false,
    };
    let tasks = activity_tasks(&imports, Some(&writing), Some((3, 9)));
    let kinds: Vec<&str> = tasks.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        ["upload", "recording", "map"],
        "finished uploads are not listed"
    );
    assert_eq!(tasks[0].href, "#/f/cs130");
    assert_eq!((tasks[0].done, tasks[0].total), (1, 3));
    assert_eq!(
        (tasks[1].done, tasks[1].total, tasks[1].href.as_str()),
        (2, 4, "#/record")
    );
    assert_eq!(tasks[2].step, "3 of 9 steps");
    let still_recording = record::RecordView {
        state: "recording",
        ..writing
    };
    assert!(
        activity_tasks(&Default::default(), Some(&still_recording), None).is_empty(),
        "a recording that is still going has its own timer, not a progress bar"
    );
}

#[test]
fn figures_the_ai_did_not_place_are_kept_under_their_own_heading() {
    let (figures, _scratch, _) = kept(&[("page 3", false), ("page 5", false)]);
    let mut n = 0;
    let mut save = |f: &Figure| {
        n += 1;
        (f.place != "page 5").then(|| format!("attachments/{n}.png"))
    };
    assert_eq!(
        settle_figures("## Heaps\n- tree", &figures, &mut save),
        "## Heaps\n- tree\n\n## More figures\n\n![Figure from page 3](attachments/1.png)"
    );
    let kept = settle_figures(
        "Intro ![a heap](figure:2) and ![x](attachments/old.png)\n![the tree](figure:1)",
        &figures,
        &mut |f: &Figure| Some(format!("attachments/{}.png", f.place.replace(' ', "-"))),
    );
    assert_eq!(
        kept,
        "Intro ![a heap](attachments/page-5.png) and ![x](attachments/old.png)\n![the tree](attachments/page-3.png)"
    );
}

#[test]
fn a_third_upload_waits_and_then_runs() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let (mut state, home, _) = state_with(&[]);
    let released = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicUsize::new(0));
    state.importer = Some(Arc::new({
        let released = Arc::clone(&released);
        let started = Arc::clone(&started);
        move |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            started.fetch_add(1, Ordering::Relaxed);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !released.load(Ordering::Relaxed) && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(Made {
                title: "Upload".into(),
                body: "Body".into(),
                figures: vec![],
                scratch: None,
            })
        }
    }));
    let begin = || {
        let body = making(&state, &[("a.txt", "text/plain", b"test")]);
        let response = run(start_import(State(state.clone()), Json(body)));
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        json_of(response)["id"].as_str().unwrap().to_string()
    };
    let state_of = |id: &str| state.imports.lock().unwrap().get(id).unwrap().state;
    let (first, second, third) = (begin(), begin(), begin());
    assert_eq!(state_of(&first), "working");
    assert_eq!(state_of(&second), "working");
    assert_eq!(state_of(&third), "waiting");
    assert!(
        home.path().join("incoming").read_dir().unwrap().count() == 3,
        "a waiting upload's files wait in incoming"
    );
    released.store(true, Ordering::Relaxed);
    for id in [&first, &second, &third] {
        assert_eq!(wait_for(&state, id).state, "done");
    }
    assert_eq!(started.load(Ordering::Relaxed), 3);
}

#[test]
fn an_upload_with_a_very_long_title_makes_a_note_with_that_title() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            Ok(Made {
                title: "Made".into(),
                body: "Body".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let title = "界".repeat(5_000);
    let body = ImportBody {
        title: Some(title.clone()),
        ..making(&state, &[("a.txt", "text/plain", b"test")])
    };
    let response = run(start_import(State(state.clone()), Json(body)));
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let id = json_of(response)["id"].as_str().unwrap().to_string();
    let job = wait_for(&state, &id);
    assert_eq!(job.state, "done", "{job:?}");
    let note = state.fresh().find_note(&job.note.unwrap()).unwrap().clone();
    assert_eq!(note.title, title);
    let names: Vec<String> = std::fs::read_dir(&state.notes_dir)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".md"))
        .collect();
    assert_eq!(names.len(), 1);
    let stem = names[0].trim_end_matches(".md");
    assert!(stem.len() <= 200 && stem.chars().count() <= 100, "{stem}");
}

#[test]
fn thirty_files_make_thirty_notes() {
    let (mut state, _home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            Ok(Made {
                title: files[0].name.trim_end_matches(".txt").to_string(),
                body: String::from_utf8(bytes_of(&files[0])).unwrap(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let names: Vec<String> = (0..30).map(|n| format!("file {n}.txt")).collect();
    let sent: Vec<(&str, &str, &[u8])> = names
        .iter()
        .map(|name| (name.as_str(), "text/plain", name.as_bytes()))
        .collect();
    let body = ImportBody {
        each: true,
        ..making(&state, &sent)
    };
    let response = run(start_import(State(state.clone()), Json(body)));
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let job = wait_for(&state, json_of(response)["id"].as_str().unwrap());
    assert_eq!(job.state, "done", "{job:?}");
    assert_eq!(job.notes.len(), 30);
    assert_eq!(job.error, None);
}

#[test]
fn a_file_arrives_in_pieces_and_is_whole_on_disk() {
    let (state, home, _) = state_with(&[]);
    let data: Vec<u8> = (0..5 * 1024 * 1024).map(|i| (i % 253) as u8).collect();
    let pieces: Vec<_> = data
        .chunks(1024)
        .map(|piece| Ok::<_, std::io::Error>(axum::body::Bytes::copy_from_slice(piece)))
        .collect();
    let response = run(put_file(
        State(state),
        Path(("up-1".into(), "item-1".into())),
        Query(Named {
            name: "lecture.pdf".into(),
        }),
        axum::body::Body::from_stream(futures_util::stream::iter(pieces)),
    ));
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let kept = std::fs::read(home.path().join("incoming/up-1/item-1")).unwrap();
    assert_eq!(kept, data);
    let folder: Vec<_> = std::fs::read_dir(home.path().join("incoming/up-1"))
        .unwrap()
        .collect();
    assert_eq!(folder.len(), 1, "no half-written file is left beside it");
}

#[test]
fn a_second_send_of_one_item_is_refused_and_replaces_nothing() {
    let (state, home, _) = state_with(&[]);
    let send = |bytes: &'static [u8]| {
        run(put_file(
            State(state.clone()),
            Path(("up-2".into(), "item-2".into())),
            Query(Named::default()),
            axum::body::Body::from(bytes),
        ))
        .status()
    };
    assert_eq!(send(b"first"), StatusCode::NO_CONTENT);
    assert_eq!(send(b"second"), StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read(home.path().join("incoming/up-2/item-2")).unwrap(),
        b"first"
    );
}

#[test]
fn an_upload_name_cannot_leave_its_folder() {
    let (state, home, _) = state_with(&[]);
    for (upload, item) in [
        ("..", "item"),
        ("up", ".."),
        ("up", "a.b"),
        ("a/b", "item"),
        ("", "item"),
        ("up", "../../escape"),
    ] {
        let response = run(put_file(
            State(state.clone()),
            Path((upload.into(), item.into())),
            Query(Named::default()),
            axum::body::Body::from("x"),
        ));
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{upload}/{item}"
        );
    }
    assert!(!home.path().join("escape").exists());
    assert!(!home.path().join("incoming").exists());
}

#[test]
fn two_files_with_one_name_keep_both_contents() {
    let (mut state, _home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            let text = String::from_utf8(bytes_of(&files[0])).unwrap();
            Ok(Made {
                title: text,
                body: "Body".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let read = |note: &str, name: &str| {
        let response = run(get_original(
            State(state.clone()),
            Path((note.to_string(), name.to_string())),
            Query(OriginalView::default()),
        ));
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        run(axum::body::to_bytes(response.into_body(), usize::MAX))
            .unwrap()
            .to_vec()
    };
    let same = [
        ("lecture.pdf", "application/pdf", b"first".as_slice()),
        ("lecture.pdf", "application/pdf", b"second".as_slice()),
    ];
    let alike = [
        ("a:b.pdf", "application/pdf", b"colon".as_slice()),
        ("a?b.pdf", "application/pdf", b"query".as_slice()),
    ];
    for files in [same, alike] {
        let body = ImportBody {
            each: true,
            ..making(&state, &files)
        };
        let response = run(start_import(State(state.clone()), Json(body)));
        let job = wait_for(&state, json_of(response)["id"].as_str().unwrap());
        assert_eq!(job.state, "done", "{job:?}");
        assert_eq!(job.notes.len(), 2);
        for (note, (name, _, bytes)) in job.notes.iter().zip(files) {
            assert_eq!(read(note, &safe_file_name(name)), bytes, "{name}");
        }
    }
}

#[test]
fn files_dropped_on_a_note_are_added_to_it_under_their_own_heading() {
    let (mut state, _d, ids) = state_with(&[("Week 3", "")]);
    let note = ids[0].clone();
    let saved = run(crate::routes::notes::update_note(
        State(state.clone()),
        Path(note.clone()),
        Json(
            serde_json::from_value(serde_json::json!({ "body": "My own notes on heaps." }))
                .unwrap(),
        ),
    ))
    .expect("the note's own text is saved");
    assert_eq!(saved.0.body, "My own notes on heaps.");
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, wants: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(files.len(), 2, "files added to a note are read together");
            assert_eq!(wants, "just the proofs");
            Ok(Made {
                title: "Heap proofs".into(),
                body: "Sift down keeps the heap order.".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let body = |into: &str| ImportBody {
        wants: "just the proofs".into(),
        each: true,
        into: Some(into.into()),
        ..making(
            &state,
            &[
                ("slides.pdf", "application/pdf", b"slides"),
                ("sheet.pdf", "application/pdf", b"sheet"),
            ],
        )
    };
    assert_eq!(
        run(start_import(State(state.clone()), Json(body("nosuchnote")))).status(),
        StatusCode::NOT_FOUND
    );
    let add = || {
        let response = run(start_import(State(state.clone()), Json(body(&note))));
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let job = wait_for(&state, &id);
        assert_eq!(job.state, "done", "{job:?}");
        assert_eq!(
            job.note.as_deref(),
            Some(note.as_str()),
            "no new note is made"
        );
        job.section
            .expect("the section comes back for the page to insert")
    };
    let section = add();
    assert!(
        section.starts_with("## Heap proofs\n\nSift down keeps the heap order."),
        "{section}"
    );
    assert!(
        section.contains("*From slides.pdf, sheet.pdf, uploaded"),
        "{section}"
    );
    let notes = run(state.with_store(|store| Ok(store.notes.clone()))).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].body, "My own notes on heaps.",
        "the page inserts the section through the note's own version, so typing never clashes"
    );
    let again = add();
    assert!(
        again.contains("*From slides (2).pdf, sheet (2).pdf, uploaded"),
        "the same files again keep the first ones: {again}"
    );
    let originals = json_of(run(list_originals(
        State(state.clone()),
        Path(note.clone()),
    )));
    let names: Vec<&str> = originals
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["sheet (2).pdf", "sheet.pdf", "slides (2).pdf", "slides.pdf"]
    );
}

#[test]
fn a_long_cjk_file_name_stays_within_200_bytes_and_keeps_its_extension() {
    let name = format!("{}.pdf", "论".repeat(120));
    let safe = safe_file_name(&name);
    assert!(safe.len() <= 200, "{}", safe.len());
    assert!(safe.ends_with(".pdf"), "{safe}");
    assert!(safe.chars().count() > 50);
    assert_eq!(safe_file_name(&safe), safe);
}

#[test]
fn a_long_file_name_keeps_its_extension_and_a_numbered_copy_still_fits() {
    let long = format!(
        "Smith et al. - 2023 - {}.pdf",
        "a very long paper title ".repeat(8)
    );
    assert!(long.chars().count() > 120);
    let safe = safe_file_name(&long);
    assert!(safe.ends_with(".pdf"), "{safe}");
    assert_eq!(safe.chars().count(), 120);
    assert_eq!(safe_file_name(&safe), safe);
    let legacy = "x".repeat(116) + ".pdf";
    assert_eq!(safe_file_name(&legacy), legacy);
    let unrelated = format!("{}.{}", "y".repeat(60), "z".repeat(80));
    assert_eq!(safe_file_name(&unrelated).chars().count(), 120);
}

#[test]
fn same_named_originals_never_overwrite_each_other_or_overrun_the_name_limit() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(files.len(), 2);
            Ok(Made {
                title: "Slides".into(),
                body: "Body".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let file = |name: &'static str, text: &'static str| (name, "application/pdf", text.as_bytes());
    let import = |state: &AppState, body: ImportBody| {
        let response = run(start_import(State(state.clone()), Json(body)));
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let job = wait_for(state, &id);
        assert_eq!(job.state, "done", "{job:?}");
        assert_eq!(job.error, None);
        job
    };
    let job = import(
        &state,
        making(
            &state,
            &[file("slides.pdf", "first"), file("slides.pdf", "second")],
        ),
    );
    let note = job.note.unwrap();
    let names = |state: &AppState, note: &str| -> Vec<String> {
        json_of(run(list_originals(State(state.clone()), Path(note.into()))))
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names(&state, &note), ["slides (2).pdf", "slides.pdf"]);
    assert!(state
        .fresh()
        .find_note(&note)
        .unwrap()
        .body
        .contains("*From slides.pdf, slides (2).pdf, uploaded"));
    let read = |name: &str| {
        let response = run(get_original(
            State(state.clone()),
            Path((note.clone(), name.to_string())),
            Query(OriginalView::default()),
        ));
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap()
    };
    assert_eq!(&read("slides.pdf")[..], b"first");
    assert_eq!(&read("slides (2).pdf")[..], b"second");

    let wide: &'static str = Box::leak(format!("{}.pdf", "w".repeat(116)).into_boxed_str());
    let target = state
        .fresh()
        .create_note("Target", "", "")
        .unwrap()
        .id
        .clone();
    for _ in 0..2 {
        import(
            &state,
            ImportBody {
                into: Some(target.clone()),
                ..making(&state, &[file(wide, "one"), file("other.pdf", "two")])
            },
        );
    }
    let kept = names(&state, &target);
    assert_eq!(kept.len(), 4, "{kept:?}");
    assert!(kept
        .iter()
        .all(|n| n.chars().count() <= 120 && n.ends_with(".pdf")));
}

#[test]
fn an_original_is_sent_whole_and_byte_exact() {
    let (state, home, ids) = state_with(&[("Big", "")]);
    let folder = home.path().join("attachments").join(&ids[0]);
    std::fs::create_dir_all(&folder).unwrap();
    let data: Vec<u8> = (0..3 * 1024 * 1024 + 17).map(|i| (i % 251) as u8).collect();
    std::fs::write(folder.join("deck.pdf"), &data).unwrap();
    let response = run(get_original(
        State(state),
        Path((ids[0].clone(), "deck.pdf".into())),
        Query(OriginalView::default()),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(
        response.headers()[header::CONTENT_LENGTH],
        data.len().to_string().as_str()
    );
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"deck.pdf\""
    );
    let sent = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    assert_eq!(sent.as_ref(), data.as_slice());
}

#[test]
fn the_incoming_folder_is_gone_when_the_job_ends() {
    let (mut state, home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            assert!(files[0].path.as_ref().unwrap().is_file());
            if files[0].name == "broken.txt" {
                anyhow::bail!("there is no text in broken.txt");
            }
            Ok(Made {
                title: "Fine".into(),
                body: "Body".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let incoming = home.path().join("incoming");
    for (name, state_after) in [("fine.txt", "done"), ("broken.txt", "failed")] {
        let body = making(&state, &[(name, "text/plain", b"text")]);
        let upload = body.upload.clone();
        let response = run(start_import(State(state.clone()), Json(body)));
        let job = wait_for(&state, json_of(response)["id"].as_str().unwrap());
        assert_eq!(job.state, state_after, "{job:?}");
        assert!(!incoming.join(&upload).exists(), "{name}");
    }
    let kept = state.fresh().notes.clone();
    let original = home
        .path()
        .join("attachments")
        .join(&kept[0].id)
        .join("fine.txt");
    assert_eq!(std::fs::read(original).unwrap(), b"text");
}

#[test]
fn old_incoming_folders_are_swept_and_new_ones_stay() {
    let (state, home, _) = state_with(&[]);
    let incoming = home.path().join("incoming");
    for name in ["old", "new"] {
        std::fs::create_dir_all(incoming.join(name)).unwrap();
        std::fs::write(incoming.join(name).join("item"), "x").unwrap();
    }
    let mut open = std::fs::File::options();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        open.write(true).custom_flags(0x0200_0000);
    }
    #[cfg(not(windows))]
    open.read(true);
    let old = open.open(incoming.join("old")).unwrap();
    old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 3600))
        .unwrap();
    crate::routes::uploads::sweep_incoming(&state.notes_dir);
    assert!(!incoming.join("old").exists());
    assert!(incoming.join("new/item").exists());
}

fn resident_kb() -> u64 {
    leo_core::children::quiet_command("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
        .unwrap_or(0)
}

fn peak_while<T>(work: impl FnOnce() -> T) -> (T, u64) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let baseline = resident_kb();
    let peak = Arc::new(AtomicU64::new(baseline));
    let done = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (peak, done) = (Arc::clone(&peak), Arc::clone(&done));
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                peak.fetch_max(resident_kb(), Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        })
    };
    let out = work();
    done.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    (out, peak.load(Ordering::SeqCst).saturating_sub(baseline))
}

#[test]
#[ignore = "writes 500 MB and prints the memory it took"]
fn a_large_text_file_becomes_a_note() {
    const MEGABYTES: usize = 500;
    let (mut state, home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(
                std::fs::metadata(files[0].path.as_ref().unwrap())
                    .unwrap()
                    .len(),
                (MEGABYTES * 1024 * 1024) as u64
            );
            Ok(Made {
                title: "Big".into(),
                body: "Body".into(),
                figures: vec![],
                scratch: None,
            })
        },
    ));
    let upload = "big".to_string();
    let mut line = vec![b'a'; 1023];
    line.push(10);
    let line = axum::body::Bytes::from(line);
    let (sent, received) = peak_while(|| {
        let pieces = (0..MEGABYTES * 1024).map({
            let line = line.clone();
            move |_| Ok::<_, std::io::Error>(line.clone())
        });
        run(put_file(
            State(state.clone()),
            Path((upload.clone(), "item".into())),
            Query(Named {
                name: "big.txt".into(),
            }),
            axum::body::Body::from_stream(futures_util::stream::iter(pieces)),
        ))
    });
    assert_eq!(sent.status(), StatusCode::NO_CONTENT);
    let body = ImportBody {
        upload,
        directory: String::new(),
        title: None,
        wants: String::new(),
        files: vec![SentFile {
            item: "item".into(),
            name: "big.txt".into(),
            mime: "text/plain".into(),
        }],
        each: false,
        into: None,
    };
    let (job, made) = peak_while(|| {
        let response = run(start_import(State(state.clone()), Json(body)));
        wait_for(&state, json_of(response)["id"].as_str().unwrap())
    });
    assert_eq!(job.state, "done", "{job:?}");
    assert!(!home.path().join("incoming/big").exists());
    println!("receiving {MEGABYTES} MB grew memory by {received} KB; making the note by {made} KB");
}

#[test]
fn unplaced_figures_are_kept_when_some_are_placed() {
    let (mut state, _home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            let (figures, scratch, _) =
                kept(&[("page 1", false), ("page 2", false), ("page 3", false)]);
            Ok(Made {
                title: "Heaps".into(),
                body: "## Heaps\n\n![the tree](figure:1)".into(),
                figures,
                scratch: Some(scratch),
            })
        },
    ));
    let body = making(&state, &[("heaps.pdf", "application/pdf", b"%PDF")]);
    let response = run(start_import(State(state.clone()), Json(body)));
    let job = wait_for(&state, json_of(response)["id"].as_str().unwrap());
    assert_eq!(job.state, "done", "{job:?}");
    let note = state.fresh().find_note(&job.note.unwrap()).unwrap().clone();
    assert!(
        note.body
            .starts_with("## Heaps\n\n![the tree](attachments/"),
        "{}",
        note.body
    );
    let (placed, more) = note.body.split_once("\n\n## More figures\n\n").unwrap();
    assert_eq!(placed.matches("attachments/").count(), 1, "{placed}");
    assert!(
        more.contains("![Figure from page 2](attachments/"),
        "{more}"
    );
    assert!(
        more.contains("![Figure from page 3](attachments/"),
        "{more}"
    );
    assert!(!more.contains("page 1"), "{more}");
}

#[test]
fn the_scratch_folder_is_gone_after_the_upload() {
    let (mut state, _home, _) = state_with(&[]);
    let folder = Arc::new(Mutex::new(None));
    state.importer = Some(Arc::new({
        let folder = Arc::clone(&folder);
        move |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            let (figures, scratch, root) = kept(&[("page 1", false)]);
            *folder.lock().unwrap() = Some(root);
            Ok(Made {
                title: "Heaps".into(),
                body: "Body".into(),
                figures,
                scratch: Some(scratch),
            })
        }
    }));
    let body = making(&state, &[("heaps.pdf", "application/pdf", b"%PDF")]);
    let response = run(start_import(State(state.clone()), Json(body)));
    let job = wait_for(&state, json_of(response)["id"].as_str().unwrap());
    assert_eq!(job.state, "done", "{job:?}");
    let root = folder.lock().unwrap().clone().unwrap();
    assert!(!root.exists(), "{}", root.display());
}

#[test]
fn the_status_of_an_upload_into_a_note_counts_its_section_and_never_carries_it() {
    let job = ImportJob {
        state: "done",
        label: String::new(),
        dir: String::new(),
        step: String::new(),
        done: 1,
        total: 1,
        note: None,
        notes: Vec::new(),
        error: None,
        section: Some("## Heap proofs\n\nSift down.".into()),
    };
    let shown = serde_json::to_value(&job).unwrap();
    assert_eq!(shown["section_chars"], 26);
    assert!(shown.get("section").is_none());
    assert!(!shown.to_string().contains("Sift down"));
    let none = serde_json::to_value(ImportJob {
        section: None,
        ..job
    })
    .unwrap();
    assert!(none.get("section_chars").is_none());
}

#[test]
fn a_photo_over_twenty_megabytes_is_kept_beside_the_note() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            let (figures, scratch, _) = kept(&[("big.png", true)]);
            let mut big = PICTURE.to_vec();
            big.resize(21 * 1024 * 1024, 5);
            std::fs::write(&figures[0].path, big).unwrap();
            Ok(Made {
                title: "Big photo".to_string(),
                body: "A photo.".to_string(),
                figures,
                scratch: Some(scratch),
            })
        },
    ));
    let body = making(&state, &[("big.png", "image/png", b"x")]);
    let response = run(start_import(State(state.clone()), Json(body)));
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = wait_for(&state, &id);
    assert_eq!(job.state, "done", "{job:?}");
    let note = state.fresh().find_note(&job.note.unwrap()).unwrap().clone();
    let link = note
        .body
        .split("](")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap()
        .to_string();
    let saved = state.fresh().notes_dir.join(&link);
    assert_eq!(std::fs::metadata(saved).unwrap().len(), 21 * 1024 * 1024);
}
