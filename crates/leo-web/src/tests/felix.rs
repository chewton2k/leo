use super::*;

#[test]
fn a_document_given_to_felix_is_kept_as_text_and_read_with_the_question() {
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    state.reader = Some(Arc::new(|file: UploadFile, _: &mut dyn FnMut(&str)| {
        assert_eq!(file.bytes, b"%PDF fake");
        Ok(format!("Text of {}: Dijkstra uses a heap.", file.name))
    }));
    let seen = Arc::new(Mutex::new(String::new()));
    let saw = Arc::clone(&seen);
    state.chat = Some(Arc::new(
        move |_: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            *saw.lock().unwrap() = user.to_string();
            piece("It says Dijkstra uses a heap (slides.pdf).");
            Ok("done".into())
        },
    ));
    let upload = |name: &str, data: &[u8]| {
        run(add_chat_file(
            State(state.clone()),
            Path("chat-docs-0001".into()),
            Json(ImportFileBody {
                name: name.into(),
                mime: "application/pdf".into(),
                data: base64::engine::general_purpose::STANDARD.encode(data),
            }),
        ))
    };
    let added = upload("../../slides.pdf", b"%PDF fake");
    assert_eq!(added.status(), StatusCode::CREATED);
    let doc = json_of(added);
    assert_eq!(doc["name"], "slides.pdf");
    let on_disk: Vec<String> = crate::chat_files::texts(
        &state.chats,
        "chat-docs-0001",
        &[doc["id"].as_str().unwrap().to_string()],
    )
    .into_iter()
    .map(|(_, text)| text)
    .collect();
    assert_eq!(on_disk.len(), 1);
    assert!(
        !on_disk[0].contains("%PDF"),
        "only the text is kept, never the file"
    );

    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "what do my slides say?".into(),
        }],
        note: None,
        refs: vec![],
        chat: Some("chat-docs-0001".into()),
        files: vec![doc["id"].as_str().unwrap().to_string()],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
        selection: None,
        session: None,
    };
    run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
    });
    let prompt = seen.lock().unwrap().clone();
    assert!(
        prompt.contains(&format!(
            "<document_{} id=\"d1\" name=\"slides.pdf\">",
            crate::chat::fence()
        )),
        "{prompt}"
    );
    assert!(prompt.contains("Text of slides.pdf: Dijkstra uses a heap."));

    let listed = json_of(run(list_chat_files(
        State(state.clone()),
        Path("chat-docs-0001".into()),
    )));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let gone = run(remove_chat_file(
        State(state.clone()),
        Path(("chat-docs-0001".into(), doc["id"].as_str().unwrap().into())),
    ));
    assert_eq!(gone, StatusCode::NO_CONTENT);
    let bad = run(list_chat_files(State(state.clone()), Path("../x".into())));
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    state.reader = Some(Arc::new(|_: UploadFile, _: &mut dyn FnMut(&str)| {
        anyhow::bail!("leo cannot read song.mp3 yet")
    }));
    let unreadable = run(add_chat_file(
        State(state.clone()),
        Path("chat-docs-0001".into()),
        Json(ImportFileBody {
            name: "song.mp3".into(),
            mime: "audio/mpeg".into(),
            data: "AAAA".into(),
        }),
    ));
    assert_eq!(unreadable.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json_of(unreadable)["error"]
        .as_str()
        .unwrap()
        .contains("song.mp3"));
}

#[test]
fn a_chat_reply_streams_its_sources_then_the_answer() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "A binary heap backs a priority queue.".into();
        store.save().unwrap();
    }
    let seen = Arc::new(Mutex::new(String::new()));
    let saw = Arc::clone(&seen);
    state.chat = Some(Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            *saw.lock().unwrap() = format!("{system}\n{user}");
            piece("Heaps keep the minimum on top ");
            piece("[n1].");
            Ok("done".into())
        },
    ));
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "how do heaps work?".into(),
        }],
        note: Some(ids[0].clone()),
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
        selection: None,
        session: None,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        lines[0]["answer"].is_string(),
        "the answer's id comes first, for steering"
    );
    assert_eq!(lines[1]["sources"][0]["title"], "Heaps");
    assert_eq!(lines[1]["sources"][0]["why"], "open");
    assert_eq!(lines[2]["t"], "Heaps keep the minimum on top ");
    assert_eq!(lines[3]["t"], "[n1].");
    assert_eq!(lines[4]["done"], true);
    let prompt = seen.lock().unwrap().clone();
    assert!(prompt.contains("there are no modes to choose"), "{prompt}");
    assert!(prompt.contains("A binary heap backs a priority queue."));
    assert!(prompt.contains("User: how do heaps work?"));
}

#[test]
fn a_chat_without_ai_or_a_question_is_refused() {
    let (state, _d, _ids) = state_with(&[]);
    let ask = |text: &str| chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: text.into(),
        }],
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
        selection: None,
        session: None,
    };
    let status = run(async {
        chat_reply(State(state.clone()), Json(ask("hi")))
            .await
            .status()
    });
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let mut with = state.clone();
    with.chat = Some(Arc::new(
        |_: &str,
         _: &str,
         _: u32,
         _: &mut dyn FnMut(&str),
         _: &mut dyn FnMut(),
         _: &dyn Fn() -> bool| { anyhow::bail!("no writing AI is chosen") },
    ));
    let status = run(async {
        chat_reply(State(with.clone()), Json(ask("   ")))
            .await
            .status()
    });
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = run(async {
        let response = chat_reply(State(with.clone()), Json(ask("hi"))).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    let line = text.lines().last().unwrap();
    assert!(line.contains(leo_core::NO_WRITING_AI), "{text}");
    assert!(line.contains("no writing AI is chosen"), "{text}");
}

fn chat_lines(state: &AppState, question: &str) -> Vec<serde_json::Value> {
    chat_lines_with(state, question, None)
}

fn chat_lines_with(
    state: &AppState,
    question: &str,
    access: Option<&str>,
) -> Vec<serde_json::Value> {
    chat_lines_in(state, question, access, None, None)
}

fn chat_lines_in(
    state: &AppState,
    question: &str,
    access: Option<&str>,
    session: Option<tools::Session>,
    selection: Option<chat::Selection>,
) -> Vec<serde_json::Value> {
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: question.into(),
        }],
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: access.map(str::to_string),
        recent: vec![],
        practice: false,
        mark: false,
        selection,
        session,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn scripted(replies: Vec<&'static str>) -> (chat::Streamer, Arc<Mutex<Vec<String>>>) {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&prompts);
    let replies = Arc::new(Mutex::new(replies.into_iter()));
    let streamer: chat::Streamer = Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            seen.lock().unwrap().push(format!("{system}\n{user}"));
            let reply = replies.lock().unwrap().next().unwrap_or("Out of script.");
            for chunk in reply.chars().collect::<Vec<_>>().chunks(7) {
                piece(&chunk.iter().collect::<String>());
            }
            Ok(reply.into())
        },
    );
    (streamer, prompts)
}

#[test]
fn auto_applies_a_margin_note_through_the_real_path_and_undo_takes_it_off() {
    let (mut state, _dir, ids) = state_with(&[("Passage note", "")]);
    let _ = run(update_note(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(
            serde_json::from_value(serde_json::json!({ "body": "checks if it matches" })).unwrap(),
        ),
    ));
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"add_margin_note\", \"note\": \"Passage note\", \"quote\": \"if it matches\", \"text\": \"Hash it first.\"}</tool>",
        "Done.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "leave a margin note", Some("auto"));
    let card = lines
        .iter()
        .find_map(|l| l.get("proposal"))
        .expect("a card");
    assert_eq!(card["state"], "applied");
    assert_eq!(card["kind"], "add_margin_note");
    let threads = crate::margin::open_on(&state.chats, &ids[0]);
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].source, "chat");
    let undone = json_of(run(crate::routes::organize::act_route(
        State(state.clone()),
        Json(serde_json::from_value(card["undo"].clone()).unwrap()),
    )));
    assert_eq!(undone["state"], "applied");
    assert!(crate::margin::open_on(&state.chats, &ids[0]).is_empty());
}

#[test]
fn gathered_notes_respect_the_provider_budget_in_every_chat_style() {
    for practice in [false, true] {
        let (mut state, _dir, ids) = state_with(&[("Queues", "")]);
        {
            let mut store = state.fresh();
            store.find_note_mut(&ids[0]).unwrap().body = "A queue is FIFO.\n".repeat(2_000);
            store.save().unwrap();
        }
        state.room = Some(Arc::new(|| 2_048));
        let (streamer, prompts) = scripted(vec!["A queue removes the oldest item first."]);
        state.chat = Some(streamer);
        let body = serde_json::from_value(serde_json::json!({
            "practice": practice,
            "note": ids[0],
            "messages": [{"role":"user", "text":"Explain queues."}]
        }))
        .unwrap();
        run(async {
            let response = chat_reply(State(state), Json(body)).await;
            assert_eq!(response.status(), StatusCode::OK);
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap();
        });
        let prompts = prompts.lock().unwrap();
        let notes = prompts[0]
            .split_once("<notes>\n")
            .unwrap()
            .1
            .split_once("</notes>")
            .unwrap()
            .0;
        assert!(notes.chars().count() <= 2_048, "practice={practice}");
        assert!(notes.contains("A queue is FIFO."));
    }
}

#[test]
fn the_steps_say_which_note_was_read_when_it_was_read_again_and_what_a_retried_call_got_wrong() {
    let (mut state, _d, ids) = state_with(&[("Pipelining", "cs152")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "A five stage pipeline overlaps instructions.\n".repeat(30);
        store.save().unwrap();
    }
    let (streamer, _prompts) = scripted(vec![
        "<tool>{\"name\": \"open_note\", \"note\": \"Pipelining\"}</tool>",
        "<tool>{\"name\": \"open_note\", \"note\": \"Pipelining\"}</tool>",
        "<tool>{\"name\": \"open_note\", \"note\": \"Pipelining\", \"part\": \"zero\"}</tool>",
        "<tool>{\"name\": \"open_note\"}</tool>",
        "Done.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what does my pipelining note say?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps[0], "Read “Pipelining”");
    assert_eq!(steps[1], "Read “Pipelining” again (unchanged)");
    assert_eq!(
        steps[2],
        "Asked for a part of “Pipelining” that does not exist"
    );
    assert_eq!(steps[3], "Retried open note: \"note\" is missing.");
    assert!(steps.iter().all(|s| !s.contains("Fixed a tool call")));
    assert!(lines
        .iter()
        .filter(|l| l["step"].as_str().is_some_and(|s| s.starts_with("Retried")))
        .all(|l| l["found"].as_array().is_some_and(|f| f.is_empty())));
}

#[test]
fn felix_searches_opens_and_suggests_a_change_with_tools_then_answers() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "cs130"), ("Calendar", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "BFS takes the newest vertex from a queue.".into();
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"search_notes\", \"query\": \"queue\"}</tool>",
        "Let me check it.\n<tool>{\"name\": \"edit_note\", \"note\": \"n1\", \"find\": \"newest\", \"replace\": \"oldest\", \"why\": \"a queue is first in, first out\"}</tool>",
        "Your note says BFS takes the newest vertex [n1], but a queue gives the oldest; I suggested a fix.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "is my BFS note right?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(
        steps,
        [
            "Searched your notes for “queue”",
            "Suggested a change to “Graph traversals”"
        ]
    );
    let sources = lines.iter().rfind(|l| l.get("sources").is_some()).unwrap();
    assert_eq!(sources["sources"][0]["title"], "Graph traversals");
    let proposal = lines.iter().find_map(|l| l.get("proposal")).unwrap();
    assert_eq!(proposal["kind"], "edit");
    assert_eq!(proposal["note"], ids[0].as_str());
    assert_eq!(proposal["find"], "newest");
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert!(
        !shown.contains("<tool"),
        "a tool call never reaches the page: {shown}"
    );
    assert!(
        lines.iter().all(|l| l.get("restart").is_none()),
        "words before a tool call are kept, not taken back"
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Let me check it.\n\n\nYour note says BFS takes the newest vertex [n1], but a queue gives the oldest; I suggested a fix.");
    assert_eq!(lines.last().unwrap()["done"], true);
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].contains(&tools::manual()));
    assert!(
        prompts[0].trim_end().ends_with(tools::REMINDER),
        "the reminder comes last"
    );
    assert!(prompts[1].contains("<tool_result name=\"search_notes\">\n[n1] \"Graph traversals\" in cs130: BFS takes the newest"), "{}", prompts[1]);
    assert!(prompts[2].contains("Suggested. The user sees the change"));
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the newest vertex from a queue.",
        "a suggestion changes nothing by itself"
    );
}

#[test]
fn teaching_from_ten_documents_reads_every_part_of_each_before_the_first_question() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.room = Some(Arc::new(|| 36_000));
    let mut files = Vec::new();
    for n in 1..=10 {
        files.push(
            crate::chat_files::add(
                &state.chats,
                "chat-teach-0001",
                &format!("lecture{n}.pdf"),
                &format!("L{n:02} ").repeat(5_250),
                chrono::Utc::now(),
            )
            .unwrap()
            .id,
        );
    }
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&prompts);
    let pass = Arc::new(Mutex::new(1));
    let streamer: chat::Streamer = Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            seen.lock().unwrap().push(format!("{system}\n{user}"));
            let mut at = pass.lock().unwrap();
            let result = user.rsplit("<tool_result").next().unwrap();
            let left = result
                .split("read_document them next: ")
                .nth(1)
                .map(|rest| {
                    rest.split(".]")
                        .next()
                        .unwrap()
                        .split(", ")
                        .map(|entry| entry.split(' ').next().unwrap().to_string())
                        .collect::<Vec<_>>()
                });
            let reply = match left {
                Some(ids) => read_of(&ids.join(","), *at),
                None if *at == 1 && result.contains("document name=") => {
                    *at = 2;
                    read_of("d1,d2,d3,d4,d5,d6,d7,d8,d9,d10", 2)
                }
                None if *at == 1 => read_of("d1,d2,d3,d4,d5,d6,d7,d8,d9,d10", 1),
                None => "Ready to teach.".to_string(),
            };
            piece(&reply);
            Ok(reply.into())
        },
    );
    fn read_of(ids: &str, part: usize) -> String {
        format!("<tool>{{\"name\": \"read_document\", \"document\": \"{ids}\", \"part\": \"{part}\"}}</tool>")
    }
    state.chat = Some(streamer);
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "teach me these for my midterm".into(),
        }],
        note: None,
        refs: vec![],
        chat: Some("chat-teach-0001".into()),
        files,
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
        selection: None,
        session: None,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(lines.iter().all(|l| l.get("error").is_none()));
    let prompts = prompts.lock().unwrap();
    let heard = prompts[1..].join("\n");
    for n in 1..=10 {
        for part in 1..=2 {
            assert!(
                heard.contains(&format!("name=\"lecture{n}.pdf\" part=\"{part}\" of=\"2\"")),
                "lecture{n} part {part} was not read"
            );
        }
    }
    assert!(
        prompts[0].contains("read every part the request covers"),
        "the instructions must tell Felix to read them all first"
    );
}

#[test]
fn felix_stops_using_tools_after_the_most_steps_and_answers() {
    let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>";
        tools::MOST_STEPS
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "loop forever");
    assert_eq!(
        lines.iter().filter(|l| l.get("step").is_some()).count(),
        tools::MOST_STEPS
    );
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), tools::MOST_STEPS + 1);
    let last = &prompts[tools::MOST_STEPS];
    assert!(last.contains("You have used all the tools"));
    assert!(!last.contains(tools::REMINDER));
    assert!(!last.contains("### search_notes"));
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert!(shown.ends_with("Out of script."));
}

#[test]
fn a_broken_tool_call_is_explained_to_the_model_instead_of_failing() {
    let (mut state, _d, _ids) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{search_notes: heaps}</tool>",
        "Sorry, here is the answer.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "heaps?");
    assert_eq!(lines.last().unwrap()["done"], true);
    assert!(
        prompts.lock().unwrap()[1].contains("That did not work: that tool call is not valid JSON")
    );
}

