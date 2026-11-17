use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> String {
        self.head
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
            .unwrap_or_default()
    }

    fn shape(&self) -> String {
        let text = String::from_utf8_lossy(&self.body);
        let kind = if self.body.is_empty() {
            "empty".to_string()
        } else {
            match serde_json::from_slice::<serde_json::Value>(&self.body) {
                Ok(serde_json::Value::Object(map)) => {
                    format!(
                        "json{{{}}}",
                        map.keys().cloned().collect::<Vec<_>>().join(",")
                    )
                }
                Ok(_) => "json(other)".to_string(),
                Err(_) => "text".to_string(),
            }
        };
        let shown: String = text.chars().take(140).collect();
        format!(
            "status={} type={:?} shape={} body={:?}",
            self.status,
            self.header("content-type"),
            kind,
            shown
        )
    }

    fn worded(&self) -> bool {
        serde_json::from_slice::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .is_some_and(|e| !e.is_empty())
    }
}

async fn http(
    port: u16,
    method: &str,
    path: &str,
    cookie: &str,
    body: Vec<u8>,
    extra: &str,
) -> Reply {
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let (mut read, mut write) = stream.into_split();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let writer = tokio::spawn(async move {
        let _ = write.write_all(head.as_bytes()).await;
        let _ = write.write_all(&body).await;
        write
    });
    let mut reply = Vec::new();
    let _ = read.read_to_end(&mut reply).await;
    let _ = writer.await;
    let split = reply
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(reply.len());
    let head = String::from_utf8_lossy(&reply[..split]).to_string();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = reply.get(split + 4..).unwrap_or_default().to_vec();
    Reply { status, head, body }
}

async fn serve(store: Store, sessions: std::path::PathBuf) -> Running {
    start_in(
        Places { store, sessions },
        StartOptions {
            port: 0,
            token: TOKEN.into(),
        },
        Powers::default(),
    )
    .await
    .unwrap()
}

async fn sign_in(port: u16) -> String {
    let reply = http(port, "GET", &format!("/?token={TOKEN}"), "", vec![], "").await;
    assert_eq!(reply.status, 303, "{}", reply.head);
    let cookie = reply.header("set-cookie");
    cookie.split(';').next().unwrap().to_string()
}

fn json(value: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}

