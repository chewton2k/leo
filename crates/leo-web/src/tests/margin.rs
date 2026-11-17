use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use leo_core::store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::margin;
use crate::{start_in, Places, Powers, Running, StartOptions, Writer};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

struct Engine {
    running: Running,
    dir: tempfile::TempDir,
    port: u16,
    cookie: String,
}

async fn raw(port: u16, request: String) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).await.unwrap();
    String::from_utf8_lossy(&reply).to_string()
}

impl Engine {
    async fn with(powers: Powers) -> Engine {
        let dir = tempfile::tempdir().unwrap();
        let places = Places {
            store: Store::load_from(&dir.path().join("notes")).unwrap(),
            sessions: dir.path().join("sessions.json"),
        };
        let options = StartOptions {
            port: 0,
            token: TOKEN.into(),
        };
        let running = start_in(places, options, powers).await.unwrap();
        let port = running.port();
        let signed_in = raw(
            port,
            format!("GET /?token={TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
        )
        .await;
        let cookie = signed_in
            .lines()
            .find_map(|line| {
                line.strip_prefix("set-cookie: ")
                    .or_else(|| line.strip_prefix("Set-Cookie: "))
            })
            .and_then(|value| value.split(';').next())
            .unwrap()
            .to_string();
        Engine {
            running,
            dir,
            port,
            cookie,
        }
    }

    fn chats(&self) -> std::path::PathBuf {
        crate::chats::dir_for(&self.dir.path().join("notes"))
    }

    async fn send_as(
        &self,
        cookie: &str,
        method: &str,
        path: &str,
        body: Option<String>,
    ) -> (u16, serde_json::Value) {
        let port = self.port;
        let body = body.unwrap_or_default();
        let reply = raw(
            port,
            format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        )
        .await;
        let status = reply
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let (head, text) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
        let text = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            text.lines().nth(1).unwrap_or_default()
        } else {
            text
        };
        (status, serde_json::from_str(text).unwrap_or_default())
    }

    async fn get(&self, path: &str) -> (u16, serde_json::Value) {
        self.send_as(&self.cookie, "GET", path, None).await
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
        self.send_as(&self.cookie, "POST", path, Some(body.to_string()))
            .await
    }

    async fn note(&self, title: &str, body: &str) -> (String, String) {
        let (status, made) = self
            .post(
                "/api/notes",
                serde_json::json!({ "title": title, "body": "" }),
            )
            .await;
        assert!(status < 300, "{status}");
        let id = made["id"].as_str().unwrap().to_string();
        margin::set_mode(&self.chats(), &id, "along");
        let version = self.edit(&id, body).await;
        (id, version)
    }

    async fn edit(&self, id: &str, body: &str) -> String {
        let (status, saved) = self
            .send_as(
                &self.cookie,
                "PATCH",
                &format!("/api/notes/{id}"),
                Some(serde_json::json!({ "body": body }).to_string()),
            )
            .await;
        assert_eq!(status, 200);
        saved["version"].as_str().unwrap().to_string()
    }

    async fn read(&self, id: &str, version: &str, blocks: &[&str]) -> (u16, serde_json::Value) {
        self.post(
            &format!("/api/notes/{id}/margin/read"),
            serde_json::json!({ "version": version, "blocks": blocks }),
        )
        .await
    }

    async fn analyze(
        &self,
        id: &str,
        version: &str,
        blocks: &[&str],
        most: Option<usize>,
    ) -> (u16, serde_json::Value) {
        self.post(
            &format!("/api/notes/{id}/margin/analyze"),
            serde_json::json!({ "version": version, "blocks": blocks, "most": most }),
        )
        .await
    }

    async fn plain(&self, body: &str) -> (String, String) {
        let (_, made) = self
            .post(
                "/api/notes",
                serde_json::json!({ "title": "Plain", "body": body }),
            )
            .await;
        let id = made["id"].as_str().unwrap().to_string();
        margin::set_mode(&self.chats(), &id, "ask");
        (id, made["version"].as_str().unwrap().to_string())
    }

    async fn stop(self) {
        self.running.stop().await;
    }
}

fn answering(text: &'static str) -> (Writer, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let writer: Writer = Arc::new(move |_, _, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(text.to_string())
    });
    (writer, calls)
}

async fn reading(text: &'static str) -> (Engine, Arc<AtomicUsize>) {
    let (writer, calls) = answering(text);
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        ..Default::default()
    })
    .await;
    (engine, calls)
}

const PASSAGE: &str = "then login just checks if it matches";
const SAYS: &str = r#"{"quote":"just checks if it matches","text":"In plain text?"}"#;

#[tokio::test]
async fn read_along_is_the_default_for_new_notes_on_a_fresh_install() {
    let (engine, calls) = reading(SAYS).await;
    let (_, settings) = engine.get("/api/margin").await;
    assert_eq!(settings["default_mode"], "along");
    assert_eq!(settings["cap"], margin::DAILY_CAP);
    assert_eq!(settings["gap"], margin::READ_GAP);
    assert_eq!(settings["today"], 0);
    let (_, made) = engine
        .post(
            "/api/notes",
            serde_json::json!({ "title": "New", "body": "" }),
        )
        .await;
    let id = made["id"].as_str().unwrap();
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(view["mode"], "along");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    engine.stop().await;
}

#[tokio::test]
async fn a_new_default_reaches_new_notes_and_leaves_old_ones_alone() {
    let (engine, _) = reading(SAYS).await;
    let (_, old) = engine
        .post(
            "/api/notes",
            serde_json::json!({ "title": "Old", "body": "" }),
        )
        .await;
    let (status, _) = engine
        .post("/api/margin", serde_json::json!({ "default_mode": "ask" }))
        .await;
    assert_eq!(status, 200);
    let (status, _) = engine
        .post(
            "/api/margin",
            serde_json::json!({ "default_mode": "along" }),
        )
        .await;
    assert_eq!(status, 200);
    let (_, new) = engine
        .post(
            "/api/notes",
            serde_json::json!({ "title": "New", "body": "" }),
        )
        .await;
    let mode = |view: serde_json::Value| view["mode"].as_str().unwrap().to_string();
    let old = old["id"].as_str().unwrap();
    let new = new["id"].as_str().unwrap();
    assert_eq!(
        mode(engine.get(&format!("/api/notes/{old}/margin")).await.1),
        "ask"
    );
    assert_eq!(
        mode(engine.get(&format!("/api/notes/{new}/margin")).await.1),
        "along"
    );
    let (status, _) = engine
        .post(
            &format!("/api/notes/{old}/margin"),
            serde_json::json!({ "mode": "along" }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        mode(engine.get(&format!("/api/notes/{old}/margin")).await.1),
        "along"
    );
    let (status, _) = engine
        .post(
            &format!("/api/notes/{old}/margin"),
            serde_json::json!({ "mode": "shout" }),
        )
        .await;
    assert_eq!(status, 400);
    for good in [
        serde_json::json!({ "pace": "fast" }),
        serde_json::json!({ "gap": 180 }),
    ] {
        assert_eq!(engine.post("/api/margin", good).await.0, 200);
    }
    for bad in [
        serde_json::json!({ "cap": 7 }),
        serde_json::json!({ "gap": 7 }),
        serde_json::json!({ "layout": "left" }),
        serde_json::json!({ "pace": "turbo" }),
        serde_json::json!({ "default_mode": "x" }),
    ] {
        assert_eq!(engine.post("/api/margin", bad).await.0, 400);
    }
    engine.stop().await;
}

#[tokio::test]
async fn turning_the_default_off_keeps_notes_made_while_it_was_on() {
    let (engine, _) = reading(SAYS).await;
    let along = serde_json::json!({ "default_mode": "along" });
    engine.post("/api/margin", along).await;
    let (_, kept) = engine
        .post(
            "/api/notes",
            serde_json::json!({ "title": "Kept", "body": "" }),
        )
        .await;
    let kept = kept["id"].as_str().unwrap();
    let ask = serde_json::json!({ "default_mode": "ask" });
    assert_eq!(engine.post("/api/margin", ask).await.0, 200);
    let (_, view) = engine.get(&format!("/api/notes/{kept}/margin")).await;
    assert_eq!(view["mode"], "along");
    engine.stop().await;
}

#[tokio::test]
async fn a_read_saves_one_note_anchored_in_the_text() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine
        .note("Signup", &format!("First.\n\n{PASSAGE}, I think?"))
        .await;
    let (status, read) = engine
        .read(&id, &version, &[&format!("{PASSAGE}, I think?")])
        .await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(
        read["note"]["quote"],
        "then login just checks if it matches, I think?"
    );
    assert_eq!(read["note"]["prefix"], "First.\n\n");
    assert_eq!(read["note"]["suffix"], "");
    assert_eq!(read["note"]["messages"][0]["text"], "In plain text?");
    assert_eq!(read["note"]["status"], "open");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(view["threads"].as_array().unwrap().len(), 1);
    assert_eq!(engine.get("/api/margin").await.1["today"], 1);
    let file = std::fs::read_dir(engine.dir.path().join("notes"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|f| f.path().extension().is_some_and(|e| e == "md"))
        .map(|f| std::fs::read_to_string(f.path()).unwrap())
        .collect::<String>();
    assert!(!file.contains("In plain text"), "{file}");
    engine.stop().await;
}

#[tokio::test]
async fn a_read_sees_the_earlier_text_as_context_and_cannot_quote_it() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&prompts);
    let writer: Writer = Arc::new(move |_, user, _| {
        seen.lock().unwrap().push(user.to_string());
        Ok(r#"{"quote":"Sessions last a day","text":"Is that long?"}"#.to_string())
    });
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        ..Default::default()
    })
    .await;
    let body = format!("Sessions last a day, said the first paragraph.\n\n{PASSAGE}, I think.");
    let (id, version) = engine.note("Earlier", &body).await;
    let (status, read) = engine
        .read(&id, &version, &[&format!("{PASSAGE}, I think.")])
        .await;
    assert_eq!((status, &read["note"]), (200, &serde_json::Value::Null));
    let asked = prompts.lock().unwrap()[0].clone();
    let (before, changed) = asked.split_once("<changed_text_").unwrap();
    let earlier = before.split("<earlier_text_").nth(1).unwrap();
    assert!(earlier.contains("Sessions last a day, said the first paragraph."));
    assert!(!earlier.contains(PASSAGE));
    assert!(changed.contains(PASSAGE));
    assert!(!changed.contains("Sessions last a day"));
    engine.stop().await;
}

#[tokio::test]
async fn a_read_without_earlier_text_has_no_earlier_block() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&prompts);
    let writer: Writer = Arc::new(move |_, user, _| {
        seen.lock().unwrap().push(user.to_string());
        Ok("{}".to_string())
    });
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("First", PASSAGE).await;
    assert_eq!(engine.read(&id, &version, &[PASSAGE]).await.0, 200);
    assert!(!prompts.lock().unwrap()[0].contains("earlier_text"));
    engine.stop().await;
}

#[tokio::test]
async fn silence_is_a_success_and_saves_nothing() {
    for answer in ["{}", "", "```json\n{}\n```"] {
        let (engine, calls) = reading(answer).await;
        let (id, version) = engine.note("Quiet", PASSAGE).await;
        let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
        assert_eq!((status, &read["note"]), (200, &serde_json::Value::Null));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(engine.get("/api/margin").await.1["today"], 1);
        assert!(engine.get("/api/margin").await.1["problem"].is_null());
        let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
        assert!(view["threads"].as_array().unwrap().is_empty());
        engine.stop().await;
    }
}

