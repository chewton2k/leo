use super::*;

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
    use base64::Engine;
    let (mut state, home, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            Ok(Made {
                title: "Example".into(),
                body: "Example body".into(),
                figures: vec![],
            })
        },
    ));
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("attachments")).unwrap();
    let made = json_of(run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![ImportFileBody {
                name: "example.txt".into(),
                mime: "text/plain".into(),
                data: base64::engine::general_purpose::STANDARD.encode("EXAMPLE ORIGINAL"),
            }],
            each: false,
            into: None,
        }),
    )));
    let job = wait_for(&state, made["id"].as_str().unwrap());
    assert_eq!(job.state, "failed");
    assert!(job.error.unwrap().contains("originals"));
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn several_files_make_a_note_each_named_from_its_own_file_and_one_failure_spares_the_rest() {
    use base64::Engine;
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
            })
        },
    ));
    let file = |name: &str| ImportFileBody {
        name: name.into(),
        mime: "application/pdf".into(),
        data: base64::engine::general_purpose::STANDARD.encode(name.as_bytes()),
    };
    let body = ImportBody {
        directory: "ma161".into(),
        title: Some("Ignored for separate notes".into()),
        wants: "just the proofs".into(),
        files: vec![
            file("lecture 1.pdf"),
            file("broken.pdf"),
            file("lecture 2.pdf"),
        ],
        each: true,
        into: None,
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
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![file("broken.pdf"), file("broken.pdf")],
            each: true,
            into: None,
        }),
    )));
    let failed = wait_for(&state, one["id"].as_str().unwrap());
    assert_eq!(failed.state, "failed");
    assert!(failed.notes.is_empty() && failed.note.is_none());
}

#[test]
fn an_upload_becomes_a_note_in_its_folder_and_keeps_the_original() {
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, wants: &str, progress: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(wants, "the problem, then the code");
            progress("Writing the note", 0, 1);
            assert_eq!(files[0].bytes, b"%PDF fake");
            Ok(Made {
                title: "Graph search".to_string(),
                body: "## BFS\n- uses a queue\n\n![The BFS tree](figure:1)\n\n![again](figure:1)\n![nothing](figure:9)".to_string(),
                figures: vec![
                    Figure {
                        place: "slide 2".into(),
                        bytes: PICTURE.to_vec(),
                        photo: false,
                    },
                    Figure {
                        place: "board.jpg".into(),
                        bytes: PICTURE.to_vec(),
                        photo: true,
                    },
                ],
            })
        },
    ));
    let body = ImportBody {
        directory: "cs130".into(),
        title: None,
        wants: "the problem, then the code".into(),
        files: vec![ImportFileBody {
            name: "../../lecture 4.pdf".into(),
            mime: "application/pdf".into(),
            data: base64::engine::general_purpose::STANDARD.encode(b"%PDF fake"),
        }],
        each: false,
        into: None,
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
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    let file = || ImportFileBody {
        name: "board.jpg".into(),
        mime: "image/jpeg".into(),
        data: base64::engine::general_purpose::STANDARD.encode([1, 2, 3]),
    };
    let none = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![file()],
            each: false,
            into: None,
        }),
    ));
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
    let response = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![file()],
            each: false,
            into: None,
        }),
    ));
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
            title: None,
            wants: String::new(),
            files: vec![file()],
            each: false,
            into: None,
        }),
    ));
    assert_eq!(outside.status(), StatusCode::BAD_REQUEST);
    let empty = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![],
            each: false,
            into: None,
        }),
    ));
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    let garbled = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![ImportFileBody {
                name: "x.pdf".into(),
                mime: String::new(),
                data: "%%%".into(),
            }],
            each: false,
            into: None,
        }),
    ));
    assert_eq!(garbled.status(), StatusCode::BAD_REQUEST);
    assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
    assert_eq!(safe_file_name("C:\\x\\notes?.pdf"), "notes-.pdf");
    assert_eq!(safe_file_name(".."), "upload");
    assert_eq!(safe_file_name("CON.pdf"), "CON-.pdf");
    assert_eq!(safe_file_name("aux"), "aux-");
    assert_eq!(safe_file_name("draft. "), "draft");
    assert_eq!(safe_file_name("Console.pdf"), "Console.pdf");
}

