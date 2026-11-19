use super::catalog::{catalog, Catalog, DIARIZER};
use super::library::Library;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let mut data = catalog().clone();
    data.models.retain(|m| m.id == "parakeet-tdt-0.6b-v3");
    let file = data.models[0].files.get_mut("Q8_0").unwrap();
    file.size = 5;
    file.sha256 = crate::providers::hex(&<sha2::Sha256 as sha2::Digest>::digest(b"model"));
    let library = Library::with_catalog(
        dir.path().join("models"),
        dir.path().join("config"),
        Arc::new(data),
    );
    (dir, library)
}

fn model_server(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let _ = stream.write_all(body);
    });
    (url, thread)
}

#[test]
fn catalog_has_the_29_curated_models_with_pinned_integrity() {
    let c = catalog();
    assert_eq!(c.curated().count(), 29);
    for m in c.curated() {
        assert!(!m.languages.is_empty());
        assert!(m.file().size > 0);
        assert_eq!(m.file().sha256.len(), 64);
        assert!(m.url().starts_with("https://huggingface.co/"));
        assert_eq!(m.revision.len(), 40);
    }
    for id in ["../x", "/x", "a/b", "a\\b", "", "unknown"] {
        assert!(c.get(id).is_err());
    }
    let roundtrip: Catalog = serde_json::from_str(include_str!("catalog.json")).unwrap();
    assert_eq!(roundtrip.curated().count(), 29);
}

#[test]
fn activation_requires_a_verified_model_and_supported_language() {
    let (_dir, l) = fixture();
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "en").is_err());
    let (url, t) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    t.join().unwrap();
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "invalid").is_err());
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    assert_eq!(l.selected().unwrap().language, "en");
    let view = l.view().unwrap();
    assert_eq!(view["disk_bytes"], 5);
    assert_eq!(view["models"][0]["downloaded"], true);
}

