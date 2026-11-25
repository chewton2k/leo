use super::*;
use crate::routes::combine;

fn with_writer(state: &mut AppState, reply: &'static str) -> Arc<Mutex<Vec<String>>> {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&asked);
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |system: &str, user: &str, _| {
            heard.lock().unwrap().push(format!("{system}\n{user}"));
            Ok(reply.to_string())
        })),
    ));
    asked
}

fn bodies(state: &AppState, ids: &[String], texts: [&str; 2]) {
    store_now(state, |store| {
        for (id, text) in ids.iter().zip(texts) {
            store.find_note_mut(id).unwrap().body = text.into();
        }
        store.save().unwrap();
        Ok(())
    })
    .unwrap();
}

pub(crate) fn transcript(id: &str) -> leo_core::recording::Archive {
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

fn started(state: &AppState, into: &str, with: &str) -> Response {
    run(combine::start(
        State(state.clone()),
        Path(into.to_string()),
        Json(serde_json::from_value(serde_json::json!({ "with": with })).unwrap()),
    ))
}

fn finished(state: &AppState, id: &str) -> serde_json::Value {
    for _ in 0..12_000 {
        let job = json_of(run(combine::job(
            State(state.clone()),
            Path(id.to_string()),
        )));
        if job["state"] != "working" {
            return job;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the combine never finished");
}

fn keep(state: &AppState, into: &str, value: serde_json::Value) -> Response {
    run(combine::keep(
        State(state.clone()),
        Path(into.to_string()),
        Json(serde_json::from_value(value).unwrap()),
    ))
}

#[test]
fn notes_combine_in_the_background_then_wait_for_review_and_keep_both_transcripts() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(
        &state,
        &ids,
        [
            "A graph has nodes and edges.",
            "BFS uses a queue.\n\n- [x] read chapter 3\n\n![Tree](attachments/tree.png)",
        ],
    );
    let notes = state.fresh().notes_dir.clone();
    leo_core::recording::save(&notes, &ids[0], &transcript("session-graphs")).unwrap();
    leo_core::recording::save(&notes, &ids[1], &transcript("session-bfs01")).unwrap();
    let asked = with_writer(
        &mut state,
        "## Graphs\nA graph has nodes and edges.\n\n## BFS\nBFS uses a queue.\n\n- [x] read chapter 3",
    );
    let job = json_of(started(&state, &ids[0], &ids[1]));
    assert_eq!(
        (job["state"].as_str(), job["into_title"].as_str()),
        (Some("working"), Some("Graphs"))
    );
    let id = job["id"].as_str().unwrap().to_string();
    let done = finished(&state, &id);
    assert_eq!(done["state"], "ready", "{done}");
    let page = &done["result"];
    let body = page["body"].as_str().unwrap().to_string();
    assert!(
        body.contains("![Tree](attachments/tree.png)"),
        "the AI's omission is put back: {body}"
    );
    assert_eq!(page["added"].as_array().unwrap().len(), 1);
    let prompt = asked.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("A graph has nodes and edges.") && prompt.contains("BFS uses a queue.")
    );
    assert!(
        state.fresh().find_note(&ids[1]).is_some(),
        "nothing changes until it is saved"
    );
    let panel = run(crate::routes::activity::activity(State(state.clone()))).0;
    let row = panel["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == "combine")
        .unwrap()
        .clone();
    assert_eq!(
        (row["step"].as_str(), row["ready"].as_bool()),
        (Some("Ready to review"), Some(true))
    );
    assert_eq!(row["href"], format!("#combine/{id}"));

    let saved = json_of(keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": body, "base": page["base"], "with_base": page["with_base"] }),
    ));
    assert_eq!(saved["before"], "A graph has nodes and edges.");
    assert_eq!(saved["removed"], ids[1].as_str());
    let store = state.fresh();
    assert!(store.find_note(&ids[1]).is_none());
    assert!(
        store.trashed().iter().any(|t| t.id == ids[1]),
        "the other note can be restored"
    );
    drop(store);
    let both = leo_core::recording::list(&notes, &ids[0]).unwrap();
    assert!(both
        .iter()
        .any(|s| s.id == "session-bfs01" && s.from == "BFS"));
    assert_eq!(both.len(), 2, "both transcripts stay, each labelled");
    let undone = run(combine::uncombine(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({ "from": saved["removed"] })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    assert_eq!(leo_core::recording::list(&notes, &ids[0]).unwrap().len(), 1);
    assert_eq!(
        run(combine::forget(State(state.clone()), Path(id.clone()))).status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        run(combine::job(State(state.clone()), Path(id))).status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn a_note_changed_while_combining_is_not_overwritten() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(&state, &ids, ["edges", "queue"]);
    with_writer(&mut state, "edges and queue");
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    let page = &done["result"];
    bodies(&state, &ids, ["edges, edited meanwhile", "queue"]);
    let refused = keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": page["body"], "base": page["base"], "with_base": page["with_base"] }),
    );
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().body,
        "edges, edited meanwhile"
    );
    assert!(store.find_note(&ids[1]).is_some());
}

#[test]
fn a_note_changed_during_the_copy_is_not_combined() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(&state, &ids, ["edges", "queue"]);
    let notes = state.fresh().notes_dir.clone();
    leo_core::recording::save(&notes, &ids[1], &transcript("session-bfs01")).unwrap();
    with_writer(&mut state, "edges and queue");
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    let page = done["result"].clone();
    let meanwhile = state.clone();
    let wanted = ids.clone();
    combine::AFTER_COPY.lock().unwrap().push((
        ids[0].clone(),
        Box::new(move || bodies(&meanwhile, &wanted, ["edges, edited meanwhile", "queue"])),
    ));
    let refused = keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": page["body"], "base": page["base"], "with_base": page["with_base"] }),
    );
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    assert!(
        leo_core::recording::list(&notes, &ids[0])
            .unwrap_or_default()
            .is_empty(),
        "the copies are removed"
    );
    assert_eq!(
        leo_core::recording::list(&notes, &ids[1]).unwrap().len(),
        1,
        "the other note keeps its own"
    );
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().body,
        "edges, edited meanwhile"
    );
    assert!(store.find_note(&ids[1]).is_some());
}