#[tokio::test]
async fn malformed_reads_report_failure_instead_of_successful_silence() {
    for answer in [
        "No note needed.",
        "{bad json}",
        r#"{"quote":3,"text":"Hm."}"#,
    ] {
        let (engine, calls) = reading(answer).await;
        let (id, version) = engine.note("Invalid", PASSAGE).await;
        let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
        assert_eq!(status, 200);
        assert_eq!(read["failed"], true);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(engine.get("/api/margin").await.1["today"], 0);
        assert_eq!(
            engine.get("/api/margin").await.1["problem"],
            margin::PROBLEM
        );
        assert!(
            engine.get(&format!("/api/notes/{id}/margin")).await.1["threads"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        engine.stop().await;
    }
}

#[tokio::test]
async fn a_quote_that_is_not_in_the_note_is_dropped() {
    let (engine, calls) = reading(r#"{"quote":"words the model made up","text":"Hm."}"#).await;
    let (id, version) = engine.note("Made up", PASSAGE).await;
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, &read["note"]), (200, &serde_json::Value::Null));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert!(view["threads"].as_array().unwrap().is_empty());
    engine.stop().await;
}

#[tokio::test]
async fn reads_wait_a_while_and_only_changed_blocks_reach_the_model() {
    let (engine, calls) = reading("{}").await;
    let (id, version) = engine.note("Busy", PASSAGE).await;
    assert_eq!(engine.read(&id, &version, &[PASSAGE]).await.0, 200);
    let (status, why) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, why["why"].as_str()), (429, Some("soon")));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    engine.stop().await;
}

#[tokio::test]
async fn a_recent_comment_does_not_add_a_five_minute_read_lockout() {
    let (engine, calls) = reading(SAYS).await;
    let body = format!("Spoken already.\n{PASSAGE}");
    let (id, version) = engine.note("Spoke", &body).await;
    let held = margin::new_thread(
        &id,
        "read",
        margin::placed(&body, "Spoken already", &[]).unwrap(),
        "Spoken already",
        vec![margin::felix_says("Hm.")],
    );
    assert!(margin::put(&engine.chats(), &held, margin::MOST_OPEN));
    let (status, reply) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(status, 200);
    assert!(reply["note"].is_object());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    engine.stop().await;
}

#[tokio::test]
async fn a_sentence_with_a_margin_note_gets_no_second_one() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.note("Once", PASSAGE).await;
    let held = margin::new_thread(
        &id,
        "read",
        margin::placed(PASSAGE, "then login", &[]).unwrap(),
        "then login",
        vec![margin::felix_says("Hm.")],
    );
    assert!(margin::put(&engine.chats(), &held, margin::MOST_OPEN));
    let (status, reply) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, reply["note"].is_null()), (200, true));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    engine.stop().await;
}

const WAS: &str =
    "Passwords use bcrypt. then login simply checks if it matches. Sessions last a day.";
const NOW: &str =
    "Passwords use bcrypt. then login just checks if it matches. Sessions last a day.";
const REWORDED: &str = "then login just checks if it matches.";

fn old_note(
    engine: &Engine,
    id: &str,
    was: &str,
    quote: &str,
    source: &str,
    said: Vec<margin::Message>,
) -> String {
    let held = margin::new_thread(
        id,
        source,
        margin::placed(was, quote, &[]).unwrap(),
        quote,
        said,
    );
    assert!(margin::put(&engine.chats(), &held, 2 * margin::MOST_OPEN));
    held.id
}

fn untouched() -> Vec<margin::Message> {
    vec![margin::felix_says("Hm.")]
}

fn answered() -> Vec<margin::Message> {
    vec![margin::felix_says("Hm."), margin::you_say("Why?")]
}

async fn read_changed(engine: &Engine, id: &str, body: serde_json::Value) -> serde_json::Value {
    let (status, reply) = engine
        .post(&format!("/api/notes/{id}/margin/read"), body)
        .await;
    assert_eq!(status, 200);
    reply
}

async fn open_ids(engine: &Engine, id: &str) -> Vec<String> {
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    view["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

async fn reworded(said: Vec<margin::Message>) -> (Engine, String, String, serde_json::Value) {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Reworded", NOW).await;
    let old = old_note(
        &engine,
        &id,
        WAS,
        "then login simply checks if it matches.",
        "read",
        said,
    );
    let reply = read_changed(
        &engine,
        &id,
        serde_json::json!({ "version": version, "blocks": [NOW], "changed": [{ "id": old, "text": REWORDED }] }),
    )
    .await;
    (engine, id, old, reply)
}

#[tokio::test]
async fn a_reworded_sentence_trades_its_untouched_note_for_the_new_one() {
    let (engine, id, old, reply) = reworded(untouched()).await;
    assert!(reply["note"].is_object());
    assert_eq!(reply["resolved"], serde_json::json!([old]));
    assert_eq!(
        open_ids(&engine, &id).await,
        [reply["note"]["id"].as_str().unwrap()]
    );
    engine.stop().await;
}

#[tokio::test]
async fn a_reworded_sentence_keeps_a_note_you_answered() {
    let (engine, id, old, reply) = reworded(answered()).await;
    assert!(reply["note"].is_null());
    assert_eq!(open_ids(&engine, &id).await, [old]);
    engine.stop().await;
}

#[tokio::test]
async fn a_reworded_sentence_keeps_a_pinned_open_busy_or_asked_for_note() {
    for case in ["pinned", "keep", "replying", "through", "ask"] {
        let (engine, _) = reading(SAYS).await;
        let (id, version) = engine.note("Reworded", NOW).await;
        let source = if matches!(case, "through" | "ask") {
            case
        } else {
            "read"
        };
        let old = old_note(
            &engine,
            &id,
            WAS,
            "then login simply checks if it matches.",
            source,
            untouched(),
        );
        if case == "pinned" {
            assert!(margin::set_pinned(&engine.chats(), &id, &old, true).is_some());
        }
        let keep = (case == "keep").then(|| old.clone());
        let held =
            (case == "replying").then(|| engine.running.state.margin.replying(&old).unwrap());
        let reply = read_changed(
            &engine,
            &id,
            serde_json::json!({ "version": version, "blocks": [NOW], "keep": keep, "changed": [{ "id": old, "text": REWORDED }] }),
        )
        .await;
        drop(held);
        assert!(reply["note"].is_null(), "{case}");
        assert_eq!(
            open_ids(&engine, &id).await,
            std::slice::from_ref(&old),
            "{case}"
        );
        engine.stop().await;
    }
}

#[tokio::test]
async fn a_changed_wording_counts_only_where_its_own_note_sits() {
    let cases = [
        (
            "Passwords use bcrypt. it. then login just checks if it matches.",
            "Passwords use bcrypt. Hash the password with care. then login just checks if it matches.",
            "Hash the password with care",
            "it",
            SAYS,
        ),
        (
            "We use it. Logins are slow.\n\nThe cache helps. We use it.",
            "We use it. Logins are slow.\n\nThe cache helps. We rely on it.",
            "We rely on it.",
            "We use it.",
            r#"{"quote":"We use it.","text":"Which part?"}"#,
        ),
        (
            NOW,
            "Cookies are signed. Tokens rotate weekly. Logs are kept.",
            "Tokens rotate weekly.",
            REWORDED,
            SAYS,
        ),
    ];
    for (now, was, quote, text, answer) in cases {
        for said in [untouched(), answered()] {
            let (engine, _) = reading(answer).await;
            let (id, version) = engine.note("Elsewhere", now).await;
            let block = now.split("\n\n").next().unwrap();
            let old = old_note(&engine, &id, was, quote, "read", said);
            let reply = read_changed(
                &engine,
                &id,
                serde_json::json!({ "version": version, "blocks": [block], "changed": [{ "id": old, "text": text }] }),
            )
            .await;
            assert!(reply["note"].is_object(), "{now}");
            assert_eq!(reply["resolved"], serde_json::json!([]), "{now}");
            assert_eq!(open_ids(&engine, &id).await.len(), 2, "{now}");
            engine.stop().await;
        }
    }
}

#[tokio::test]
async fn one_answered_note_in_a_reworded_sentence_keeps_every_note_there() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Merged", NOW).await;
    let first = old_note(&engine, &id, WAS, "then login simply", "read", untouched());
    let second = old_note(
        &engine,
        &id,
        "Passwords use bcrypt. then login just verifies it matches. Sessions last a day.",
        "verifies it",
        "read",
        answered(),
    );
    let reply = read_changed(
        &engine,
        &id,
        serde_json::json!({ "version": version, "blocks": [NOW], "changed": [
            { "id": first, "text": "then login just" },
            { "id": second, "text": "checks if it" },
        ] }),
    )
    .await;
    assert!(reply["note"].is_null());
    assert_eq!(open_ids(&engine, &id).await, [first, second]);
    engine.stop().await;
}

#[tokio::test]
async fn read_through_drops_every_note_on_a_reworded_sentence() {
    let (engine, _) = reading(
        r#"{"notes":[{"quote":"then login just","text":"A"},{"quote":"checks if it matches","text":"B"}]}"#,
    )
    .await;
    let (id, version) = engine.plain(NOW).await;
    let old = old_note(
        &engine,
        &id,
        WAS,
        "then login simply checks if it matches.",
        "read",
        untouched(),
    );
    let (status, reply) = engine
        .post(
            &format!("/api/notes/{id}/margin/analyze"),
            serde_json::json!({ "version": version, "blocks": [NOW], "changed": [{ "id": old, "text": REWORDED }] }),
        )
        .await;
    assert_eq!(
        (status, reply["notes"].clone()),
        (200, serde_json::json!([]))
    );
    assert_eq!(open_ids(&engine, &id).await, [old]);
    engine.stop().await;
}

#[tokio::test]
async fn a_swap_that_cannot_store_the_new_note_keeps_the_old_one() {
    let (engine, _) = reading(SAYS).await;
    let (id, _) = engine.note("Stuck", NOW).await;
    let old = old_note(
        &engine,
        &id,
        WAS,
        "then login simply checks if it matches.",
        "read",
        untouched(),
    );
    let fresh = margin::new_thread(
        &id,
        "read",
        margin::placed(NOW, REWORDED, &[]).unwrap(),
        REWORDED,
        untouched(),
    );
    let swap = margin::put_replacing(
        &engine.chats(),
        &engine.running.state.margin,
        &fresh,
        std::slice::from_ref(&old),
        None,
        0,
    );
    assert!(matches!(swap, margin::Swap::Failed));
    assert_eq!(open_ids(&engine, &id).await, [old]);
    engine.stop().await;
}

#[tokio::test]
async fn a_swap_on_a_full_margin_frees_only_the_replaced_note() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Full", NOW).await;
    for _ in 1..2 * margin::MOST_OPEN {
        old_note(
            &engine,
            &id,
            NOW,
            "Sessions last a day.",
            "ask",
            untouched(),
        );
    }
    let old = old_note(
        &engine,
        &id,
        WAS,
        "then login simply checks if it matches.",
        "read",
        untouched(),
    );
    let reply = read_changed(
        &engine,
        &id,
        serde_json::json!({ "version": version, "blocks": [NOW], "changed": [{ "id": old, "text": REWORDED }] }),
    )
    .await;
    assert!(reply["note"].is_object());
    assert_eq!(reply["resolved"], serde_json::json!([old]));
    let open = open_ids(&engine, &id).await;
    assert_eq!(open.len(), 2 * margin::MOST_OPEN);
    assert!(!open.contains(&old));
    engine.stop().await;
}

#[tokio::test]
async fn a_useful_diagram_survives_the_read_and_saved_thread() {
    let text = "Chloroplasts make sugar; mitochondria release its energy.\n\n```mermaid\nflowchart TD\n Sun-->Sugar\n Sugar-->Energy\n```";
    let answer = r#"{"quote":"just checks if it matches","text":"Chloroplasts make sugar; mitochondria release its energy.\n\n```mermaid\nflowchart TD\n Sun-->Sugar\n Sugar-->Energy\n```"}"#;
    let (engine, calls) = reading(answer).await;
    let (id, version) = engine.note("Diagram", PASSAGE).await;
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(status, 200);
    assert_eq!(read["note"]["messages"][0]["text"], text);
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(view["threads"][0]["messages"][0]["text"], text);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    engine.stop().await;
}