#[test]
fn detect_language_is_refused_and_migrated_for_models_that_cannot_detect() {
    let c = catalog();
    assert!(!c.get("canary-180m-flash").unwrap().language("auto"));
    assert!(c.get("canary-180m-flash").unwrap().language("de"));
    assert!(c.get("parakeet-tdt-0.6b-v3").unwrap().language("auto"));
    assert!(!c.get("canary-180m-flash").unwrap().detects_language());
    let dir = tempfile::tempdir().unwrap();
    let l = Library::with_catalog(
        dir.path().join("models"),
        dir.path().join("config"),
        Arc::new(c.clone()),
    );
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    let save = |id: &str| {
        std::fs::write(
            dir.path().join("config/speech-model.json"),
            format!(r#"{{"id":"{id}","language":"auto"}}"#),
        )
        .unwrap()
    };
    save("canary-180m-flash");
    let migrated = l.selection().unwrap();
    assert_eq!(migrated.id, "canary-180m-flash");
    assert!(c
        .get("canary-180m-flash")
        .unwrap()
        .language(&migrated.language));
    assert_ne!(migrated.language, "auto");
    save("parakeet-tdt-0.6b-v3");
    assert_eq!(l.selection().unwrap().language, "auto");
}

#[test]
fn a_model_that_cannot_detect_prefers_the_system_language_then_english_then_its_first() {
    use crate::speech::catalog::choose_language;
    let listed = |all: &[&str]| all.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    let three = listed(&["bg", "de", "en"]);
    assert_eq!(choose_language(&three, Some("de")).as_deref(), Some("de"));
    assert_eq!(choose_language(&three, Some("ja")).as_deref(), Some("en"));
    assert_eq!(choose_language(&three, None).as_deref(), Some("en"));
    assert_eq!(
        choose_language(&listed(&["bg", "de"]), Some("ja")).as_deref(),
        Some("bg")
    );
    assert_eq!(choose_language(&[], Some("en")), None);
}

#[test]
fn choosing_detect_language_for_a_model_that_cannot_detect_picks_a_listed_language() {
    let dir = tempfile::tempdir().unwrap();
    let mut data = catalog().clone();
    data.models.retain(|m| m.id == "canary-180m-flash");
    let quant = data.models[0].quant.clone();
    let file = data.models[0].files.get_mut(&quant).unwrap();
    file.size = 5;
    file.sha256 = crate::providers::hex(&<sha2::Sha256 as sha2::Digest>::digest(b"model"));
    let l = Library::with_catalog(
        dir.path().join("models"),
        dir.path().join("config"),
        Arc::new(data),
    );
    let (url, t) = model_server(b"model");
    l.fetch(
        "canary-180m-flash",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    t.join().unwrap();
    l.use_model("canary-180m-flash", "auto").unwrap();
    let language = l.selected().unwrap().language;
    assert!(["en", "de", "es", "fr"].contains(&language.as_str()));
}

#[test]
fn the_default_dictation_model_is_fast_enough_for_live_text() {
    assert!(catalog().live("parakeet-unified-en-0.6b"));
}

#[test]
fn a_verified_model_is_not_hashed_again_after_a_restart() {
    let (dir, l) = fixture();
    let (url, t) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    t.join().unwrap();
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    let path = l.selected().unwrap().path;
    let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();

    let restart = || {
        let mut data = catalog().clone();
        data.models.retain(|m| m.id == "parakeet-tdt-0.6b-v3");
        let file = data.models[0].files.get_mut("Q8_0").unwrap();
        file.size = 5;
        file.sha256 = crate::providers::hex(&<sha2::Sha256 as sha2::Digest>::digest(b"model"));
        Library::with_catalog(
            dir.path().join("models"),
            dir.path().join("config"),
            Arc::new(data),
        )
    };

    let rewrite = |bytes: &[u8], modified| {
        std::fs::write(&path, bytes).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(modified).unwrap();
    };
    rewrite(b"wrong", stamp);
    assert!(restart().selected().is_ok());
    rewrite(b"wrong", stamp + std::time::Duration::from_secs(60));
    assert!(restart().selected().is_err());
}

#[test]
fn a_bad_download_never_replaces_a_good_model() {
    let (_dir, l) = fixture();
    for (bytes, good) in [
        (b"model".as_slice(), true),
        (b"wrong", false),
        (b"oversize", false),
        (b"tiny", false),
    ] {
        let (url, t) = model_server(bytes);
        assert_eq!(
            l.fetch(
                "parakeet-tdt-0.6b-v3",
                &url,
                &AtomicBool::new(false),
                &|_, _| {}
            )
            .is_ok(),
            good
        );
        t.join().unwrap();
        assert_eq!(
            std::fs::read(l.model_path("parakeet-tdt-0.6b-v3").unwrap()).unwrap(),
            b"model"
        );
    }
}

#[test]
fn cancellation_before_promotion_leaves_no_model_or_partial() {
    let (_dir, l) = fixture();
    let cancel = AtomicBool::new(false);
    let (url, t) = model_server(b"model");
    let result = l.fetch("parakeet-tdt-0.6b-v3", &url, &cancel, &|_, _| {
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    t.join().unwrap();
    assert!(result.is_err());
    assert!(!l.model_path("parakeet-tdt-0.6b-v3").unwrap().exists());
    assert_eq!(std::fs::read_dir(l.models_dir()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn model_roots_targets_and_selection_refuse_symbolic_links() {
    use std::os::unix::fs::symlink;
    let (dir, l) = fixture();
    std::fs::create_dir_all(l.models_dir()).unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "model").unwrap();
    symlink(&outside, l.model_path("parakeet-tdt-0.6b-v3").unwrap()).unwrap();
    assert!(l.selected().is_err());
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "en").is_err());
    assert!(l.delete("parakeet-tdt-0.6b-v3").is_err());
    let linked = dir.path().join("linked");
    symlink(l.models_dir(), &linked).unwrap();
    let linked_library = Library::new(linked, dir.path().join("c2"));
    assert!(linked_library.model_path("parakeet-tdt-0.6b-v3").is_err());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "model");
}

#[test]
fn protocol_rejects_oversized_and_nonfinite_pcm() {
    use super::protocol::{encode_message, read_message, Request};
    let request = Request::Hello {
        list_devices: false,
    };
    assert!(encode_message(&request, Some(&[f32::NAN])).is_err());
    let bytes = (u32::MAX).to_le_bytes();
    assert!(read_message::<Request>(&mut bytes.as_slice()).is_err());
}

#[test]
fn lecture_chunking_preserves_all_300_seconds_in_order() {
    let pcm: Vec<f32> = (0..300 * 16_000)
        .map(|n| (n % 1000) as f32 / 1000.0)
        .collect();
    let chunks = super::prepare(pcm.clone()).unwrap();
    assert!(chunks.len() >= 8);
    assert!(chunks.iter().all(|c| c.len() <= 40 * 16_000));
    assert_eq!(chunks.concat(), pcm);
}

#[cfg(unix)]
#[test]
fn models_and_settings_refuse_linked_ancestors_and_hard_linked_files() {
    use std::os::unix::fs::symlink;
    let (dir, l) = fixture();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = dir.path().join("link");
    symlink(&real, &link).unwrap();
    let linked = Library::new(link.join("speech"), dir.path().join("config"));
    assert!(linked.model_path("parakeet-tdt-0.6b-v3").is_err());
    std::fs::create_dir_all(l.models_dir()).unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "model").unwrap();
    std::fs::hard_link(&outside, l.model_path("parakeet-tdt-0.6b-v3").unwrap()).unwrap();
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "en").is_err());
}

