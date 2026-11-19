use super::*;
struct Fake(Mutex<Vec<String>>);
impl crate::dictation::DictationApi for Fake {
    fn view(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({"settings":{}}))
    }
    fn change(&self, change: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(change["action"] == "copy", "Choose a dictation action.");
        self.0
            .lock()
            .unwrap()
            .push(change["text"].as_str().unwrap().into());
        self.view()
    }
}
#[test]
fn dictation_copy_uses_only_the_kept_text_and_successes_are_json() {
    let (mut state, dir, _) = state_with(&[]);
    let fake = Arc::new(Fake(Mutex::new(Vec::new())));
    state.dictation = Some(fake.clone());
    crate::dictation::remember(dir.path(), "kept text").unwrap();
    let response = run(crate::dictation::get(State(state.clone())));
    assert_eq!(response.status(), StatusCode::OK);
    let view = json_of(response);
    let id = view["recent"][0]["id"].clone();
    let response = run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(
            serde_json::json!({"action":"copy","id":id,"text":"untrusted replacement"}),
        )),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_of(response)["recent"].is_array());
    assert_eq!(*fake.0.lock().unwrap(), vec!["kept text"]);
    let response = run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(serde_json::json!({"action":"copy","id":999}))),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = run(crate::dictation::change(
        State(state),
        Ok(Json(serde_json::json!({"action":"clear"}))),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response)["recent"], serde_json::json!([]));
}
#[test]
fn dictation_edit_updates_the_row_and_refuses_an_unknown_id() {
    let (mut state, dir, _) = state_with(&[]);
    state.dictation = Some(Arc::new(Fake(Mutex::new(Vec::new()))));
    crate::dictation::remember(dir.path(), "ship it to leo seal I").unwrap();
    let id = json_of(run(crate::dictation::get(State(state.clone()))))["recent"][0]["id"].clone();
    let response = run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(
            serde_json::json!({"action":"edit","id":id,"text":"Ship it to Leo CLI"}),
        )),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    let view = json_of(response);
    assert_eq!(view["recent"][0]["text"], "Ship it to Leo CLI");
    assert_eq!(view["recent"][0]["edited"], true);
    let response = run(crate::dictation::change(
        State(state),
        Ok(Json(
            serde_json::json!({"action":"edit","id":999,"text":"x"}),
        )),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
fn long_dictation() -> String {
    ['a', 'é', '😀', ' '].iter().cycle().take(299_999).collect()
}
#[test]
fn a_long_dictation_is_copied_whole_and_opened_on_demand() {
    let (mut state, dir, _) = state_with(&[]);
    let fake = Arc::new(Fake(Mutex::new(Vec::new())));
    state.dictation = Some(fake.clone());
    let text = long_dictation();
    crate::dictation::remember(dir.path(), &text).unwrap();
    let view = json_of(run(crate::dictation::get(State(state.clone()))));
    assert_eq!(view["recent"][0]["cut"], true);
    let id = view["recent"][0]["id"].clone();
    let response = run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(serde_json::json!({"action":"copy","id":id}))),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(*fake.0.lock().unwrap(), vec![text.clone()]);
    let view = json_of(run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(serde_json::json!({"action":"full","id":id}))),
    )));
    assert_eq!(view["full"]["text"], text);
    assert_eq!(view["full"]["id"], id);
    assert!(view["full"]["original"].is_null());
    let response = run(crate::dictation::change(
        State(state),
        Ok(Json(serde_json::json!({"action":"full","id":999}))),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn a_long_edit_reaches_the_dictation_route() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn send(port: u16, request: String) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            String::from_utf8_lossy(&response).into_owned()
        })
        .await
        .unwrap()
    }
    let dir = tempfile::tempdir().unwrap();
    let places = crate::Places {
        store: Store::load_from(&dir.path().join("notes")).unwrap(),
        sessions: dir.path().join("sessions.json"),
    };
    let powers = crate::Powers {
        dictation: Some(Arc::new(Fake(Mutex::new(Vec::new())))),
        ..Default::default()
    };
    let running = crate::start_in(
        places,
        crate::StartOptions {
            port: 0,
            token: "dictation-test-token".into(),
        },
        powers,
    )
    .await
    .unwrap();
    let port = running.port();
    let data = crate::db::data_dir(&running.state.chats).to_path_buf();
    crate::dictation::remember(&data, "short").unwrap();
    let id = crate::dictation::recent(&data).unwrap()[0]["id"].clone();
    let signed = send(
        port,
        format!("GET /?token=dictation-test-token HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    let cookie = signed
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie: "))
        .unwrap()
        .split_once(": ")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let text = long_dictation();
    let body = serde_json::json!({"action":"edit","id":id,"text":text}).to_string();
    assert!(body.len() > 300_000);
    let answer = send(
        port,
        format!("POST /api/dictation HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()),
    )
    .await;
    assert!(
        answer.starts_with("HTTP/1.1 200"),
        "{}",
        answer.lines().next().unwrap_or("")
    );
    let id = id.as_i64().unwrap();
    assert_eq!(
        crate::dictation::text_of(&data, id).unwrap().unwrap().0,
        text.trim()
    );
    running.stop().await;
}