#[tokio::test]
async fn no_daily_limit_keeps_reading_past_the_old_cap() {
    let (engine, calls) = reading("{}").await;
    let (status, view) = engine
        .post(
            "/api/margin",
            serde_json::json!({ "cap": margin::NO_CAP, "gap": 10 }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(view["cap"], margin::NO_CAP);
    assert_eq!(view["gap"], 10);
    let (status, _) = engine
        .post("/api/margin", serde_json::json!({ "gap": 300 }))
        .await;
    assert_eq!(status, 400);
    for _ in 0..81 {
        assert!(margin::spend_read(&engine.chats(), margin::NO_CAP));
    }
    let (id, version) = engine.note("One", PASSAGE).await;
    assert_eq!(engine.read(&id, &version, &[PASSAGE]).await.0, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(margin::reads_today(&engine.chats(), margin::NO_CAP), 82);
    engine.stop().await;
}

#[tokio::test]
async fn the_daily_cap_counts_reads_across_notes() {
    let (engine, calls) = reading("{}").await;
    margin::set_prefs(
        &engine.chats(),
        &margin::Prefs {
            cap: 1,
            ..Default::default()
        },
    );
    let (first, first_version) = engine.note("One", PASSAGE).await;
    let (second, second_version) = engine.note("Two", PASSAGE).await;
    assert_eq!(engine.read(&first, &first_version, &[PASSAGE]).await.0, 200);
    let (status, why) = engine.read(&second, &second_version, &[PASSAGE]).await;
    assert_eq!((status, why["why"].as_str()), (429, Some("daily")));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.get("/api/margin").await.1["today"], 1);
    engine.stop().await;
}

#[tokio::test]
async fn a_note_is_saved_even_when_another_read_used_the_last_one() {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let writer: Writer = Arc::new(move |_, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
        }
        let _ = released
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(10));
        Ok(SAYS.to_string())
    });
    let engine = Arc::new(
        Engine::with(Powers {
            toolless: Some(writer),
            ..Default::default()
        })
        .await,
    );
    margin::set_prefs(
        &engine.chats(),
        &margin::Prefs {
            cap: 1,
            ..Default::default()
        },
    );
    let (id, version) = engine.note("One", PASSAGE).await;
    let running = Arc::clone(&engine);
    let slow = tokio::spawn(async move { running.read(&id, &version, &[PASSAGE]).await });
    was_started.await.unwrap();
    assert!(margin::spend_read(&engine.chats(), 1));
    release.send(()).unwrap();
    let (status, saved) = slow.await.unwrap();
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["note"]["status"], "open");
    Arc::into_inner(engine).unwrap().stop().await;
}

#[tokio::test]
async fn reads_never_overlap() {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let writer: Writer = Arc::new(move |_, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
        }
        let _ = released.lock().unwrap().recv();
        Ok("{}".to_string())
    });
    let engine = Arc::new(
        Engine::with(Powers {
            toolless: Some(writer),
            ..Default::default()
        })
        .await,
    );
    let (first, first_version) = engine.note("One", PASSAGE).await;
    let (second, second_version) = engine.note("Two", PASSAGE).await;
    let running = Arc::clone(&engine);
    let slow =
        tokio::spawn(async move { running.read(&first, &first_version, &[PASSAGE]).await.0 });
    was_started.await.unwrap();
    let (status, why) = engine.read(&second, &second_version, &[PASSAGE]).await;
    assert_eq!((status, why["why"].as_str()), (429, Some("busy")));
    release.send(()).unwrap();
    assert_eq!(slow.await.unwrap(), 200);
}

#[tokio::test]
async fn no_read_runs_while_felix_is_answering() {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let streamer: crate::chat::Streamer = Arc::new(move |_, _, _, _, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
        }
        let _ = released.lock().unwrap().recv();
        Ok(crate::chat::Reply::from("Done.".to_string()))
    });
    let (writer, calls) = answering(SAYS);
    let engine = Arc::new(
        Engine::with(Powers {
            toolless: Some(writer),
            chat: Some(streamer),
            ..Default::default()
        })
        .await,
    );
    let (id, version) = engine.note("Busy Felix", PASSAGE).await;
    let chatting = Arc::clone(&engine);
    let answer = tokio::spawn(async move {
        chatting
            .post(
                "/api/chat",
                serde_json::json!({ "messages": [{ "role": "user", "text": "hello" }] }),
            )
            .await
            .0
    });
    was_started.await.unwrap();
    let (status, why) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, why["why"].as_str()), (429, Some("felix")));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
    assert_eq!(answer.await.unwrap(), 200);
}

#[tokio::test]
async fn opening_or_browsing_a_note_cannot_start_a_read() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.note("Browse", PASSAGE).await;
    let _ = engine.get(&format!("/api/notes/{id}")).await;
    let _ = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let bare = {
        let (_, made) = engine
            .post(
                "/api/notes",
                serde_json::json!({ "title": "T", "body": PASSAGE }),
            )
            .await;
        let id = made["id"].as_str().unwrap().to_string();
        margin::set_mode(&engine.chats(), &id, "along");
        (id, made["version"].as_str().unwrap().to_string())
    };
    let (status, why) = engine.read(&bare.0, &bare.1, &[PASSAGE]).await;
    assert_eq!((status, why["why"].as_str()), (403, Some("unedited")));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(engine.read(&id, &version, &[PASSAGE]).await.0, 200);
    engine.stop().await;
}

#[tokio::test]
async fn a_read_is_refused_for_bad_ids_versions_and_sizes() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.note("Checks", PASSAGE).await;
    assert_eq!(engine.read(&id, "0", &[PASSAGE]).await.0, 409);
    assert_eq!(engine.read(&id, "../x", &[PASSAGE]).await.0, 400);
    assert_eq!(
        engine.read("00000000-dead", &version, &[PASSAGE]).await.0,
        404
    );
    assert_eq!(engine.read("a%20b", &version, &[PASSAGE]).await.0, 400);
    assert_eq!(engine.read(&id, &version, &[]).await.0, 400);
    assert_eq!(
        engine.read(&id, &version, &["not in the note"]).await.0,
        400
    );
    let long = "x".repeat(margin::MOST_BLOCK_CHARS + 1);
    assert_eq!(engine.read(&id, &version, &[&long]).await.0, 413);
    let many = vec![PASSAGE; margin::MOST_BLOCKS + 1];
    assert_eq!(engine.read(&id, &version, &many).await.0, 413);
    let enough = "y".repeat(margin::MOST_BLOCK_CHARS);
    let wide = vec![enough.as_str(); 3];
    assert_eq!(engine.read(&id, &version, &wide).await.0, 413);
    let huge = "z".repeat(margin::BODY_BYTES + 1);
    assert_eq!(engine.read(&id, &version, &[&huge]).await.0, 413);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    engine.stop().await;
}

#[tokio::test]
async fn the_margin_needs_a_session_and_the_right_host() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.note("Private", PASSAGE).await;
    let path = format!("/api/notes/{id}/margin/read");
    let body = serde_json::json!({ "version": version, "blocks": [PASSAGE] }).to_string();
    let (status, _) = engine
        .send_as("leo_session=nope", "POST", &path, Some(body.clone()))
        .await;
    assert_eq!(status, 401);
    for (method, path) in [
        ("GET", "/api/margin"),
        ("POST", "/api/margin"),
        ("GET", "/api/notes/abc/margin"),
        ("POST", "/api/notes/abc/margin/threads"),
        ("POST", "/api/notes/abc/margin/analyze"),
        ("POST", "/api/notes/abc/margin/threads/t"),
        ("DELETE", "/api/notes/abc/margin/threads/t"),
        ("POST", "/api/notes/abc/margin/threads/t/reply"),
    ] {
        assert_eq!(
            engine.send_as("", method, path, Some("{}".into())).await.0,
            401
        );
    }
    let port = engine.port;
    let foreign = raw(
        port,
        format!(
            "POST {path} HTTP/1.1\r\nHost: evil.example:{port}\r\nCookie: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            engine.cookie,
            body.len()
        ),
    )
    .await;
    assert!(foreign.starts_with("HTTP/1.1 421"), "{foreign}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    engine.stop().await;
}

#[tokio::test]
async fn a_missing_or_failing_ai_stays_silent_and_leaves_one_line() {
    let engine = Engine::with(Powers::default()).await;
    let (id, version) = engine.note("No AI", PASSAGE).await;
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, &read["note"]), (200, &serde_json::Value::Null));
    assert!(engine.get("/api/margin").await.1["problem"].is_string());
    engine.stop().await;

    let fails = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let failing = Arc::clone(&fails);
    let writer: Writer = Arc::new(move |_, _, _| {
        if failing.load(Ordering::SeqCst) {
            anyhow::bail!("secret provider detail")
        }
        Ok("{}".to_string())
    });
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Broken", PASSAGE).await;
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, &read["note"]), (200, &serde_json::Value::Null));
    let (_, settings) = engine.get("/api/margin").await;
    assert_eq!(
        settings["problem"],
        "Your writing AI didn't answer just now (unavailable). Try again in a moment."
    );
    assert!(!settings.to_string().contains("secret provider detail"));
    assert_eq!(settings["today"], 0);
    fails.store(false, Ordering::SeqCst);
    let (other, other_version) = engine.note("Fine", PASSAGE).await;
    assert_eq!(engine.read(&other, &other_version, &[PASSAGE]).await.0, 200);
    assert_eq!(engine.get("/api/margin").await.1["today"], 1);
    assert!(engine.get("/api/margin").await.1["problem"].is_null());
    engine.stop().await;
}

fn talking(text: &'static str, prompts: Arc<Mutex<Vec<String>>>) -> crate::chat::Streamer {
    Arc::new(move |_, user, _, _, _, _| {
        prompts.lock().unwrap().push(user.to_string());
        Ok(crate::chat::Reply::from(text.to_string()))
    })
}

async fn thread_on(engine: &Engine, id: &str) -> String {
    let body = engine.get(&format!("/api/notes/{id}")).await.1["body"]
        .as_str()
        .unwrap()
        .to_string();
    let thread = margin::new_thread(
        id,
        "read",
        margin::placed(&body, PASSAGE, &[]).unwrap(),
        PASSAGE,
        vec![margin::felix_says("In plain text?")],
    );
    margin::put(&engine.chats(), &thread, margin::MOST_OPEN);
    thread.id
}

#[tokio::test]
async fn a_thread_takes_replies_within_its_limits() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let engine = Engine::with(Powers {
        toolless_chat: Some(talking("Yes. Argon2id.", Arc::clone(&prompts))),
        ..Default::default()
    })
    .await;
    let (id, _) = engine.note("Thread", PASSAGE).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}/reply");
    let (status, replied) = engine
        .post(&path, serde_json::json!({ "text": "Hash it?" }))
        .await;
    assert_eq!(status, 200);
    let said: Vec<&str> = replied["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(said, ["In plain text?", "Hash it?", "Yes. Argon2id."]);
    let asked = prompts.lock().unwrap()[0].clone();
    assert!(asked.contains("In plain text?") && asked.contains("Hash it?"));
    assert_eq!(
        engine
            .post(&path, serde_json::json!({ "text": "  " }))
            .await
            .0,
        400
    );
    let long = "x".repeat(margin::MOST_ASK + 1);
    assert_eq!(
        engine
            .post(&path, serde_json::json!({ "text": long }))
            .await
            .0,
        413
    );
    let elsewhere = format!("/api/notes/{id}/margin/threads/nope/reply");
    assert_eq!(
        engine
            .post(&elsewhere, serde_json::json!({ "text": "Hi" }))
            .await
            .0,
        404
    );
    let unknown = format!("/api/notes/missing/margin/threads/{thread}/reply");
    assert_eq!(
        engine
            .post(&unknown, serde_json::json!({ "text": "Hi" }))
            .await
            .0,
        404
    );
    let mut full = margin::one(&engine.chats(), &id, &thread).unwrap();
    full.messages = vec![margin::you_say("x"); margin::MOST_MESSAGES];
    margin::save_messages(&engine.chats(), &full);
    let (status, why) = engine
        .post(&path, serde_json::json!({ "text": "Hi" }))
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("long")));
    engine.stop().await;
}