#[test]
fn a_download_cancel_interrupts_a_server_that_stops_sending() {
    let (_dir, l) = fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let (ready, started) = std::sync::mpsc::channel();
    let (done, finish) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nm")
            .unwrap();
        ready.send(()).unwrap();
        let _ = finish.recv_timeout(std::time::Duration::from_secs(3));
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let worker =
        std::thread::spawn(move || l.fetch("parakeet-tdt-0.6b-v3", &url, &flag, &|_, _| {}));
    started
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let now = std::time::Instant::now();
    cancel.store(true, std::sync::atomic::Ordering::Release);
    assert!(worker.join().unwrap().is_err());
    let elapsed = now.elapsed();
    let _ = done.send(());
    server.join().unwrap();
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "cancellation took {elapsed:?}"
    );
}

#[test]
fn a_damaged_model_in_use_can_be_deleted_and_downloaded_again() {
    let (_dir, l) = fixture();
    let (url, server) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    server.join().unwrap();
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    std::fs::write(l.model_path("parakeet-tdt-0.6b-v3").unwrap(), "bad").unwrap();
    assert!(!l.view().unwrap()["models"][0]["in_use"].as_bool().unwrap());
    l.delete("parakeet-tdt-0.6b-v3").unwrap();
    assert!(!l.model_path("parakeet-tdt-0.6b-v3").unwrap().exists());
}

#[test]
fn native_speech_and_rust_bindings_have_the_same_version() {
    let lock: toml::Value = toml::from_str(include_str!("../../../../Cargo.lock")).unwrap();
    let packages = lock["package"].as_array().unwrap();
    let version = |name: &str| {
        packages
            .iter()
            .find(|p| p["name"].as_str() == Some(name))
            .unwrap()["version"]
            .as_str()
            .unwrap()
    };
    assert_eq!(version("transcribe-cpp"), version("transcribe-cpp-sys"));
}

#[test]
fn deleting_the_only_downloaded_model_clears_the_choice() {
    let (_dir, l) = fixture();
    let (url, server) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    server.join().unwrap();
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    l.delete("parakeet-tdt-0.6b-v3").unwrap();
    assert!(l.selection().unwrap().id.is_empty());
    assert!(!l.view().unwrap()["ready"].as_bool().unwrap());
}