#[test]
fn a_suggested_change_is_applied_only_while_the_text_is_still_there() {
    let (state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS takes the newest vertex.".into();
        store.save().unwrap();
    }
    let apply = |find: &str, replace: &str| {
        run(apply_suggestion(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(Suggestion {
                find: find.into(),
                replace: replace.into(),
            }),
        ))
    };
    let applied = apply("newest", "oldest");
    assert_eq!(applied.status(), StatusCode::OK);
    let applied = json_of(applied);
    assert_eq!(applied["before"], "BFS takes the newest vertex.");
    assert_eq!(applied["body"], "BFS takes the oldest vertex.");
    let after = applied["version"].as_str().unwrap().to_string();
    let undo = |base: &str| {
        run(update_note(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(UpdateBody {
                title: None,
                body: Some("BFS takes the newest vertex.".into()),
                tags: None,
                pinned: None,
                base: Some(base.into()),
                written: None,
            }),
        ))
    };
    assert!(
        undo("stale-version").is_err(),
        "an undo after another edit is refused"
    );
    assert!(undo(&after).is_ok());
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the newest vertex."
    );
    assert_eq!(apply("newest", "oldest").status(), StatusCode::OK);
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the oldest vertex."
    );
    assert_eq!(apply("newest", "oldest").status(), StatusCode::CONFLICT);
    assert_eq!(
        apply("", "## Practice\n- trace BFS").status(),
        StatusCode::OK
    );
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the oldest vertex.\n\n## Practice\n- trace BFS"
    );
    let missing = run(apply_suggestion(
        State(state.clone()),
        Path("nope".into()),
        Json(Suggestion {
            find: String::new(),
            replace: "x".into(),
        }),
    ));
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[test]
fn an_answer_is_one_request_and_is_never_taken_back_or_asked_again() {
    let shown = |lines: &[serde_json::Value]| -> String {
        lines.iter().filter_map(|l| l["t"].as_str()).collect()
    };
    for (question, reply) in [
        (
            "based on this transcription, can you answer these questions: 6. Can you describe a time you took initiative to make a positive difference? 8. If you were given the opportunity to improve one aspect of Clubhouse's platform, what would you focus on and why?",
            "6. When our club lost its venue, I organised a new one.",
        ),
        (
            "fix anything wrong in my heaps note",
            "Your heaps note looks right to me.",
        ),
        ("how would you improve this note?", "I would add an example."),
    ] {
        let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
        let (streamer, prompts) = scripted(vec![reply, "A second answer nobody asked for."]);
        state.chat = Some(streamer);
        let lines = chat_lines(&state, question);
        assert_eq!(shown(&lines), reply, "{question}");
        assert!(lines
            .iter()
            .all(|l| l.get("restart").is_none() && l.get("reset").is_none()));
        assert_eq!(prompts.lock().unwrap().len(), 1, "{question}");
    }

    let (mut state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS uses a stack.".into();
        store.save().unwrap();
    }
    let (streamer, _prompts) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"find\": \"stack\", \"replace\": \"queue\"}</tool>",
        "I suggested changing stack to queue; apply it if it looks right.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "Please fix my graph traversals note");
    assert!(lines.iter().any(|l| l.get("proposal").is_some()));
    assert_eq!(
        shown(&lines),
        "I suggested changing stack to queue; apply it if it looks right."
    );
}

#[test]
fn a_model_that_says_its_tools_are_missing_is_told_once_that_they_are_not() {
    let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
    let (streamer, prompts) = scripted(vec![
        "I can't access the search_notes tool in this chat.",
        "<tool>{\"name\": \"search_notes\", \"query\": \"heaps\"}</tool>",
        "Found it [n1].",
        "unused",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what do my notes say about heaps?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps, ["Searched your notes for “heaps”"]);
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains(tools::UNSTUCK));
    assert_eq!(prompts.len(), 3);
}

#[test]
fn what_every_step_of_an_answer_spent_is_added_up_and_sent_before_done() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let replies = Arc::new(Mutex::new(
        vec![
            "<tool>{\"name\": \"search_notes\", \"query\": \"queue\"}</tool>",
            "A queue is first in, first out.",
        ]
        .into_iter(),
    ));
    state.chat = Some(Arc::new(
        move |_: &str,
              _: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            let text = replies.lock().unwrap().next().unwrap_or("Out of script.");
            piece(text);
            Ok(chat::Reply {
                text: text.into(),
                spent: Some(chat::Spent {
                    by: "Anthropic".into(),
                    model: Some("claude-sonnet-5-5".into()),
                    input: 1000,
                    output: 100,
                    cost: Some(0.003),
                    ..chat::Spent::default()
                }),
            })
        },
    ));
    let lines = chat_lines(&state, "what is a queue?");
    let at = lines.iter().position(|l| l.get("spent").is_some()).unwrap();
    assert_eq!(lines[at + 1]["done"], true);
    let spent = &lines[at]["spent"];
    assert_eq!(
        (spent["input"].as_u64(), spent["output"].as_u64()),
        (Some(2000), Some(200))
    );
    assert_eq!(spent["steps"], 2);
    assert!((spent["cost"].as_f64().unwrap() - 0.006).abs() < 1e-9);
    assert_eq!(spent["model"], "claude-sonnet-5-5");
}

#[test]
fn chat_titles_are_kept_locally_without_spending_a_model_request() {
    let (mut state, dir, _) = state_with(&[]);
    let asked = Arc::new(Mutex::new(0));
    let count = Arc::clone(&asked);
    let writer: crate::graph::Writer = Arc::new(move |system: &str, _: &str, _: u32| {
        assert!(system.contains("You name a conversation"));
        *count.lock().unwrap() += 1;
        Ok("\"Queues in breadth-first search\"".to_string())
    });
    state.graphs = Arc::new(crate::graph::Graphs::for_notes(
        &dir.path().join("notes"),
        Some(writer),
    ));
    let put = |messages: serde_json::Value| {
        let body: crate::chats::Saving =
            serde_json::from_value(serde_json::json!({ "messages": messages })).unwrap();
        json_of(run(crate::routes::felix::put_chat(
            State(state.clone()),
            Path("chat-name-0001".into()),
            Json(body),
        )))
    };
    let first = put(serde_json::json!([{ "role": "user", "text": "help with bfs" }]));
    assert_eq!(first["title"], "help with bfs");
    put(
        serde_json::json!([{ "role": "user", "text": "help with bfs" }, { "role": "assistant", "text": "BFS uses a queue." }]),
    );
    let started = std::time::Instant::now();
    while crate::chats::load(&state.chats, "chat-name-0001").is_none_or(|c| !c.named) {
        assert!(started.elapsed().as_secs() < 5, "the chat was never named");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let later = put(
        serde_json::json!([{ "role": "user", "text": "help with bfs" }, { "role": "assistant", "text": "BFS uses a queue." }, { "role": "user", "text": "and dfs?" }]),
    );
    assert_eq!(later["title"], "help with bfs");
    assert!(later["about"]
        .as_str()
        .unwrap()
        .starts_with("BFS uses a queue."));
    assert_eq!(*asked.lock().unwrap(), 0);
}

#[test]
fn in_auto_felix_changes_and_makes_notes_at_once_and_links_instead_of_citing_itself() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "cs130"), ("Heaps", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS takes the newest vertex.".into();
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"search_notes\", \"query\": \"heaps\"}</tool>",
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"find\": \"newest\", \"replace\": \"oldest [n1]\"}</tool>",
        "<tool>{\"name\": \"create_note\", \"title\": \"Queues\", \"body\": \"First in, first out. See [n1].\", \"folder\": \"cs130\"}</tool>",
        "I fixed it and made a note on queues.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(
        &state,
        "fix my bfs note and make one on queues",
        Some("auto"),
    );
    let shown: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("proposal")).collect();
    assert_eq!(shown.len(), 2);
    assert!(shown.iter().all(|p| p["state"] == "applied"));
    assert_eq!(shown[0]["before"], "BFS takes the newest vertex.");
    assert!(shown[0]["after"].as_str().is_some_and(|v| !v.is_empty()));
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().body,
        "BFS takes the oldest vertex."
    );
    let made = store.find_note(shown[1]["made"].as_str().unwrap()).unwrap();
    assert_eq!(
        (
            made.title.as_str(),
            made.directory.as_str(),
            made.body.as_str()
        ),
        (
            "Queues",
            "cs130",
            "First in, first out. See [[Graph traversals]]."
        )
    );
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("Leo applies changes with Undo"));
    assert!(prompts[2].contains("Changed. leo applied it"));
}

#[test]
fn read_only_felix_has_no_change_tools_and_is_refused_if_it_tries() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"replace\": \"more\"}</tool>",
        "I would add a line about queues; switch to Ask or Auto to let me.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "please fix my note", Some("read"));
    assert!(lines.iter().all(|l| l.get("proposal").is_none()));
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().body, "");
    let prompts = prompts.lock().unwrap();
    assert!(!prompts[0].contains("### edit_note") && !prompts[0].contains("### create_note"));
    assert!(prompts[0].contains("Read only"));
    assert!(prompts[0].trim_end().ends_with(tools::READ_REMINDER));
    assert!(prompts[1].contains("this chat is read only"));
    assert_eq!(prompts.len(), 2, "no nudge to change anything");
}

struct Native {
    script: Vec<(&'static str, serde_json::Value)>,
    answer: &'static str,
    results: Arc<Mutex<Vec<String>>>,
    fail: bool,
}

impl chat::Conversation for Native {
    fn native(&self) -> bool {
        true
    }