#[tokio::test]
async fn a_resent_reply_gets_the_first_answer_and_adds_one_message() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let engine = Engine::with(Powers {
        toolless_chat: Some(talking("Yes. Argon2id.", Arc::clone(&prompts))),
        ..Default::default()
    })
    .await;
    let (id, _) = engine.note("Thread", PASSAGE).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}/reply");
    let body = serde_json::json!({ "text": "Hash it?", "request": "press-one" });
    let (first_status, first) = engine.post(&path, body.clone()).await;
    let (second_status, second) = engine.post(&path, body).await;
    assert_eq!((first_status, second_status), (200, 200));
    assert_eq!(first, second);
    assert_eq!(prompts.lock().unwrap().len(), 1);
    let stored = margin::one(&engine.chats(), &id, &thread).unwrap();
    assert_eq!(stored.messages.len(), 3);
    let other = serde_json::json!({ "text": "And salt?", "request": "press-two" });
    assert_eq!(engine.post(&path, other).await.0, 200);
    let stored = margin::one(&engine.chats(), &id, &thread).unwrap();
    assert_eq!(stored.messages.len(), 5);
    engine.stop().await;
}

#[tokio::test]
async fn a_resent_reply_while_the_first_still_runs_is_busy_then_gets_its_answer() {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let streamer: crate::chat::Streamer = Arc::new(move |_, _, _, _, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
        }
        let _ = released
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(10));
        Ok(crate::chat::Reply::from("Yes. Argon2id.".to_string()))
    });
    let engine = Arc::new(
        Engine::with(Powers {
            toolless_chat: Some(streamer),
            ..Default::default()
        })
        .await,
    );
    let (id, _) = engine.note("Thread", PASSAGE).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}/reply");
    let body = serde_json::json!({ "text": "Hash it?", "request": "press-one" });
    let running = Arc::clone(&engine);
    let (first_path, first_body) = (path.clone(), body.clone());
    let slow = tokio::spawn(async move { running.post(&first_path, first_body).await });
    was_started.await.unwrap();
    let (status, why) = engine.post(&path, body.clone()).await;
    assert_eq!((status, why["why"].as_str()), (409, Some("busy")));
    release.send(()).unwrap();
    assert_eq!(slow.await.unwrap().0, 200);
    let (status, again) = engine.post(&path, body).await;
    assert_eq!(status, 200);
    assert_eq!(again["messages"].as_array().unwrap().len(), 3);
    Arc::into_inner(engine).unwrap().stop().await;
}

#[tokio::test]
async fn a_weave_rewrites_only_the_paragraph_and_keeps_the_note_safe() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let engine = Engine::with(Powers {
        toolless_chat: Some(talking(
            "So login checks a hash with Argon2id.",
            Arc::clone(&prompts),
        )),
        ..Default::default()
    })
    .await;
    let old = format!("Intro.\n\nSo {PASSAGE}.\n\nEnd.");
    let (id, version) = engine.note("Weave", &old).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}/weave");
    let (status, _) = engine
        .post(&path, serde_json::json!({ "base": "0000000000000000" }))
        .await;
    assert_eq!(status, 409);
    assert!(prompts.lock().unwrap().is_empty());
    let ask = serde_json::json!({ "base": version, "request": "r1" });
    let (status, woven) = engine.post(&path, ask.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(
        woven["body"],
        "Intro.\n\nSo login checks a hash with Argon2id.\n\nEnd."
    );
    assert_eq!(woven["before"], old.as_str());
    assert_eq!(
        (woven["from"].as_u64(), woven["to"].as_u64()),
        (Some(3), Some(3))
    );
    let asked = prompts.lock().unwrap()[0].clone();
    assert!(asked.contains(&format!("So {PASSAGE}.")) && asked.contains("In plain text?"));
    assert!(!asked.contains("Intro.") && !asked.contains("End."));
    assert_eq!(engine.post(&path, ask).await.1, woven);
    assert_eq!(prompts.lock().unwrap().len(), 1);

    let version = engine.edit(&id, "Nothing left here.").await;
    let (status, why) = engine
        .post(&path, serde_json::json!({ "base": version }))
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("lost")));
    let long = format!("{} {PASSAGE}", "word ".repeat(margin::MOST_WEAVE / 5));
    let version = engine.edit(&id, &long).await;
    let (status, why) = engine
        .post(&path, serde_json::json!({ "base": version }))
        .await;
    assert_eq!((status, why["why"].as_str()), (413, Some("long")));
    let version = engine.edit(&id, &format!("```\n{PASSAGE}\n\nmore")).await;
    let (status, why) = engine
        .post(&path, serde_json::json!({ "base": version }))
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("code")));
    let version = engine
        .edit(
            &id,
            &format!("```\nfn main() {{}}\n\n{PASSAGE}\n\nmore\n```"),
        )
        .await;
    let (status, why) = engine
        .post(&path, serde_json::json!({ "base": version }))
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("code")));
    let version = engine.edit(&id, &old).await;
    margin::set_status(&engine.chats(), &id, &thread, "resolved", margin::MOST_OPEN);
    let (status, _) = engine
        .post(&path, serde_json::json!({ "base": version }))
        .await;
    assert_eq!(status, 409);
    engine.stop().await;
}

#[tokio::test]
async fn a_weave_refuses_when_the_note_changed_while_felix_wrote() {
    let (asked, wait) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (seen, held) = (Arc::clone(&asked), Arc::clone(&wait));
    let engine = Arc::new(
        Engine::with(Powers {
            toolless_chat: Some(Arc::new(move |_, _, _, _, _, _| {
                seen.store(1, Ordering::SeqCst);
                while held.load(Ordering::SeqCst) == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Ok(crate::chat::Reply::from(
                    "So login checks a hash with Argon2id.".to_string(),
                ))
            })),
            ..Default::default()
        })
        .await,
    );
    let (id, version) = engine.note("Raced", &format!("So {PASSAGE}.")).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}/weave");
    let racing = Arc::clone(&engine);
    let weaving = tokio::spawn(async move {
        racing
            .post(&path, serde_json::json!({ "base": version }))
            .await
    });
    while asked.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    engine
        .edit(&id, &format!("So {PASSAGE}, typed meanwhile."))
        .await;
    wait.store(1, Ordering::SeqCst);
    let (status, why) = weaving.await.unwrap();
    assert_eq!((status, why["why"].as_str()), (409, Some("changed")));
    let body = engine.get(&format!("/api/notes/{id}")).await.1["body"].clone();
    assert_eq!(body, format!("So {PASSAGE}, typed meanwhile."));
    if let Ok(engine) = Arc::try_unwrap(engine) {
        engine.stop().await;
    }
}