#[test]
fn downloaded_models_include_empty_and_uncatalogued_files() {
    let (dir, library) = fixture();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    std::fs::write(library.model_path("parakeet-tdt-0.6b-v3").unwrap(), b"").unwrap();
    std::fs::write(dir.path().join("models/older.gguf"), b"old model").unwrap();
    let view = library.view().unwrap();
    assert_eq!(view["models"][0]["downloaded"], true);
    assert_eq!(view["extra_files"][0]["name"], "older.gguf");
    assert_eq!(view["disk_bytes"], 9);
    assert!(library
        .change(&serde_json::json!({"action":"delete_file","id":"../older.gguf"}))
        .is_err());
    library
        .change(&serde_json::json!({"action":"delete_file","id":"older.gguf"}))
        .unwrap();
    assert!(!dir.path().join("models/older.gguf").exists());
}

#[test]
fn catalog_ratings_use_language_accuracy_and_the_reference_geometric_mean() {
    let mut c = catalog().clone();
    let id = "parakeet-tdt-0.6b-v3";
    c.results.insert(id.into(), serde_json::json!({"accuracy":{"en":{"error":4.0},"es":{"error":16.0}},"performance":{"cpu":10.0,"accelerated":20.0}}));
    assert!((c.accuracy_for(id, "auto").unwrap() - 92.0).abs() < 0.00001);
    assert_eq!(c.accuracy_for(id, "en"), Some(96.0));
    assert_eq!(c.accuracy_for(id, "es"), Some(84.0));
    assert_eq!(c.accuracy_for(id, "ja"), None);
    assert_eq!(c.accuracy_for("missing", "auto"), None);
    c.results.insert(
        "negative".into(),
        serde_json::json!({"accuracy":{"en":{"error":-1.0}}}),
    );
    assert_eq!(c.accuracy_for("negative", "en"), None);
}

#[test]
fn downloads_write_a_recognisable_partial_that_a_later_download_clears() {
    let (dir, l) = fixture();
    let models = dir.path().join("models");
    let (url, t) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {
            assert!(std::fs::read_dir(&models).unwrap().flatten().any(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with(".leo-download-")));
        },
    )
    .unwrap();
    t.join().unwrap();
    std::fs::write(models.join(".leo-download-stale"), b"part").unwrap();
    std::fs::File::options()
        .write(true)
        .open(models.join(".leo-download-stale"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    std::fs::write(models.join(".leo-download-running"), b"part").unwrap();
    std::fs::write(models.join("older.gguf"), b"old").unwrap();
    l.drop_partials();
    assert!(!models.join(".leo-download-stale").exists());
    assert!(models.join(".leo-download-running").exists());
    assert!(models.join("older.gguf").exists());
    assert!(l.model_path("parakeet-tdt-0.6b-v3").unwrap().exists());
}

#[test]
fn a_finished_download_is_not_hashed_again() {
    let (_dir, l) = fixture();
    let (url, t) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    t.join().unwrap();
    let path = l.model_path("parakeet-tdt-0.6b-v3").unwrap();
    let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, b"wrong").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
}

#[test]
fn a_choice_the_catalog_no_longer_offers_still_opens_speech_models() {
    let (dir, l) = fixture();
    let config = dir.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    for stored in [
        r#"{"id":"retired-model","language":"en"}"#,
        r#"{"id":"parakeet-tdt-0.6b-v3","language":"xx"}"#,
    ] {
        std::fs::write(config.join("speech-model.json"), stored).unwrap();
        assert_eq!(l.view().unwrap()["selected"]["id"], "");
        assert!(l
            .selected()
            .unwrap_err()
            .to_string()
            .contains("Choose and download"));
    }
}