    fn say(&mut self, _text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
        if self.fail {
            anyhow::bail!("the session could not start");
        }
        for (name, args) in self.script.drain(..) {
            let result = (exchange.call)(name, &args);
            self.results.lock().unwrap().push(result);
        }
        (exchange.piece)(self.answer);
        Ok(chat::Reply {
            text: self.answer.into(),
            spent: None,
        })
    }
}

#[test]
fn a_native_session_answers_once_even_when_no_change_was_proposed() {
    struct Twice(Arc<Mutex<Vec<String>>>, Vec<&'static str>);
    impl chat::Conversation for Twice {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.0.lock().unwrap().push(text.to_string());
            let reply = self.1.remove(0);
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    let (mut state, _d, _) = state_with(&[("Interview prep", "")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let ears = Arc::clone(&heard);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            Some(Box::new(Twice(
                Arc::clone(&ears),
                vec![
                    "Here are your interview answers.",
                    "No note change is needed. Here are the interview responses again: ...",
                ],
            )) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines(&state, "write my interview answers into my prep note");
    assert_eq!(heard.lock().unwrap().len(), 1);
    assert!(lines.iter().all(|l| l.get("restart").is_none()));
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Here are your interview answers.");
}

#[test]
fn a_model_with_native_tools_calls_leos_tools_directly_and_answers() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let results = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&results);
    let instructions = Arc::new(Mutex::new(String::new()));
    let told = Arc::clone(&instructions);
    state.converse = Some(Arc::new(
        move |given: &chat::Instructions, specs: &[chat::ToolSpec]| {
            *told.lock().unwrap() = given.native.to_string();
            assert!(specs
                .iter()
                .any(|s| s.name == "search_notes" && s.schema["required"][0] == "query"));
            Some(Box::new(Native {
                script: vec![
                    ("search_notes", serde_json::json!({ "query": "graph" })),
                    ("open_note", serde_json::json!({})),
                ],
                answer: "BFS uses a queue [n1].",
                results: Arc::clone(&seen),
                fail: false,
            }) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines(&state, "what is in my graph note?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps[0], "Searched your notes for “graph”");
    let results = results.lock().unwrap();
    assert!(results[0].contains("Graph traversals"));
    assert!(
        results[1].starts_with("That did not work:"),
        "{}",
        results[1]
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "BFS uses a queue [n1].");
    assert!(instructions.lock().unwrap().contains("Call Leo tools"));
    assert!(!instructions.lock().unwrap().contains("<tool>{"));
}

#[test]
fn a_session_hears_only_what_is_new_and_a_failed_native_start_falls_back() {
    struct Session(Arc<Mutex<Vec<String>>>, Vec<&'static str>);
    impl chat::Conversation for Session {
        fn native(&self) -> bool {
            false
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{text}|{}", exchange.tail));
            let reply = self.1.remove(0);
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let ears = Arc::clone(&heard);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            Some(Box::new(Session(
                Arc::clone(&ears),
                vec![
                    "<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>",
                    "Heaps keep the minimum on top.",
                ],
            )) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines(&state, "what is a heap?");
    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 2);
    assert!(heard[0].contains("<conversation>") && heard[0].ends_with(tools::REMINDER));
    assert!(
        heard[1].starts_with("<tool_result name=\"search_notes\">"),
        "{}",
        heard[1]
    );
    assert!(
        !heard[1].contains("<conversation>"),
        "only the new part goes to a live session"
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Heaps keep the minimum on top.");

    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    state.converse = Some(Arc::new(|_: &chat::Instructions, _: &[chat::ToolSpec]| {
        Some(Box::new(Native {
            script: vec![],
            answer: "",
            results: Default::default(),
            fail: true,
        }) as Box<dyn chat::Conversation>)
    }));
    let (streamer, prompts) = scripted(vec!["Answered the old way."]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what is a heap?");
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Answered the old way.");
    assert_eq!(prompts.lock().unwrap().len(), 1);
    assert_eq!(lines.last().unwrap()["done"], true);
}

#[test]
fn the_study_rules_reach_felix_once_however_the_study_starts() {
    struct Session(Arc<Mutex<Vec<String>>>, Vec<&'static str>);
    impl chat::Conversation for Session {
        fn native(&self) -> bool {
            false
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.0.lock().unwrap().push(text.to_string());
            let reply = if self.1.is_empty() {
                "Done."
            } else {
                self.1.remove(0)
            };
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    const QUIZ: &str = "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What does a heap keep at its root?\", \"answer\": \"The smallest item\"}</tool>";
    const RULES: &str = "<tool>{\"name\": \"study_rules\"}</tool>";
    const MARK: &str = "Find the edge first";
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(vec![RULES, QUIZ]));
    let ears = Arc::clone(&heard);
    let lines = Arc::clone(&replies);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            let script = std::mem::take(&mut *lines.lock().unwrap());
            Some(Box::new(Session(Arc::clone(&ears), script)) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);

    let lines_out = chat_lines(&state, "teach me heaps");
    assert!(lines_out
        .iter()
        .any(|l| l["step"] == "Checked how to teach this"));
    {
        let mut said = heard.lock().unwrap();
        assert!(!said[0].contains(MARK), "an answer starts without them");
        assert!(said[1].contains(MARK), "asking for them returns them");
        assert!(!said[2].contains(MARK), "a quiz after them repeats nothing");
        said.clear();
    }

    *replies.lock().unwrap() = vec![QUIZ];
    chat_lines(&state, "quiz me on heaps");
    {
        let mut said = heard.lock().unwrap();
        assert!(!said[0].contains(MARK));
        assert!(said[1].contains(MARK), "a first quiz brings them along");
        said.clear();
    }

    let practice = |chat: Option<&str>, messages: Vec<chat::Turn>| {
        let body = chat::ChatBody {
            messages,
            note: None,
            refs: vec![],
            chat: chat.map(str::to_string),
            files: vec![],
            access: None,
            recent: vec![],
            practice: true,
            mark: false,
            selection: None,
            session: None,
        };
        run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
        });
    };
    let turn = |role: &str, text: &str| chat::Turn {
        role: role.into(),
        text: text.into(),
    };
    *replies.lock().unwrap() = vec![RULES, RULES, "Done."];
    chat_lines(&state, "teach me");
    {
        let mut said = heard.lock().unwrap();
        assert!(said[1].contains(MARK));
        assert!(
            !said[2].contains(MARK),
            "a second call does not resend them"
        );
        assert!(said[2].contains(tools::RULES_ALREADY));
        said.clear();
    }

    *replies.lock().unwrap() = vec![QUIZ];
    practice(None, vec![turn("user", "go")]);
    {
        let mut said = heard.lock().unwrap();
        assert!(said[0].contains(MARK), "practice starts with them");
        assert!(!said[1].contains(MARK));
        said.clear();
    }

    let mut talk = vec![turn("user", "go")];
    practice(Some("study-rules-0001"), talk.clone());
    talk.extend([turn("assistant", "Done."), turn("user", "next")]);
    practice(Some("study-rules-0001"), talk);
    let said = heard.lock().unwrap();
    assert_eq!(said.len(), 2);
    assert!(said[0].contains(MARK));
    assert!(!said[1].contains(MARK), "a warm session has them already");
}

#[test]
fn a_failed_session_leaves_the_study_rules_to_be_sent_again() {
    struct Breaks(u8);
    impl chat::Conversation for Breaks {
        fn native(&self) -> bool {
            false
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.0 += 1;
            anyhow::ensure!(self.0 == 1, "the session broke");
            let quiz = "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What does a heap keep at its root?\", \"answer\": \"The smallest item\"}</tool>";
            (exchange.piece)(quiz);
            Ok(chat::Reply::from(quiz))
        }
    }
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    state.converse = Some(Arc::new(|_: &chat::Instructions, _: &[chat::ToolSpec]| {
        Some(Box::new(Breaks(0)) as Box<dyn chat::Conversation>)
    }));
    let (writer, prompts) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What does a heap keep at its root?\", \"answer\": \"The smallest item\"}</tool>",
        "Done.",
    ]);
    state.chat = Some(writer);
    chat_lines(&state, "quiz me on heaps");
    let prompts = prompts.lock().unwrap();
    assert!(
        prompts.iter().any(|p| p.contains("Find the edge first")),
        "the fallback starts fresh, so its first quiz brings the rules"
    );
}

#[test]
fn a_failing_light_writer_gives_way_to_the_chosen_model_and_says_so() {
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    state.chat = Some(Arc::new(
        |_: &str,
         _: &str,
         _: u32,
         piece: &mut dyn FnMut(&str),
         _: &mut dyn FnMut(),
         _: &dyn Fn() -> bool| {
            piece("Chosen model.");
            let mut reply = chat::Reply::from("Chosen model.");
            reply.spent = Some(chat::Spent {
                steps: 1,
                ..Default::default()
            });
            Ok(reply)
        },
    ));
    state.light_chat = Some(Arc::new(
        |_: &str,
         _: &str,
         _: u32,
         _: &mut dyn FnMut(&str),
         _: &mut dyn FnMut(),
         _: &dyn Fn() -> bool| anyhow::bail!("rate limited"),
    ));
    let lines = chat_lines(&state, "when is lab 3 due?");
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Chosen model.");
    let spent = lines.iter().find_map(|l| l.get("spent")).unwrap();
    assert_eq!(spent["auto"], false);
}

#[test]
fn no_lighter_model_means_no_routing() {
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    state.chat = Some(scripted(vec!["Chosen model."]).0);
    state.light_chat = Some(scripted(vec!["Light model."]).0);
    state.lighter = Some(Arc::new(|| false));
    let lines = chat_lines(&state, "when is lab 3 due?");
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Chosen model.");
}

#[test]
fn a_lookup_uses_the_cheaper_writer_when_no_session_can_open() {
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    state.chat = Some(scripted(vec!["Chosen model."]).0);
    state.light_chat = Some(scripted(vec!["Light model.", "Light model."]).0);
    let shown = |lines: Vec<serde_json::Value>| {
        lines
            .iter()
            .filter_map(|l| l["t"].as_str())
            .collect::<String>()
    };
    assert_eq!(
        shown(chat_lines(&state, "when is lab 3 due?")),
        "Light model."
    );
    assert_eq!(shown(chat_lines(&state, "explain lab 3")), "Chosen model.");
    state.light_chat = None;
    state.chat = Some(scripted(vec!["Chosen model."]).0);
    assert_eq!(
        shown(chat_lines(&state, "when is lab 3 due?")),
        "Chosen model."
    );
}

#[test]
fn a_lookup_starts_on_the_light_model_moves_up_when_needed_and_never_back_down() {
    struct Quick;
    impl chat::Conversation for Quick {
        fn native(&self) -> bool {
            false
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.piece)("Done.");
            let mut reply = chat::Reply::from("Done.");
            reply.spent = Some(chat::Spent {
                steps: 1,
                ..Default::default()
            });
            Ok(reply)
        }
    }
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&opened);
    state.converse = Some(Arc::new(
        move |given: &chat::Instructions, _: &[chat::ToolSpec]| {
            seen.lock().unwrap().push(given.light);
            Some(Box::new(Quick) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let turn = |role: &str, text: &str| chat::Turn {
        role: role.into(),
        text: text.into(),
    };
    let ask = |chat: &str, messages: &[chat::Turn]| {
        let body = chat::ChatBody {
            messages: messages.to_vec(),
            note: None,
            refs: vec![],
            chat: Some(chat.to_string()),
            files: vec![],
            access: None,
            recent: vec![],
            practice: false,
            mark: false,
            selection: None,
            session: None,
        };
        let text = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            String::from_utf8(bytes.to_vec()).unwrap()
        });
        text.lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .find_map(|l| l.get("spent").cloned())
            .unwrap()
    };
    let mut talk = vec![turn("user", "when is lab 3 due?")];
    assert_eq!(ask("light-chat-0001", &talk)["auto"], true);
    talk.extend([
        turn("assistant", "Done."),
        turn("user", "when is the final?"),
    ]);
    assert_eq!(ask("light-chat-0001", &talk)["auto"], true);
    assert_eq!(
        *opened.lock().unwrap(),
        [true],
        "a warm light chat continues"
    );
    talk.extend([
        turn("assistant", "Done."),
        turn("user", "explain why it is on that day"),
    ]);
    assert_eq!(ask("light-chat-0001", &talk)["auto"], false);
    assert_eq!(*opened.lock().unwrap(), [true, false], "it moves up once");
    let mut full = vec![turn("user", "explain the krebs cycle")];
    assert_eq!(ask("light-chat-0003", &full)["auto"], false);
    full.extend([
        turn("assistant", "Done."),
        turn("user", "when is lab 4 due?"),
    ]);
    assert_eq!(ask("light-chat-0003", &full)["auto"], false);
    assert_eq!(
        *opened.lock().unwrap(),
        [true, false, false],
        "a warm full chat never moves down"
    );

    let mut prefs = crate::margin::prefs(&state.chats);
    prefs.auto_model = false;
    assert!(crate::margin::set_prefs(&state.chats, &prefs));
    let talk = [turn("user", "when is lab 5 due?")];
    assert_eq!(ask("light-chat-0002", &talk)["auto"], false);
    assert_eq!(opened.lock().unwrap().last(), Some(&false));
}

#[test]
fn felix_looks_at_a_notes_pictures_and_their_captions_come_along_next_time() {
    let (mut state, dir, ids) = state_with(&[("Heaps", "")]);
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(notes.join("attachments")).unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&[0; 80]);
    std::fs::write(notes.join("attachments/heap.png"), &png).unwrap();
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "![a heap](attachments/heap.png)".into();
        store.save().unwrap();
    }
    state.seer = Some(Arc::new(
        |_: &str, user: &str, pictures: Vec<crate::captions::Picture>| {
            assert_eq!(pictures.len(), 1);
            assert_eq!(user, "Describe this picture.");
            Ok("A binary min heap with 2 at the root.".into())
        },
    ));
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"look_at_picture\", \"note\": \"Heaps\"}</tool>",
        "It shows a min heap.",
        "Still a min heap.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what is in my heaps picture?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps, ["Looked at the pictures in “Heaps”"]);
    assert!(prompts.lock().unwrap()[1]
        .contains("Picture 1: a heap (heap.png): A binary min heap with 2 at the root."));
    chat_lines(&state, "and the heaps picture again?");
    assert!(
        prompts.lock().unwrap()[2].contains(
            "![a heap](attachments/heap.png) [Picture: A binary min heap with 2 at the root.]"
        ),
        "the caption is now part of the note Felix reads"
    );
}

#[test]
fn felix_gives_a_practice_question_then_stops_and_waits_for_the_answer() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"multiple_choice\", \"question\": \"What does BFS use?\", \"options\": \"a stack | a queue\", \"answer\": \"a queue\"}</tool>",
        "<tool>{\"name\": \"quiz\", \"kind\": \"multiple_choice\", \"question\": \"What does BFS use?\", \"options\": \"a stack | a queue | a heap\", \"answer\": \"a queue\", \"explain\": \"Oldest first.\"}</tool>",
        "Give it a try!",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me on BFS");
    let quizzes: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("quiz")).collect();
    assert_eq!(quizzes.len(), 1, "only the valid quiz is shown: {lines:?}");
    assert_eq!(quizzes[0]["kind"], "multiple_choice");
    let mut options: Vec<&str> = quizzes[0]["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|o| o.as_str())
        .collect();
    options.sort();
    assert_eq!(
        options,
        ["a heap", "a queue", "a stack"],
        "the same choices, in shuffled order"
    );
    assert_eq!(quizzes[0]["answer"], "a queue");
    assert_eq!(quizzes[0]["explain"], "Oldest first.");
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains("a multiple_choice quiz needs explain"));
    assert!(prompts[2].contains(tools::ASKED));
    assert!(
        prompts[2].contains("You have used all the tools"),
        "after asking, Felix only finishes his reply"
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Give it a try!");
    assert!(lines.first().unwrap()["answer"].is_string());
}

#[test]
fn a_quiz_felix_wrote_a_little_off_is_shown_without_a_failed_step() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"Quiz\", \"kind\": \"MCQ\", \"question\": \"What does BFS use?\", \"options\": [\"a stack\", \"a queue\", \"a heap\"], \"answer\": \"b\", \"explain\": \"Oldest first.\"}</tool>",
        "Give it a try!",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me on BFS");
    let quizzes: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("quiz")).collect();
    assert_eq!(quizzes.len(), 1, "{lines:?}");
    assert_eq!(quizzes[0]["kind"], "multiple_choice");
    assert_eq!(quizzes[0]["answer"], "a queue");
    let steps: Vec<&serde_json::Value> = lines.iter().filter(|l| l.get("step").is_some()).collect();
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0]["step"], "Asked you a practice question");
    assert_eq!(steps[0]["failed"], false);
    assert!(prompts.lock().unwrap()[1].contains(tools::ASKED));
}

#[test]
fn felix_asks_a_question_with_choices_once_per_reply() {
    let (mut state, _d, _) = state_with(&[]);
    let results = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&results);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, specs: &[chat::ToolSpec]| {
            assert!(specs.iter().any(|s| s.name == "ask_user"));
            assert!(specs.iter().any(|s| s.name == "quiz"));
            Some(Box::new(Native {
                script: vec![
                    (
                        "ask_user",
                        serde_json::json!({ "question": "Which week?", "options": "Week 1 | Week 2 |  | Week 3" }),
                    ),
                    ("ask_user", serde_json::json!({ "question": "And again?" })),
                    ("search_notes", serde_json::json!({ "query": "heap" })),
                ],
                answer: "Tell me which week.",
                results: Arc::clone(&seen),
                fail: false,
            }) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines_with(&state, "summarise the lecture", Some("read"));
    let asks: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("ask")).collect();
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["question"], "Which week?");
    assert_eq!(
        asks[0]["options"],
        serde_json::json!(["Week 1", "Week 2", "Week 3"])
    );
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(
        steps,
        ["Asked you a question"],
        "a call after asking shows no step"
    );
    let results = results.lock().unwrap();
    assert_eq!(results[0], tools::ASKED);
    assert_eq!(results[1], tools::ALREADY_ASKED);
    assert_eq!(
        results[2],
        tools::ALREADY_ASKED,
        "nothing else runs once the user was asked"
    );
}

#[test]
fn a_message_sent_while_felix_works_reaches_him_at_his_next_step() {
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    let steering = state.steering.clone();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&prompts);
    let step = Arc::new(Mutex::new(0));
    state.chat = Some(Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            seen.lock().unwrap().push(format!("{system}\n{user}"));
            let mut at = step.lock().unwrap();
            *at += 1;
            let reply = if *at == 1 {
                for id in steering.open_ids() {
                    steering.add(&id, "only the min-heap part").unwrap();
                }
                "<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>"
            } else {
                "A min-heap keeps the smallest at the root."
            };
            piece(reply);
            Ok(reply.into())
        },
    ));
    let lines = chat_lines(&state, "explain heaps");
    let steered: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("steered")).collect();
    assert_eq!(steered, [&serde_json::json!(["only the min-heap part"])]);
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains("User: only the min-heap part"));
    assert!(prompts[1].contains("take it into account"));
    assert!(
        state.steering.open_ids().is_empty(),
        "a finished answer takes no more messages"
    );
}

#[test]
fn messages_can_be_added_only_to_an_answer_still_being_written() {
    let (state, _d, _) = state_with(&[]);
    let steer = state.steering.open();
    let post = |id: &str, text: &str| {
        run(steer_answer(
            State(state.clone()),
            Path(id.to_string()),
            Json(crate::routes::felix::Steering { text: text.into() }),
        ))
    };
    let accepted = post(steer.id(), "shorter please");
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    assert_eq!(json_of(accepted)["waiting"], 1);
    assert_eq!(post(steer.id(), " ").status(), StatusCode::BAD_REQUEST);
    let id = steer.id().to_string();
    drop(steer);
    let late = post(&id, "too late");
    assert_eq!(late.status(), StatusCode::GONE);
    assert_eq!(json_of(late)["error"], "That answer has finished.");
}

#[test]
fn permissions_changed_during_a_reply_gate_notes_and_repeating_tasks_in_both_protocols() {
    struct Changing {
        steering: crate::steer::Steering,
        access: tools::Access,
        call: tools::Call,
    }
    impl chat::Conversation for Changing {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            for id in self.steering.open_ids() {
                self.steering.set_access(&id, self.access).unwrap();
            }
            (exchange.call)(&self.call.name, &self.call.args);
            (exchange.piece)("Ready.");
            Ok("Ready.".into())
        }
    }
    for native in [false, true] {
        for (before, after) in [
            ("auto", "ask"),
            ("ask", "auto"),
            ("auto", "read"),
            ("read", "auto"),
            ("ask", "read"),
            ("read", "ask"),
        ] {
            for kind in ["note", "task"] {
                let (mut state, _dir, _) = state_with(&[]);
                let steering = state.steering.clone();
                let access = tools::Access::named(after);
                let call = if kind == "task" {
                    tools::Call {
                        name: "add_task".into(),
                        args: serde_json::json!({"title":"Interview studying", "date":"2026-10-12", "repeat":"mon, tue, wed, thu, fri"}),
                    }
                } else {
                    tools::Call {
                        name: "create_note".into(),
                        args: serde_json::json!({"title":"Interview studying", "body":"Practise interview questions", "folder":""}),
                    }
                };
                if native {
                    let call = call.clone();
                    state.converse = Some(Arc::new(
                        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
                            Some(Box::new(Changing {
                                steering: steering.clone(),
                                access,
                                call: call.clone(),
                            })
                                as Box<dyn chat::Conversation>)
                        },
                    ));
                    state.chat = Some(scripted(vec!["Ready."]).0);
                } else {
                    let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
                    state.chat = Some(Arc::new(
                        move |_: &str,
                              _: &str,
                              _: u32,
                              piece: &mut dyn FnMut(&str),
                              _: &mut dyn FnMut(),
                              _: &dyn Fn() -> bool| {
                            let reply = if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                for id in steering.open_ids() {
                                    steering.set_access(&id, access).unwrap();
                                }
                                let mut args = call.args.clone();
                                args["name"] = serde_json::json!(call.name);
                                format!("<tool>{args}</tool>")
                            } else {
                                "Ready.".into()
                            };
                            piece(&reply);
                            Ok(reply.into())
                        },
                    ));
                }
                let lines = chat_lines_with(&state, "Prepare it", Some(before));
                let proposals: Vec<_> = lines.iter().filter_map(|l| l.get("proposal")).collect();
                assert_eq!(
                    proposals.len(),
                    usize::from(after != "read"),
                    "{native} {before}->{after} kind={kind}: {lines:?}"
                );
                if let Some(proposal) = proposals.first() {
                    assert_eq!(proposal["state"] == "applied", after == "auto");
                }
                let count = if kind == "task" {
                    crate::tasks::count(&state.chats)
                } else {
                    state.store.lock().unwrap().store.notes.len()
                };
                assert_eq!(count, usize::from(after == "auto"));
                if kind == "task" && after == "auto" {
                    assert_eq!(
                        crate::tasks::load_all(&state.chats)[0].repeat,
                        [0, 1, 2, 3, 4]
                    );
                }
                assert!(lines.iter().all(|l| l.get("error").is_none()));
            }
        }
    }
}