#[test]
fn combining_needs_an_ai_and_two_different_notes_and_a_failed_ai_changes_nothing() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    assert_eq!(
        started(&state, &ids[0], &ids[1]).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    with_writer(&mut state, "  ");
    assert_eq!(
        started(&state, &ids[0], &ids[0]).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        started(&state, &ids[0], "nope").status(),
        StatusCode::NOT_FOUND
    );
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "failed");
    assert!(done["error"]
        .as_str()
        .unwrap()
        .contains("Nothing was changed"));
    assert!(state.fresh().find_note(&ids[1]).is_some());
}

#[test]
fn long_notes_combine_in_bounded_chunks_and_can_be_saved() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    let a = (0..2500)
        .map(|i| format!("First fact {i}: café 😀 stays here.\n"))
        .collect::<String>();
    let b = (0..2500)
        .map(|i| format!("Second fact {i}: details stay here.\n"))
        .collect::<String>();
    assert!(a.chars().count() + b.chars().count() > 120_000);
    bodies(&state, &ids, [&a, &b]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&asked);
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |_, user: &str, _| {
            heard.lock().unwrap().push(user.to_string());
            let bodies = user
                .split("<note title=")
                .skip(1)
                .map(|part| {
                    part.split_once('\n')
                        .unwrap()
                        .1
                        .split_once("\n</note>")
                        .unwrap()
                        .0
                })
                .collect::<Vec<_>>();
            Ok(bodies.join("\n\n"))
        })),
    ));
    let response = started(&state, &ids[0], &ids[1]);
    assert_eq!(response.status(), StatusCode::OK);
    let job = json_of(response);
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "ready", "{done}");
    let page = &done["result"];
    let body = page["body"].as_str().unwrap();
    for line in a.lines().chain(b.lines()) {
        assert!(body.contains(line), "{line}");
    }
    let calls = asked.lock().unwrap();
    assert!(calls.len() > 1 && calls.len() <= 128);
    assert!(calls.iter().all(|call| call.chars().count() < 42_000));
    drop(calls);
    let saved = keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": body, "base": page["base"], "with_base": page["with_base"] }),
    );
    assert_eq!(saved.status(), StatusCode::OK);
}