#[test]
fn the_speaker_model_is_listed_apart_and_can_never_become_the_speech_model() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::with_catalog(
        dir.path().join("models"),
        dir.path().join("config"),
        Arc::new(catalog().clone()),
    );
    let speaker = catalog().known(super::catalog::DIARIZER).unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    std::fs::write(
        dir.path().join("models").join(&speaker.file().filename),
        b"partial",
    )
    .unwrap();
    let view = library.view().unwrap();
    assert_eq!(view["speakers"]["id"], super::catalog::DIARIZER);
    assert_eq!(view["speakers"]["size"], speaker.file().size);
    assert_eq!(view["speakers"]["verified"], false);
    assert!(view["extra_files"].as_array().unwrap().is_empty());
    assert!(library.use_model(super::catalog::DIARIZER, "en").is_err());
    assert!(catalog().get(super::catalog::DIARIZER).is_err());
    assert!(library.speaker_model().is_err());
    assert_eq!(catalog().curated().count(), 29);
}

fn speaker_fixture() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let mut data = catalog().clone();
    data.models.retain(|m| m.id == DIARIZER);
    let quant = data.models[0].quant.clone();
    let file = data.models[0].files.get_mut(&quant).unwrap();
    file.size = 5;
    file.sha256 = crate::providers::hex(&<sha2::Sha256 as sha2::Digest>::digest(b"model"));
    let library = Library::with_catalog(
        dir.path().join("models"),
        dir.path().join("config"),
        Arc::new(data),
    );
    (dir, library)
}

#[test]
fn a_ready_speaker_model_is_used_without_starting_a_download() {
    let (_dir, l) = speaker_fixture();
    let (url, t) = model_server(b"model");
    l.fetch(DIARIZER, &url, &AtomicBool::new(false), &|_, _| {})
        .unwrap();
    t.join().unwrap();
    let path = l
        .ensure_speaker_model(&|| panic!("started a download"), &|| false, &|_, _| {})
        .unwrap();
    assert_eq!(path, l.model_path(DIARIZER).unwrap());
}

#[test]
fn a_missing_speaker_model_is_downloaded_and_waited_for() {
    let (_dir, l) = speaker_fixture();
    let (url, t) = model_server(b"model");
    let reported = std::sync::atomic::AtomicUsize::new(0);
    let path = l
        .ensure_speaker_model(
            &|| l.download_from(DIARIZER, &url),
            &|| false,
            &|_, total| {
                assert_eq!(total, 5);
                reported.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap();
    t.join().unwrap();
    assert!(reported.load(Ordering::SeqCst) > 0);
    assert_eq!(path, l.model_path(DIARIZER).unwrap());
}

#[test]
fn a_failed_speaker_model_download_reports_the_download_error() {
    let (_dir, l) = speaker_fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.read(&mut [0; 4096]);
        let _ = stream.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    });
    let error = l
        .ensure_speaker_model(&|| l.download_from(DIARIZER, &url), &|| false, &|_, _| {})
        .unwrap_err();
    server.join().unwrap();
    assert_eq!(
        error.to_string(),
        "The model download failed. Check your connection and try again."
    );
}

#[test]
fn finishing_during_the_speaker_model_download_leaves_it_running() {
    let (_dir, l) = speaker_fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let error = l
        .ensure_speaker_model(&|| l.download_from(DIARIZER, &url), &|| true, &|_, _| {})
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Stopped while the speaker model was downloading"
    );
    assert!(l.downloading(DIARIZER));
    l.cancel(DIARIZER).unwrap();
    drop(listener);
}

#[test]
fn a_speaker_model_job_that_already_failed_reports_its_error() {
    let (_dir, l) = speaker_fixture();
    l.seed_job(DIARIZER, "failed", Some("The model download was damaged."));
    let error = l
        .ensure_speaker_model(&|| Ok(()), &|| false, &|_, _| {})
        .unwrap_err();
    assert_eq!(error.to_string(), "The model download was damaged.");
}