#[test]
fn permission_updates_validate_access_and_only_change_the_live_answer() {
    let (state, _dir, _) = state_with(&[]);
    let steer = state.steering.open();
    let other = state.steering.open();
    let post = |access: &str| {
        run(crate::routes::felix::change_answer_access(
            State(state.clone()),
            Path(steer.id().into()),
            Json(crate::routes::felix::Permission {
                access: access.into(),
            }),
        ))
    };
    for (name, access) in [
        ("auto", tools::Access::Auto),
        ("ask", tools::Access::Ask),
        ("read", tools::Access::Read),
    ] {
        assert_eq!(post(name).status(), StatusCode::OK);
        assert_eq!(steer.access(tools::Access::Ask), access);
        assert_eq!(other.access(tools::Access::Ask), tools::Access::Ask);
    }
    assert_eq!(post("anything").status(), StatusCode::BAD_REQUEST);
    assert_eq!(steer.access(tools::Access::Auto), tools::Access::Read);
    let id = steer.id().to_string();
    drop(steer);
    assert_eq!(
        run(crate::routes::felix::change_answer_access(
            State(state),
            Path(id),
            Json(crate::routes::felix::Permission {
                access: "auto".into()
            })
        ))
        .status(),
        StatusCode::GONE
    );
}

#[test]
fn closing_the_reply_prevents_further_automatic_changes() {
    let (tx, rx) = crate::reply::channel();
    let mut desk = tools::Desk::new(vec![], 32000)
        .with_access(tools::Access::Auto)
        .with_reply(tx);
    assert_eq!(desk.current_access(), tools::Access::Auto);
    drop(rx);
    assert_eq!(desk.current_access(), tools::Access::Read);
}

#[test]
fn a_long_chat_is_remembered_in_a_summary_instead_of_forgotten() {
    let (mut state, _dir, _) = state_with(&[]);
    let summaries = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::clone(&summaries);
    let writer: crate::graph::Writer = Arc::new(move |system: &str, user: &str, _: u32| {
        if system.contains("You name a conversation") {
            return Ok("A name".into());
        }
        assert!(system.contains("You keep the memory of a long study chat"));
        told.lock().unwrap().push(user.to_string());
        Ok("The student is revising graph search for a Friday exam.".into())
    });
    state.housekeeping = Some(writer);
    let (streamer, prompts) = scripted(vec!["First.", "Second."]);
    state.chat = Some(streamer);
    let mut messages: Vec<chat::Turn> = (0..31)
        .map(|i| chat::Turn {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            text: if i == 0 {
                "My exam is on Friday; I study graph search.".into()
            } else {
                format!("message {i}")
            },
        })
        .collect();
    let id = "chat-memory-0001";
    crate::chats::save(
        &state.chats,
        id,
        serde_json::from_value(serde_json::json!({ "messages": [] })).unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    let ask = |messages: &[chat::Turn]| {
        let body = chat::ChatBody {
            messages: messages.to_vec(),
            note: None,
            refs: vec![],
            chat: Some(id.into()),
            files: vec![],
            access: None,
            recent: vec![],
            practice: false,
            mark: false,
            selection: None,
            session: None,
        };
        run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains("done"));
        });
    };
    ask(&messages);
    let first = prompts.lock().unwrap()[0].clone();
    assert!(
        first.contains("<earlier_in_this_chat>") && first.contains("User: My exam is on Friday"),
        "before a summary exists, older messages come along shortened"
    );
    let started = std::time::Instant::now();
    while crate::chats::load(&state.chats, id).is_none_or(|c| c.memory.is_none()) {
        assert!(started.elapsed().as_secs() < 5, "nothing was remembered");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let memory = crate::chats::load(&state.chats, id)
        .unwrap()
        .memory
        .unwrap();
    assert_eq!(memory.upto, 31 - chat::turns_for(chat::ROOM));
    assert!(summaries.lock().unwrap()[0].contains("My exam is on Friday"));
    crate::chats::save(
        &state.chats,
        id,
        serde_json::from_value(
            serde_json::json!({ "messages": [{ "role": "user", "text": "x" }] }),
        )
        .unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    assert!(
        crate::chats::load(&state.chats, id)
            .unwrap()
            .memory
            .is_some(),
        "the page saving the chat keeps the memory"
    );
    messages.push(chat::Turn {
        role: "assistant".into(),
        text: "message 31".into(),
    });
    messages.push(chat::Turn {
        role: "user".into(),
        text: "what was my deadline?".into(),
    });
    ask(&messages);
    let second = prompts.lock().unwrap()[1].clone();
    assert!(second.contains("What the chat covered before (a summary):\nThe student is revising graph search for a Friday exam."));
    assert!(
        !second.contains("User: My exam is on Friday"),
        "what the summary covers is not repeated"
    );
    assert!(second.contains("User: message 18"), "{second}");
}

#[test]
fn planning_is_in_the_standing_rules_and_never_added_because_of_a_numbered_list() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec!["Done.", "Done."]);
    state.chat = Some(streamer);
    chat_lines_with(&state, "fix every typo in all my notes", Some("read"));
    chat_lines_with(
        &state,
        "answer these:\n1. Tell us about yourself.\n2. Why Clubhouse?\n3. Describe a team.",
        Some("read"),
    );
    let prompts = prompts.lock().unwrap();
    for prompt in prompts.iter() {
        assert!(!prompt.contains("## This is a big request"));
        assert!(prompt.contains("Long messages or numbered questions alone do not need a plan"));
    }
}

#[test]
fn felix_and_search_find_a_note_that_means_the_same_without_sharing_a_word() {
    let (mut state, _d, ids) = state_with(&[("Graph search", ""), ("Biology", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "Take the oldest item from a FIFO queue.".into();
        store.find_note_mut(&ids[1]).unwrap().body = "Leaves turn light into sugar.".into();
        store.save().unwrap();
    }
    let meaning = crate::vectors::fake_meaning();
    state.meaning = Some(Arc::clone(&meaning));
    crate::vectors::catch_up(&state.fresh(), &state.vectors, &meaning, 100);
    let (streamer, prompts) = scripted(vec!["It uses a queue [n1]."]);
    state.chat = Some(streamer);
    chat_lines(&state, "explain breadth first please");
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("close in meaning to the question"),
        "{prompt}"
    );
    assert!(prompt.contains("oldest item from a FIFO queue"));
    assert!(!prompt.contains("Leaves turn light"));
    let found = json_of(
        run(crate::routes::notes::search_notes(
            State(state.clone()),
            Query(serde_json::from_value(serde_json::json!({ "q": "breadth first" })).unwrap()),
        ))
        .map(axum::response::IntoResponse::into_response)
        .unwrap(),
    );
    assert_eq!(found[0]["title"], "Graph search");
    assert_eq!(found[0]["why"]["kind"], "meaning");
}

#[test]
fn felix_works_numbers_out_with_the_calculator_instead_of_guessing() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"calculate\", \"steps\": \"f(x) = x^2 + 4*cos(x)\\nx = 1\\nx = x - (2x - 4 sin(x))/(2 - 4 cos(x))\\nf(1.381966)\"}</tool>",
        "Newton jumps to x = -7.472741.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "do question 1 of my homework", Some("read"));
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps, ["Calculated 4 steps"]);
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("### calculate"));
    assert!(prompts[0].contains("never estimate them mentally"));
    assert!(prompts[0].contains("\"Start with question 1\" includes every part of question 1"));
    assert!(prompts[1].contains("x = -7.4727"), "{}", prompts[1]);
    assert!(prompts[1].contains("f(1.381966) = 2.66067"));
}

#[test]
fn a_practice_answer_is_marked_once_and_never_nudged_into_changing_a_note() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        "[[incorrect]] Not quite: compare both ends.",
        "unused",
    ]);
    state.chat = Some(streamer);
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "Quiz answer. Question: why f'(x)=0?\nMy answer: dunno\nMark it: start your reply with [[correct]] or [[incorrect]]".into(),
        }],
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: true,
        mark: false,
        selection: None,
        session: None,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    assert!(
        !text.contains("restart"),
        "the marking is never taken back: {text}"
    );
    assert_eq!(
        prompts.lock().unwrap().len(),
        1,
        "no second try asking for a note change"
    );
    assert!(!prompts.lock().unwrap()[0].contains("## This is a big request"));
    assert!(
        !prompts.lock().unwrap()[0].contains("### edit_note"),
        "marking an answer is read only, so the note-fixing reminder never comes up"
    );
}

#[test]
fn an_agent_that_breaks_mid_answer_is_replaced_cleanly_by_the_text_protocol() {
    struct Breaks;
    impl chat::Conversation for Breaks {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.piece)("Half an answer");
            let _ = (exchange.call)(
                "quiz",
                &serde_json::json!({ "kind": "free_response", "question": "Why?", "answer": "Because." }),
            );
            anyhow::bail!("claude stopped: the model's tool call could not be parsed")
        }
    }
    let (mut state, _d, _) = state_with(&[]);
    state.converse = Some(Arc::new(|_: &chat::Instructions, _: &[chat::ToolSpec]| {
        Some(Box::new(Breaks) as Box<dyn chat::Conversation>)
    }));
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"Why a queue?\", \"answer\": \"Oldest first.\"}</tool>",
        "Try the card below.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me");
    let reset = lines
        .iter()
        .position(|l| l.get("reset").is_some())
        .expect("what was shown is cleared");
    let after: Vec<&serde_json::Value> = lines[reset..]
        .iter()
        .filter_map(|l| l.get("quiz"))
        .collect();
    assert_eq!(after.len(), 1, "the redo can ask its own card: {lines:?}");
    assert_eq!(after[0]["question"], "Why a queue?");
    let shown: String = lines[reset..]
        .iter()
        .filter_map(|l| l["t"].as_str())
        .collect();
    assert_eq!(shown, "Try the card below.");
    assert_eq!(
        lines.last().unwrap()["done"],
        true,
        "no error reaches the user"
    );
}

#[test]
fn a_failed_answer_is_tried_once_more_before_the_user_sees_an_error() {
    let (mut state, _d, _) = state_with(&[]);
    let calls = Arc::new(Mutex::new(0));
    let count = Arc::clone(&calls);
    state.chat = Some(Arc::new(
        move |_: &str,
              _: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut(),
              _: &dyn Fn() -> bool| {
            let mut n = count.lock().unwrap();
            *n += 1;
            if *n == 1 {
                piece("Broken start");
                anyhow::bail!("the model's tool call could not be parsed");
            }
            piece("A min-heap keeps the smallest on top.");
            Ok(chat::Reply {
                text: "A min-heap keeps the smallest on top.".into(),
                spent: None,
            })
        },
    ));
    let lines = chat_lines(&state, "what is a heap?");
    assert!(lines.iter().any(|l| l.get("reset").is_some()));
    assert!(lines.iter().all(|l| l.get("error").is_none()), "{lines:?}");
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[test]
fn a_marking_without_a_verdict_gets_one_from_a_short_check() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec!["Close, but you left out the endpoints.", "incorrect"]);
    state.chat = Some(streamer);
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "Quiz answer. Question: what do you compare?\nMy answer: the middle\nA model answer to compare with: f(a), f(b) and critical points".into(),
        }],
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: true,
        mark: true,
        selection: None,
        session: None,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    assert!(text.contains("{\"verdict\":\"incorrect\"}"), "{text}");
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].starts_with(crate::routes::felix::JUDGE));
}

#[test]
fn felix_quotes_the_uploaded_file_behind_a_note_and_says_where_it_is() {
    let (mut state, dir, ids) = state_with(&[("Lecture 4 slides", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "## Shortest paths\nDijkstra fails with negative edges.".into();
        store.save().unwrap();
    }
    let folder = dir.path().join("attachments").join(&ids[0]);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("week4.pptx"), b"slides").unwrap();
    state.reader = Some(Arc::new(|file: UploadFile, _: &mut dyn FnMut(&str)| {
        assert_eq!(file.name, "week4.pptx");
        Ok("## Slide 1\nWeek 4\n\n## Slide 2\n\"Dijkstra's algorithm assumes all edge weights are non-negative.\"\nUse Bellman-Ford otherwise.".into())
    }));
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"read_original\", \"note\": \"n1\", \"find\": \"non-negative\"}</tool>",
        "The slide says \"Dijkstra's algorithm assumes all edge weights are non-negative.\" [f1 p2] Your note says it more briefly [n1 L2].",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(
        &state,
        "what exactly did the slides say about negative edges?",
    );
    let files: Vec<&serde_json::Value> =
        lines.iter().filter(|l| l.get("files").is_some()).collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["files"][0]["name"], "week4.pptx");
    assert_eq!(files[0]["files"][0]["note"], ids[0].as_str());
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("Made from an uploaded file (week4.pptx)"));
    assert!(
        prompts[0].contains("2| Dijkstra fails with negative edges."),
        "note lines are numbered"
    );
    assert!(
        prompts[1].contains("<file id=\"f1\" name=\"week4.pptx\""),
        "{}",
        prompts[1]
    );
    assert!(prompts[1].contains("(slide 2)\n4| ## Slide 2\n5| \"Dijkstra's algorithm assumes all edge weights are non-negative.\""), "{}", prompts[1]);
    assert!(prompts[1].contains("[f1 p4]"));
    drop(prompts);

    let shown = json_of(run(crate::routes::felix::original_passage(
        State(state.clone()),
        Path((ids[0].clone(), "week4.pptx".into())),
        axum::extract::Query(serde_json::from_value(serde_json::json!({"at": "p2"})).unwrap()),
    )));
    assert_eq!(shown["label"], "slide 2");
    assert_eq!(
        shown["lines"][1]["text"],
        "\"Dijkstra's algorithm assumes all edge weights are non-negative.\""
    );
    let outside = run(crate::routes::felix::original_passage(
        State(state.clone()),
        Path((ids[0].clone(), "../notes".into())),
        axum::extract::Query(serde_json::from_value(serde_json::json!({"at": "p2"})).unwrap()),
    ));
    assert_eq!(outside.status(), StatusCode::NOT_FOUND);
}

#[test]
fn an_edit_copied_with_line_numbers_still_finds_its_text() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "## BFS\nBFS uses a stack.".into();
        store.save().unwrap();
    }
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"n1\", \"find\": \"2| BFS uses a stack.\", \"replace\": \"2| BFS uses a queue.\"}</tool>",
        "Suggested.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "please fix my graph traversals note");
    let proposal = lines
        .iter()
        .find_map(|l| l.get("proposal"))
        .expect("a suggestion");
    assert_eq!(proposal["find"], "BFS uses a stack.");
    assert_eq!(proposal["replace"], "BFS uses a queue.");
}

#[test]
fn a_study_session_plans_names_ideas_and_refuses_questions_once_covered() {
    let (mut state, _d, _) = state_with(&[("Heaps", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"study_plan\", \"ideas\": \"Root | Sift down\"}</tool>",
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What is at the root?\", \"answer\": \"the minimum\"}</tool>",
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What is at the root?\", \"answer\": \"the minimum\", \"idea\": \"1\"}</tool>",
        "Try it.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me on heaps");
    let plan: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("plan")).collect();
    assert_eq!(plan, [&serde_json::json!({"ideas": ["Root", "Sift down"]})]);
    let quizzes: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("quiz")).collect();
    assert_eq!(quizzes.len(), 1, "{lines:?}");
    assert_eq!(quizzes[0]["idea"], 1);
    assert!(prompts.lock().unwrap()[2]
        .contains("give \"idea\": the number of an idea still open: 1 (Root), 2 (Sift down)"));

    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"More?\", \"answer\": \"x\", \"idea\": \"2\"}</tool>",
        "You covered both ideas well.",
    ]);
    state.chat = Some(streamer);
    let covered = tools::Session {
        ideas: vec!["Root".into(), "Sift down".into()],
        closed: vec![true, true],
        asked: 2,
        ..Default::default()
    };
    let lines = chat_lines_in(&state, "[[correct]] yes", None, Some(covered), None);
    assert!(lines.iter().all(|l| l.get("quiz").is_none()), "{lines:?}");
    assert!(
        lines.iter().all(|l| l.get("step").is_none()),
        "a finished session refuses quietly: {lines:?}"
    );
    assert!(prompts.lock().unwrap()[1].contains(tools::SESSION_DONE));
    assert!(
        !prompts.lock().unwrap()[0].contains("<study_session>"),
        "a finished session sends no state"
    );

    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"How does it sift down?\", \"answer\": \"swap with the smaller child\"}</tool>",
        "Try it.",
    ]);
    state.chat = Some(streamer);
    let one_open = tools::Session {
        ideas: vec!["Root".into(), "Sift down".into()],
        closed: vec![true, false],
        asked: 1,
        ..Default::default()
    };
    let lines = chat_lines_in(
        &state,
        "[[correct]] the minimum",
        None,
        Some(one_open),
        None,
    );
    let quizzes: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("quiz")).collect();
    assert_eq!(quizzes.len(), 1, "{lines:?}");
    assert_eq!(quizzes[0]["idea"], 2, "the one open idea");

    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"idea\": \"1\", \"question\": \"Where is the minimum?\", \"answer\": \"at the root\"}</tool>",
        "Try it.",
    ]);
    state.chat = Some(streamer);
    let one_open = tools::Session {
        ideas: vec!["Root".into(), "Sift down".into()],
        closed: vec![true, false],
        asked: 1,
        ..Default::default()
    };
    let lines = chat_lines_in(
        &state,
        "[[correct]] the minimum",
        None,
        Some(one_open),
        None,
    );
    assert!(
        lines.iter().all(|l| l.get("quiz").is_none()),
        "a question about a covered idea is not moved to another idea: {lines:?}"
    );
    assert!(prompts.lock().unwrap()[1].contains("That did not work"));

    let (streamer, ended) = scripted(vec![
        "<tool>{\"name\": \"end_session\"}</tool>",
        "Good work today.",
    ]);
    state.chat = Some(streamer);
    let open = tools::Session {
        ideas: vec!["Root".into()],
        closed: vec![false],
        asked: 1,
        ..Default::default()
    };
    let lines = chat_lines_in(&state, "that's enough for today", None, Some(open), None);
    assert!(
        lines
            .iter()
            .any(|l| l.get("session_end") == Some(&serde_json::json!(true))),
        "{lines:?}"
    );
    let prompts = ended.lock().unwrap();
    assert!(
        prompts[0].contains("<study_session>\nIdeas: 1 Root (open)."),
        "{}",
        prompts[0]
    );
    assert!(!prompts[0].contains("leave out the answer"));
    assert!(prompts[1].contains(tools::ENDED));
    assert!(
        prompts[1].contains("Not shown yet: Root."),
        "{}",
        prompts[1]
    );
}