#[tokio::test]
async fn a_pin_is_kept_on_the_thread_through_reloads_and_the_export() {
    let engine = Engine::with(Powers::default()).await;
    let (id, _) = engine.note("Pinned", PASSAGE).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}");
    let pin = |body: &str| engine.send_as(&engine.cookie, "PATCH", &path, Some(body.into()));
    let view = format!("/api/notes/{id}/margin");
    assert_eq!(engine.get(&view).await.1["threads"][0]["pinned"], false);
    let (code, pinned) = pin(r#"{"pinned":true}"#).await;
    assert_eq!((code, &pinned["pinned"]), (200, &serde_json::json!(true)));
    assert_eq!(engine.get(&view).await.1["threads"][0]["pinned"], true);
    assert!(margin::every(&engine.chats())[0].pinned);
    let parts = crate::export::Parts {
        chats: true,
        ..Default::default()
    };
    let mut zip = std::io::Cursor::new(Vec::new());
    crate::export::write_zip(
        &mut zip,
        &engine.dir.path().join("notes"),
        &engine.chats(),
        parts,
    )
    .unwrap();
    zip.set_position(0);
    let mut archive = zip::ZipArchive::new(zip).unwrap();
    let exported: serde_json::Value =
        serde_json::from_reader(archive.by_name("leo/margin.json").unwrap()).unwrap();
    assert_eq!(exported["threads"][0]["pinned"], true);
    assert_eq!(pin(r#"{"pinned":false}"#).await.1["pinned"], false);
    assert_eq!(pin(r#"{"pinned":"yes"}"#).await.0, 422);
    let wrong = format!("/api/notes/{id}/margin/threads/{}", uuid::Uuid::new_v4());
    let (missing, _) = engine
        .send_as(
            &engine.cookie,
            "PATCH",
            &wrong,
            Some(r#"{"pinned":true}"#.into()),
        )
        .await;
    assert_eq!(missing, 404);
    engine.stop().await;
}

#[tokio::test]
async fn dismissing_and_resolving_take_a_thread_off_the_page_and_undo_brings_it_back() {
    let engine = Engine::with(Powers::default()).await;
    let (id, _) = engine.note("Done", PASSAGE).await;
    let thread = thread_on(&engine, &id).await;
    let path = format!("/api/notes/{id}/margin/threads/{thread}");
    let open = |view: serde_json::Value| view["threads"].as_array().unwrap().len();
    for status in ["dismissed", "resolved"] {
        let (code, _) = engine
            .post(&path, serde_json::json!({ "status": status }))
            .await;
        assert_eq!(code, 200);
        assert_eq!(
            open(engine.get(&format!("/api/notes/{id}/margin")).await.1),
            0
        );
        let reply = format!("{path}/reply");
        assert_eq!(
            engine
                .post(&reply, serde_json::json!({ "text": "Hi" }))
                .await
                .0,
            409
        );
        engine
            .post(&path, serde_json::json!({ "status": "open" }))
            .await;
        assert_eq!(
            open(engine.get(&format!("/api/notes/{id}/margin")).await.1),
            1
        );
    }
    assert_eq!(
        engine
            .post(&path, serde_json::json!({ "status": "gone" }))
            .await
            .0,
        400
    );
    let wrong = format!("/api/notes/{id}/margin/threads/nope");
    assert_eq!(
        engine
            .post(&wrong, serde_json::json!({ "status": "open" }))
            .await
            .0,
        404
    );
    engine.stop().await;
}

#[tokio::test]
async fn asking_about_selected_words_needs_no_read_along_and_the_words_must_be_there() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let engine = Engine::with(Powers {
        toolless_chat: Some(talking("Because it is.", prompts)),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Ask", "same words\n\nsame words").await;
    margin::set_mode(&engine.chats(), &id, "ask");
    let path = format!("/api/notes/{id}/margin/threads");
    let ask = |quote: &str, prefix: &str, version: &str| serde_json::json!({ "version": version, "quote": quote, "prefix": prefix, "question": "Why?" });
    let (status, thread) = engine
        .post(&path, ask("same words", "\n\n", &version))
        .await;
    assert_eq!(status, 200, "{thread}");
    assert_eq!(thread["prefix"], "same words\n\n");
    assert_eq!(thread["source"], "ask");
    assert_eq!(thread["messages"][1]["text"], "Because it is.");
    assert_eq!(engine.post(&path, ask("same words", "", "0")).await.0, 409);
    let (status, why) = engine.post(&path, ask("not there", "", &version)).await;
    assert_eq!((status, why["why"].as_str()), (409, Some("lost")));
    let long = "x".repeat(margin::MOST_ASKED + 1);
    assert_eq!(engine.post(&path, ask(&long, "", &version)).await.0, 413);
    let wide = "word ".repeat(200);
    let (wide_id, wide_version) = engine.note("Wide", &wide).await;
    margin::set_mode(&engine.chats(), &wide_id, "ask");
    let (status, thread) = engine
        .post(
            &format!("/api/notes/{wide_id}/margin/threads"),
            ask(wide.trim(), "", &wide_version),
        )
        .await;
    assert_eq!(status, 200, "{thread}");
    let (status, why) = engine
        .post(&path, ask("same words", "not beside it ", &version))
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("lost")));
    let (status, why) = engine.post(&path, ask("sa", "", &version)).await;
    assert_eq!((status, why["why"].as_str()), (400, Some("short")));
    let (status, silent) = engine
        .post(&path, serde_json::json!({ "version": version, "quote": "same words", "prefix": "\n\n", "question": "" }))
        .await;
    assert_eq!(status, 200, "{silent}");
    assert_eq!(silent["messages"].as_array().unwrap().len(), 1);
    assert_eq!(silent["messages"][0]["from"], "felix");
    engine.stop().await;
}

#[tokio::test]
async fn threads_follow_their_note_through_the_trash() {
    let engine = Engine::with(Powers::default()).await;
    let (keep, _) = engine.note("Keep", PASSAGE).await;
    let (id, _) = engine.note("Delete", PASSAGE).await;
    thread_on(&engine, &keep).await;
    let thread = thread_on(&engine, &id).await;
    let (status, _) = engine
        .send_as(&engine.cookie, "DELETE", &format!("/api/notes/{id}"), None)
        .await;
    assert_eq!(status, 204);
    assert_eq!(engine.get(&format!("/api/notes/{id}/margin")).await.0, 404);
    assert!(margin::one(&engine.chats(), &id, &thread).is_some());
    let (status, _) = engine
        .post(&format!("/api/trash/{id}/restore"), serde_json::json!({}))
        .await;
    assert!(status < 300, "{status}");
    assert_eq!(
        engine.get(&format!("/api/notes/{id}/margin")).await.1["threads"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    engine
        .send_as(&engine.cookie, "DELETE", &format!("/api/notes/{id}"), None)
        .await;
    let (status, _) = engine
        .post("/api/trash/delete", serde_json::json!({ "all": true }))
        .await;
    assert_eq!(status, 200);
    assert!(margin::one(&engine.chats(), &id, &thread).is_none());
    assert_eq!(margin::open_on(&engine.chats(), &keep).len(), 1);
    engine.stop().await;
}

#[tokio::test]
async fn deleting_the_last_note_for_good_takes_its_threads_at_once() {
    let engine = Engine::with(Powers::default()).await;
    let (id, _) = engine.note("Only one", PASSAGE).await;
    let thread = thread_on(&engine, &id).await;
    engine
        .send_as(&engine.cookie, "DELETE", &format!("/api/notes/{id}"), None)
        .await;
    assert!(margin::one(&engine.chats(), &id, &thread).is_some());
    let (status, _) = engine
        .post(
            "/api/trash/delete",
            serde_json::json!({ "ids": [id.clone()] }),
        )
        .await;
    assert_eq!(status, 200);
    assert!(margin::every(&engine.chats()).is_empty());
    engine.stop().await;
}

#[test]
fn an_unreadable_file_stops_the_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes");
    let mut store = Store::load_from(&notes).unwrap();
    store.create_note("Kept", "", vec![], "").unwrap();
    store.save().unwrap();
    std::fs::write(
        notes.join("Broken.md"),
        "---\ntitle: [unclosed\n---\nbody\n",
    )
    .unwrap();
    let store = Store::load_from(&notes).unwrap();
    assert!(!store.unreadable.is_empty());
    let chats = crate::chats::dir_for(&store.notes_dir);
    margin::put(
        &chats,
        &margin::new_thread(
            "elsewhere",
            "read",
            margin::Place {
                prefix: String::new(),
                suffix: String::new(),
            },
            "words",
            Vec::new(),
        ),
        margin::MOST_OPEN,
    );
    let start = chrono::Utc::now();
    assert_eq!(margin::sweep_store(&chats, &store), 0);
    let held = margin::every(&chats);
    assert_eq!(held.len(), 1);
    let kept = std::collections::HashSet::new();
    margin::sweep(&chats, &kept, start);
    assert_eq!(
        margin::sweep(&chats, &kept, start + chrono::Duration::days(8)),
        1,
        "without the guard the same thread would go"
    );
}

#[tokio::test]
async fn bad_thread_ids_are_refused_before_anything_runs() {
    let engine = Engine::with(Powers::default()).await;
    let (id, _) = engine.note("Ids", PASSAGE).await;
    let reply = format!("/api/notes/{id}/margin/threads/a%20b/reply");
    let status = format!("/api/notes/{id}/margin/threads/a%20b");
    assert_eq!(
        engine
            .post(&reply, serde_json::json!({ "text": "Hi" }))
            .await
            .0,
        400
    );
    assert_eq!(
        engine
            .post(&status, serde_json::json!({ "status": "open" }))
            .await
            .0,
        400
    );
    engine.stop().await;
}

struct Healthy;

impl crate::app_settings::AppApi for Healthy {
    fn view(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
    fn change(&self, _: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
    fn health(&self, _: &std::path::Path) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({ "sections": [] }))
    }
}

#[tokio::test]
async fn health_shows_a_failed_read_through_its_route() {
    let writer: Writer = Arc::new(|_, _, _| anyhow::bail!("down"));
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        app_settings: Some(Arc::new(Healthy)),
        ..Default::default()
    })
    .await;
    let (_, view) = engine.get("/api/health").await;
    assert_eq!(view["sections"].as_array().unwrap().len(), 0);
    let (id, version) = engine.note("Broken", PASSAGE).await;
    let (_, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(read["failed"], true, "the page keeps its baseline");
    let (_, view) = engine.get("/api/health").await;
    assert_eq!(view["sections"][0]["title"], "Felix");
    assert_eq!(
        view["sections"][0]["checks"][0]["detail"],
        "Your writing AI didn't answer just now (unavailable). Try again in a moment."
    );
    engine.stop().await;
}

#[tokio::test]
async fn the_same_request_adds_once_even_when_its_base_has_moved() {
    let engine = Engine::with(Powers::default()).await;
    let (id, version) = engine.note("Twice", PASSAGE).await;
    let path = format!("/api/notes/{id}/insert");
    let first = serde_json::json!({ "text": "Added.", "base": version, "request": "press-one" });
    let (status, done) = engine.post(&path, first).await;
    assert_eq!(status, 200);
    let moved = done["note"]["version"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let moved = if moved.is_empty() {
        done["version"].as_str().unwrap().to_string()
    } else {
        moved
    };
    let again = serde_json::json!({ "text": "Added.", "base": moved, "request": "press-one" });
    assert_eq!(engine.post(&path, again).await.0, 200);
    let body = engine.get(&format!("/api/notes/{id}")).await.1["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(body.matches("Added.").count(), 1, "{body}");
    engine.stop().await;
}

#[test]
fn the_sweep_keeps_everything_while_notes_cannot_be_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::load_from(&dir.path().join("notes")).unwrap();
    let chats = crate::chats::dir_for(&store.notes_dir);
    let thread = margin::new_thread(
        "note",
        "read",
        margin::Place {
            prefix: String::new(),
            suffix: String::new(),
        },
        "words",
        Vec::new(),
    );
    margin::put(&chats, &thread, margin::MOST_OPEN);
    assert_eq!(margin::sweep_store(&chats, &store), 0);
    assert_eq!(margin::open_on(&chats, "note").len(), 1);
}

#[test]
fn the_tool_follows_each_access_mode_and_offers_undo() {
    use crate::tools::{self, Access};
    let (state, _dir, ids) = super::state_with(&[("Passage note", "")]);
    let _ = super::run(crate::routes::notes::update_note(
        axum::extract::State(state.clone()),
        axum::extract::Path(ids[0].clone()),
        axum::Json(serde_json::from_value(serde_json::json!({ "body": PASSAGE })).unwrap()),
    ))
    .unwrap();
    let call = |quote: &str| tools::Call {
        name: "add_margin_note".into(),
        args: serde_json::json!({ "note": "Passage note", "quote": quote, "text": "Hash it first." }),
    };
    assert!(tools::manual_for(Access::Ask).contains("add_margin_note"));
    assert!(tools::manual_for(Access::Auto).contains("add_margin_note"));
    assert!(!tools::manual_for(Access::Read).contains("add_margin_note"));
    let native = |access| {
        tools::native_specs(access)
            .iter()
            .any(|spec| spec.name == "add_margin_note")
    };
    assert!(native(Access::Ask) && native(Access::Auto) && !native(Access::Read));

    super::run(async {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let mut desk = tools::Desk::new(vec![], 8000);
            let read =
                crate::routes::organize::run(&state, &mut desk, Access::Read, &call(PASSAGE));
            assert!(read.proposal.is_none(), "{}", read.result);
            assert!(read.result.contains("read only"));

            let bad =
                crate::routes::organize::run(&state, &mut desk, Access::Ask, &call("not here"));
            assert!(bad.proposal.is_none());
            assert!(bad.result.contains("copied exactly"));

            let done = crate::routes::organize::run(&state, &mut desk, Access::Ask, &call(PASSAGE));
            let proposal = done.proposal.expect("a card");
            assert!(
                margin::open_on(&state.chats, &ids[0]).is_empty(),
                "asked first"
            );
            assert!(matches!(proposal, tools::Proposal::AddMarginNote { .. }));

            let shown = crate::routes::organize::act_now(&state, &proposal).unwrap();
            assert_eq!(shown["state"], "applied");
            let threads = margin::open_on(&state.chats, &ids[0]);
            assert_eq!(threads.len(), 1);
            assert_eq!(threads[0].source, "chat");
            assert_eq!(shown["undo"]["kind"], "remove_margin_note");

            let undo: tools::Proposal = serde_json::from_value(shown["undo"].clone()).unwrap();
            let mut replied = threads[0].clone();
            replied.messages.push(margin::you_say("Good point."));
            margin::save_messages(&state.chats, &replied);
            let refused = crate::routes::organize::act_now(&state, &undo).unwrap_err();
            assert!(refused.contains("dismiss"), "{refused}");
            assert_eq!(margin::open_on(&state.chats, &ids[0]).len(), 1);
            let mut quiet = replied;
            quiet.messages.pop();
            margin::save_messages(&state.chats, &quiet);
            let undone = crate::routes::organize::act_now(&state, &undo).unwrap();
            assert!(margin::open_on(&state.chats, &ids[0]).is_empty());
            assert_eq!(undone["undo"]["kind"], "add_margin_note");
        })
        .await
        .unwrap();
    });
}

#[test]
fn gathering_includes_open_threads_after_the_notes_within_the_budget() {
    let thread = margin::new_thread(
        "n",
        "read",
        margin::Place {
            prefix: String::new(),
            suffix: String::new(),
        },
        PASSAGE,
        vec![margin::felix_says("In plain text?")],
    );
    let block = margin::context(std::slice::from_ref(&thread), 1_000);
    assert!(block.contains("In plain text?") && block.contains(PASSAGE));

    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    let id = store
        .create_note("Long", "word ".repeat(20_000), vec![], "")
        .unwrap()
        .id
        .clone();
    let cache = crate::graph::Cache::default();
    let room = 12_000;
    let (_, text) = crate::chat::gather_seeing(
        &store,
        &cache,
        Some(&id),
        &[],
        &[],
        "word",
        room,
        None,
        &[],
        &block,
    );
    assert!(text.ends_with(&block), "the threads come after the notes");
    assert!(
        text.chars().count() <= room + 64,
        "{}",
        text.chars().count()
    );
    let (_, plain) = crate::chat::gather(&store, &cache, Some(&id), &[], "word", room);
    assert!(!plain.contains("margin_threads"));
}

#[test]
fn a_thread_on_a_trashed_note_exports_only_with_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    let id = store
        .create_note("Gone", "words here", vec![], "")
        .unwrap()
        .id
        .clone();
    store.save().unwrap();
    store.delete_notes(std::slice::from_ref(&id));
    store.save().unwrap();
    let chats = crate::chats::dir_for(&store.notes_dir);
    margin::put(
        &chats,
        &margin::new_thread(
            &id,
            "read",
            margin::placed("words here", "words", &[]).unwrap(),
            "words",
            Vec::new(),
        ),
        margin::MOST_OPEN,
    );
    let packed = |trash: bool| {
        let parts = crate::export::Parts {
            chats: true,
            trash,
            ..Default::default()
        };
        let mut zip = std::io::Cursor::new(Vec::new());
        crate::export::write_zip(&mut zip, &store.notes_dir, &chats, parts).unwrap();
        zip.set_position(0);
        let mut archive = zip::ZipArchive::new(zip).unwrap();
        let found = archive.by_name("leo/margin.json").is_ok();
        found
    };
    assert!(!packed(false));
    assert!(packed(true));
}

#[test]
fn exports_cover_margin_threads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::load_from(&dir.path().join("notes")).unwrap();
    let chats = crate::chats::dir_for(&store.notes_dir);
    let thread = margin::new_thread(
        "note",
        "read",
        margin::Place {
            prefix: String::new(),
            suffix: String::new(),
        },
        "words",
        vec![margin::felix_says("Kept here.")],
    );
    margin::put(&chats, &thread, margin::MOST_OPEN);
    let mut zip = std::io::Cursor::new(Vec::new());
    crate::export::write_zip(&mut zip, &store.notes_dir, &chats, Default::default()).unwrap();
    zip.set_position(0);
    let mut archive = zip::ZipArchive::new(zip).unwrap();
    assert!(
        archive.by_name("leo/margin.json").is_err(),
        "chats were not chosen"
    );
    let with_chats = crate::export::Parts {
        chats: true,
        ..Default::default()
    };
    let mut zip = std::io::Cursor::new(Vec::new());
    crate::export::write_zip(&mut zip, &store.notes_dir, &chats, with_chats).unwrap();
    zip.set_position(0);
    let mut archive = zip::ZipArchive::new(zip).unwrap();
    assert!(archive.by_name("leo/margin.json").is_ok());
}

const FIVE: &str = r#"{"notes":[
{"quote":"just checks if it matches","text":"A"},
{"quote":"Save the password","text":"D"},
{"quote":"just checks if it matches","text":"B"},
{"quote":"not anywhere in the note","text":"C"},
{"quote":"in the users table","text":"E"}]}"#;
const FOUR: &str = r#"{"notes":[
{"quote":"Save the password","text":"A"},
{"quote":"in the users table","text":"B"},
{"quote":"just checks if it matches","text":"C"},
{"quote":"Then login","text":"D"}]}"#;
const TWO_BLOCKS: &str =
    "Save the password in the users table.\n\nThen login just checks if it matches, I think.";