#[test]
fn a_speaker_model_download_started_elsewhere_meanwhile_is_waited_for() {
    let (_dir, l) = speaker_fixture();
    let (url, t) = model_server(b"model");
    let path = l
        .ensure_speaker_model(
            &|| {
                l.seed_job(DIARIZER, "downloading", None);
                let (l, url) = (l.clone(), url.clone());
                std::thread::spawn(move || {
                    l.fetch(DIARIZER, &url, &AtomicBool::new(false), &|_, _| {})
                        .unwrap();
                    l.seed_job(DIARIZER, "ready", None);
                });
                anyhow::bail!("This model is already downloading.")
            },
            &|| false,
            &|_, _| {},
        )
        .unwrap();
    t.join().unwrap();
    assert_eq!(path, l.model_path(DIARIZER).unwrap());
}

#[test]
fn a_cancelled_speaker_model_download_says_so() {
    let (_dir, l) = speaker_fixture();
    l.seed_job(DIARIZER, "cancelled", None);
    let error = l
        .ensure_speaker_model(&|| Ok(()), &|| false, &|_, _| {})
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "The speaker model download was cancelled"
    );
}

#[test]
fn a_speaker_model_download_refused_for_lack_of_a_slot_waits_and_retries() {
    let (_dir, l) = speaker_fixture();
    l.seed_job("first", "downloading", None);
    l.seed_job("second", "downloading", None);
    let (url, t) = model_server(b"model");
    let starts = std::sync::atomic::AtomicUsize::new(0);
    let path = l
        .ensure_speaker_model(
            &|| {
                if starts.fetch_add(1, Ordering::SeqCst) == 1 {
                    l.seed_job("first", "ready", None);
                }
                l.download_from(DIARIZER, &url)
            },
            &|| false,
            &|_, _| {},
        )
        .unwrap();
    t.join().unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    assert_eq!(path, l.model_path(DIARIZER).unwrap());
}

#[test]
fn a_speaker_model_download_waiting_for_a_slot_stops_when_asked() {
    let (_dir, l) = speaker_fixture();
    l.seed_job("first", "downloading", None);
    l.seed_job("second", "downloading", None);
    let error = l
        .ensure_speaker_model(
            &|| l.download_from(DIARIZER, "http://127.0.0.1:9/model"),
            &|| true,
            &|_, _| panic!("reported progress"),
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Stopped while the speaker model was downloading"
    );
}

#[test]
#[ignore = "downloads the 133 MiB speaker model from Hugging Face"]
fn the_real_speaker_model_downloads_from_hugging_face_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let l = Library::new(dir.path().join("models"), dir.path().join("config"));
    let seen = std::sync::Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    let path = l
        .ensure_speaker_model(&|| l.download(DIARIZER), &|| false, &|done, total| {
            seen.lock().unwrap().push((done, total))
        })
        .unwrap();
    let size = catalog().known(DIARIZER).unwrap().file().size;
    assert_eq!(std::fs::metadata(&path).unwrap().len(), size);
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.last(), Some(&(size, size)));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    println!(
        "downloaded {} MiB in {:.1}s with {} progress reports",
        size >> 20,
        started.elapsed().as_secs_f64(),
        seen.len()
    );
}

