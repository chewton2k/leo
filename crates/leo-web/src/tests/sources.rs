use super::*;
use crate::routes::sources;
fn source(id: &str) -> leo_core::recording::Archive {
    leo_core::recording::Archive {
        id: id.into(),
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 30,
            text: "Dijkstra finds shortest paths".into(),
            speaker: "Microphone".into(),
        }],
        points: vec![leo_core::recording::Point {
            at_secs: 5,
            text: "explain a priority queue".into(),
        }],
        context: String::new(),
        wants: String::new(),
        warnings: vec![],
        trace: vec![],
        from: String::new(),
    }
}
#[test]
fn retained_sources_are_editable_with_conflict_protection_and_regeneration_is_a_preview() {
    let (mut state, _d, ids) = state_with(&[("Graphs", "cs130")]);
    let id = ids[0].clone();
    store_now(&state, |store| {
        store.find_note_mut(&id).unwrap().body = "Existing note".into();
        store.save().unwrap();
        let mut kept = source("session-1234");
        kept.wants = "Focus on what is on the exam".into();
        leo_core::recording::save(&store.notes_dir, &id, &kept).unwrap();
        Ok(())
    })
    .unwrap();
    let page = json_of(run(sources::sources(
        State(state.clone()),
        Path(id.clone()),
        Query(serde_json::from_value(serde_json::json!({"q":"shortest"})).unwrap()),
    )));
    assert_eq!(page["matches"].as_array().unwrap().len(), 1);
    let edit = serde_json::json!({"source":"session-1234","base":page["versions"]["session-1234"],"points":[{"at_secs":5,"text":"priority queue"}],"passages":[{"start_secs":0,"end_secs":30,"speaker":"Microphone","text":"Dijkstra’s algorithm"}]});
    let put = || {
        run(sources::edit_sources(
            State(state.clone()),
            Path(id.clone()),
            Json(serde_json::from_value(edit.clone()).unwrap()),
        ))
    };
    assert_eq!(put().status(), StatusCode::OK);
    assert_eq!(put().status(), StatusCode::CONFLICT);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&asked);
    state.regenerator = Some(Arc::new(move |sources, profile| {
        assert_eq!(sources[0].passages[0].text, "Dijkstra’s algorithm");
        heard.lock().unwrap().push(profile.wants.clone());
        Ok(("Generated".into(), "Preview body".into()))
    }));
    run(sources::regenerate(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({"wants":"Just the decisions"})).unwrap()),
    ));
    let response = json_of(run(sources::regenerate(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({})).unwrap()),
    )));
    assert_eq!(response["body"], "Preview body");
    assert_eq!(
        *asked.lock().unwrap(),
        ["Just the decisions", "Focus on what is on the exam"],
        "what the user wants is kept with the recording and can be changed when writing again"
    );
    assert!(response["base"].is_string());
    state.regenerator = Some(Arc::new(|_, _| {
        anyhow::bail!("every chat provider failed. Last error: ollama: error sending request")
    }));
    let failed = run(sources::regenerate(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({})).unwrap()),
    ));
    assert_eq!(failed.status(), StatusCode::BAD_GATEWAY);
    let said = json_of(failed)["error"].as_str().unwrap().to_string();
    assert!(!said.starts_with(leo_core::NO_WRITING_AI), "{said}");
    assert!(said.contains("didn't answer just now"), "{said}");
    store_now(&state, |store| {
        assert_eq!(store.find_note(&id).unwrap().body, "Existing note");
        Ok(())
    })
    .unwrap();
}

#[test]
fn felix_reads_and_searches_the_recording_a_note_was_made_from() {
    let (state, _d, ids) = state_with(&[("Graphs", "cs130"), ("Plain", "")]);
    store_now(&state, |store| {
        leo_core::recording::save(&store.notes_dir, &ids[0], &source("session-1234")).unwrap();
        let call = |args: serde_json::Value| crate::tools::Call {
            name: "read_transcript".into(),
            args,
        };
        let cache = crate::graph::Cache::default();
        let mut desk = crate::tools::Desk::new(vec![], crate::chat::ROOM)
            .with_access(crate::tools::Access::Read);
        let all = desk.run(store, &cache, &call(serde_json::json!({"note": "Graphs"})));
        assert_eq!(all.step, "Read the recording of “Graphs”");
        assert!(all.result.contains("part=\"1\" of=\"1\""), "{}", all.result);
        assert!(all.result.contains("[0:00] Dijkstra finds shortest paths"));
        assert!(all
            .result
            .contains("[0:05] (the user typed) explain a priority queue"));
        let found = desk.run(
            store,
            &cache,
            &call(serde_json::json!({"note": "Graphs", "find": "priority"})),
        );
        assert!(found.result.contains("priority queue") && !found.result.contains("Dijkstra"));
        let none = desk.run(
            store,
            &cache,
            &call(serde_json::json!({"note": "Graphs", "find": "homework"})),
        );
        assert!(none.result.starts_with("Nothing in the recording"));
        let plain = desk.run(store, &cache, &call(serde_json::json!({"note": "Plain"})));
        assert!(plain.result.contains("was not made from a recording"));
        assert!(crate::tools::has_transcript(&store.notes_dir, &ids[0]));
        assert!(!crate::tools::has_transcript(&store.notes_dir, &ids[1]));
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_edited_typed_point_does_not_bring_back_its_old_wording() {
    let (state, _d, ids) = state_with(&[("Graphs", "cs130")]);
    let id = ids[0].clone();
    let notes = store_now(&state, |store| {
        leo_core::recording::save(&store.notes_dir, &id, &source("session-1234")).unwrap();
        Ok(store.notes_dir.clone())
    })
    .unwrap();
    let page = json_of(run(sources::sources(
        State(state.clone()),
        Path(id.clone()),
        Query(serde_json::from_value(serde_json::json!({})).unwrap()),
    )));
    let edit = serde_json::json!({"source":"session-1234","base":page["versions"]["session-1234"],"points":[{"at_secs":5,"text":"explain a heap"}],"passages":[{"start_secs":0,"end_secs":30,"speaker":"Microphone","text":"Dijkstra finds shortest paths"}]});
    let saved = run(sources::edit_sources(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(edit).unwrap()),
    ));
    assert_eq!(saved.status(), StatusCode::OK);
    let points: Vec<String> = leo_core::recording::load(&notes, &id).unwrap()[0]
        .points
        .iter()
        .map(|p| p.text.clone())
        .collect();
    assert_eq!(points, ["explain a heap"]);
}