#[tokio::test]
async fn reading_through_works_in_ask_mode_on_a_note_never_edited() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.plain(PASSAGE).await;
    let before = std::fs::read_dir(engine.dir.path().join("notes"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap_or_default())
        .collect::<Vec<_>>();
    let (status, reply) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!(status, 200);
    let notes = reply["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["source"], "through");
    assert_eq!(notes[0]["quote"], "just checks if it matches");
    assert_eq!(notes[0]["prefix"], "then login ");
    assert_eq!(notes[0]["suffix"], "");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.get("/api/margin").await.1["today"], 0);
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(
        (
            view["mode"].as_str(),
            view["threads"].as_array().unwrap().len()
        ),
        (Some("ask"), 1)
    );
    let after = std::fs::read_dir(engine.dir.path().join("notes"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(before, after);
    engine.stop().await;
}

#[tokio::test]
async fn a_part_saves_at_most_its_share_and_drops_what_it_cannot_place() {
    let (engine, _) = reading(FIVE).await;
    let (id, version) = engine.plain(TWO_BLOCKS).await;
    let seed = margin::new_thread(
        &id,
        "ask",
        margin::placed(TWO_BLOCKS, "in the users table", &[]).unwrap(),
        "in the users table",
        vec![margin::felix_says("Seeded.")],
    );
    assert!(margin::put(&engine.chats(), &seed, margin::MOST_OPEN));
    let (status, reply) = engine.analyze(&id, &version, &[TWO_BLOCKS], Some(3)).await;
    assert_eq!(status, 200);
    let quotes: Vec<&str> = reply["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["quote"].as_str().unwrap())
        .collect();
    assert_eq!(quotes, ["Then login just checks if it matches, I think."]);
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(view["threads"].as_array().unwrap().len(), 2);
    engine.stop().await;

    let (engine, _) = reading(FOUR).await;
    let (id, version) = engine.plain(TWO_BLOCKS).await;
    let (_, reply) = engine.analyze(&id, &version, &[TWO_BLOCKS], Some(9)).await;
    assert_eq!(reply["notes"].as_array().unwrap().len(), 2);
    engine.stop().await;
}

#[tokio::test]
async fn silence_a_single_object_and_bad_output() {
    for silent in [r#"{"notes":[]}"#, "{}", ""] {
        let (engine, _) = reading(silent).await;
        let (id, version) = engine.plain(PASSAGE).await;
        let (status, reply) = engine.analyze(&id, &version, &[PASSAGE], None).await;
        assert_eq!(status, 200);
        assert_eq!(reply["notes"], serde_json::json!([]));
        assert!(reply.get("failed").is_none());
        engine.stop().await;
    }
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.plain(PASSAGE).await;
    let (_, reply) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!(reply["notes"].as_array().unwrap().len(), 1);
    engine.stop().await;
    let (engine, _) = reading("I think this note is fine.").await;
    let (id, version) = engine.plain(PASSAGE).await;
    let (_, reply) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!(
        (&reply["notes"], &reply["failed"]),
        (&serde_json::json!([]), &serde_json::json!(true))
    );
    assert_eq!(
        engine.get("/api/margin").await.1["problem"].as_str(),
        Some(margin::PROBLEM)
    );
    engine.stop().await;
}

#[tokio::test]
async fn parts_skip_the_gap_but_not_overlap_or_a_streaming_reply() {
    let (engine, calls) = reading("{}").await;
    let (id, version) = engine.note("Parts", PASSAGE).await;
    assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 200);
    assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let (status, why) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!((status, why["why"].as_str()), (429, Some("soon")));
    engine.stop().await;

    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let writer: Writer = Arc::new(move |_, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
        }
        let _ = released.lock().unwrap().recv();
        Ok("{}".to_string())
    });
    let engine = Arc::new(
        Engine::with(Powers {
            toolless: Some(writer),
            ..Default::default()
        })
        .await,
    );
    let (first, first_version) = engine.plain(PASSAGE).await;
    let (second, second_version) = engine.plain(PASSAGE).await;
    let running = Arc::clone(&engine);
    let slow = tokio::spawn(async move {
        running
            .analyze(&first, &first_version, &[PASSAGE], None)
            .await
            .0
    });
    was_started.await.unwrap();
    let (status, why) = engine
        .analyze(&second, &second_version, &[PASSAGE], None)
        .await;
    assert_eq!((status, why["why"].as_str()), (429, Some("busy")));
    release.send(()).unwrap();
    assert_eq!(slow.await.unwrap(), 200);

    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let streamer: crate::chat::Streamer = Arc::new(move |_, _, _, _, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
        }
        let _ = released.lock().unwrap().recv();
        Ok(crate::chat::Reply::from("Done.".to_string()))
    });
    let (writer, calls) = answering(SAYS);
    let engine = Arc::new(
        Engine::with(Powers {
            toolless: Some(writer),
            chat: Some(streamer),
            ..Default::default()
        })
        .await,
    );
    let (id, version) = engine.plain(PASSAGE).await;
    let chatting = Arc::clone(&engine);
    let answer = tokio::spawn(async move {
        chatting
            .post(
                "/api/chat",
                serde_json::json!({ "messages": [{ "role": "user", "text": "hello" }] }),
            )
            .await
            .0
    });
    was_started.await.unwrap();
    let (status, why) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!((status, why["why"].as_str()), (429, Some("felix")));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
    assert_eq!(answer.await.unwrap(), 200);
}

#[tokio::test]
async fn parts_never_change_today_and_still_run_at_the_cap() {
    let (engine, calls) = reading("{}").await;
    margin::set_prefs(
        &engine.chats(),
        &margin::Prefs {
            cap: 1,
            ..Default::default()
        },
    );
    assert!(margin::spend_read(&engine.chats(), 1));
    let (id, version) = engine.plain(PASSAGE).await;
    for _ in 0..2 {
        assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 200);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(engine.get("/api/margin").await.1["today"], 1);
    engine.stop().await;
}

#[tokio::test]
async fn stale_bad_and_oversized_parts_are_refused() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.plain(PASSAGE).await;
    assert_eq!(engine.analyze(&id, "0", &[PASSAGE], None).await.0, 409);
    assert_eq!(engine.analyze(&id, "../x", &[PASSAGE], None).await.0, 400);
    assert_eq!(
        engine
            .analyze(&id, &version, &["not in the note"], None)
            .await
            .0,
        400
    );
    assert_eq!(engine.analyze(&id, &version, &[], None).await.0, 400);
    assert_eq!(
        engine
            .analyze("00000000-dead", &version, &[PASSAGE], None)
            .await
            .0,
        404
    );
    let long = "x".repeat(margin::MOST_BLOCK_CHARS + 1);
    assert_eq!(engine.analyze(&id, &version, &[&long], None).await.0, 413);
    let many = vec![PASSAGE; margin::MOST_BLOCKS + 1];
    assert_eq!(engine.analyze(&id, &version, &many, None).await.0, 413);
    let huge = "z".repeat(margin::BODY_BYTES + 1);
    assert_eq!(engine.analyze(&id, &version, &[&huge], None).await.0, 413);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    engine.stop().await;
}

#[tokio::test]
async fn a_full_note_and_a_missing_ai_say_why() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.plain(PASSAGE).await;
    for n in 0..margin::MOST_OPEN {
        let thread = margin::new_thread(
            &id,
            "ask",
            margin::Place {
                prefix: String::new(),
                suffix: String::new(),
            },
            &format!("quote {n}"),
            vec![margin::felix_says("Hm.")],
        );
        assert!(margin::put(&engine.chats(), &thread, margin::MOST_OPEN));
    }
    let (status, why) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!((status, why["why"].as_str()), (409, Some("full")));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    engine.stop().await;

    let engine = Engine::with(Powers::default()).await;
    let (id, version) = engine.plain(PASSAGE).await;
    let (status, why) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!((status, why["why"].as_str()), (503, Some("no-ai")));
    assert!(why["error"]
        .as_str()
        .unwrap()
        .starts_with(leo_core::NO_WRITING_AI));
    assert_eq!(
        engine.get("/api/margin").await.1["problem"].as_str(),
        Some(crate::no_writer().as_str())
    );
    engine.stop().await;
}

fn nowhere() -> margin::Place {
    margin::Place {
        prefix: String::new(),
        suffix: String::new(),
    }
}

fn fill(engine: &Engine, id: &str, source: &str, count: usize, limit: usize) -> Vec<String> {
    (0..count)
        .map(|n| {
            let thread = margin::new_thread(
                id,
                source,
                nowhere(),
                &format!("quote {n}"),
                vec![margin::felix_says("Hm.")],
            );
            assert!(margin::put(&engine.chats(), &thread, limit));
            thread.id
        })
        .collect()
}

#[tokio::test]
async fn notes_the_page_cannot_place_make_room_to_ask_reopen_and_read_through() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let (writer, _) = answering("{}");
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        toolless_chat: Some(talking("Because it is.", prompts)),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Full", PASSAGE).await;
    margin::set_mode(&engine.chats(), &id, "ask");
    let ids = fill(&engine, &id, "ask", margin::MOST_OPEN, margin::MOST_OPEN);
    let path = format!("/api/notes/{id}/margin/threads");
    let ask = |gone: &[String]| serde_json::json!({ "version": version, "quote": "login", "prefix": "then ", "question": "Why?", "gone": gone });
    let (status, why) = engine.post(&path, ask(&[])).await;
    assert_eq!((status, why["why"].as_str()), (409, Some("full")));
    let (other, _) = engine.plain(PASSAGE).await;
    let strangers = fill(&engine, &other, "ask", 5, margin::MOST_OPEN);
    assert_eq!(engine.post(&path, ask(&strangers)).await.0, 409);
    assert_eq!(engine.post(&path, ask(&ids[..1])).await.0, 200);
    assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 409);
    let through = serde_json::json!({ "version": version, "blocks": [PASSAGE], "gone": &ids[..2] });
    let (status, reply) = engine
        .post(&format!("/api/notes/{id}/margin/analyze"), through)
        .await;
    assert_eq!(status, 200, "{reply}");
    let status_path = format!("{path}/{}", ids[0]);
    margin::set_status(&engine.chats(), &id, &ids[0], "resolved", margin::MOST_OPEN);
    let (status, why) = engine
        .post(&status_path, serde_json::json!({ "status": "open" }))
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("full")));
    let (status, _) = engine
        .post(
            &status_path,
            serde_json::json!({ "status": "open", "gone": &ids[1..3] }),
        )
        .await;
    assert_eq!(status, 200);
    engine.stop().await;
}