#[test]
fn a_later_chunk_failure_leaves_both_notes_untouched() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    let text = "A fact that stays.\n".repeat(8000);
    bodies(&state, &ids, [&text, "Second fact"]);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |_, _, _| {
            if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Ok("First section".into())
            } else {
                Err(anyhow::anyhow!("offline"))
            }
        })),
    ));
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "failed");
    assert!(done["result"].is_null());
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().body, text);
    assert!(state.fresh().find_note(&ids[1]).is_some());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn a_streamed_combine_is_counted_in_usage() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(&state, &ids, ["A graph has nodes.", "BFS uses a queue."]);
    with_writer(&mut state, "unused");
    state.chat = Some(Arc::new(
        |_: &str,
         _: &str,
         _: u32,
         piece: &mut dyn FnMut(&str),
         _: &mut dyn FnMut(),
         _: &dyn Fn() -> bool| {
            let text = "## Graphs\nA graph has nodes.\n\n## BFS\nBFS uses a queue.";
            piece(text);
            Ok(chat::Reply {
                text: text.into(),
                spent: Some(chat::Spent {
                    input: 1_200,
                    output: 300,
                    ..Default::default()
                }),
            })
        },
    ));
    assert!(state.usage.saved().kinds.is_empty());
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "ready", "{done}");
    let used = serde_json::to_value(state.usage.saved().kinds).unwrap();
    assert_eq!(used.as_array().unwrap().len(), 1);
    assert_eq!(
        (&used[0]["calls"], &used[0]["input"], &used[0]["output"]),
        (
            &serde_json::json!(1),
            &serde_json::json!(1_200),
            &serde_json::json!(300)
        )
    );
}

#[test]
fn undoing_a_combine_of_1001_recordings_works() {
    let (state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    let notes = state.fresh().notes_dir.clone();
    leo_core::recording::save(&notes, &ids[0], &transcript("session-graphs")).unwrap();
    leo_core::recording::save(&notes, &ids[1], &transcript("session-bfs00")).unwrap();
    let dir = leo_core::recording::folder(&notes, &ids[1]).unwrap();
    let mut head = leo_core::recording::head(&notes, &ids[1], "session-bfs00").unwrap();
    for n in 1..1001 {
        head.id = format!("session-bfs{n:04}");
        std::fs::write(
            dir.join(format!("{}.json", head.id)),
            serde_json::to_vec(&head).unwrap(),
        )
        .unwrap();
        for kind in ["passages", "points"] {
            std::fs::copy(
                dir.join(format!("session-bfs00.1.{kind}.jsonl")),
                dir.join(format!("{}.1.{kind}.jsonl", head.id)),
            )
            .unwrap();
        }
    }
    let (base, with_base) = {
        let store = state.fresh();
        (
            crate::routes::notes::version_of(store.find_note(&ids[0]).unwrap()),
            crate::routes::notes::version_of(store.find_note(&ids[1]).unwrap()),
        )
    };
    let saved = keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": "Both.", "base": base, "with_base": with_base }),
    );
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(
        leo_core::recording::list(&notes, &ids[0]).unwrap().len(),
        1002
    );
    let undone = run(combine::uncombine(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({ "from": ids[1] })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    let left = leo_core::recording::list(&notes, &ids[0]).unwrap();
    assert_eq!(
        left.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        ["session-graphs"]
    );
    assert_eq!(
        leo_core::recording::list(&notes, &ids[1]).unwrap().len(),
        1001
    );
}

fn echoing(state: &mut AppState, asked: &Arc<std::sync::atomic::AtomicUsize>) {
    let count = Arc::clone(asked);
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |_, user: &str, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(user
                .split("<note title=")
                .skip(1)
                .map(|part| {
                    part.split_once('\n')
                        .unwrap()
                        .1
                        .split_once("\n</note>")
                        .unwrap()
                        .0
                })
                .collect::<Vec<_>>()
                .join("\n\n"))
        })),
    ));
}

fn lines(name: &str, count: usize) -> String {
    (0..count)
        .map(|i| format!("{name} fact {i:07}: stays here.\n"))
        .collect()
}

#[test]
fn two_long_notes_combine_part_by_part() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    let (a, b) = (lines("First", 12_000), lines("Second", 12_000));
    assert!(a.chars().count() + b.chars().count() > 600_000);
    bodies(&state, &ids, [&a, &b]);
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    echoing(&mut state, &asked);
    let response = started(&state, &ids[0], &ids[1]);
    assert_eq!(response.status(), StatusCode::OK);
    let job = json_of(response);
    let parts = job["parts"].as_u64().unwrap();
    assert!(parts > 10, "{job}");
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "ready", "{done}");
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::SeqCst) as u64,
        parts,
        "the start answer counts the requests"
    );
    let body = done["result"]["body"].as_str().unwrap();
    for line in a.lines().chain(b.lines()) {
        assert!(body.contains(line), "{line}");
    }
}

