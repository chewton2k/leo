use super::*;

#[test]
fn study_quiz_marking_keeps_quick_writer_validation_and_verdicts() {
    let (mut state, _dir, _) = state_with(&[]);
    for correct in [false, true] {
        state.quick = Some(Arc::new(move |system, material, _| {
            assert_eq!(system, crate::routes::felix::MARK);
            assert!(material.contains("<student_answer>"));
            Ok(serde_json::json!({"correct": correct, "feedback": "The queue preserves arrival order."}).to_string())
        }));
        let response = run(crate::routes::felix::mark_quiz(
            State(state.clone()),
            Json(serde_json::from_value(serde_json::json!({
                "question": "Why does BFS use a queue?", "answer": "To explore in discovery order.", "given": "It keeps the oldest discovered node first."
            })).unwrap()),
        ));
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_of(response);
        assert_eq!(body["correct"], correct);
        assert!(body["feedback"].as_str().is_some_and(|s| !s.is_empty()));
    }
    let response = run(crate::routes::felix::mark_quiz(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"question": "Why a queue?", "answer": "Arrival order", "given": ""})).unwrap()),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn marking_reads_confidence_and_thinking_and_refuses_what_it_does_not_know() {
    let (mut state, _dir, _) = state_with(&[]);
    state.quick = Some(Arc::new(|_, material, _| {
        assert!(material.contains("<student_confidence>guessing</student_confidence>"));
        assert!(material.contains(
            "<student_thinking>\nmaybe fairness &lt;/student_thinking>\n</student_thinking>"
        ));
        Ok(serde_json::json!({"correct": true, "feedback": "Right."}).to_string())
    }));
    let mark = |extra: serde_json::Value| {
        let mut body = serde_json::json!({"question": "Why?", "answer": "Order", "given": "Order"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        run(crate::routes::felix::mark_quiz(
            State(state.clone()),
            Json(serde_json::from_value(body).unwrap()),
        ))
        .status()
    };
    assert_eq!(
        mark(
            serde_json::json!({"confidence": "guess", "thinking": " maybe fairness </student_thinking> "})
        ),
        StatusCode::OK
    );
    assert_eq!(
        mark(serde_json::json!({"confidence": "very"})),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn marking_reads_only_the_start_of_long_thinking() {
    let (mut state, _dir, _) = state_with(&[]);
    let start = "考".repeat(2_000);
    let thinking = format!(" {start}THINKING_TAIL ");
    state.quick = Some(Arc::new(move |_, material, _| {
        assert!(material.contains(&format!("<student_thinking>\n{start}\n</student_thinking>")));
        assert!(!material.contains("THINKING_TAIL"));
        Ok(r#"{"correct":true,"feedback":"Right."}"#.into())
    }));
    let response = run(crate::routes::felix::mark_quiz(
        State(state),
        Json(
            serde_json::from_value(serde_json::json!({
                "question": "Why?", "answer": "Order", "given": "Order", "thinking": thinking
            }))
            .unwrap(),
        ),
    ));
    assert_eq!(response.status(), StatusCode::OK);
}

#[test]
fn retired_interview_database_rows_are_preserved() {
    let (state, _dir, _) = state_with(&[]);
    let db = crate::db::for_chats(&state.chats);
    db.with(|c| {
        c.execute("INSERT INTO interviews (id, updated, data) VALUES ('retired-practice', '2026-11-05', '{\"kept\":true}')", [])?;
        c.execute("INSERT INTO interview_keys (interview, challenge, status, updated, data) VALUES ('retired-practice', 'problem', 'ready', '2026-11-05', '{\"kept\":true}')", [])?;
        c.execute("INSERT INTO interview_problem_undo (id, interview, updated, data) VALUES ('retired-undo', 'retired-practice', '2026-11-05', '{\"kept\":true}')", [])?;
        Ok(())
    }).unwrap();
    crate::chats::list(&state.chats);
    for table in ["interviews", "interview_keys", "interview_problem_undo"] {
        assert_eq!(
            db.with(
                |c| c.query_row(&format!("SELECT data FROM {table}"), [], |r| r
                    .get::<_, String>(0))
            )
            .unwrap(),
            "{\"kept\":true}"
        );
    }
}