#[tokio::test]
async fn however_many_notes_are_gone_the_note_holds_twice_the_limit() {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let engine = Engine::with(Powers {
        toolless_chat: Some(talking("Because it is.", prompts)),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Many", PASSAGE).await;
    margin::set_mode(&engine.chats(), &id, "ask");
    let mut gone = fill(&engine, &id, "ask", margin::MOST_OPEN, margin::MOST_OPEN);
    gone.extend(fill(
        &engine,
        &id,
        "ask",
        margin::MOST_OPEN,
        2 * margin::MOST_OPEN,
    ));
    gone.extend((0..30).map(|n| format!("other-{n}")));
    assert_eq!(
        margin::open_on(&engine.chats(), &id).len(),
        2 * margin::MOST_OPEN
    );
    let (status, why) = engine
        .post(
            &format!("/api/notes/{id}/margin/threads"),
            serde_json::json!({ "version": version, "quote": "login", "prefix": "then ", "question": "Why?", "gone": gone }),
        )
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("full")));
    assert_eq!(
        margin::open_on(&engine.chats(), &id).len(),
        2 * margin::MOST_OPEN
    );
    engine.stop().await;
}

#[tokio::test]
async fn a_read_along_reads_a_full_note_and_frees_the_oldest_free_note_at_twice_the_limit() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.note("Full", PASSAGE).await;
    fill(&engine, &id, "ask", margin::MOST_OPEN, margin::MOST_OPEN);
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["note"]["source"], "read");
    assert_eq!(read["resolved"], serde_json::json!([]));
    assert_eq!(
        margin::open_on(&engine.chats(), &id).len(),
        margin::MOST_OPEN + 1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    engine.stop().await;

    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Twice", PASSAGE).await;
    let ids = fill(
        &engine,
        &id,
        "ask",
        2 * margin::MOST_OPEN,
        2 * margin::MOST_OPEN,
    );
    for pinned in ids.iter().filter(|t| **t != ids[1]) {
        margin::set_pinned(&engine.chats(), &id, pinned, true);
    }
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(status, 200, "{read}");
    assert!(read["note"]["id"].is_string());
    assert_eq!(read["resolved"], serde_json::json!([ids[1]]));
    assert_eq!(
        margin::one(&engine.chats(), &id, &ids[1]).unwrap().status,
        "resolved"
    );
    assert_eq!(
        margin::open_on(&engine.chats(), &id).len(),
        2 * margin::MOST_OPEN
    );
    engine.stop().await;
}

#[tokio::test]
async fn a_read_along_that_finds_nothing_to_free_after_the_model_answered_costs_nothing_and_reads_again(
) {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let (started, was_started) = tokio::sync::oneshot::channel::<()>();
    let started = Mutex::new(Some(started));
    let writer: Writer = Arc::new(move |_, _, _| {
        if let Some(started) = started.lock().unwrap().take() {
            let _ = started.send(());
            let _ = released
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10));
        }
        Ok(SAYS.to_string())
    });
    let engine = Arc::new(
        Engine::with(Powers {
            toolless: Some(writer),
            ..Default::default()
        })
        .await,
    );
    let (id, version) = engine.note("Raced", PASSAGE).await;
    let ids = fill(
        &engine,
        &id,
        "ask",
        2 * margin::MOST_OPEN,
        2 * margin::MOST_OPEN,
    );
    for pinned in &ids[1..] {
        margin::set_pinned(&engine.chats(), &id, pinned, true);
    }
    let running = Arc::clone(&engine);
    let (noted, seen) = (id.clone(), version.clone());
    let slow = tokio::spawn(async move { running.read(&noted, &seen, &[PASSAGE]).await });
    was_started.await.unwrap();
    let held = engine.running.state.margin.replying(&ids[0]);
    release.send(()).unwrap();
    let (status, read) = slow.await.unwrap();
    drop(held);
    assert_eq!(status, 200, "{read}");
    assert_eq!(
        (read["failed"].as_bool(), read["error"].as_str()),
        (Some(true), Some(margin::FULL))
    );
    assert_eq!(engine.get("/api/margin").await.1["today"], 0);
    assert_eq!(engine.running.state.margin.problem(), None);
    assert_eq!(
        engine
            .running
            .state
            .margin
            .unseen(&id, vec![PASSAGE.to_string()]),
        vec![PASSAGE.to_string()]
    );
    Arc::into_inner(engine).unwrap().stop().await;
}

#[tokio::test]
async fn a_read_along_with_nothing_free_to_resolve_says_so_and_costs_nothing() {
    let (engine, calls) = reading(SAYS).await;
    let (id, version) = engine.note("Pinned", PASSAGE).await;
    let ids = fill(
        &engine,
        &id,
        "ask",
        2 * margin::MOST_OPEN,
        2 * margin::MOST_OPEN,
    );
    for pinned in &ids[1..] {
        margin::set_pinned(&engine.chats(), &id, pinned, true);
    }
    let path = format!("/api/notes/{id}/margin/read");
    let body = |keep: Option<&str>| serde_json::json!({ "version": version, "blocks": [PASSAGE], "keep": keep });
    let held = engine.running.state.margin.replying(&ids[0]);
    let (status, read) = engine.post(&path, body(None)).await;
    assert_eq!(status, 200);
    assert_eq!(read["failed"], true);
    assert_eq!(read["error"], margin::FULL);
    drop(held);
    let (_, read) = engine.post(&path, body(Some(&ids[0]))).await;
    assert_eq!(
        (read["failed"].as_bool(), read["error"].as_str()),
        (Some(true), Some(margin::FULL))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(engine.get("/api/margin").await.1["today"], 0);
    assert_eq!(
        margin::open_on(&engine.chats(), &id).len(),
        2 * margin::MOST_OPEN
    );
    engine.stop().await;
}

fn seeded(engine: &Engine, id: &str, source: &str, said: bool, quote: &str) -> String {
    let mut messages = vec![margin::felix_says("Hm.")];
    if said {
        messages.push(margin::you_say("Why?"));
    }
    let thread = margin::new_thread(id, source, nowhere(), quote, messages);
    assert!(margin::put(&engine.chats(), &thread, margin::MOST_OPEN));
    thread.id
}

#[tokio::test]
async fn a_saved_read_along_note_quietly_resolves_the_least_recently_updated_untouched_ones() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Tidy", PASSAGE).await;
    let replied: Vec<String> = (0..3)
        .map(|n| seeded(&engine, &id, "read", true, &format!("replied {n}")))
        .collect();
    let untouched: Vec<String> = (0..4)
        .map(|n| seeded(&engine, &id, "read", false, &format!("untouched {n}")))
        .collect();
    let (status, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["resolved"], serde_json::json!(untouched[..3]));
    let open: Vec<String> = margin::open_on(&engine.chats(), &id)
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(open.len(), margin::TIDY_TO);
    for kept in replied.iter().chain([
        &untouched[3],
        &read["note"]["id"].as_str().unwrap().to_string(),
    ]) {
        assert!(open.contains(kept));
    }
    engine.stop().await;
}

#[tokio::test]
async fn the_tidy_never_takes_what_you_started_wrote_in_pinned_kept_or_are_answering() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Careful", PASSAGE).await;
    seeded(&engine, &id, "ask", false, "asked");
    seeded(&engine, &id, "chat", false, "chatted");
    seeded(&engine, &id, "through", false, "through");
    let pinned = seeded(&engine, &id, "read", false, "pinned");
    margin::set_pinned(&engine.chats(), &id, &pinned, true);
    let kept = seeded(&engine, &id, "read", false, "kept");
    let answering = seeded(&engine, &id, "read", false, "answering");
    let held = engine.running.state.margin.replying(&answering);
    let free = seeded(&engine, &id, "read", false, "free");
    let (status, read) = engine
        .post(
            &format!("/api/notes/{id}/margin/read"),
            serde_json::json!({ "version": version, "blocks": [PASSAGE], "keep": kept }),
        )
        .await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["resolved"], serde_json::json!([free]));
    assert_eq!(margin::open_on(&engine.chats(), &id).len(), 7);
    drop(held);
    engine.stop().await;
}

#[tokio::test]
async fn a_note_reopened_from_its_underline_is_the_last_to_be_tidied() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.note("Reopened", PASSAGE).await;
    for n in 0..3 {
        seeded(&engine, &id, "read", true, &format!("replied {n}"));
    }
    let untouched: Vec<String> = (0..4)
        .map(|n| seeded(&engine, &id, "read", false, &format!("untouched {n}")))
        .collect();
    let chats = engine.chats();
    margin::set_status(&chats, &id, &untouched[0], "resolved", margin::MOST_OPEN);
    margin::set_status(&chats, &id, &untouched[0], "open", margin::MOST_OPEN);
    let (_, read) = engine.read(&id, &version, &[PASSAGE]).await;
    assert_eq!(read["resolved"], serde_json::json!(untouched[1..]));
    assert_eq!(
        margin::one(&chats, &id, &untouched[0]).unwrap().status,
        "open"
    );
    engine.stop().await;
}

#[tokio::test]
async fn read_through_notes_are_stored_as_through_and_never_tidied() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.plain(PASSAGE).await;
    let (_, reply) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    assert_eq!(reply["notes"][0]["source"], "through");
    assert!(reply.get("resolved").is_none());
    engine.stop().await;
}

#[tokio::test]
async fn the_margin_lists_resolved_passages_without_their_messages_and_never_dismissed_ones() {
    let engine = Engine::with(Powers::default()).await;
    let (id, _) = engine.note("Closed", PASSAGE).await;
    let open = seeded(&engine, &id, "read", false, "open one");
    let resolved = seeded(&engine, &id, "read", true, "resolved one");
    let dismissed = seeded(&engine, &id, "read", false, "dismissed one");
    let chats = engine.chats();
    margin::set_status(&chats, &id, &resolved, "resolved", margin::MOST_OPEN);
    margin::set_status(&chats, &id, &dismissed, "dismissed", margin::MOST_OPEN);
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(view["threads"].as_array().unwrap().len(), 1);
    assert_eq!(view["threads"][0]["id"], open);
    assert_eq!(
        view["resolved"],
        serde_json::json!([{ "id": resolved, "quote": "resolved one", "prefix": "", "suffix": "" }])
    );
    let path = format!("/api/notes/{id}/margin/threads/{resolved}");
    let (status, reopened) = engine
        .post(&path, serde_json::json!({ "status": "open" }))
        .await;
    assert_eq!(
        (status, reopened["messages"].as_array().map(Vec::len)),
        (200, Some(2))
    );
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert_eq!(view["resolved"], serde_json::json!([]));
    engine.stop().await;
}