fn fake_clock(into: &str) -> Arc<Mutex<std::time::Duration>> {
    let clock = Arc::new(Mutex::new(std::time::Duration::ZERO));
    let read = Arc::clone(&clock);
    combine::CLOCKS
        .lock()
        .unwrap()
        .push((into.to_string(), Arc::new(move || *read.lock().unwrap())));
    clock
}

fn answering_in(
    state: &mut AppState,
    clock: &Arc<Mutex<std::time::Duration>>,
    takes: impl Fn(usize) -> u64 + Send + Sync + 'static,
) -> Arc<std::sync::atomic::AtomicUsize> {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (count, clock) = (Arc::clone(&asked), Arc::clone(clock));
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |_, _, _| {
            let n = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *clock.lock().unwrap() += std::time::Duration::from_secs(takes(n));
            Ok(format!("section {n}"))
        })),
    ));
    asked
}

fn many_parts(state: &AppState, ids: &[String]) {
    bodies(state, ids, [&lines("Long", 55_000), "short"]);
}

#[test]
fn a_long_combine_outlives_fifteen_minutes_in_all() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    many_parts(&state, &ids);
    let clock = fake_clock(&ids[0]);
    let asked = answering_in(&mut state, &clock, |_| 10);
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let parts = job["parts"].as_u64().unwrap();
    assert!(parts * 10 > 15 * 60, "{parts} requests of ten seconds");
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "ready", "{done}");
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::SeqCst) as u64,
        parts
    );
}

#[test]
fn one_answer_over_the_limit_stops_the_combine_and_changes_neither_note() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    many_parts(&state, &ids);
    let before = state.fresh().find_note(&ids[0]).unwrap().body.clone();
    let clock = fake_clock(&ids[0]);
    let asked = answering_in(&mut state, &clock, |n| if n == 3 { 16 * 60 } else { 1 });
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "failed", "{done}");
    assert!(done["error"].as_str().unwrap().contains("too long"));
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 4);
    let store = state.fresh();
    assert_eq!(store.find_note(&ids[0]).unwrap().body, before);
    assert!(store.find_note(&ids[1]).is_some());
}

#[test]
fn a_long_combine_can_be_stopped() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    many_parts(&state, &ids);
    let before = state.fresh().find_note(&ids[0]).unwrap().body.clone();
    let (entered, waiting) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (entered, released) = (Mutex::new(entered), Mutex::new(released));
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&asked);
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |_, _, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = entered.lock().unwrap().send(());
            let _ = released
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(20));
            Ok("section".into())
        })),
    ));
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let id = job["id"].as_str().unwrap().to_string();
    waiting
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    let stopped = run(combine::forget(State(state.clone()), Path(id.clone())));
    assert_eq!(stopped.status(), StatusCode::NO_CONTENT);
    release.send(()).unwrap();
    for _ in 0..400 {
        let seen = run(combine::job(State(state.clone()), Path(id.clone())));
        if seen.status() == StatusCode::NOT_FOUND {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        run(combine::job(State(state.clone()), Path(id))).status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    let store = state.fresh();
    assert_eq!(store.find_note(&ids[0]).unwrap().body, before);
    assert!(store.find_note(&ids[1]).is_some());
}

#[test]
fn the_result_of_a_long_combine_can_be_saved() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    bodies(&state, &ids, ["edges", "queue"]);
    with_writer(&mut state, "edges and queue");
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let done = finished(&state, job["id"].as_str().unwrap());
    let page = &done["result"];
    let long = lines("Long", 40_000);
    assert!(long.chars().count() > 960_000);
    let saved = keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": long, "base": page["base"], "with_base": page["with_base"] }),
    );
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().body, long);
}

#[test]
#[ignore = "measure: run with --release"]
fn audit_bench_p73_six_million_characters_combine() {
    let (mut state, _d, ids) = state_with(&[("First", ""), ("Second", "")]);
    let (a, b) = (lines("First", 85_000), lines("Second", 85_000));
    let total = a.chars().count() + b.chars().count();
    bodies(&state, &ids, [&a, &b]);
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    echoing(&mut state, &asked);
    let began = std::time::Instant::now();
    let job = json_of(started(&state, &ids[0], &ids[1]));
    let parts = job["parts"].as_u64().unwrap();
    let counted = began.elapsed();
    let done = finished(&state, job["id"].as_str().unwrap());
    assert_eq!(done["state"], "ready", "{done}");
    println!(
        "combine: {total} characters, {parts} requests, counted in {counted:?}, done in {:?}",
        began.elapsed()
    );
}