fn seeded_store(dir: &tempfile::TempDir) -> (Store, String, String) {
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    let a = store
        .create_note(
            "Alpha".to_string(),
            "First line\n\nPlain line".to_string(),
            vec![],
            "",
        )
        .unwrap()
        .id
        .clone();
    let b = store
        .create_note("Beta".to_string(), "Beta body".to_string(), vec![], "")
        .unwrap()
        .id
        .clone();
    store.save().unwrap();
    (Store::load_from(&dir.path().join("notes")).unwrap(), a, b)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_refusal_has_a_worded_error() {
    let dir = tempfile::tempdir().unwrap();
    let (store, a, b) = seeded_store(&dir);
    let running = serve(store, dir.path().join("s.json")).await;
    let port = running.port();
    let c = sign_in(port).await;
    let missing = "00000000-0000-4000-8000-000000000000";
    let note = http(port, "GET", &format!("/api/notes/{a}"), &c, vec![], "").await;
    let old: serde_json::Value = serde_json::from_slice(&note.body).unwrap();
    let old_version = old["version"].as_str().unwrap().to_string();
    let edit = http(
        port,
        "PATCH",
        &format!("/api/notes/{a}"),
        &c,
        json(serde_json::json!({"body": "First line\n\nPlain line\n\nmore"})),
        "",
    )
    .await;
    assert_eq!(edit.status, 200, "{}", edit.shape());
    let mut all: Vec<(&str, &str, Reply)> = Vec::new();
    all.push((
        "missing",
        "GET /api/notes/{missing}",
        http(
            port,
            "GET",
            &format!("/api/notes/{missing}"),
            &c,
            vec![],
            "",
        )
        .await,
    ));
    all.push((
        "missing",
        "POST /api/notes/{missing}/insert",
        http(
            port,
            "POST",
            &format!("/api/notes/{missing}/insert"),
            &c,
            json(serde_json::json!({"text": "x", "base": "v"})),
            "",
        )
        .await,
    ));
    all.push((
        "missing",
        "POST /api/tasks/promote",
        http(
            port,
            "POST",
            "/api/tasks/promote",
            &c,
            json(serde_json::json!({"note": missing, "line": 0})),
            "",
        )
        .await,
    ));
    all.push((
        "missing",
        "DELETE /api/tasks/{missing}",
        http(
            port,
            "DELETE",
            "/api/tasks/t0123456789abcdef",
            &c,
            vec![],
            "",
        )
        .await,
    ));
    all.push(("missing", "POST /api/felix/act delete_task", http(port, "POST", "/api/felix/act", &c, json(serde_json::json!({"kind": "delete_task", "summary": "x", "id": "t0123456789abcdef"})), "").await));
    all.push((
        "stale",
        "PATCH /api/notes/{id} base",
        http(
            port,
            "PATCH",
            &format!("/api/notes/{a}"),
            &c,
            json(serde_json::json!({"body": "other", "base": old_version})),
            "",
        )
        .await,
    ));
    all.push((
        "stale",
        "POST /api/notes/{id}/insert base",
        http(
            port,
            "POST",
            &format!("/api/notes/{a}/insert"),
            &c,
            json(serde_json::json!({"text": "x", "base": old_version})),
            "",
        )
        .await,
    ));
    all.push((
        "stale",
        "POST /api/notes/{id}/margin/read version",
        http(
            port,
            "POST",
            &format!("/api/notes/{a}/margin/read"),
            &c,
            json(serde_json::json!({"version": old_version, "blocks": ["First line"]})),
            "",
        )
        .await,
    ));
    let big = "a".repeat(3 * 1024 * 1024);
    all.push((
        "oversize",
        "POST /api/notes 3 MB",
        http(
            port,
            "POST",
            "/api/notes",
            &c,
            json(serde_json::json!({"title": "Big", "body": big})),
            "",
        )
        .await,
    ));
    all.push((
        "oversize",
        "PATCH /api/notes/{id} 3 MB",
        http(
            port,
            "PATCH",
            &format!("/api/notes/{b}"),
            &c,
            json(serde_json::json!({"body": big})),
            "",
        )
        .await,
    ));
    all.push((
        "oversize",
        "POST /api/notes/{id}/insert 210k chars",
        http(
            port,
            "POST",
            &format!("/api/notes/{b}/insert"),
            &c,
            json(serde_json::json!({"text": "a".repeat(210_000), "base": "v"})),
            "",
        )
        .await,
    ));
    all.push((
        "oversize",
        "POST /api/notes/{id}/margin/threads 50 KB",
        http(
            port,
            "POST",
            &format!("/api/notes/{b}/margin/threads"),
            &c,
            json(serde_json::json!({"quote": "a".repeat(50_000)})),
            "",
        )
        .await,
    ));
    all.push((
        "bad json",
        "POST /api/notes '{'",
        http(port, "POST", "/api/notes", &c, b"{".to_vec(), "").await,
    ));
    all.push((
        "not a checkbox",
        "POST /api/tasks/promote line 2",
        http(
            port,
            "POST",
            "/api/tasks/promote",
            &c,
            json(serde_json::json!({"note": a, "line": 2})),
            "",
        )
        .await,
    ));
    running.stop().await;
    let bare: Vec<String> = all
        .iter()
        .filter(|(_, _, r)| !r.worded())
        .map(|(kind, route, r)| format!("{kind} {route}: {}", r.shape()))
        .collect();
    assert!(bare.is_empty(), "refusals with no worded error: {bare:#?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_refused_folder_name_gets_the_folder_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let (store, a, _) = seeded_store(&dir);
    let running = serve(store, dir.path().join("s.json")).await;
    let port = running.port();
    let c = sign_in(port).await;
    let folder = "Use a folder name inside your notes, without . or ..";
    let said = |reply: &Reply| -> String {
        serde_json::from_slice::<serde_json::Value>(&reply.body).unwrap()["error"]
            .as_str()
            .unwrap()
            .to_string()
    };
    for (path, body) in [
        ("/api/dirs", serde_json::json!({ "path": "../outside" })),
        (
            "/api/dirs/move",
            serde_json::json!({ "from": "../a", "into": "" }),
        ),
        (
            "/api/dirs/rename",
            serde_json::json!({ "from": "../a", "name": "b" }),
        ),
    ] {
        let reply = http(port, "POST", path, &c, json(body), "").await;
        assert_eq!(
            (reply.status, said(&reply).as_str()),
            (400, folder),
            "{path}"
        );
    }
    let moved = http(
        port,
        "POST",
        &format!("/api/notes/{a}/move"),
        &c,
        json(serde_json::json!({ "directory": "../x" })),
        "",
    )
    .await;
    assert_eq!(moved.status, 400, "{}", moved.shape());
    assert_ne!(said(&moved), folder);
    running.stop().await;
}
