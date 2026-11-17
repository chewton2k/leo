use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;

use crate::{start_in, Places, Powers, StartOptions};

fn places() -> (Places, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let places = Places {
        store: leo_core::store::Store::load_from(&dir.path().join("notes")).unwrap(),
        sessions: dir.path().join("sessions.json"),
    };
    (places, dir)
}

#[tokio::test]
async fn showing_the_window_reconciles_external_edits_before_the_watcher_delivers() {
    let (places, _dir) = places();
    let notes = places.store.notes_dir.clone();
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    assert!(running.set_visible(false));
    assert!(!running.set_visible(false));
    assert!(!running
        .state
        .visible
        .load(std::sync::atomic::Ordering::Acquire));
    running
        .state
        .with_store(|store| Ok(store.notes.len()))
        .await
        .unwrap();
    std::fs::write(notes.join("External.md"), "Outside edit").unwrap();
    let covered =
        |state: &crate::AppState| state.covered.load(std::sync::atomic::Ordering::Acquire);
    for said in [r#"{"visible":false,"at":5}"#, r#"{"visible":true,"at":4}"#] {
        crate::routes::activity::page_visible(
            axum::extract::State(running.state.clone()),
            axum::Json(serde_json::from_str(said).unwrap()),
        )
        .await;
    }
    assert!(
        covered(&running.state),
        "an older message that arrives late changes nothing"
    );
    assert!(running.set_visible(true));
    assert!(!covered(&running.state));
    let found = running
        .state
        .with_store(|store| Ok(store.notes.iter().any(|note| note.body == "Outside edit")))
        .await
        .unwrap();
    assert!(found);
    assert!(!running
        .state
        .refresh_requested
        .load(std::sync::atomic::Ordering::Acquire));
    running.stop().await;
}

#[tokio::test]
async fn start_returns_an_error_when_the_database_cannot_open() {
    let (places, dir) = places();
    std::fs::create_dir(dir.path().join(crate::db::FILE)).unwrap();
    let failed = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await;
    let said = format!("{:#}", failed.err().unwrap());
    assert!(said.contains(crate::db::FILE), "{said}");
}

#[tokio::test]
async fn start_binds_loopback_only() {
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", running.port()))
            .await
            .is_ok()
    );
    assert!(running.sign_in_url().starts_with("http://127.0.0.1:"));
    assert!(running.sign_in_url().ends_with("/?token=t"));
    running.stop().await;
}

#[tokio::test]
async fn start_takes_the_next_port_when_one_is_busy() {
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = held.local_addr().unwrap().port();
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: busy,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    assert_ne!(running.port(), busy);
    running.stop().await;
}

#[tokio::test]
async fn stop_returns_within_the_drain_deadline_while_a_request_is_still_arriving() {
    use tokio::io::AsyncReadExt;
    let (places, _dir) = places();
    let mut running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    running.grace = Duration::from_millis(300);
    let port = running.port();
    let mut signing_in = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    signing_in
        .write_all(
            format!(
                "GET /?token=t HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut reply = String::new();
    signing_in.read_to_string(&mut reply).await.unwrap();
    let cookie = reply
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: "))
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_string();
    let mut slow = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    slow.write_all(
        format!("POST /api/notes HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: 100000\r\n\r\n{{\"title\": \"").as_bytes(),
    )
    .await
    .unwrap();
    let mut said = [0u8; 64];
    let heard = tokio::time::timeout(Duration::from_secs(10), slow.read(&mut said))
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&said[..heard]).starts_with("HTTP/1.1 100 Continue"),
        "the engine never began reading the request"
    );
    let running_grace = running.grace;
    let began = Instant::now();
    running.stop().await;
    let took = began.elapsed();
    assert!(took >= running_grace, "nothing was in flight ({took:?})");
    assert!(
        took < running_grace + Duration::from_secs(2),
        "quitting hung ({took:?})"
    );
}

#[tokio::test]
async fn a_program_listening_on_every_address_keeps_its_port() {
    let held = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let busy = held.local_addr().unwrap().port();
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: busy,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    assert_ne!(running.port(), busy);
    running.stop().await;
}

#[tokio::test]
async fn stopping_frees_the_port() {
    let (places, _dir) = places();
    let running = start_in(
        places,
        StartOptions {
            port: 0,
            token: "t".into(),
        },
        Powers::default(),
    )
    .await
    .unwrap();
    let port = running.port();
    running.stop().await;
    assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
}

#[test]
fn fresh_tokens_differ_and_look_like_saved_ones() {
    let a = crate::fresh_token();
    let b = crate::fresh_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn store_now_clears_a_forced_check() {
    let (state, _dir, _) = super::state_with(&[]);
    state
        .refresh_requested
        .store(true, std::sync::atomic::Ordering::Release);
    crate::store_now(&state, |_| Ok(())).unwrap();
    assert!(!state
        .refresh_requested
        .load(std::sync::atomic::Ordering::Acquire));
}