#[test]
fn a_selected_passage_reaches_felix_as_quoted_material() {
    let (mut state, _d, _) = state_with(&[("Heaps", "cs130")]);
    let (streamer, prompts) = scripted(vec!["It means the root is smallest."]);
    state.chat = Some(streamer);
    let selection = chat::Selection {
        title: "Heaps".into(),
        from: 3,
        to: 3,
        text: "keeps the minimum".into(),
    };
    chat_lines_in(&state, "what does this mean?", None, None, Some(selection));
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains(
        "<selected_passage note=\"Heaps\" lines=\"3\">\nkeeps the minimum\n</selected_passage>"
    ));
    assert!(
        prompt.find("<selected_passage").unwrap() < prompt.rfind("what does this mean?").unwrap()
    );
}

#[test]
fn felix_organizes_tasks_folders_and_notes_through_cards_that_undo() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "cs130"), ("Old draft", "")]);
    {
        let mut store = state.fresh();
        store.create_dir("archive");
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"add_task\", \"title\": \"Submit lab\", \"date\": \"2026-10-16\", \"time\": \"23:59\", \"type\": \"assignment\"}</tool>",
        "<tool>{\"name\": \"move_note\", \"note\": \"Heaps\", \"folder\": \"archive\"}</tool>",
        "<tool>{\"name\": \"delete_note\", \"note\": \"Old draft\"}</tool>",
        "<tool>{\"name\": \"make_folder\", \"folder\": \"archive/labs\"}</tool>",
        "Here is what I suggest.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "tidy things up");
    let cards: Vec<serde_json::Value> = lines
        .iter()
        .filter_map(|l| l.get("proposal").cloned())
        .collect();
    let kinds: Vec<&str> = cards.iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        ["add_task", "move_note", "delete_note", "make_folder"],
        "{lines:?}"
    );
    assert_eq!(
        cards[0]["summary"],
        "Add the task “Submit lab” · Fri, Oct 16 at 11:59 pm"
    );
    assert!(prompts.lock().unwrap()[1].contains("not done yet"));
    assert!(
        state.fresh().notes.iter().any(|n| n.id == ids[1]),
        "Ask first changes nothing by itself"
    );

    let act = |p: &serde_json::Value| {
        json_of(run(crate::routes::organize::act_route(
            State(state.clone()),
            Json(serde_json::from_value(p.clone()).unwrap()),
        )))
    };
    let moved = act(&cards[1]);
    assert_eq!(moved["state"], "applied");
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "archive"
    );
    act(&moved["undo"]);
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().directory, "cs130");
    let trashed = act(&cards[2]);
    assert!(state.fresh().find_note(&ids[1]).is_none());
    act(&trashed["undo"]);
    assert!(
        state.fresh().find_note(&ids[1]).is_some(),
        "Undo brings the note back from the trash"
    );
    let added = act(&cards[0]);
    let task = added["task"].as_str().unwrap().to_string();
    assert!(crate::tasks::load(&state.chats, &task).is_some());
    act(&added["undo"]);
    assert!(crate::tasks::load(&state.chats, &task).is_none());
    let folder = act(&cards[3]);
    assert!(state.fresh().dir_exists("archive/labs"));
    act(&folder["undo"]);
    assert!(!state.fresh().dir_exists("archive/labs"));
    for bad in ["", "..", "../outside", "/", "missing"] {
        let refused = run(crate::routes::organize::act_route(
            State(state.clone()),
            Json(
                serde_json::from_value(
                    serde_json::json!({"kind": "remove_folder", "summary": "x", "path": bad}),
                )
                .unwrap(),
            ),
        ));
        assert_eq!(refused.status(), 400, "{bad:?} is refused");
    }
    assert!(state.fresh().dir_exists("archive"));
    assert!(state.fresh().find_note(&ids[0]).is_some());
}

#[test]
fn apply_answers_the_routes_own_status() {
    let (state, _dir, _) = state_with(&[]);
    let missing = run(crate::routes::organize::act_route(
        State(state.clone()),
        Json(
            serde_json::from_value(
                serde_json::json!({"kind": "delete_task", "summary": "x", "id": "t0123456789abcdef"}),
            )
            .unwrap(),
        ),
    ));
    let direct = run(crate::routes::tasks::delete_task(
        State(state.clone()),
        Path("t0123456789abcdef".into()),
    ));
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(direct.status(), missing.status());
    assert_eq!(json_of(missing)["error"], "That is not there any more.");
}

#[test]
fn felix_in_auto_does_it_at_once_and_read_only_can_only_look() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "cs130")]);
    {
        let mut store = state.fresh();
        store.create_dir("archive");
        store.save().unwrap();
    }
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"move_folder\", \"folder\": \"cs130\", \"into\": \"archive\"}</tool>",
        "Moved it.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "put cs130 in archive", Some("auto"));
    let card = lines.iter().find_map(|l| l.get("proposal")).unwrap();
    assert_eq!(card["state"], "applied");
    assert_eq!(card["path"], "archive/cs130");
    assert_eq!(card["undo"]["kind"], "move_folder");
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "archive/cs130"
    );

    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"delete_note\", \"note\": \"Heaps\"}</tool>",
        "<tool>{\"name\": \"list_schedule\", \"days\": \"3\"}</tool>",
        "Nothing is due.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "delete heaps and show my week", Some("read"));
    assert!(lines.iter().all(|l| l.get("proposal").is_none()));
    let prompts = prompts.lock().unwrap();
    assert!(
        !prompts[0].contains("\"delete_note\""),
        "read only leaves the change tools out of the manual"
    );
    assert!(prompts[1].contains("read only"));
    assert!(prompts[2].contains("Tasks and events from"));
    assert!(state.fresh().find_note(&ids[0]).is_some());
}

#[test]
fn felix_renames_pins_restores_and_deletes_folders_only_when_asked_and_finds_old_chats() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "s26"), ("Gone note", "")]);
    {
        let mut store = state.fresh();
        store.delete_notes(std::slice::from_ref(&ids[1]));
        store.save().unwrap();
    }
    crate::chats::save(
        &state.chats,
        "chat-midterm-0001",
        serde_json::from_value(serde_json::json!({"title": "Midterm prep", "messages": [
            {"role": "user", "text": "What is on the midterm?"},
            {"role": "assistant", "text": "The midterm covers heaps and Dijkstra."}
        ]}))
        .unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"list_trash\"}</tool>",
        "<tool>{\"name\": \"search_chats\", \"query\": \"midterm\"}</tool>",
        "<tool>{\"name\": \"rename_note\", \"note\": \"Heaps\", \"title\": \"Heaps and priority queues\"}</tool>",
        "<tool>{\"name\": \"rename_folder\", \"folder\": \"s26\", \"new_name\": \"Spring 2026\"}</tool>",
        "<tool>{\"name\": \"pin_note\", \"note\": \"Heaps\"}</tool>",
        "<tool>{\"name\": \"restore_note\", \"note\": \"Gone note\"}</tool>",
        "<tool>{\"name\": \"delete_folder\", \"folder\": \"Spring 2026\"}</tool>",
        "Done.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "sort things out", Some("auto"));
    {
        let prompts = prompts.lock().unwrap();
        let at = prompts[1]
            .rfind("<tool_result name=\"list_trash")
            .unwrap_or(0);
        assert!(
            prompts[1].contains("“Gone note” from the top level"),
            "{}",
            &prompts[1][at..(at + 700).min(prompts[1].len())]
        );
        assert!(
            prompts[2].contains("“Midterm prep”")
                && prompts[2].contains("covers heaps and Dijkstra")
        );
    }
    let cards: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("proposal")).collect();
    let states: Vec<(&str, &str)> = cards
        .iter()
        .map(|c| {
            (
                c["kind"].as_str().unwrap(),
                c["state"].as_str().unwrap_or("new"),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("rename_note", "applied"),
            ("rename_folder", "applied"),
            ("pin_note", "applied"),
            ("restore_note", "applied"),
            ("delete_folder", "new"),
        ],
        "a folder delete waits for the user even in Auto"
    );
    let store = state.fresh();
    let heaps = store.find_note(&ids[0]).unwrap();
    assert_eq!(
        (heaps.title.as_str(), heaps.directory.as_str(), heaps.pinned),
        ("Heaps and priority queues", "Spring 2026", true)
    );
    assert!(store.find_note(&ids[1]).is_some());
    drop(store);

    let act = |p: &serde_json::Value| {
        json_of(run(crate::routes::organize::act_route(
            State(state.clone()),
            Json(serde_json::from_value(p.clone()).unwrap()),
        )))
    };
    act(&cards[1]["undo"]);
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().directory, "s26");
    let deleted = act(&serde_json::json!({"kind": "delete_folder", "summary": "x", "path": "s26"}));
    assert!(state.fresh().find_note(&ids[0]).is_none());
    act(&deleted["undo"]);
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "s26",
        "Undo brings the folder and its notes back"
    );
}

#[test]
fn a_follow_up_in_the_same_chat_reuses_one_warm_session_and_sends_only_whats_new() {
    struct Echo {
        heard: Arc<Mutex<Vec<String>>>,
        stopped: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl chat::Conversation for Echo {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.heard.lock().unwrap().push(text.to_string());
            let reply = "Heaps keep the smallest key on top [n1].";
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    impl Drop for Echo {
        fn drop(&mut self) {
            self.stopped
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let (mut state, _d, _) = state_with(&[("Heaps", "algo"), ("Graphs", "algo")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stopped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (ears, opens, stops) = (
        Arc::clone(&heard),
        Arc::clone(&opened),
        Arc::clone(&stopped),
    );
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            opens.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(Box::new(Echo {
                heard: Arc::clone(&ears),
                stopped: Arc::clone(&stops),
            }) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let ask = |chat: &str, messages: &[(&str, &str)], practice: bool| {
        let body = chat::ChatBody {
            messages: messages
                .iter()
                .map(|(role, text)| chat::Turn {
                    role: role.to_string(),
                    text: text.to_string(),
                })
                .collect(),
            note: None,
            refs: vec![],
            chat: Some(chat.into()),
            files: vec![],
            access: None,
            recent: vec![],
            practice,
            mark: false,
            selection: None,
            session: None,
        };
        let text = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            String::from_utf8(
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap()
        });
        text.lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .collect::<Vec<_>>()
    };
    let alive = || {
        opened.load(std::sync::atomic::Ordering::SeqCst)
            - stopped.load(std::sync::atomic::Ordering::SeqCst)
    };
    let first = ask("warm-chat-0001", &[("user", "what is a heap?")], false);
    assert!(first.iter().any(|l| l.get("done").is_some()));
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(alive(), 1, "the finished answer's session stays warm");
    let second = ask(
        "warm-chat-0001",
        &[
            ("user", "what is a heap?"),
            ("assistant", "Heaps keep the smallest key on top."),
            ("user", "and how does heap sort use it?"),
        ],
        false,
    );
    assert!(second.iter().any(|l| l.get("done").is_some()));
    assert_eq!(
        opened.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the follow-up reused the warm session"
    );
    let said = heard.lock().unwrap().clone();
    assert!(said[0].contains("<conversation>") && said[0].contains("title=\"Heaps\""));
    assert!(!said[1].contains("<conversation>"), "{}", said[1]);
    assert!(
        !said[1].contains("title=\"Heaps\""),
        "a note already read is not sent again"
    );
    assert!(said[1].contains("User: and how does heap sort use it?"));
    assert!(said[1].contains("<today>"));
    let sources = second.iter().find_map(|l| l.get("sources")).unwrap();
    assert!(sources
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["n"] == 1 && s["title"] == "Heaps"));
    ask("warm-chat-0002", &[("user", "what is a graph?")], false);
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(alive(), 2, "another chat retains the first session");
    ask(
        "warm-chat-0002",
        &[
            ("user", "what is a graph?"),
            ("assistant", "x"),
            ("user", "mark my answer"),
        ],
        true,
    );
    assert_eq!(
        alive(),
        2,
        "practice marking replaces only its own chat when access changes"
    );
    ask(
        "warm-chat-0002",
        &[
            ("user", "edited question"),
            ("assistant", "x"),
            ("user", "and then?"),
        ],
        false,
    );
    assert_eq!(
        alive(),
        2,
        "an edited history replaces only its own session"
    );
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[test]
fn felix_task_descriptions_use_the_same_cards_permissions_and_undo_in_both_protocols() {
    struct TaskCall(tools::Call);
    impl chat::Conversation for TaskCall {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.call)(&self.0.name, &self.0.args);
            (exchange.piece)("Ready.");
            Ok("Ready.".into())
        }
    }
    for native in [false, true] {
        for access in ["ask", "auto", "read"] {
            let (mut state, _dir, _) = state_with(&[]);
            let install = |state: &mut AppState, name: &str, args: serde_json::Value| {
                let call = tools::Call {
                    name: name.into(),
                    args,
                };
                if native {
                    state.converse = Some(Arc::new(
                        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
                            Some(Box::new(TaskCall(call.clone())) as Box<dyn chat::Conversation>)
                        },
                    ));
                    state.chat = Some(scripted(vec!["Ready."]).0);
                } else {
                    let mut args = call.args.clone();
                    args["name"] = serde_json::json!(call.name);
                    let text = format!("<tool>{args}</tool>");
                    let replies =
                        Arc::new(Mutex::new(vec![text, "Ready.".to_string()].into_iter()));
                    state.chat = Some(Arc::new(
                        move |_: &str,
                              _: &str,
                              _: u32,
                              piece: &mut dyn FnMut(&str),
                              _: &mut dyn FnMut(),
                              _: &dyn Fn() -> bool| {
                            let reply = replies
                                .lock()
                                .unwrap()
                                .next()
                                .unwrap_or_else(|| "Ready.".into());
                            piece(&reply);
                            Ok(reply.into())
                        },
                    ));
                }
            };
            let act = |state: &AppState, proposal: &serde_json::Value| {
                json_of(run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(serde_json::from_value(proposal.clone()).unwrap()),
                )))
            };
            let settle = |state: &AppState, lines: &[serde_json::Value]| {
                let proposal = lines.iter().find_map(|line| line.get("proposal"));
                if access == "read" {
                    assert!(proposal.is_none());
                    return None;
                }
                let proposal = proposal.unwrap();
                Some(if access == "ask" {
                    act(state, proposal)
                } else {
                    proposal.clone()
                })
            };
            install(
                &mut state,
                "add_task",
                serde_json::json!({"title": "Read heaps", "description": "Read chapter 3\nBring café notes"}),
            );
            let lines =
                chat_lines_with(&state, "add a study task with these details", Some(access));
            settle(&state, &lines);
            if access == "read" {
                assert!(crate::tasks::load_all(&state.chats).is_empty());
                continue;
            }
            let task = crate::tasks::load_all(&state.chats).remove(0);
            assert_eq!(task.description, "Read chapter 3\nBring café notes");
            for value in ["Focus on examples", ""] {
                install(
                    &mut state,
                    "update_task",
                    serde_json::json!({"task": task.id, "description": value}),
                );
                let lines = chat_lines_with(&state, "change the task description", Some(access));
                let applied = settle(&state, &lines).unwrap();
                assert_eq!(
                    crate::tasks::load(&state.chats, &task.id)
                        .unwrap()
                        .description,
                    value
                );
                act(&state, &applied["undo"]);
                assert_eq!(
                    crate::tasks::load(&state.chats, &task.id)
                        .unwrap()
                        .description,
                    task.description
                );
            }
            install(
                &mut state,
                "list_schedule",
                serde_json::json!({"days": "7"}),
            );
            let mut desk = tools::Desk::new(vec![], 8000);
            let read = crate::routes::organize::run(
                &state,
                &mut desk,
                tools::Access::Read,
                &tools::Call {
                    name: "list_schedule".into(),
                    args: serde_json::json!({"days":"7"}),
                },
            );
            assert!(read.result.contains("Read chapter 3\\nBring café notes"));
            let mut before_delete = crate::tasks::load(&state.chats, &task.id).unwrap();
            before_delete.repeat = vec![0, 2];
            before_delete.done_on = vec![chrono::NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()];
            before_delete.note = Some("note-example".into());
            crate::tasks::put(&state.chats, &before_delete).unwrap();
            install(
                &mut state,
                "delete_task",
                serde_json::json!({"task": task.id}),
            );
            let lines = chat_lines_with(&state, "delete that study task", Some(access));
            let deleted = settle(&state, &lines).unwrap();
            assert!(crate::tasks::load(&state.chats, &task.id).is_none());
            act(&state, &deleted["undo"]);
            let restored = crate::tasks::load(&state.chats, &task.id).unwrap();
            assert_eq!(restored.repeat, before_delete.repeat);
            assert_eq!(restored.done_on, before_delete.done_on);
            assert_eq!(restored.note, before_delete.note);
            assert!(restored.updated_at > before_delete.updated_at);
            assert_eq!(
                crate::tasks::load(&state.chats, &task.id)
                    .unwrap()
                    .description,
                task.description
            );
        }
    }
}

#[test]
fn repeated_identical_failed_calls_get_one_final_answer_without_exhausting_the_budget() {
    for broken in [true, false] {
        let (mut state, _dir, _) = state_with(&[]);
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let called = Arc::clone(&count);
        state.chat = Some(Arc::new(move |system, _, _, piece, _, _| {
            called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let text = if system.contains(tools::NO_MORE_TOOLS) {
                "I could not open that note. Choose an existing note and try again."
            } else if broken {
                "<tool>{read_note: missing}</tool>"
            } else {
                "<tool>{\"name\":\"read_note\",\"note\":\"n999\"}</tool>"
            };
            piece(text);
            Ok(text.to_string().into())
        }));
        let lines = chat_lines(&state, "Read the note");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(lines.iter().filter(|l| l.get("step").is_some()).count(), 2);
        let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
        assert!(shown.contains("Choose an existing note"));
        assert!(!shown.contains("<tool>"));
    }
}

#[test]
fn hidden_study_grading_is_counted_and_only_runs_when_the_verdict_is_missing() {
    for marker in [false, true] {
        for judge in ["correct", "not a verdict", "not correct"] {
            let (mut state, _dir, _) = state_with(&[]);
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let called = Arc::clone(&count);
            state.chat = Some(Arc::new(move |system, _, _, piece, _, _| {
                called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let text = if system == crate::routes::felix::JUDGE {
                    judge
                } else if marker {
                    "[[correct]] A queue is first in, first out."
                } else {
                    "A queue is first in, first out."
                };
                piece(text);
                Ok(chat::Reply {
                    text: text.into(),
                    spent: Some(chat::Spent {
                        input: 100,
                        output: 10,
                        ..chat::Spent::default()
                    }),
                })
            }));
            let body = serde_json::from_value(serde_json::json!({"practice":true,"mark":true,"messages":[{"role":"user","text":"A queue is first in, first out."}]})).unwrap();
            let bytes = run(async {
                let response = chat_reply(State(state.clone()), Json(body)).await;
                axum::body::to_bytes(response.into_body(), 1_000_000)
                    .await
                    .unwrap()
            });
            let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let calls = if marker { 1 } else { 2 };
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), calls);
            let spent = lines.iter().find_map(|l| l.get("spent")).unwrap();
            assert_eq!(spent["input"], calls * 100);
            assert_eq!(spent["output"], calls * 10);
            assert_eq!(spent["steps"], calls);
        }
    }
}