#[test]
fn an_upload_dropped_while_it_is_decoded_gives_its_place_back() {
    use base64::Engine;
    use std::future::Future;
    let (mut state, _dir, _) = state_with(&[]);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            anyhow::bail!("not reached")
        },
    ));
    let body = ImportBody {
        directory: String::new(),
        title: None,
        wants: String::new(),
        files: vec![ImportFileBody {
            name: "big.pdf".into(),
            mime: String::new(),
            data: base64::engine::general_purpose::STANDARD.encode(vec![0u8; 8 * 1024 * 1024]),
        }],
        each: false,
        into: None,
    };
    run(async {
        let mut upload = Box::pin(start_import(State(state.clone()), Json(body)));
        let started = std::time::Instant::now();
        loop {
            let ready = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(upload.as_mut().poll(cx).is_ready())
            })
            .await;
            assert!(!ready, "the upload finished before it could be dropped");
            if !state.imports.lock().unwrap().is_empty() {
                break;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            tokio::task::yield_now().await;
        }
    });
    assert!(state.imports.lock().unwrap().is_empty());
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
    let figures = vec![
        Figure {
            place: "page 3".into(),
            bytes: PICTURE.to_vec(),
            photo: false,
        },
        Figure {
            place: "page 5".into(),
            bytes: PICTURE.to_vec(),
            photo: false,
        },
    ];
    let mut n = 0;
    let mut save = |f: &Figure| {
        n += 1;
        (f.place != "page 5").then(|| format!("attachments/{n}.png"))
    };
    assert_eq!(
        settle_figures("## Heaps\n- tree", &figures, &mut save),
        "## Heaps\n- tree\n\n## Figures\n\n![Figure from page 3](attachments/1.png)"
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
fn uploads_bound_file_count_and_parallel_imports_before_starting_more_workers() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let (mut state, _home, _) = state_with(&[]);
    let released = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicUsize::new(0));
    state.importer = Some(Arc::new({
        let released = Arc::clone(&released);
        let started = Arc::clone(&started);
        move |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            started.fetch_add(1, Ordering::Relaxed);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !released.load(Ordering::Relaxed) && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(Made {
                title: "Upload".into(),
                body: "Body".into(),
                figures: vec![],
            })
        }
    }));
    let body = |count| ImportBody {
        directory: String::new(),
        title: None,
        wants: String::new(),
        files: (0..count)
            .map(|n| ImportFileBody {
                name: format!("{n}.txt"),
                mime: "text/plain".into(),
                data: "dGVzdA==".into(),
            })
            .collect(),
        each: false,
        into: None,
    };
    assert_eq!(
        run(start_import(State(state.clone()), Json(body(21)))).status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(started.load(Ordering::Relaxed), 0);
    let first = json_of(run(start_import(State(state.clone()), Json(body(1)))));
    let second = json_of(run(start_import(State(state.clone()), Json(body(1)))));
    let third = run(start_import(State(state.clone()), Json(body(1))));
    released.store(true, Ordering::Relaxed);
    assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        wait_for(&state, first["id"].as_str().unwrap()).state,
        "done"
    );
    assert_eq!(
        wait_for(&state, second["id"].as_str().unwrap()).state,
        "done"
    );
    assert_eq!(started.load(Ordering::Relaxed), 2);
}

#[test]
fn files_dropped_on_a_note_are_added_to_it_under_their_own_heading() {
    use base64::Engine;
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
            })
        },
    ));
    let file = |name: &str| ImportFileBody {
        name: name.into(),
        mime: "application/pdf".into(),
        data: base64::engine::general_purpose::STANDARD.encode(name.as_bytes()),
    };
    let body = |into: &str| ImportBody {
        directory: String::new(),
        title: None,
        wants: "just the proofs".into(),
        files: vec![file("slides.pdf"), file("sheet.pdf")],
        each: true,
        into: Some(into.into()),
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
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(files.len(), 2);
            Ok(Made {
                title: "Slides".into(),
                body: "Body".into(),
                figures: vec![],
            })
        },
    ));
    let file = |name: &str, text: &str| ImportFileBody {
        name: name.into(),
        mime: "application/pdf".into(),
        data: base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
    };
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
        ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![file("slides.pdf", "first"), file("slides.pdf", "second")],
            each: false,
            into: None,
        },
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

    let wide = format!("{}.pdf", "w".repeat(116));
    let target = state
        .fresh()
        .create_note("Target", "", vec![], "")
        .unwrap()
        .id
        .clone();
    for _ in 0..2 {
        import(
            &state,
            ImportBody {
                directory: String::new(),
                title: None,
                wants: String::new(),
                files: vec![file(&wide, "one"), file("other.pdf", "two")],
                each: false,
                into: Some(target.clone()),
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