fn gguf_values_named(bytes: &[u8], wanted: &str) -> Option<Vec<(String, String)>> {
    fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Option<&'a [u8]> {
        let got = bytes.get(*at..at.checked_add(n)?)?;
        *at += n;
        Some(got)
    }
    fn number(bytes: &[u8], at: &mut usize, n: usize) -> Option<u64> {
        let mut buf = [0u8; 8];
        buf[..n].copy_from_slice(take(bytes, at, n)?);
        Some(u64::from_le_bytes(buf))
    }
    fn text(bytes: &[u8], at: &mut usize) -> Option<String> {
        let len = number(bytes, at, 8)? as usize;
        Some(String::from_utf8_lossy(take(bytes, at, len)?).into_owned())
    }
    fn value(bytes: &[u8], at: &mut usize, kind: u64) -> Option<String> {
        let width = |kind| match kind {
            0 | 1 | 7 => Some(1),
            2 | 3 => Some(2),
            4..=6 => Some(4),
            10..=12 => Some(8),
            _ => None,
        };
        match kind {
            8 => text(bytes, at),
            9 => {
                let inner = number(bytes, at, 4)?;
                let count = number(bytes, at, 8)?;
                for _ in 0..count {
                    value(bytes, at, inner)?;
                }
                Some("[array]".into())
            }
            _ => {
                let n = width(kind)?;
                Some(number(bytes, at, n)?.to_string())
            }
        }
    }
    let mut at = 0;
    if take(bytes, &mut at, 4)? != b"GGUF" {
        return None;
    }
    number(bytes, &mut at, 4)?;
    number(bytes, &mut at, 8)?;
    let pairs = number(bytes, &mut at, 8)?;
    let mut found = Vec::new();
    for _ in 0..pairs {
        let key = text(bytes, &mut at)?;
        let kind = number(bytes, &mut at, 4)?;
        let held = value(bytes, &mut at, kind)?;
        if key.contains(wanted) {
            found.push((key, held));
        }
    }
    Some(found)
}

#[test]
fn the_gguf_reader_finds_the_number_of_mel_bins() {
    let mut file = b"GGUF".to_vec();
    file.extend(3u32.to_le_bytes());
    file.extend(0u64.to_le_bytes());
    file.extend(3u64.to_le_bytes());
    let key = |file: &mut Vec<u8>, name: &str| {
        file.extend((name.len() as u64).to_le_bytes());
        file.extend(name.as_bytes());
    };
    key(&mut file, "general.name");
    file.extend(8u32.to_le_bytes());
    key(&mut file, "sortformer");
    key(&mut file, "tokens");
    file.extend(9u32.to_le_bytes());
    file.extend(8u32.to_le_bytes());
    file.extend(2u64.to_le_bytes());
    key(&mut file, "a");
    key(&mut file, "bc");
    key(&mut file, "sortformer.fe_num_mels");
    file.extend(4u32.to_le_bytes());
    file.extend(128u32.to_le_bytes());
    assert_eq!(
        gguf_values_named(&file, "fe_num_mels"),
        Some(vec![("sortformer.fe_num_mels".into(), "128".into())])
    );
    assert_eq!(gguf_values_named(b"nope", "x"), None);
}