#[test]
fn closing_a_reply_stops_follow_up_tools_grading_and_fallbacks() {
    for failure in [false, true] {
        let (mut state, _dir, _) = state_with(&[]);
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let (resume, continued) = std::sync::mpsc::sync_channel(1);
        let continued = Arc::new(Mutex::new(continued));
        let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&called);
        state.chat = Some(Arc::new(move |_, _, _, piece, _, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ready.send(()).unwrap();
            continued
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            if failure {
                anyhow::bail!("fixture failure after stop");
            }
            let text = "<tool>{\"name\":\"search_notes\",\"query\":\"queue\"}</tool>";
            piece(text);
            Ok(text.to_string().into())
        }));
        run(async {
            let body = serde_json::from_value(serde_json::json!({"practice":true,"mark":true,"messages":[{"role":"user","text":"Check my answer"}]})).unwrap();
            let response = chat_reply(State(state.clone()), Json(body)).await;
            let began = tokio::task::spawn_blocking(move || {
                started.recv_timeout(std::time::Duration::from_secs(5))
            })
            .await
            .unwrap();
            began.unwrap();
            drop(response);
            resume.send(()).unwrap();
            let wait = std::time::Instant::now();
            while !state.steering.open_ids().is_empty()
                && wait.elapsed() < std::time::Duration::from_secs(5)
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            assert!(state.steering.open_ids().is_empty());
        });
        assert_eq!(called.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

#[test]
fn native_repeated_failures_finish_without_more_tools_or_a_refusal_retry() {
    struct Failed {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl chat::Conversation for Failed {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            for _ in 0..=tools::MOST_STEPS {
                if (exchange.stopped)() {
                    return Ok("".to_string().into());
                }
                if (exchange.tools_done)() {
                    (exchange.piece)("Choose an existing note and try again.");
                    return Ok("Choose an existing note and try again.".to_string().into());
                }
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                (exchange.call)("read_note", &serde_json::json!({"note":"n999"}));
            }
            anyhow::bail!("fixture exceeded the failed call budget")
        }
    }
    let (mut state, _dir, _) = state_with(&[]);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    state.chat = Some(Arc::new(|_, _, _, _, _, _| {
        anyhow::bail!("unexpected fallback")
    }));
    state.converse = Some(Arc::new(move |_, _| {
        Some(Box::new(Failed {
            calls: Arc::clone(&count),
        }))
    }));
    let lines = chat_lines(&state, "Read my note");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Choose an existing note and try again.");
    assert!(lines.iter().all(|l| l.get("restart").is_none()
        && l.get("reset").is_none()
        && l.get("error").is_none()));
}

#[test]
fn foreground_answers_are_bounded_before_context_preparation() {
    let (mut state, _dir, _) = state_with(&[]);
    state.chat = Some(Arc::new(|_, _, _, _, _, _| {
        anyhow::bail!("busy answers must not reach the model")
    }));
    let mut held = Vec::new();
    while let Some(slot) = state.work.foreground() {
        held.push(slot);
    }
    let body = serde_json::from_value(
        serde_json::json!({"messages":[{"role":"user","text":"A question"}]}),
    )
    .unwrap();
    let response = run(chat_reply(State(state.clone()), Json(body)));
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(json_of(response)["error"]
        .as_str()
        .unwrap()
        .contains("Wait"));
    drop(held);
    assert!(state.work.foreground().is_some());
}

#[test]
fn structured_study_verdicts_avoid_hidden_grading_on_both_protocols_and_do_not_mark_hints() {
    struct Mark {
        verdict: &'static str,
    }
    impl chat::Conversation for Mark {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.piece)("A queue takes the oldest item first.");
            (exchange.call)(
                "quiz",
                &serde_json::json!({"kind":"free_response","question":"What is the invariant?","answer":"FIFO","verdict":self.verdict}),
            );
            Ok("A queue takes the oldest item first.".into())
        }
    }
    for native in [false, true] {
        for marking in [false, true] {
            for verdict in ["correct", "incorrect"] {
                let (mut state, _dir, _) = state_with(&[]);
                let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let called = Arc::clone(&calls);
                state.chat = Some(Arc::new(move |system, _, _, piece, _, _| {
                    assert_ne!(
                        system,
                        crate::routes::felix::JUDGE,
                        "a structured verdict needs no hidden request"
                    );
                    let text = if called.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        format!("A queue takes the oldest item first.\n<tool>{{\"name\":\"quiz\",\"kind\":\"free_response\",\"question\":\"What is the invariant?\",\"answer\":\"FIFO\",\"verdict\":\"{verdict}\"}}</tool>")
                    } else {
                        "Your next question is ready.".into()
                    };
                    piece(&text);
                    Ok(text.into())
                }));
                if native {
                    state.converse = Some(Arc::new(move |_, _| {
                        Some(Box::new(Mark { verdict }) as Box<dyn chat::Conversation>)
                    }));
                }
                let body = serde_json::from_value(serde_json::json!({"practice":true,"mark":marking,"messages":[{"role":"user","text":"A queue takes the oldest item first."}]})).unwrap();
                let bytes = run(async {
                    let response = chat_reply(State(state.clone()), Json(body)).await;
                    axum::body::to_bytes(response.into_body(), 1_000_000)
                        .await
                        .unwrap()
                });
                let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
                    .unwrap()
                    .lines()
                    .map(|s| serde_json::from_str(s).unwrap())
                    .collect();
                assert!(lines.iter().any(|l| l.get("quiz").is_some()), "{lines:?}");
                assert_eq!(
                    lines.iter().find_map(|l| l["verdict"].as_str()),
                    marking.then_some(verdict)
                );
                let usage = serde_json::to_value(state.usage.snapshot()).unwrap();
                assert!(!usage
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["purpose"] == "study_grading"));
                assert_eq!(
                    calls.load(std::sync::atomic::Ordering::SeqCst),
                    if native { 0 } else { 2 }
                );
            }
        }
    }
}

#[test]
fn repeated_note_reads_reuse_only_identical_results_and_fresh_can_repeat_the_body() {
    struct Reader;
    impl chat::Conversation for Reader {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            let first = (exchange.call)("open_note", &serde_json::json!({"note":"n1"}));
            let second = (exchange.call)("open_note", &serde_json::json!({"note":"n1"}));
            let fresh =
                (exchange.call)("open_note", &serde_json::json!({"note":"n1","fresh":"yes"}));
            assert!(first.contains("Grounded material"));
            assert!(second.starts_with("Unchanged:"));
            assert!(fresh.contains("Grounded material"));
            (exchange.piece)("Read it.");
            Ok("Read it.".into())
        }
    }
    for native in [false, true] {
        let (mut state, _dir, ids) = state_with(&[("Graphs", "algorithms")]);
        {
            let mut store = state.fresh();
            store.find_note_mut(&ids[0]).unwrap().body = "Grounded material.\n".repeat(100);
            store.save().unwrap();
        }
        let (streamer, prompts) = scripted(vec![
            r#"<tool>{"name":"open_note","note":"n1"}</tool>"#,
            r#"<tool>{"name":"open_note","note":"n1"}</tool>"#,
            r#"<tool>{"name":"open_note","note":"n1","fresh":"yes"}</tool>"#,
            "Read it.",
        ]);
        state.chat = Some(streamer);
        if native {
            state.converse = Some(Arc::new(|_, _| {
                Some(Box::new(Reader) as Box<dyn chat::Conversation>)
            }));
        }
        let body = serde_json::from_value(serde_json::json!({"note":ids[0],"messages":[{"role":"user","text":"Read this note."}]})).unwrap();
        let bytes = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        assert!(!std::str::from_utf8(&bytes).unwrap().contains("\"error\""));
        if !native {
            let prompts = prompts.lock().unwrap();
            assert!(prompts[2].contains("Unchanged: the saved content was rechecked"));
            assert!(
                prompts[3].matches("Grounded material").count()
                    > prompts[2].matches("Grounded material").count()
            );
        }
        let tools = serde_json::to_value(state.usage.tools()).unwrap();
        assert_eq!(
            (tools["calls"].as_u64(), tools["reused"].as_u64()),
            (Some(3), Some(1))
        );
    }
}

#[test]
fn a_study_recap_can_carry_the_last_grade_without_a_hidden_request() {
    struct Recap;
    impl chat::Conversation for Recap {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.call)("end_session", &serde_json::json!({"verdict":"incorrect"}));
            (exchange.piece)("Review FIFO: the oldest item comes out first.");
            Ok("Review FIFO: the oldest item comes out first.".into())
        }
    }
    for native in [false, true] {
        let (mut state, _dir, _) = state_with(&[]);
        let (streamer, prompts) = scripted(vec![
            r#"<tool>{"name":"end_session","verdict":"incorrect"}</tool>"#,
            "Review FIFO: the oldest item comes out first.",
        ]);
        state.chat = Some(streamer);
        if native {
            state.converse = Some(Arc::new(|_, _| {
                Some(Box::new(Recap) as Box<dyn chat::Conversation>)
            }));
        }
        let body = serde_json::from_value(serde_json::json!({"practice":true,"mark":true,"session":{"ideas":["FIFO"],"closed":[false],"asked":1},"messages":[{"role":"user","text":"The newest queue item leaves first. Please finish the session after marking this."}]})).unwrap();
        let bytes = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(lines.iter().any(|l| l["session_end"] == true));
        assert!(lines.iter().any(|l| l["verdict"] == "incorrect"));
        let shown: String = lines.iter().filter_map(|line| line["t"].as_str()).collect();
        assert!(shown.contains("oldest"));
        assert_eq!(prompts.lock().unwrap().len(), if native { 0 } else { 2 });
    }
}

#[test]
fn conflicting_study_verdicts_are_checked_and_an_invalid_check_does_not_assign_a_grade() {
    struct Conflict;
    impl chat::Conversation for Conflict {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.piece)("[[correct]] Review the queue invariant.");
            (exchange.call)(
                "quiz",
                &serde_json::json!({"kind":"free_response","question":"What leaves first?","answer":"The oldest item.","verdict":"incorrect"}),
            );
            Ok("[[correct]] Review the queue invariant.".into())
        }
    }
    for verdict in ["correct", "incorrect", "not correct"] {
        let (mut state, _dir, _) = state_with(&[]);
        let (streamer, prompts) = scripted(vec![verdict]);
        state.chat = Some(streamer);
        state.converse = Some(Arc::new(|_, _| {
            Some(Box::new(Conflict) as Box<dyn chat::Conversation>)
        }));
        let body = serde_json::from_value(serde_json::json!({"chat":"conflicting-study-01","practice":true,"mark":true,"messages":[{"role":"user","text":"The oldest queue item leaves first."}]})).unwrap();
        let bytes = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let expected = if verdict == "not correct" {
            "ungraded"
        } else {
            verdict
        };
        assert!(lines.iter().any(|l| l["verdict"] == expected));
        assert_eq!(prompts.lock().unwrap().len(), 1);
        assert!(
            !state.warm.holding(),
            "the inconsistent provider history must not be reused"
        );
        let usage = serde_json::to_value(state.usage.snapshot()).unwrap();
        assert_eq!(
            usage
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["purpose"] == "study_grading")
                .unwrap()["calls"],
            1
        );
    }
}

