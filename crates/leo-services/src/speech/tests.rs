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
