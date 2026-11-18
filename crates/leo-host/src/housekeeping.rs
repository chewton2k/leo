use std::path::Path;

use anyhow::Result;

use leo_services::session::{self, Session};
use leo_web::record::Housekeeper;

pub struct Housekeeping;

fn recordings_root(root: &Path) -> Result<bool> {
    if !root.exists() {
        return Ok(false);
    }
    anyhow::ensure!(
        !std::fs::symlink_metadata(root)?.file_type().is_symlink(),
        "Recording folders cannot use symbolic links"
    );
    Ok(true)
}

fn tracks_of(root: &Path, id: &str) -> Result<Vec<std::path::PathBuf>> {
    anyhow::ensure!(
        leo_core::recording::valid_id(id),
        "That is not a recording leo knows."
    );
    let mut dirs = Vec::new();
    for found in session::find(root, id) {
        let lock = leo_core::paths::contained_path(&found.dir, Path::new("lock"))?;
        if lock.exists() {
            anyhow::ensure!(
                std::fs::metadata(lock)?.len() <= 64,
                "The recording lock could not be checked. Try again later."
            );
        }
        anyhow::ensure!(
            !session::locked(&found.dir),
            "The recording is still finishing. Try again in a moment."
        );
        dirs.push(found.dir.clone());
    }
    Ok(dirs)
}