#[test]
fn a_free_answer_is_marked_by_the_quick_model_without_a_full_felix_reply() {
    let (mut state, _d, _) = state_with(&[]);
    let mark = |state: &AppState, given: &str| {
        run(crate::routes::felix::mark_quiz(
            State(state.clone()),
            Json(serde_json::from_value(serde_json::json!({"question":"What does BFS use?","answer":"a queue","given":given,"explain":"Oldest first."})).unwrap()),
        ))
    };
    assert_eq!(
        mark(&state, "a queue").status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let asked = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&asked);
    state.quick = Some(Arc::new(move |system: &str, user: &str, most: u32| {
        heard
            .lock()
            .unwrap()
            .push((system.to_string(), user.to_string(), most));
        Ok(r#"Sure: {"correct": false, "feedback": "BFS uses a queue, not a stack: it visits the oldest node first."}"#.to_string())
    }));
    assert_eq!(mark(&state, "  ").status(), StatusCode::BAD_REQUEST);
    let marked = json_of(mark(&state, "a stack"));
    assert_eq!(marked["correct"], false);
    assert!(marked["feedback"]
        .as_str()
        .unwrap()
        .contains("oldest node first"));
    let (system, user, most) = asked.lock().unwrap()[0].clone();
    assert!(system.contains("Reply with only JSON") && most <= 600);
    assert!(
        user.contains("<student_answer>\na stack")
            && user.contains("<model_answer>\na queue")
            && user.contains("Oldest first.")
    );
    state.quick = Some(Arc::new(|_: &str, _: &str, _| Ok("no idea".to_string())));
    assert_eq!(mark(&state, "a stack").status(), StatusCode::BAD_GATEWAY);
}

fn a_mark() -> Json<crate::routes::felix::MarkBody> {
    Json(
        serde_json::from_value(
            serde_json::json!({"question": "Why a queue?", "answer": "Order", "given": "Order"}),
        )
        .unwrap(),
    )
}

#[test]
fn marking_runs_two_at_a_time() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (mut state, _dir, _) = state_with(&[]);
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let (now, top) = (Arc::clone(&running), Arc::clone(&most));
    state.quick = Some(Arc::new(move |_: &str, _: &str, _| {
        let at = now.fetch_add(1, Ordering::AcqRel) + 1;
        top.fetch_max(at, Ordering::AcqRel);
        std::thread::sleep(std::time::Duration::from_millis(300));
        now.fetch_sub(1, Ordering::AcqRel);
        Ok(serde_json::json!({"correct": true, "feedback": "Right."}).to_string())
    }));
    let statuses: Vec<StatusCode> = run(async {
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let state = state.clone();
                tokio::spawn(async move {
                    crate::routes::felix::mark_quiz(State(state), a_mark())
                        .await
                        .status()
                })
            })
            .collect();
        let mut out = Vec::new();
        for handle in handles {
            out.push(handle.await.unwrap());
        }
        out
    });
    assert!(
        statuses.iter().all(|s| *s == StatusCode::OK),
        "{statuses:?}"
    );
    assert_eq!(most.load(Ordering::Acquire), 2);
}

#[test]
fn an_abandoned_quiz_mark_stops_its_work() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (mut state, _dir, _) = state_with(&[]);
    let started = Arc::new(AtomicBool::new(false));
    let returned = Arc::new(Mutex::new(None::<std::time::Instant>));
    let (begun, ended) = (Arc::clone(&started), Arc::clone(&returned));
    state.quick = Some(Arc::new(move |_: &str, _: &str, _| {
        begun.store(true, Ordering::Release);
        let since = std::time::Instant::now();
        while !leo_core::children::current_stop().is_some_and(|s| s.asked())
            && since.elapsed() < std::time::Duration::from_secs(10)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        *ended.lock().unwrap() = Some(std::time::Instant::now());
        Ok(serde_json::json!({"correct": true, "feedback": "Right."}).to_string())
    }));
    let dropped = run(async {
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            crate::routes::felix::mark_quiz(State(state.clone()), a_mark()),
        )
        .await;
        std::time::Instant::now()
    });
    assert!(started.load(Ordering::Acquire));
    let waited = std::time::Instant::now();
    while returned.lock().unwrap().is_none() && waited.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let at = returned.lock().unwrap().expect("the writer never returned");
    assert!(at.duration_since(dropped) < std::time::Duration::from_secs(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quiz_mark_whose_connection_closes_stops_its_work() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    let started = Arc::new(AtomicBool::new(false));
    let returned = Arc::new(Mutex::new(None::<std::time::Instant>));
    let (begun, ended) = (Arc::clone(&started), Arc::clone(&returned));
    let quick: crate::graph::Writer = Arc::new(move |_: &str, _: &str, _| {
        begun.store(true, Ordering::Release);
        let since = std::time::Instant::now();
        while !leo_core::children::current_stop().is_some_and(|s| s.asked())
            && since.elapsed() < std::time::Duration::from_secs(10)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        *ended.lock().unwrap() = Some(std::time::Instant::now());
        Ok(serde_json::json!({"correct": true, "feedback": "Right."}).to_string())
    });
    let dir = tempfile::tempdir().unwrap();
    let places = crate::Places {
        store: Store::load_from(&dir.path().join("notes")).unwrap(),
        sessions: dir.path().join("sessions.json"),
    };
    let options = crate::StartOptions {
        port: 0,
        token: TOKEN.into(),
    };
    let powers = Powers {
        quick: Some(quick),
        ..Default::default()
    };
    let running = crate::start_in(places, options, powers).await.unwrap();
    let port = running.port();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!("GET /?token={TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut signed_in = Vec::new();
    stream.read_to_end(&mut signed_in).await.unwrap();
    let cookie = String::from_utf8_lossy(&signed_in)
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
                .and_then(|value| value.split(';').next())
                .map(str::to_string)
        })
        .unwrap();
    let body = serde_json::json!({"question": "Why a queue?", "answer": "Order", "given": "Order"})
        .to_string();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!(
                "POST /api/quiz/mark HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let waited = std::time::Instant::now();
    while !started.load(Ordering::Acquire) && waited.elapsed() < std::time::Duration::from_secs(5) {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(started.load(Ordering::Acquire), "the mark never started");
    drop(stream);
    let closed = std::time::Instant::now();
    while returned.lock().unwrap().is_none() && closed.elapsed() < std::time::Duration::from_secs(5)
    {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let at = returned.lock().unwrap().expect("the writer never returned");
    assert!(at.duration_since(closed) < std::time::Duration::from_secs(1));
    running.stop().await;
}

#[test]
fn an_idle_chat_is_compacted_so_the_next_message_sends_a_summary_and_the_last_turns() {
    let (mut state, _dir, _) = state_with(&[]);
    let writer: crate::graph::Writer = Arc::new(|system: &str, _: &str, _: u32| {
        assert!(system.contains("You keep the memory of a long study chat"));
        Ok("They practised heaps and missed sift down.".into())
    });
    state.housekeeping = Some(writer);
    let (streamer, prompts) = scripted(vec!["Sure."]);
    state.chat = Some(streamer);
    let id = "chat-compact-001";
    let compact = |messages: &[chat::Turn], chat: &str| {
        json_of(run(crate::routes::felix::compact_chat(
            State(state.clone()),
            Json(crate::routes::felix::CompactBody {
                chat: chat.into(),
                messages: messages.to_vec(),
            }),
        )))
    };
    let mut messages: Vec<chat::Turn> = (0..10)
        .map(|i| chat::Turn {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            text: format!("turn {i} {}", "about heaps ".repeat(60)),
        })
        .collect();
    assert_eq!(
        run(crate::routes::felix::compact_chat(
            State(state.clone()),
            Json(crate::routes::felix::CompactBody {
                chat: id.into(),
                messages: messages.clone(),
            }),
        ))
        .status(),
        404,
        "only a saved chat is compacted"
    );
    crate::chats::save(
        &state.chats,
        id,
        serde_json::from_value(serde_json::json!({ "messages": [] })).unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(
        compact(&messages[..5], id)["compacting"],
        false,
        "a short chat costs less to send again than to summarise"
    );
    assert_eq!(compact(&messages, id)["compacting"], true);
    let started = std::time::Instant::now();
    while crate::chats::load(&state.chats, id).is_none_or(|c| c.memory.is_none()) {
        assert!(started.elapsed().as_secs() < 5, "nothing was compacted");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let memory = crate::chats::load(&state.chats, id)
        .unwrap()
        .memory
        .unwrap();
    assert_eq!(memory.upto, 10 - chat::COMPACT_KEEP);
    assert_eq!(
        compact(&messages, id)["compacting"],
        false,
        "nothing new to compact"
    );
    messages.push(chat::Turn {
        role: "user".into(),
        text: "next question please".into(),
    });
    let body = chat::ChatBody {
        messages: messages.clone(),
        note: None,
        refs: vec![],
        chat: Some(id.into()),
        files: vec![],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
        selection: None,
        session: None,
    };
    run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
    });
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("They practised heaps and missed sift down."));
    assert!(
        !prompt.contains("turn 1 about"),
        "the summary replaces the older turns"
    );
    assert!(prompt.contains("turn 6 about") && prompt.contains("turn 9 about"));
}

fn last_result(prompt: &str) -> &str {
    prompt.rfind("<tool_result").map_or("", |at| &prompt[at..])
}

fn sprint(state: &AppState) -> crate::boards::Board {
    let board = crate::boards::new_board("Sprint", chrono::Utc::now());
    crate::boards::put_board(&state.chats, &board).unwrap();
    let made = run(crate::routes::boards::create_card(
        State(state.clone()),
        Path(board.id.clone()),
        Json(serde_json::from_value(serde_json::json!({ "title": "Login" })).unwrap()),
    ));
    assert_eq!(made.status(), StatusCode::CREATED);
    board
}

fn board_card(state: &AppState, title: &str) -> Option<crate::boards::Card> {
    crate::boards::all_cards(&state.chats)
        .into_iter()
        .find(|c| c.title == title)
}

fn act(state: &AppState, card: &serde_json::Value) -> serde_json::Value {
    let response = run(crate::routes::organize::act_route(
        State(state.clone()),
        Json(serde_json::from_value(card.clone()).unwrap()),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    json_of(response)
}

#[test]
fn felix_reads_and_changes_boards_in_auto_with_undo_and_asks_before_deleting_one() {
    let (mut state, _d, _) = state_with(&[]);
    let board = sprint(&state);
    let (streamer, prompts) = scripted(vec![
        r#"<tool>{"name":"list_boards"}</tool>"#,
        r#"<tool>{"name":"open_board"}</tool>"#,
        r#"<tool>{"name":"add_card","column":"to do","title":"Write tests","priority":"High","top":true}</tool>"#,
        r#"<tool>{"name":"move_card","card":"login","column":"Done"}</tool>"#,
        r#"<tool>{"name":"update_card","card":"Write tests","description":"Cover the edge cases"}</tool>"#,
        r#"<tool>{"name":"delete_board","board":"Sprint"}</tool>"#,
        "Added the card, moved Login to Done and asked before deleting the board.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "sort out my sprint board", Some("auto"));
    let prompts = prompts.lock().unwrap();
    for (i, prompt) in prompts.iter().enumerate().skip(1) {
        assert!(
            !last_result(prompt).contains("That did not work"),
            "turn {i}: {}",
            last_result(prompt)
        );
    }
    assert!(prompts[0].contains("### add_card") && prompts[0].contains("### delete_board"));
    assert!(prompts[1].contains("“Sprint”") && prompts[1].contains("To do"));
    assert!(prompts[2].contains("“Login”"));
    let shown: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("proposal")).collect();
    assert_eq!(shown.len(), 4);
    assert!(shown[..3].iter().all(|p| p["state"] == "applied"));
    assert_eq!(shown[3]["kind"], "delete_board");
    assert!(
        shown[3].get("state").is_none(),
        "deleting a board always asks"
    );
    let todo = board.columns[0].id.clone();
    let done = board.columns[2].id.clone();
    let tests = board_card(&state, "Write tests").unwrap();
    assert_eq!(
        (tests.column.as_str(), tests.priority.as_str()),
        (todo.as_str(), "high")
    );
    assert_eq!(tests.description, "Cover the edge cases");
    assert_eq!(board_card(&state, "Login").unwrap().column, done);
    let order: Vec<String> = crate::boards::cards_of(&state.chats, &board)
        .into_iter()
        .filter(|c| c.column == todo)
        .map(|c| c.title)
        .collect();
    assert_eq!(order, ["Write tests"]);

    act(&state, &shown[1]["undo"]);
    assert_eq!(board_card(&state, "Login").unwrap().column, todo);
    act(&state, &shown[2]["undo"]);
    assert_eq!(board_card(&state, "Write tests").unwrap().description, "");

    let deleted = act(&state, shown[3]);
    assert!(crate::boards::board(&state.chats, &board.id).is_none());
    let restored = act(&state, &deleted["undo"]);
    assert_eq!(restored["undo"]["kind"], "delete_board");
    let back = crate::boards::board(&state.chats, &board.id).unwrap();
    assert_eq!(crate::boards::cards_of(&state.chats, &back).len(), 2);
    act(&state, &shown[0]["undo"]);
    assert!(board_card(&state, "Write tests").is_none());
}

#[test]
fn board_changes_are_cards_in_ask_refused_in_read_only_and_natively_called_without_failures() {
    let (mut state, _d, _) = state_with(&[]);
    let board = sprint(&state);
    let (streamer, _) = scripted(vec![
        r#"<tool>{"name":"add_card","board":"sprint","title":"Ship it","subheading":"Friday"}</tool>"#,
        r#"<tool>{"name":"change_board","columns":"To do | Doing | Review | Done","new_name":"Sprint 2"}</tool>"#,
        "Suggested.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "add a card and a review column", Some("ask"));
    let shown: Vec<serde_json::Value> = lines
        .iter()
        .filter_map(|l| l.get("proposal").cloned())
        .collect();
    assert_eq!(shown.len(), 2, "{lines:?}");
    assert!(board_card(&state, "Ship it").is_none(), "Ask only suggests");
    let added = act(&state, &shown[0]);
    assert_eq!(board_card(&state, "Ship it").unwrap().subheading, "Friday");
    act(&state, &shown[1]);
    let changed = crate::boards::board(&state.chats, &board.id).unwrap();
    let names: Vec<&str> = changed.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        (changed.name.as_str(), names),
        ("Sprint 2", vec!["To do", "Doing", "Review", "Done"])
    );
    assert_eq!(
        changed.columns[0].id, board.columns[0].id,
        "a kept column keeps its cards"
    );
    let undone = act(&state, &added["undo"]);
    assert!(board_card(&state, "Ship it").is_none());
    act(&state, &undone["undo"]);
    assert!(board_card(&state, "Ship it").is_some());

    let (streamer, prompts) = scripted(vec![
        r#"<tool>{"name":"add_card","title":"Nope"}</tool>"#,
        "Switch to Ask or Auto.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "add a card", Some("read"));
    assert!(lines.iter().all(|l| l.get("proposal").is_none()));
    assert!(board_card(&state, "Nope").is_none());
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("### open_board") && !prompts[0].contains("### add_card"));
    assert!(prompts[1].contains("read only"));

    let results = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&results);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, specs: &[chat::ToolSpec]| {
            assert!(specs.iter().any(|s| s.name == "move_card"));
            assert!(specs.iter().all(|s| !s.name.starts_with("interview_")));
            Some(Box::new(Native {
                script: vec![
                    (
                        "open_board",
                        serde_json::json!({ "board": "Sprint 2", "card": "Login" }),
                    ),
                    (
                        "move_card",
                        serde_json::json!({ "card": "Login", "column": "Review", "position": "top" }),
                    ),
                    (
                        "update_card",
                        serde_json::json!({ "card": "Login", "priority": "urgent" }),
                    ),
                    (
                        "add_card",
                        serde_json::json!({ "title": "Native", "top": true, "priority": null }),
                    ),
                    (
                        "add_board",
                        serde_json::json!({ "title": "Thesis", "columns": ["Ideas", "Writing"] }),
                    ),
                ],
                answer: "Done.",
                results: Arc::clone(&seen),
                fail: false,
            }) as Box<dyn chat::Conversation>)
        },
    ));
    let lines = chat_lines_with(&state, "move login to review", Some("auto"));
    let results = results.lock().unwrap();
    for result in results.iter() {
        assert!(!result.starts_with("That did not work"), "{result}");
    }
    let review = changed.columns[2].id.clone();
    let login = board_card(&state, "Login").unwrap();
    assert_eq!(
        (login.column.as_str(), login.priority.as_str()),
        (review.as_str(), "high")
    );
    assert!(board_card(&state, "Native").is_some());
    let thesis = crate::boards::boards(&state.chats)
        .into_iter()
        .find(|b| b.name == "Thesis")
        .unwrap();
    assert_eq!(
        thesis
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Ideas", "Writing"]
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l["proposal"]["state"] == "applied")
            .count(),
        4
    );
}

#[test]
fn felix_brings_a_trashed_folder_back_with_its_notes_and_offers_to_trash_it_again() {
    let (mut state, _d, ids) = state_with(&[("Week one", "cs130"), ("Lab", "cs130/labs")]);
    let _ = run(move_to_trash(
        State(state.clone()),
        Json(
            serde_json::from_value::<TrashMove>(serde_json::json!({ "dirs": ["cs130"] })).unwrap(),
        ),
    ))
    .unwrap();
    assert!(state.fresh().find_note(&ids[0]).is_none());
    let (streamer, prompts) = scripted(vec![
        r#"<tool>{"name":"restore_folder","folder":"cs130/"}</tool>"#,
        "It is back.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "bring cs130 back", Some("auto"));
    let result = last_result(&prompts.lock().unwrap()[1]).to_string();
    assert!(!result.contains("That did not work"), "{result}");
    let card = lines.iter().find_map(|l| l.get("proposal")).unwrap();
    assert_eq!(
        (card["kind"].as_str(), card["state"].as_str()),
        (Some("restore_folder"), Some("applied"))
    );
    assert_eq!(card["undo"]["kind"], "delete_folder");
    let store = state.fresh();
    assert_eq!(store.find_note(&ids[0]).unwrap().directory, "cs130");
    assert_eq!(store.find_note(&ids[1]).unwrap().directory, "cs130/labs");
}

#[test]
fn tool_arguments_that_are_not_text_are_read_as_text() {
    let call = tools::Call {
        name: "add_board".into(),
        args: serde_json::json!({ "title": "Plan", "top": true, "n": 3, "columns": ["A", "B"], "gone": null }),
    };
    let fixed = call.normalized();
    assert_eq!(fixed.text("top"), "yes");
    assert_eq!(fixed.text("n"), "3");
    assert_eq!(fixed.text("columns"), r#"["A","B"]"#);
    assert!(fixed.args["gone"].is_null());
    assert!(tools::check(&fixed).is_ok());
    assert_eq!(
        crate::board_tools::column_names(&fixed.text("columns")).unwrap(),
        ["A", "B"]
    );
    let asked = tools::Call {
        name: "ask_user".into(),
        args: serde_json::json!({ "question": "Which?", "options": ["BFS", "DFS", 2] }),
    }
    .normalized();
    assert_eq!(asked.text("options"), "BFS | DFS | 2");
    assert_eq!(
        tools::asked_from(&asked).unwrap().options,
        ["BFS", "DFS", "2"]
    );
    let planned = tools::Call {
        name: "study_plan".into(),
        args: serde_json::json!({ "ideas": ["Root", "Sift down"] }),
    }
    .normalized();
    assert_eq!(tools::plan_from(&planned).unwrap(), ["Root", "Sift down"]);
}

#[test]
fn a_retired_interview_request_uses_chat_and_refuses_removed_tools_cleanly() {
    let (mut state, _dir, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        r#"<tool>{"name":"interview_challenge","title":"Old client request"}</tool>"#,
        "We can talk through the question here.",
    ]);
    state.chat = Some(streamer);
    let body = serde_json::from_value(serde_json::json!({
        "mode": "interview", "interview_problem": "retired-problem", "chat": "retired-chat-0001",
        "messages": [{"role":"user", "text":"Continue our conversation"}]
    }))
    .unwrap();
    let text = run(async {
        let response = chat_reply(State(state), Json(body)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(lines
        .iter()
        .all(|l| l.get("proposal").is_none() && l.get("error").is_none()));
    assert!(lines.iter().any(|l| l["done"] == true));
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("there are no modes to choose"));
    assert!(!prompts[0].contains("### interview_challenge"));
    assert!(!prompts[0].contains("<interview_material>"));
    assert!(prompts[1].contains("there is no tool called"));
}

#[test]
fn taking_back_the_last_reply_removes_it_and_refuses_a_stale_count() {
    use crate::routes::felix::{drop_last_messages, DropBody};
    let (state, _d, _ids) = state_with(&[]);
    let said = |role: &str| serde_json::json!({ "role": role, "text": role });
    chats::save(
        &state.chats,
        "chat-take-0001",
        chats::Saving {
            existed: false,
            title: String::new(),
            refs: vec![],
            messages: vec![said("user"), said("assistant")],
        },
        chrono::Utc::now(),
    )
    .unwrap();
    let drop = |total: usize| {
        run(drop_last_messages(
            State(state.clone()),
            Path("chat-take-0001".into()),
            Json(DropBody { count: 2, total }),
        ))
    };
    assert_eq!(drop(4), axum::http::StatusCode::CONFLICT);
    assert_eq!(
        chats::load(&state.chats, "chat-take-0001")
            .unwrap()
            .messages
            .len(),
        2
    );
    assert_eq!(drop(2), axum::http::StatusCode::NO_CONTENT);
    assert!(chats::load(&state.chats, "chat-take-0001")
        .unwrap()
        .messages
        .is_empty());
}

#[test]
fn a_redo_after_a_broken_answer_is_told_what_leo_already_changed() {
    struct MakesThenBreaks;
    impl chat::Conversation for MakesThenBreaks {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            let _ = (exchange.call)(
                "create_note",
                &serde_json::json!({ "title": "Lab 3", "body": "Submit on Friday." }),
            );
            anyhow::bail!("claude stopped before its final text")
        }
    }
    let (mut state, _d, _) = state_with(&[]);
    state.converse = Some(Arc::new(|_: &chat::Instructions, _: &[chat::ToolSpec]| {
        Some(Box::new(MakesThenBreaks) as Box<dyn chat::Conversation>)
    }));
    let (streamer, prompts) = scripted(vec!["The note for the lab is made."]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(
        &state,
        "make a note to submit lab 3 on friday",
        Some("auto"),
    );
    assert!(lines.iter().any(|l| l.get("reset").is_some()), "{lines:?}");
    let prompts = prompts.lock().unwrap();
    assert!(
        prompts[0].contains("do not make them again"),
        "{}",
        prompts[0]
    );
    assert!(prompts[0].contains("create \"Lab 3\""), "{}", prompts[0]);
    let made = state
        .fresh()
        .notes
        .iter()
        .filter(|n| n.title == "Lab 3")
        .count();
    assert_eq!(made, 1);
}

#[test]
fn an_auto_edit_of_a_huge_note_still_sends_its_card_and_finishes() {
    let (mut state, _d, ids) = state_with(&[("Lecture transcript", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            format!("Intro line.\n{}", "word ".repeat(120_000));
        store.save().unwrap();
    }
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"Lecture transcript\", \"find\": \"Intro line.\", \"replace\": \"Opening line.\"}</tool>",
        "I renamed the opening line.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(
        &state,
        "fix the first line of my lecture transcript",
        Some("auto"),
    );
    let shown: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("proposal")).collect();
    assert_eq!(
        shown.len(),
        1,
        "{:?}",
        lines
            .iter()
            .map(|l| l.to_string().chars().take(120).collect::<String>())
            .collect::<Vec<_>>()
    );
    assert_eq!(shown[0]["state"], "applied");
    assert_eq!(shown[0]["note"], ids[0].as_str());
    assert!(shown[0].get("before").is_none());
    assert_eq!(lines.last().unwrap()["done"], true);
    assert!(state
        .fresh()
        .find_note(&ids[0])
        .unwrap()
        .body
        .starts_with("Opening line."));
}

#[test]
fn chat_database_reads_wait_off_the_async_worker() {
    let (mut state, _d, _) = state_with(&[]);
    let elsewhere = tempfile::tempdir().unwrap();
    state.chats = elsewhere.path().join("chats");
    state.chat = Some(scripted(vec!["A heap keeps the smallest on top."]).0);
    let db = chats::db_of(&state.chats);
    let (held, holding) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        db.with(|_| {
            held.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(600));
            Ok(())
        })
        .unwrap()
    });
    holding.recv().unwrap();
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "what is a heap?".into(),
        }],
        note: None,
        refs: vec![],
        chat: Some("chat-wait-0001".into()),
        files: vec![],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
        selection: None,
        session: None,
    };
    let longest = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let reply = tokio::spawn(chat_reply(State(state.clone()), Json(body)));
            let mut longest = std::time::Duration::ZERO;
            let mut last = std::time::Instant::now();
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                longest = longest.max(last.elapsed());
                last = std::time::Instant::now();
            }
            let _ = reply.await;
            longest
        });
    holder.join().unwrap();
    assert!(
        longest < std::time::Duration::from_millis(300),
        "{longest:?}"
    );
}