#[cfg(unix)]
fn speech_worker_kilobytes() -> u64 {
    let Some(owned) = leo_core::children::owned() else {
        return 0;
    };
    owned
        .iter()
        .filter(|p| {
            p.args
                .split_whitespace()
                .any(|a| a == leo_core::children::WORKER_FLAG)
        })
        .filter_map(|p| {
            let out = leo_core::children::quiet_command("ps")
                .args(["-o", "rss=", "-p", &p.pid.to_string()])
                .output()
                .ok()?;
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .sum()
}

#[cfg(unix)]
#[test]
#[ignore = "needs LEO_HOME with a chosen speech model; downloads the speaker model and runs a two-hour pass"]
fn the_speaker_pass_stays_in_memory_bounds_at_two_hours() {
    use crate::session::wav;
    let selected = super::library::library().unwrap().selected().unwrap();
    println!("speech model {} {}", selected.id, selected.language);
    let speaker_model = super::library::library()
        .unwrap()
        .ensure_speaker_model(
            &|| super::library::library()?.download(DIARIZER),
            &|| false,
            &|_, _| {},
        )
        .unwrap();
    let mels = std::fs::read(&speaker_model)
        .ok()
        .and_then(|bytes| gguf_values_named(&bytes, "num_mels"));
    println!("speaker model mel bins: {mels:?}");
    let dir = tempfile::tempdir().unwrap();
    let part_secs = 300usize;
    let lead = crate::session::OVERLAP_SECS as usize;
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut parts = Vec::new();
    for index in 0..24u32 {
        let extra = if index == 0 { 0 } else { lead };
        let noise: Vec<i16> = (0..(part_secs + extra) * wav::RATE as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as i64 % 6001 - 3000) as i16
            })
            .collect();
        let path = dir.path().join(wav::done_name(index));
        wav::write(&path, &noise).unwrap();
        parts.push((path, (extra * 1000) as u64));
    }
    let engine = super::engine::engine();
    let before = speech_worker_kilobytes();
    let peak = Arc::new(std::sync::atomic::AtomicU64::new(before));
    let done = Arc::new(AtomicBool::new(false));
    let sampler = {
        let (peak, done) = (peak.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                peak.fetch_max(speech_worker_kilobytes(), Ordering::AcqRel);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
    };
    let started = std::time::Instant::now();
    let turns = engine.diarize(parts, &AtomicBool::new(false), &|_, _| {});
    done.store(true, Ordering::Release);
    sampler.join().unwrap();
    let peak = peak.load(Ordering::Acquire);
    println!(
        "speaker pass at two hours: peak speech worker {} MiB (before the pass {} MiB), {:.1}s, {} turns, result ok: {}",
        peak >> 10,
        before >> 10,
        started.elapsed().as_secs_f64(),
        turns.as_ref().map_or(0, |t| t.len()),
        turns.is_ok()
    );
    turns.unwrap();
    assert!(peak > 0, "the speech worker was never seen");
    assert!(peak < 1 << 20, "the speaker pass peaked over 1 GiB");
}

fn library_over(models: &std::path::Path, root: &std::path::Path) -> Library {
    let mut data = catalog().clone();
    data.models.retain(|m| m.id == "parakeet-tdt-0.6b-v3");
    Library::with_catalog(models.to_path_buf(), root.join("config"), Arc::new(data))
}

fn partial_aged(
    models: &std::path::Path,
    name: &str,
    age: std::time::Duration,
) -> std::path::PathBuf {
    std::fs::create_dir_all(models).unwrap();
    let partial = models.join(name);
    std::fs::write(&partial, vec![0u8; 4096]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&partial)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - age)
        .unwrap();
    partial
}

#[test]
fn a_stale_partial_download_is_removed_when_the_library_starts() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    let stale = partial_aged(
        &models,
        ".leo-download-AbC123",
        std::time::Duration::from_secs(3600),
    );
    let suffixed = partial_aged(
        &models,
        ".leo-download-x.part",
        std::time::Duration::from_secs(3600),
    );
    let library = library_over(&models, dir.path());
    assert!(!stale.exists());
    assert!(!suffixed.exists());
    let view = library.view().unwrap();
    assert_eq!(view["disk_bytes"], 0);
}

#[test]
fn a_fresh_partial_download_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    let fresh = partial_aged(&models, ".leo-download-AbC123", std::time::Duration::ZERO);
    let nearly = partial_aged(
        &models,
        ".leo-download-y.part",
        std::time::Duration::from_secs(14 * 60),
    );
    let library = library_over(&models, dir.path());
    library.view().unwrap();
    assert!(fresh.exists());
    assert!(nearly.exists());
}

#[test]
fn a_damaged_model_is_not_hashed_again_while_unchanged() {
    let (_dir, library) = fixture();
    let path = library.model_path("parakeet-tdt-0.6b-v3").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"mode!").unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let verified = |library: &Library| {
        library.view().unwrap()["models"][0]["verified"]
            .as_bool()
            .unwrap()
    };
    assert!(!verified(&library));
    std::fs::write(&path, b"model").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(before)
        .unwrap();
    assert!(!verified(&library));
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(before + std::time::Duration::from_secs(5))
        .unwrap();
    assert!(verified(&library));
}