#[tokio::test]
async fn forgetting_a_thread_deletes_it_unless_you_replied() {
    let (engine, _) = reading(SAYS).await;
    let (id, version) = engine.plain(PASSAGE).await;
    let (_, reply) = engine.analyze(&id, &version, &[PASSAGE], None).await;
    let thread = reply["notes"][0]["id"].as_str().unwrap().to_string();
    let path = format!("/api/notes/{id}/margin/threads/{thread}");
    let (other, _) = engine.plain(PASSAGE).await;
    let wrong = format!("/api/notes/{other}/margin/threads/{thread}");
    assert_eq!(
        engine
            .send_as(&engine.cookie, "DELETE", &wrong, None)
            .await
            .0,
        404
    );
    assert_eq!(
        engine
            .send_as(&engine.cookie, "DELETE", &path, None)
            .await
            .0,
        204
    );
    let (_, view) = engine.get(&format!("/api/notes/{id}/margin")).await;
    assert!(view["threads"].as_array().unwrap().is_empty());
    assert_eq!(
        engine
            .send_as(&engine.cookie, "DELETE", &path, None)
            .await
            .0,
        404
    );

    let mut kept = margin::new_thread(
        &id,
        "ask",
        margin::Place {
            prefix: String::new(),
            suffix: String::new(),
        },
        "words",
        vec![margin::felix_says("Hm."), margin::you_say("Why?")],
    );
    kept.id = "kept-thread".into();
    assert!(margin::put(&engine.chats(), &kept, margin::MOST_OPEN));
    let kept_path = format!("/api/notes/{id}/margin/threads/kept-thread");
    let (status, why) = engine
        .send_as(&engine.cookie, "DELETE", &kept_path, None)
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("replied")));
    let held = engine.running.state.margin.replying("kept-thread");
    assert!(held.is_some());
    let (status, why) = engine
        .send_as(&engine.cookie, "DELETE", &kept_path, None)
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("replying")));
    let bad = format!("/api/notes/{id}/margin/threads/a%20b");
    assert_eq!(
        engine.send_as(&engine.cookie, "DELETE", &bad, None).await.0,
        400
    );
    engine.stop().await;
}

#[tokio::test]
async fn a_persona_shapes_the_system_prompt_and_reader_leaves_it_alone() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = Arc::clone(&seen);
    let writer: Writer = Arc::new(move |system, _, _| {
        kept.lock().unwrap().push(system.to_string());
        Ok("{}".to_string())
    });
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Styled", PASSAGE).await;
    let path = format!("/api/notes/{id}/margin");
    assert_eq!(engine.get(&path).await.1["persona"], "reader");
    assert_eq!(engine.read(&id, &version, &[PASSAGE]).await.0, 200);
    assert_eq!(seen.lock().unwrap()[0], margin::READ_SYSTEM);
    let (status, set) = engine
        .post(&path, serde_json::json!({ "persona": "critic" }))
        .await;
    assert_eq!(
        (status, &set),
        (200, &serde_json::json!({ "persona": "critic" }))
    );
    assert_eq!(engine.get(&path).await.1["persona"], "critic");
    assert_eq!(engine.get(&path).await.1["mode"], "along");
    assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 200);
    let system = seen.lock().unwrap()[1].clone();
    assert!(system.contains("demanding critic"));
    assert!(system.find("demanding critic") < system.find("Reply with JSON only"));
    let (status, set) = engine
        .post(&path, serde_json::json!({ "persona": "reader" }))
        .await;
    assert_eq!(
        (status, &set),
        (200, &serde_json::json!({ "persona": "reader" }))
    );
    assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 200);
    assert_eq!(seen.lock().unwrap()[2], margin::THROUGH_SYSTEM);
    assert_eq!(engine.post(&path, serde_json::json!({})).await.0, 400);
    assert_eq!(
        engine
            .post(&path, serde_json::json!({ "mode": "bogus" }))
            .await
            .0,
        400
    );
    assert_eq!(
        engine
            .post(&path, serde_json::json!({ "persona": "nobody" }))
            .await
            .0,
        400
    );
    engine.stop().await;
}

#[tokio::test]
async fn a_note_keeps_its_own_instructions_and_every_margin_prompt_carries_them() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = Arc::clone(&seen);
    let writer: Writer = Arc::new(move |system, _, _| {
        kept.lock().unwrap().push(system.to_string());
        Ok("{}".to_string())
    });
    let heard = Arc::clone(&seen);
    let streamer: crate::chat::Streamer = Arc::new(move |system, _, _, _, _, _| {
        heard.lock().unwrap().push(system.to_string());
        Ok(crate::chat::Reply::from("Sourced.".to_string()))
    });
    let engine = Engine::with(Powers {
        toolless: Some(writer),
        toolless_chat: Some(streamer),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Told", PASSAGE).await;
    let path = format!("/api/notes/{id}/margin");
    assert_eq!(engine.get(&path).await.1["instructions"], "");
    let (status, set) = engine
        .post(
            &path,
            serde_json::json!({ "instructions": "  Check every claim has a source.  " }),
        )
        .await;
    assert_eq!(
        (status, &set),
        (
            200,
            &serde_json::json!({ "instructions": "Check every claim has a source." })
        )
    );
    assert_eq!(
        engine.get(&path).await.1["instructions"],
        "Check every claim has a source."
    );
    assert_eq!(engine.read(&id, &version, &[PASSAGE]).await.0, 200);
    assert_eq!(engine.analyze(&id, &version, &[PASSAGE], None).await.0, 200);
    let (status, _) = engine
        .post(
            &format!("{path}/threads"),
            serde_json::json!({ "version": version, "quote": PASSAGE, "question": "Why?" }),
        )
        .await;
    assert_eq!(status, 200);
    let systems = seen.lock().unwrap().clone();
    assert_eq!(systems.len(), 3);
    for system in &systems {
        assert!(system.contains("Instruction: Check every claim has a source."));
    }
    assert_eq!(
        engine
            .post(&path, serde_json::json!({ "instructions": "a\u{7}b" }))
            .await
            .0,
        400
    );
    let (status, set) = engine
        .post(&path, serde_json::json!({ "instructions": " " }))
        .await;
    assert_eq!((status, set["instructions"].as_str()), (200, Some("")));
    assert_eq!(engine.get(&path).await.1["instructions"], "");
    engine.stop().await;
}

#[tokio::test]
async fn asks_and_replies_take_the_note_persona_and_only_a_real_question_is_told_to_answer_first() {
    let systems = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = Arc::clone(&systems);
    let streamer: crate::chat::Streamer = Arc::new(move |system, _, _, _, _, _| {
        kept.lock().unwrap().push(system.to_string());
        Ok(crate::chat::Reply::from("Nice work.".to_string()))
    });
    let engine = Engine::with(Powers {
        toolless_chat: Some(streamer),
        ..Default::default()
    })
    .await;
    let (id, version) = engine.note("Styled asks", PASSAGE).await;
    let margin_path = format!("/api/notes/{id}/margin");
    assert_eq!(
        engine
            .post(&margin_path, serde_json::json!({ "persona": "encourager" }))
            .await
            .0,
        200
    );
    let (status, asked) = engine
        .post(
            &format!("{margin_path}/threads"),
            serde_json::json!({ "version": version, "quote": PASSAGE, "question": "" }),
        )
        .await;
    assert_eq!(status, 200);
    let thread = asked["id"].as_str().unwrap().to_string();
    let reply = format!("{margin_path}/threads/{thread}/reply");
    assert_eq!(
        engine
            .post(&reply, serde_json::json!({ "text": "Why?" }))
            .await
            .0,
        200
    );
    let systems = systems.lock().unwrap().clone();
    assert_eq!(systems.len(), 2);
    assert!(systems.iter().all(|s| s.contains("encouraging mentor")));
    assert!(!systems[0].contains("answer it directly first"));
    assert!(systems[1].contains("answer it directly first"));
    engine.stop().await;
}

#[tokio::test]
async fn custom_personas_are_listed_and_the_default_follows_the_list() {
    let (engine, _) = reading("{}").await;
    let (_, view) = engine.get("/api/margin").await;
    assert_eq!(view["default_persona"], "reader");
    assert_eq!(view["most_personas"], margin::MOST_PERSONAS);
    assert_eq!(view["personas"].as_array().unwrap().len(), 3);
    assert_eq!(view["personas"][0]["details"], margin::READER_DETAILS);
    assert_eq!(view["personas"][0]["custom"], false);
    let (status, view) = engine
        .post(
            "/api/margin",
            serde_json::json!({ "personas": [{ "name": " Chemist ", "icon": "flask", "says": "Check the chemistry." }] }),
        )
        .await;
    assert_eq!(status, 200);
    let mine = &view["personas"][3];
    assert_eq!(
        (mine["name"].as_str(), mine["custom"].as_bool()),
        (Some("Chemist"), Some(true))
    );
    assert_eq!(mine["details"], "Check the chemistry.");
    assert_eq!(mine["line"], "");
    let id = mine["id"].as_str().unwrap().to_string();
    let (status, view) = engine
        .post("/api/margin", serde_json::json!({ "default_persona": id }))
        .await;
    assert_eq!(
        (status, &view["default_persona"]),
        (200, &serde_json::json!(id))
    );
    let (_, made) = engine
        .post(
            "/api/notes",
            serde_json::json!({ "title": "N", "body": "" }),
        )
        .await;
    let note = made["id"].as_str().unwrap();
    assert_eq!(
        engine.get(&format!("/api/notes/{note}/margin")).await.1["persona"],
        serde_json::json!(id)
    );
    for bad in [
        serde_json::json!({ "default_persona": "nobody" }),
        serde_json::json!({ "personas": [{ "name": "", "icon": "flask", "says": "x" }] }),
    ] {
        assert_eq!(engine.post("/api/margin", bad).await.0, 400);
    }
    let (status, view) = engine
        .post("/api/margin", serde_json::json!({ "personas": [] }))
        .await;
    assert_eq!(
        (status, &view["default_persona"]),
        (200, &serde_json::json!("reader"))
    );
    assert_eq!(
        engine.get(&format!("/api/notes/{note}/margin")).await.1["persona"],
        "reader"
    );
    engine.stop().await;
}

#[tokio::test]
async fn a_thread_is_claimed_before_it_is_read_for_a_reply_or_forget() {
    let (engine, _) = reading(SAYS).await;
    let (id, _) = engine.plain(PASSAGE).await;
    let held = engine.running.state.margin.replying("claimed-thread");
    assert!(held.is_some());
    let path = format!("/api/notes/{id}/margin/threads/claimed-thread");
    let (status, why) = engine.send_as(&engine.cookie, "DELETE", &path, None).await;
    assert_eq!((status, why["why"].as_str()), (409, Some("replying")));
    let (status, why) = engine
        .post(
            &format!("{path}/reply"),
            serde_json::json!({ "text": "why?" }),
        )
        .await;
    assert_eq!((status, why["why"].as_str()), (409, Some("busy")));
    drop(held);
    let (status, _) = engine.send_as(&engine.cookie, "DELETE", &path, None).await;
    assert_eq!(status, 404);
    engine.stop().await;
}

#[test]
fn a_changed_wording_is_placed_between_its_own_note_s_context() {
    let thread = |was: &str, quote: &str| {
        margin::new_thread(
            "n",
            "read",
            margin::placed(was, quote, &[]).unwrap(),
            quote,
            Vec::new(),
        )
    };
    let short = thread(
        "Passwords use bcrypt. Hash the password with care. then login just checks if it matches.",
        "Hash the password with care",
    );
    let body = "Passwords use bcrypt. it. then login just checks if it matches.";
    assert_eq!(margin::by_context(body, &short, "it"), Some(22));
    assert_eq!(margin::by_context(body, &short, " "), None);
    let twice = thread(
        "We use it. Logins are slow.\n\nThe cache helps. We rely on it.",
        "We rely on it.",
    );
    let body = "We use it. Logins are slow.\n\nThe cache helps. We use it.";
    assert_eq!(
        margin::by_context(body, &twice, "We use it."),
        Some(body.len() - "We use it.".len())
    );
    assert_eq!(
        margin::by_context(
            "We use it. Logins are slow.\n\nThe cache helps. We rely on it.",
            &twice,
            "We rely on it."
        ),
        None
    );
    let quote = "Hash the password with care";
    let long = |n: usize| "x".repeat(n);
    for (n, placed) in [
        (2 * quote.len() + 120, true),
        (2 * quote.len() + 121, false),
    ] {
        let body = format!(
            "Passwords use bcrypt. {}. then login just checks if it matches.",
            long(n)
        );
        assert_eq!(
            margin::by_context(&body, &short, &long(n)).is_some(),
            placed,
            "{n}"
        );
    }
    let astral = thread(
        "Notes 🎉 on passwords: hash them first. Then store the digest.",
        "hash them first.",
    );
    let body = "Notes 🎉 on passwords: salt and hash them. Then store the digest.";
    assert_eq!(
        margin::by_context(body, &astral, "salt and hash them."),
        body.find("salt")
    );
}