#[test]
fn the_search_tool_embeds_outside_the_notes_lock() {
    use crate::routes::felix::execute_tool;
    let (state, _d, ids) = state_with(&[("Queues", "")]);
    let note = state.store.lock().unwrap().store.notes[0].clone();
    state.vectors.put(
        &[vectors::Work {
            id: ids[0].clone(),
            hash: vectors::hash_of(&note),
            pieces: vec![(0, "x".into())],
            kept: Vec::new(),
        }],
        vec![vec![1.0; 8]],
    );
    let free = Arc::new(Mutex::new(None));
    let probe = Arc::clone(&state.store);
    let seen = Arc::clone(&free);
    let meaning: vectors::Meaning = Arc::new(move |texts: &[String], _| {
        *seen.lock().unwrap() = Some(probe.try_lock().is_ok());
        Ok(texts.iter().map(|_| vec![1.0; 8]).collect())
    });
    let mut desk = tools::Desk::new(Vec::new(), 40_000)
        .with_meaning(Some(meaning), Arc::clone(&state.vectors));
    let call = tools::Call {
        name: "search_notes".into(),
        args: serde_json::json!({"query": "queue order"}),
    };
    execute_tool(&state, &mut desk, &call, &|_| {}).unwrap();
    assert_eq!(*free.lock().unwrap(), Some(true));
}

pub(super) fn big_board(state: &AppState, columns: usize) -> crate::boards::Board {
    let mut board = crate::boards::new_board("Huge", chrono::Utc::now());
    board.columns.truncate(columns.max(1));
    crate::boards::put_board(&state.chats, &board).unwrap();
    for i in 0..300 {
        let now = chrono::Utc::now();
        crate::boards::put_card(
            &state.chats,
            &crate::boards::Card {
                id: format!("card-{i:04}"),
                board: board.id.clone(),
                column: board.columns[i % board.columns.len()].id.clone(),
                rank: i as f64,
                title: format!("Card {i}"),
                subheading: String::new(),
                description: "d".repeat(crate::boards::MOST_DESCRIPTION),
                priority: String::new(),
                created_at: now,
                updated_at: now,
                deleted: false,
            },
        )
        .unwrap();
    }
    board
}

#[test]
fn a_very_large_board_deletes_and_comes_back() {
    let (state, _d, _) = state_with(&[]);
    let board = big_board(&state, 3);
    let gone = run(crate::routes::organize::act_route(
        State(state.clone()),
        Json(
            serde_json::from_value(
                serde_json::json!({ "kind": "delete_board", "summary": "x", "board": board.id }),
            )
            .unwrap(),
        ),
    ));
    assert_eq!(gone.status(), StatusCode::OK);
    let gone = json_of(gone);
    assert!(crate::boards::board(&state.chats, &board.id).is_none());
    assert_eq!(gone["undo"]["kind"], "undelete_board");
    assert!(gone["undo"].to_string().len() < 1024);
    let back = act(&state, &gone["undo"]);
    assert_eq!(back["undo"]["kind"], "delete_board");
    let back = crate::boards::board(&state.chats, &board.id).unwrap();
    let cards = crate::boards::cards_of(&state.chats, &back);
    assert_eq!(cards.len(), 300);
    assert!(cards
        .iter()
        .all(|c| c.description.len() == crate::boards::MOST_DESCRIPTION));
}

#[test]
fn a_column_change_on_a_very_large_board_can_be_undone() {
    let (state, _d, _) = state_with(&[]);
    let board = big_board(&state, 3);
    let places = |state: &AppState| -> Vec<(String, String, f64)> {
        let mut all: Vec<_> = crate::boards::all_cards(&state.chats)
            .into_iter()
            .map(|c| (c.id, c.column, c.rank))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    };
    let before = places(&state);
    let kept: Vec<serde_json::Value> = board.columns[..2]
        .iter()
        .map(|c| serde_json::json!({ "id": c.id, "name": c.name }))
        .collect();
    let changed = act(
        &state,
        &serde_json::json!({
            "kind": "change_board", "summary": "x", "board": board.id,
            "columns": kept, "move_to": board.columns[0].id,
        }),
    );
    assert_eq!(changed["undo"]["kind"], "undo_board_change");
    assert!(changed["undo"].to_string().len() < 1024);
    assert_eq!(
        crate::boards::board(&state.chats, &board.id)
            .unwrap()
            .columns
            .len(),
        2
    );
    let undone = act(&state, &changed["undo"]);
    let back = crate::boards::board(&state.chats, &board.id).unwrap();
    assert_eq!(back.columns, board.columns);
    assert_eq!(places(&state), before);
    act(&state, &undone["undo"]);
    assert_eq!(
        crate::boards::board(&state.chats, &board.id)
            .unwrap()
            .columns
            .len(),
        2
    );
    assert!(crate::boards::all_cards(&state.chats)
        .iter()
        .all(|c| c.column != board.columns[2].id));
}

#[test]
fn a_saved_restore_board_card_still_applies() {
    let (state, _d, _) = state_with(&[]);
    let board = sprint(&state);
    let card = board_card(&state, "Login").unwrap();
    crate::boards::remove_board(&state.chats, &board, chrono::Utc::now()).unwrap();
    let restored = act(
        &state,
        &serde_json::json!({
            "kind": "restore_board", "summary": "Bring it back", "board": board, "cards": [card],
        }),
    );
    assert_eq!(restored["undo"]["kind"], "delete_board");
    assert!(crate::boards::board(&state.chats, &board.id).is_some());
    assert!(board_card(&state, "Login").is_some());
}

fn put_saved(
    state: &AppState,
    id: &str,
    existed: bool,
) -> (axum::http::StatusCode, serde_json::Value) {
    let body: crate::chats::Saving = serde_json::from_value(serde_json::json!({
        "messages": [{ "role": "user", "text": "hello" }],
        "existed": existed,
    }))
    .unwrap();
    let response = run(crate::routes::felix::put_chat(
        State(state.clone()),
        Path(id.into()),
        Json(body),
    ));
    (response.status(), json_of(response))
}

#[test]
fn a_save_that_says_the_chat_existed_creates_nothing_when_leo_no_longer_has_it() {
    let (state, _d, _) = state_with(&[]);
    let (status, _) = put_saved(&state, "chat-gone-0001", true);
    assert_eq!(status, axum::http::StatusCode::GONE);
    assert!(crate::chats::load(&state.chats, "chat-gone-0001").is_none());
}

#[test]
fn a_save_that_says_the_chat_existed_still_saves_when_leo_has_it() {
    let (state, _d, _) = state_with(&[]);
    assert_eq!(
        put_saved(&state, "chat-kept-0001", false).0,
        axum::http::StatusCode::OK
    );
    let (status, summary) = put_saved(&state, "chat-kept-0001", true);
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(summary["id"], "chat-kept-0001");
    assert!(crate::chats::load(&state.chats, "chat-kept-0001").is_some());
}

#[test]
fn a_first_save_without_the_flag_still_creates_the_chat() {
    let (state, _d, _) = state_with(&[]);
    assert_eq!(
        put_saved(&state, "chat-new-00001", false).0,
        axum::http::StatusCode::OK
    );
    assert!(crate::chats::load(&state.chats, "chat-new-00001").is_some());
}

#[test]
fn a_chat_expired_by_tidy_is_not_recreated_by_a_later_save_that_carries_the_flag() {
    let (state, _d, _) = state_with(&[]);
    assert_eq!(
        put_saved(&state, "chat-aged-0001", false).0,
        axum::http::StatusCode::OK
    );
    let later = chrono::Utc::now() + chrono::Duration::days(40);
    assert_eq!(crate::chats::tidy(&state.chats, Some(30), later), 1);
    assert_eq!(
        put_saved(&state, "chat-aged-0001", true).0,
        axum::http::StatusCode::GONE
    );
    assert!(crate::chats::load(&state.chats, "chat-aged-0001").is_none());
}