fn remove_tracks(dirs: Vec<std::path::PathBuf>) -> Result<()> {
    for dir in dirs {
        anyhow::ensure!(
            !session::locked(&dir),
            "The recording is still finishing. Try again in a moment."
        );
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn discard_recording(root: &Path, id: &str) -> Result<()> {
    anyhow::ensure!(
        leo_core::recording::valid_id(id),
        "That is not a recording leo knows."
    );
    if !recordings_root(root)? {
        return Ok(());
    }
    remove_tracks(tracks_of(root, id)?)
}

fn discard_recordings(root: &Path, ids: &[String]) -> usize {
    match recordings_root(root) {
        Ok(false) => {
            return ids
                .iter()
                .filter(|id| leo_core::recording::valid_id(id))
                .count()
        }
        Err(_) => return 0,
        Ok(true) => {}
    }
    ids.iter()
        .filter(|id| tracks_of(root, id).and_then(remove_tracks).is_ok())
        .count()
}

fn expire_saved(root: &Path, before: chrono::DateTime<chrono::Utc>) -> usize {
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return 0;
    }
    session::scan(root, session::Walk::OlderThan(before))
        .sessions
        .into_iter()
        .filter(|b| b.saved && b.started < before)
        .filter(|b| {
            Session::open(&b.dir).is_ok_and(|s| {
                s.manifest.id == b.id && s.manifest.saved && s.manifest.started < before
            })
        })
        .filter(|b| !session::locked(&b.dir))
        .filter(|b| std::fs::remove_dir_all(&b.dir).is_ok())
        .count()
}

fn writing_ready(
    config: &leo_services::config::Config,
    store: &dyn leo_services::config::secret::SecretStore,
) -> bool {
    let local = leo_services::config::choice::local_models(config);
    leo_services::health::writing(config, store, Some(&local)).state
        == leo_services::health::WritingState::Ready
}

impl Housekeeper for Housekeeping {
    fn checks(&self) -> leo_web::record::Checks {
        leo_web::record::Checks {
            speech: leo_services::speech::library::library().is_ok_and(|l| l.selected().is_ok()),
            writing: writing_ready(
                &leo_services::config::Config::load(),
                leo_services::config::secret::default_store().as_ref(),
            ),
            headset: session::mic::output_is_headset(),
        }
    }
    fn pending_page(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<leo_web::record::Pending>, Option<String>)> {
        Ok(crate::record::recordings_root()?
            .map(|root| crate::record::pending(&root, after, limit))
            .unwrap_or_default())
    }
    fn pending_recording(&self, id: &str) -> Result<Option<leo_web::record::Pending>> {
        Ok(
            crate::record::recordings_root()?
                .and_then(|root| crate::record::pending_one(&root, id)),
        )
    }
    fn old_recordings(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<leo_web::record::Pending>> {
        Ok(crate::record::recordings_root()?
            .map(|root| crate::record::pending_before(&root, before))
            .unwrap_or_default())
    }
    fn discard_recording(&self, id: &str) -> Result<()> {
        discard_recording(&session::root()?, id)
    }
    fn discard_recordings(&self, ids: &[String]) -> usize {
        session::root().map_or(0, |root| discard_recordings(&root, ids))
    }
    fn expire_kept_audio(&self, before: chrono::DateTime<chrono::Utc>) -> usize {
        session::root().map_or(0, |root| expire_saved(&root, before))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discarding_a_call_removes_both_tracks_and_keeps_other_recordings() {
        let tmp = tempfile::tempdir().unwrap();
        let create = |id: &str| {
            let mut manifest = session::Manifest::new(None, None, "", false);
            manifest.id = id.into();
            Session::create(tmp.path(), manifest).unwrap().dir
        };
        let you = create("call-failed-01-you");
        let others = create("call-failed-01-others");
        let keep = create("keep-01");
        discard_recording(tmp.path(), "call-failed-01").unwrap();
        assert!(!you.exists());
        assert!(!others.exists());
        assert!(keep.exists());
        discard_recording(tmp.path(), "call-failed-01").unwrap();
        assert!(discard_recording(tmp.path(), "../outside").is_err());
    }

    #[test]
    fn a_batch_discard_removes_every_track_of_each_recording() {
        let tmp = tempfile::tempdir().unwrap();
        let create = |id: &str| {
            let mut manifest = session::Manifest::new(None, None, "", false);
            manifest.id = id.into();
            Session::create(tmp.path(), manifest).unwrap().dir
        };
        let you = create("call-0001-you");
        let others = create("call-0001-others");
        let single = create("single-0001");
        let keep = create("keep-01");
        let running = create("running-01");
        std::fs::write(running.join("lock"), std::process::id().to_string()).unwrap();
        let ids: Vec<String> = [
            "call-0001",
            "single-0001",
            "running-01",
            "../outside",
            "gone-0001",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(discard_recordings(tmp.path(), &ids), 3);
        assert!(!you.exists() && !others.exists() && !single.exists());
        assert!(keep.exists() && running.exists());
        assert_eq!(discard_recordings(&tmp.path().join("absent"), &ids[..1]), 1);
    }

    #[test]
    fn kept_audio_past_its_time_goes_and_everything_else_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("recordings");
        std::fs::create_dir(&sessions).unwrap();
        let create = |id: &str, saved: bool, days: i64| {
            let mut manifest = session::Manifest::new(None, None, "", false);
            manifest.id = id.into();
            manifest.saved = saved;
            manifest.started = chrono::Utc::now() - chrono::Duration::days(days);
            Session::create(&sessions, manifest).unwrap().dir
        };
        let old_saved = create("old-saved-01", true, 40);
        let new_saved = create("new-saved-01", true, 2);
        let old_pending = create("old-pending-01", false, 40);
        let locked = create("old-locked-01", true, 40);
        std::fs::write(locked.join("lock"), std::process::id().to_string()).unwrap();
        let before = chrono::Utc::now() - chrono::Duration::days(30);
        assert_eq!(expire_saved(&sessions, before), 1);
        assert!(!old_saved.exists());
        assert!(new_saved.exists() && old_pending.exists() && locked.exists());
    }

    #[test]
    fn ollama_without_a_model_is_not_ready_to_record() {
        use leo_services::config::{provider::ProviderConfig, secret::MemoryStore, Config};
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0u8; 8192];
                if stream.read(&mut buffer).unwrap_or(0) > 0 {
                    let body = "{\"models\":[]}";
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            }
        });
        let mut config = Config::default();
        config.chat.chain = vec!["ollama".into()];
        config.providers.insert(
            "ollama".into(),
            ProviderConfig {
                kind: Some(leo_services::config::provider::ProviderKind::Openai),
                base_url: Some(url),
                ..Default::default()
            },
        );
        assert!(!writing_ready(&config, &MemoryStore::default()));
    }

    #[test]
    fn recovery_and_delete_find_a_recording_past_the_first_page() {
        let tmp = tempfile::tempdir().unwrap();
        let base = chrono::Utc::now() - chrono::Duration::days(500);
        for n in 0..1200 {
            let mut manifest = session::Manifest::new(None, None, "", false);
            manifest.id = format!("rec-{n:05}");
            manifest.started = base + chrono::Duration::seconds(n * 7);
            Session::create(tmp.path(), manifest).unwrap();
        }
        let (first, after) = crate::record::pending(tmp.path(), None, 50);
        assert_eq!(first.len(), 50);
        assert_eq!(first[0].id, "rec-01199");
        assert!(after.is_some());
        assert!(first.iter().all(|p| p.id != "rec-00000"));
        let oldest = crate::record::pending_one(tmp.path(), "rec-00000").unwrap();
        assert_eq!(oldest.id, "rec-00000");
        discard_recording(tmp.path(), "rec-00000").unwrap();
        assert!(crate::record::pending_one(tmp.path(), "rec-00000").is_none());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1199);
    }

    #[test]
    fn a_delete_reads_the_manifest_from_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manifest = session::Manifest::new(None, None, "", false);
        manifest.id = "cached-01".into();
        manifest.started = chrono::Utc::now() - chrono::Duration::days(3);
        let dir = Session::create(tmp.path(), manifest.clone()).unwrap().dir;
        let path = dir.join("session.json");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let age = |path: &Path| {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(old)
                .unwrap()
        };
        age(&path);
        assert_eq!(crate::record::pending(tmp.path(), None, 50).0.len(), 1);
        manifest.id = "another-01".into();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let replaced = dir.join("session.json.new");
        std::fs::write(&replaced, bytes).unwrap();
        age(&replaced);
        std::fs::rename(&replaced, &path).unwrap();
        discard_recording(tmp.path(), "cached-01").unwrap();
        assert!(dir.exists());
        discard_recordings(tmp.path(), &["cached-01".into()]);
        assert!(dir.exists());
    }

    #[test]
    fn a_recording_in_use_is_never_discarded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manifest = session::Manifest::new(None, None, "", false);
        manifest.id = "running-01".into();
        let dir = Session::create(tmp.path(), manifest).unwrap().dir;
        std::fs::write(dir.join("lock"), std::process::id().to_string()).unwrap();
        assert!(discard_recording(tmp.path(), "running-01").is_err());
        assert!(dir.exists());
    }
}
