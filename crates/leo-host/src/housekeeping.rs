use std::collections::HashSet;
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
        unlocked(&found.dir)?;
        dirs.push(found.dir.clone());
    }
    Ok(dirs)
}

fn unlocked(dir: &Path) -> Result<()> {
    let lock = leo_core::paths::contained_path(dir, Path::new("lock"))?;
    if lock.exists() {
        anyhow::ensure!(
            std::fs::metadata(lock)?.len() <= 64,
            "The recording lock could not be checked. Try again later."
        );
    }
    anyhow::ensure!(
        !session::locked(dir),
        "The recording is still finishing. Try again in a moment."
    );
    Ok(())
}

fn kept_by(root: &Path, owned: impl Fn(&str) -> bool) -> Vec<session::Brief> {
    session::scan(root, session::Walk::All)
        .sessions
        .into_iter()
        .filter(|b| b.saved && b.owner().is_some_and(&owned))
        .collect()
}

fn session_dir(root: &Path, name: &str) -> Result<std::path::PathBuf> {
    anyhow::ensure!(
        session::folder_name(name),
        "That is not a recording leo knows."
    );
    let dir = leo_core::paths::contained_path(root, Path::new(name))?;
    anyhow::ensure!(
        std::fs::symlink_metadata(&dir)?.is_dir(),
        "Recording folders cannot use symbolic links"
    );
    Ok(dir)
}

fn name_of(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
fn remove_kept(root: &Path, dir: &Path, notes: &[String]) -> Result<bool> {
    let dir = session_dir(root, &name_of(dir))?;
    let found = Session::open(&dir)?;
    if !found.manifest.saved || !found.owner().is_some_and(|o| notes.contains(&o)) {
        return Ok(false);
    }
    unlocked(&dir)?;
    std::fs::remove_dir_all(dir)?;
    Ok(true)
}

#[cfg(test)]
fn discard_kept_for_notes(root: &Path, notes: &[String]) -> Result<Vec<String>> {
    if !recordings_root(root)? {
        return Ok(Vec::new());
    }
    let mut retry: Vec<String> = kept_by(root, |owner| notes.iter().any(|n| n == owner))
        .into_iter()
        .filter(|brief| remove_kept(root, &brief.dir, notes).is_err())
        .filter_map(|brief| brief.owner().map(String::from))
        .collect();
    retry.sort();
    retry.dedup();
    Ok(retry)
}

const SESSION_BYTES: u64 = 1024 * 1024 * 1024;

fn identity(found: &Session) -> leo_web::record::KeptSession {
    leo_web::record::KeptSession {
        digest: None,
        folder: name_of(&found.dir),
        session: found.manifest.id.clone(),
        started: found.manifest.started,
    }
}

fn kept_snapshot(found: &Session) -> leo_web::record::KeptSession {
    let mut expected = identity(found);
    expected.digest = leo_core::paths::digest_directory(&found.dir, SESSION_BYTES, &["lock"])
        .ok()
        .map(|(digest, _)| digest);
    expected
}

fn kept_sessions(
    root: &Path,
    note: &str,
    describe: fn(&Session) -> leo_web::record::KeptSession,
) -> Result<Vec<leo_web::record::KeptSession>> {
    if !recordings_root(root)? {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for brief in kept_by(root, |owner| owner == note) {
        let found = Session::open(&session_dir(root, &name_of(&brief.dir))?)?;
        if found.manifest.saved && found.owner().as_deref() == Some(note) {
            sessions.push(describe(&found));
        }
    }
    Ok(sessions)
}

fn held_session(
    root: &Path,
    note: &str,
    expected: &leo_web::record::KeptSession,
) -> Result<Option<std::path::PathBuf>> {
    let dir = match session_dir(root, &expected.folder) {
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None)
        }
        result => result?,
    };
    let found = Session::open(&dir)?;
    let same = found.manifest.id == expected.session
        && found.manifest.started == expected.started
        && found.manifest.saved
        && found.owner().as_deref() == Some(note);
    if same {
        unlocked(&dir)?;
    }
    Ok(same.then_some(dir))
}

fn verify_held_audio(
    root: &Path,
    note: &str,
    sessions: &[leo_web::record::KeptSession],
) -> Result<Vec<Option<leo_core::paths::FolderStamps>>> {
    if !recordings_root(root)? {
        return Ok(vec![None; sessions.len()]);
    }
    let mut verified = Vec::new();
    for expected in sessions {
        let Some(dir) = held_session(root, note, expected)? else {
            verified.push(None);
            continue;
        };
        anyhow::ensure!(
            expected.digest.is_some(),
            "The held recording was never checked. leo kept its audio."
        );
        let (digest, stamps) = leo_core::paths::digest_directory(&dir, SESSION_BYTES, &["lock"])?;
        anyhow::ensure!(
            expected.digest == Some(digest),
            "The held recording changed. leo kept its audio."
        );
        verified.push(Some(stamps));
    }
    Ok(verified)
}

fn discard_held_audio(
    root: &Path,
    note: &str,
    sessions: &[leo_web::record::KeptSession],
    verified: &[Option<leo_core::paths::FolderStamps>],
) -> Result<()> {
    anyhow::ensure!(
        verified.len() == sessions.len(),
        "The held recording was not checked. leo kept its audio."
    );
    if !recordings_root(root)? {
        return Ok(());
    }
    for (expected, stamps) in sessions.iter().zip(verified) {
        let Some(stamps) = stamps else { continue };
        let Some(dir) = held_session(root, note, expected)? else {
            continue;
        };
        anyhow::ensure!(
            leo_core::paths::folder_stamps(&dir, &["lock"])? == *stamps,
            "The held recording changed. leo kept its audio."
        );
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn kept_for_notes(root: &Path) -> Vec<String> {
    if !recordings_root(root).unwrap_or(false) {
        return Vec::new();
    }
    let mut owners: Vec<String> = kept_by(root, |_| true)
        .into_iter()
        .filter_map(|brief| brief.owner().map(String::from))
        .collect();
    owners.sort();
    owners.dedup();
    owners
}

fn plan_retarget(root: &Path, from: &str, target: &str) -> Result<Vec<leo_web::record::Moved>> {
    if from == target {
        return Ok(Vec::new());
    }
    let mut moved = Vec::new();
    for expected in kept_sessions(root, from, identity)? {
        let found = Session::open(&session_dir(root, &expected.folder)?)?;
        anyhow::ensure!(
            !session::locked(&found.dir),
            "A recording of this note is being recovered; try again in a moment."
        );
        if identity(&found) == expected && found.owner().as_deref() == Some(from) {
            moved.push(leo_web::record::Moved {
                session: expected.folder.clone(),
                identity: Some(identity(&found)),
                previous: found.manifest.note,
                target: target.to_owned(),
            });
        }
    }
    Ok(moved)
}

fn apply_retarget(
    root: &Path,
    from: &str,
    target: &str,
    planned: &[leo_web::record::Moved],
) -> Result<()> {
    let mut applied = Vec::new();
    for each in planned {
        let Ok(dir) = session_dir(root, &each.session) else {
            continue;
        };
        let mut found = Session::open(&dir)?;
        if !found.manifest.saved
            || each.target != target
            || each
                .identity
                .as_ref()
                .is_some_and(|expected| *expected != identity(&found))
            || found.manifest.note.as_deref() == Some(target)
            || found.manifest.note != each.previous
            || found.owner().as_deref() != Some(from)
        {
            continue;
        }
        let saved = unlocked(&dir).and_then(|()| {
            found.manifest.note = Some(target.to_string());
            found.save()
        });
        if let Err(e) = saved {
            let _ = restore_kept(root, &applied);
            return Err(e);
        }
        applied.push(each.clone());
    }
    Ok(())
}

#[cfg(test)]
fn retarget_kept(root: &Path, from: &str, target: &str) -> Result<Vec<leo_web::record::Moved>> {
    let planned = plan_retarget(root, from, target)?;
    apply_retarget(root, from, target, &planned)?;
    Ok(planned)
}

fn restore_kept(root: &Path, moved: &[leo_web::record::Moved]) -> Result<()> {
    for each in moved {
        let Ok(dir) = session_dir(root, &each.session) else {
            continue;
        };
        let Ok(mut found) = Session::open(&dir) else {
            continue;
        };
        if found.manifest.note.as_deref() != Some(each.target.as_str())
            || each
                .identity
                .as_ref()
                .is_some_and(|expected| *expected != identity(&found))
        {
            continue;
        }
        unlocked(&dir)?;
        found.manifest.note = each.previous.clone();
        found.save()?;
    }
    Ok(())
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

fn newest(dir: &Path) -> Option<std::time::SystemTime> {
    let mut newest = std::fs::symlink_metadata(dir).ok()?.modified().ok()?;
    for entry in std::fs::read_dir(dir).ok()? {
        newest = newest.max(entry.ok()?.metadata().ok()?.modified().ok()?);
    }
    Some(newest)
}

fn old_and_damaged(dir: &Path, before: chrono::DateTime<chrono::Utc>) -> bool {
    !session::locked(dir)
        && newest(dir).is_some_and(|newest| newest < std::time::SystemTime::from(before))
}

fn kept_audio(root: &Path, before: chrono::DateTime<chrono::Utc>) -> Vec<std::path::PathBuf> {
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Vec::new();
    }
    let scan = session::scan(root, session::Walk::OlderThan(before));
    let saved = scan
        .sessions
        .into_iter()
        .filter(|b| b.saved && b.started < before && !session::locked(&b.dir))
        .filter(|b| !session::has_retry(&b.dir))
        .map(|b| b.dir);
    let damaged = scan
        .malformed
        .into_iter()
        .filter(|dir| old_and_damaged(dir, before));
    saved.chain(damaged).collect()
}

fn still_kept(
    dir: &Path,
    before: chrono::DateTime<chrono::Utc>,
    protected: &HashSet<String>,
) -> bool {
    match Session::open(dir) {
        Ok(found) => {
            found.manifest.saved
                && found.manifest.started < before
                && !session::locked(dir)
                && !session::has_retry(dir)
                && (protected.is_empty()
                    || found
                        .owner()
                        .is_some_and(|owner| !protected.contains(&owner)))
        }
        Err(_) => protected.is_empty() && session::malformed(dir) && old_and_damaged(dir, before),
    }
}

fn expire_kept_audio(
    root: &Path,
    before: chrono::DateTime<chrono::Utc>,
    protected: &HashSet<String>,
) -> usize {
    kept_audio(root, before)
        .into_iter()
        .filter(|dir| still_kept(dir, before, protected))
        .filter(|dir| std::fs::remove_dir_all(dir).is_ok())
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
    fn kept_audio_count(&self, before: chrono::DateTime<chrono::Utc>) -> usize {
        session::root().map_or(0, |root| kept_audio(&root, before).len())
    }
    fn expire_kept_audio(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        protected: &HashSet<String>,
    ) -> usize {
        session::root().map_or(0, |root| expire_kept_audio(&root, before, protected))
    }
    fn kept_sessions(&self, note: &str) -> Result<Vec<leo_web::record::KeptSession>> {
        kept_sessions(&session::root()?, note, kept_snapshot)
    }
    fn kept_sessions_for(
        &self,
        notes: &std::collections::HashSet<String>,
    ) -> Result<std::collections::HashMap<String, Vec<leo_web::record::KeptSession>>> {
        let root = session::root()?;
        let mut inventory: std::collections::HashMap<_, Vec<_>> = notes
            .iter()
            .cloned()
            .map(|note| (note, Vec::new()))
            .collect();
        if recordings_root(&root)? {
            for brief in kept_by(&root, |owner| notes.contains(owner)) {
                let found = Session::open(&session_dir(&root, &name_of(&brief.dir))?)?;
                if found.manifest.saved {
                    if let Some(sessions) =
                        found.owner().and_then(|owner| inventory.get_mut(&owner))
                    {
                        sessions.push(kept_snapshot(&found));
                    }
                }
            }
        }
        Ok(inventory)
    }
    fn verify_held_audio(
        &self,
        note: &str,
        sessions: &[leo_web::record::KeptSession],
    ) -> Result<Vec<Option<leo_core::paths::FolderStamps>>> {
        verify_held_audio(&session::root()?, note, sessions)
    }
    fn discard_held_audio(
        &self,
        note: &str,
        sessions: &[leo_web::record::KeptSession],
        verified: &[Option<leo_core::paths::FolderStamps>],
    ) -> Result<()> {
        discard_held_audio(&session::root()?, note, sessions, verified)
    }
    fn kept_for_notes(&self) -> Vec<String> {
        session::root().map_or_else(|_| Vec::new(), |root| kept_for_notes(&root))
    }
    fn plan_retarget(&self, from: &str, target: &str) -> Result<Vec<leo_web::record::Moved>> {
        plan_retarget(&session::root()?, from, target)
    }
    fn apply_retarget(
        &self,
        from: &str,
        target: &str,
        moved: &[leo_web::record::Moved],
    ) -> Result<()> {
        apply_retarget(&session::root()?, from, target, moved)
    }
    fn restore_kept(&self, moved: &[leo_web::record::Moved]) -> Result<()> {
        restore_kept(&session::root()?, moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(
        root: &Path,
        minute: i64,
        note: Option<&str>,
        trace: Option<&str>,
    ) -> std::path::PathBuf {
        let mut manifest = session::Manifest::new(None, None, "", false);
        manifest.id = format!("kept-{minute:04}");
        manifest.saved = true;
        manifest.note = note.map(String::from);
        manifest.started =
            chrono::Utc::now() - chrono::Duration::days(2) + chrono::Duration::minutes(minute);
        let found = Session::create(root, manifest).unwrap();
        if let Some(note) = trace {
            found.trace("saved", &format!("Note {note} saved"));
        }
        settle(&found.dir);
        found.dir
    }

    fn settle(dir: &Path) {
        std::fs::File::options()
            .write(true)
            .open(dir.join("session.json"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }

    fn note_of(dir: &Path) -> Option<String> {
        Session::open(dir).unwrap().manifest.note
    }

    fn hold(dir: &Path) {
        std::fs::write(dir.join("lock"), std::process::id().to_string()).unwrap();
    }

    const A: &str = "note-aaaa";
    const B: &str = "note-bbbb";
    const C: &str = "note-cccc";

    #[test]
    fn an_old_kept_recording_is_tied_through_its_trace() {
        let tmp = tempfile::tempdir().unwrap();
        let tied = kept(tmp.path(), 0, None, Some(A));
        let other = kept(tmp.path(), 1, None, Some(B));
        let none = kept(tmp.path(), 2, None, None);
        assert_eq!(
            discard_kept_for_notes(tmp.path(), &[A.into()]).unwrap(),
            Vec::<String>::new()
        );
        assert!(!tied.exists());
        assert!(other.exists() && none.exists());
        std::fs::remove_file(other.join("trace.jsonl")).unwrap();
        std::fs::create_dir(other.join("trace.jsonl")).unwrap();
        assert_eq!(kept_for_notes(tmp.path()), [B]);
        assert!(discard_kept_for_notes(tmp.path(), &[A.into()])
            .unwrap()
            .is_empty());
        assert!(other.exists() && none.exists());
    }

    #[test]
    fn kept_audio_leaves_with_its_note_only() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, Some(A), None);
        let b = kept(tmp.path(), 1, Some(B), None);
        let old = kept(tmp.path(), 2, None, None);
        let busy = kept(tmp.path(), 3, Some(A), None);
        hold(&busy);
        assert_eq!(
            discard_kept_for_notes(tmp.path(), &[A.into()]).unwrap(),
            [A]
        );
        assert!(!a.exists());
        assert!(b.exists() && old.exists() && busy.exists());
    }

    #[test]
    fn a_hold_preserves_a_recording_whose_audio_was_edited() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = kept(tmp.path(), 0, Some(A), None);
        let wav = dir.join("seg-00000.wav");
        std::fs::write(&wav, b"audio").unwrap();
        let expected = kept_sessions(tmp.path(), A, kept_snapshot).unwrap();
        let clock = std::fs::metadata(&wav).unwrap().modified().unwrap();
        std::fs::write(&wav, b"other").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&wav)
            .unwrap()
            .set_modified(clock)
            .unwrap();
        let _ = discard_held_audio(
            tmp.path(),
            A,
            &expected,
            &verify_held_audio(tmp.path(), A, &expected).unwrap_or_default(),
        );
        assert_eq!(std::fs::read(wav).unwrap(), b"other");
    }

    #[test]
    #[ignore]
    fn held_audio_lock_benchmark() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = kept(tmp.path(), 0, Some(A), None);
        let wav = std::fs::File::create(dir.join("seg-00000.wav")).unwrap();
        let chunk = vec![7u8; 1 << 20];
        let mut out = std::io::BufWriter::new(wav);
        for _ in 0..1023 {
            std::io::Write::write_all(&mut out, &chunk).unwrap();
        }
        drop(out);
        let start = std::time::Instant::now();
        let expected = kept_sessions(tmp.path(), A, kept_snapshot).unwrap();
        let enrolled = start.elapsed();
        let start = std::time::Instant::now();
        let verified = verify_held_audio(tmp.path(), A, &expected).unwrap();
        let outside = start.elapsed();
        let start = std::time::Instant::now();
        discard_held_audio(tmp.path(), A, &expected, &verified).unwrap();
        let under_lock = start.elapsed();
        assert!(!dir.exists());
        eprintln!(
            "held audio 1 GiB: hashed when held {:.1} ms, checked again outside the lock {:.1} ms, under the notes lock at deletion {:.1} ms",
            enrolled.as_secs_f64() * 1000.0,
            outside.as_secs_f64() * 1000.0,
            under_lock.as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn audio_replaced_after_its_check_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = kept(tmp.path(), 0, Some(A), None);
        let wav = dir.join("seg-00000.wav");
        std::fs::write(&wav, b"audio").unwrap();
        let expected = kept_sessions(tmp.path(), A, kept_snapshot).unwrap();
        let verified = verify_held_audio(tmp.path(), A, &expected).unwrap();
        std::fs::remove_file(&wav).unwrap();
        std::fs::write(&wav, b"other").unwrap();
        assert!(discard_held_audio(tmp.path(), A, &expected, &verified).is_err());
        assert_eq!(std::fs::read(wav).unwrap(), b"other");
    }

    #[test]
    fn a_stale_lock_left_after_the_hold_does_not_keep_its_audio() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = kept(tmp.path(), 0, Some(A), None);
        std::fs::write(dir.join("seg-00000.wav"), b"audio").unwrap();
        let expected = kept_sessions(tmp.path(), A, kept_snapshot).unwrap();
        let lock = dir.join("lock");
        std::fs::write(&lock, "1").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        discard_held_audio(
            tmp.path(),
            A,
            &expected,
            &verify_held_audio(tmp.path(), A, &expected).unwrap_or_default(),
        )
        .unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn a_hold_deletes_only_unchanged_sessions_it_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let matched = kept(tmp.path(), 0, Some(A), None);
        let transferred = kept(tmp.path(), 1, Some(A), None);
        let replaced = kept(tmp.path(), 2, Some(A), None);
        let planned = kept_sessions(tmp.path(), A, kept_snapshot).unwrap();
        assert_eq!(planned.len(), 3);
        let later = kept(tmp.path(), 3, Some(A), None);
        let mut change = Session::open(&transferred).unwrap();
        change.manifest.note = Some(B.to_string());
        change.save().unwrap();
        let mut change = Session::open(&replaced).unwrap();
        change.manifest.id = "fresh-id".into();
        change.save().unwrap();
        discard_held_audio(
            tmp.path(),
            A,
            &planned,
            &verify_held_audio(tmp.path(), A, &planned).unwrap_or_default(),
        )
        .unwrap();
        assert!(!matched.exists());
        assert!(later.exists() && transferred.exists() && replaced.exists());
        discard_held_audio(
            tmp.path(),
            A,
            &planned,
            &verify_held_audio(tmp.path(), A, &planned).unwrap_or_default(),
        )
        .unwrap();
    }

    #[test]
    fn kept_audio_follows_a_combined_note() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, Some(A), None);
        assert_eq!(retarget_kept(tmp.path(), A, B).unwrap().len(), 1);
        assert!(discard_kept_for_notes(tmp.path(), &[A.into()])
            .unwrap()
            .is_empty());
        assert!(a.exists());
        assert_eq!(note_of(&a).as_deref(), Some(B));
    }

    #[test]
    fn a_retarget_moves_only_the_absorbed_notes_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, Some(A), None);
        let b = kept(tmp.path(), 1, Some(B), None);
        let before = std::fs::read(b.join("session.json")).unwrap();
        let moved = retarget_kept(tmp.path(), A, B).unwrap();
        assert_eq!(
            moved,
            [leo_web::record::Moved {
                session: name_of(&a),
                identity: Some(identity(&Session::open(&a).unwrap())),
                previous: Some(A.into()),
                target: B.into(),
            }]
        );
        assert_eq!(std::fs::read(b.join("session.json")).unwrap(), before);
    }

    #[test]
    fn restoring_a_retarget_gives_the_audio_back() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, Some(A), None);
        let b = kept(tmp.path(), 1, Some(B), None);
        let moved = retarget_kept(tmp.path(), A, B).unwrap();
        restore_kept(tmp.path(), &moved).unwrap();
        assert!(discard_kept_for_notes(tmp.path(), &[B.into()])
            .unwrap()
            .is_empty());
        assert!(!b.exists());
        assert!(a.exists());
        assert_eq!(note_of(&a).as_deref(), Some(A));
    }

    #[test]
    fn restore_puts_back_an_old_session_without_a_note_field() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, None, Some(A));
        let moved = retarget_kept(tmp.path(), A, B).unwrap();
        assert_eq!(moved[0].previous, None);
        assert_eq!(note_of(&a).as_deref(), Some(B));
        restore_kept(tmp.path(), &moved).unwrap();
        assert_eq!(note_of(&a), None);
        assert_eq!(Session::open(&a).unwrap().owner().as_deref(), Some(A));
    }

    #[test]
    fn restore_leaves_a_folder_owned_by_another_note() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, Some(A), None);
        let moved = retarget_kept(tmp.path(), A, B).unwrap();
        let mut found = Session::open(&a).unwrap();
        found.manifest.note = Some(C.into());
        found.save().unwrap();
        restore_kept(tmp.path(), &moved).unwrap();
        assert_eq!(note_of(&a).as_deref(), Some(C));
        let outside = [leo_web::record::Moved {
            session: "../x".into(),
            identity: None,
            previous: None,
            target: C.into(),
        }];
        restore_kept(tmp.path(), &outside).unwrap();
        assert_eq!(note_of(&a).as_deref(), Some(C));
    }

    #[test]
    fn a_retarget_refuses_while_a_recording_is_locked() {
        let tmp = tempfile::tempdir().unwrap();
        let a = kept(tmp.path(), 0, Some(A), None);
        let busy = kept(tmp.path(), 1, Some(A), None);
        hold(&busy);
        let before = std::fs::read(a.join("session.json")).unwrap();
        let refused = retarget_kept(tmp.path(), A, B).unwrap_err().to_string();
        assert_eq!(
            refused,
            "A recording of this note is being recovered; try again in a moment."
        );
        assert_eq!(std::fs::read(a.join("session.json")).unwrap(), before);
        assert_eq!(note_of(&busy).as_deref(), Some(A));
    }

    #[cfg(unix)]
    #[test]
    fn a_half_done_retarget_puts_back_what_it_changed() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let older = kept(tmp.path(), 0, Some(A), None);
        let newer = kept(tmp.path(), 1, Some(A), None);
        use std::os::unix::fs::MetadataExt;
        let inode = || std::fs::metadata(newer.join("session.json")).unwrap().ino();
        let first = inode();
        std::fs::set_permissions(&older, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = retarget_kept(tmp.path(), A, B);
        std::fs::set_permissions(&older, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_ne!(
            inode(),
            first,
            "the newer session was written, then put back"
        );
        assert_eq!(note_of(&newer).as_deref(), Some(A));
        assert_eq!(note_of(&older).as_deref(), Some(A));
    }

    #[test]
    fn an_owner_changed_on_disk_is_read_before_deleting() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = kept(tmp.path(), 0, Some(A), None);
        assert_eq!(kept_for_notes(tmp.path()), [A]);
        let path = dir.join("session.json");
        let old = std::fs::metadata(&path).unwrap().modified().unwrap();
        let text = std::fs::read_to_string(&path).unwrap().replace(A, B);
        std::fs::write(&path, text).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(discard_kept_for_notes(tmp.path(), &[A.into()])
            .unwrap()
            .is_empty());
        assert!(!remove_kept(tmp.path(), &dir, &[A.into()]).unwrap());
        assert!(dir.exists());
        assert_eq!(note_of(&dir).as_deref(), Some(B));
    }

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
    fn an_unresolved_combine_protects_its_saved_recordings_from_keeping() {
        let tmp = tempfile::tempdir().unwrap();
        let waiting = kept(tmp.path(), 0, Some(A), None);
        let other = kept(tmp.path(), 1, Some(B), None);
        assert_eq!(
            expire_kept_audio(
                tmp.path(),
                chrono::Utc::now(),
                &HashSet::from([A.to_string()])
            ),
            1
        );
        assert!(waiting.exists());
        assert!(!other.exists());
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
        assert_eq!(expire_kept_audio(&sessions, before, &HashSet::new()), 1);
        assert!(!old_saved.exists());
        assert!(new_saved.exists() && old_pending.exists() && locked.exists());
    }

    fn set_old(path: &Path, days: u64) {
        let mut open = std::fs::File::options();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.write(true).custom_flags(0x0200_0000);
        }
        #[cfg(not(windows))]
        if path.is_dir() {
            open.read(true);
        } else {
            open.write(true);
        }
        open.open(path)
            .unwrap()
            .set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400),
            )
            .unwrap();
    }

    fn damaged(root: &Path, days: i64, manifest: Option<&str>, wav: bool) -> std::path::PathBuf {
        let started = chrono::Utc::now() - chrono::Duration::days(days);
        let dir = root.join(started.format("%Y%m%d-%H%M%S").to_string());
        std::fs::create_dir(&dir).unwrap();
        if let Some(text) = manifest {
            std::fs::write(dir.join("session.json"), text).unwrap();
            set_old(&dir.join("session.json"), days as u64);
        }
        if wav {
            std::fs::write(dir.join("seg-00000.wav"), vec![0u8; 4096]).unwrap();
            set_old(&dir.join("seg-00000.wav"), days as u64);
        }
        set_old(&dir, days as u64);
        dir
    }

    fn session_of(root: &Path, id: &str, saved: bool, days: i64) -> std::path::PathBuf {
        let mut manifest = session::Manifest::new(None, None, "", false);
        manifest.id = id.into();
        manifest.saved = saved;
        manifest.started = chrono::Utc::now() - chrono::Duration::days(days);
        Session::create(root, manifest).unwrap().dir
    }

    #[test]
    fn the_audio_preview_counts_what_expiry_deletes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let kept = session_of(root, "kept-for-retry", true, 40);
        std::fs::write(kept.join("seg-00000.txt"), "said").unwrap();
        std::fs::write(kept.join("seg-00001.wav"), vec![0u8; 4096]).unwrap();
        std::fs::write(kept.join("seg-00001.err"), "boom").unwrap();
        let broken = damaged(root, 41, Some("{"), true);
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        let old: Vec<String> = crate::record::pending_before(root, cutoff)
            .into_iter()
            .map(|p| p.id)
            .collect();
        let preview = old.len() + kept_audio(root, cutoff).len();
        let deleted =
            discard_recordings(root, &old) + expire_kept_audio(root, cutoff, &HashSet::new());
        assert_eq!((preview, deleted), (2, 2));
        assert!(!kept.exists() && !broken.exists());
    }

    #[test]
    fn kept_audio_is_not_counted_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let retry = session_of(root, "kept-for-retry", true, 40);
        std::fs::write(retry.join("seg-00001.wav"), vec![0u8; 4096]).unwrap();
        std::fs::write(retry.join("seg-00001.err"), "boom").unwrap();
        let done = session_of(root, "kept-no-retry", true, 41);
        std::fs::write(done.join("seg-00001.err"), "boom").unwrap();
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        let old: Vec<String> = crate::record::pending_before(root, cutoff)
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(old, ["kept-for-retry"]);
        assert_eq!(kept_audio(root, cutoff), std::slice::from_ref(&done));
        assert_eq!(expire_kept_audio(root, cutoff, &HashSet::new()), 1);
        assert!(retry.exists() && !done.exists());
    }

    #[test]
    fn old_folders_with_damaged_metadata_are_expired() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let empty = damaged(root, 40, None, false);
        let broken = damaged(root, 41, Some("{"), true);
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        assert_eq!(kept_audio(root, cutoff).len(), 2);
        assert_eq!(expire_kept_audio(root, cutoff, &HashSet::new()), 2);
        assert!(!empty.exists() && !broken.exists());
    }

    #[cfg(unix)]
    #[test]
    fn damaged_folders_that_are_new_or_unreadable_are_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let fresh = damaged(root, 0, Some("{"), true);
        let started_now = root.join("20990101-000000");
        std::fs::create_dir(&started_now).unwrap();
        let touched = damaged(root, 40, Some("{"), false);
        std::fs::write(touched.join("seg-00000.wav"), vec![0u8; 4096]).unwrap();
        set_old(&touched, 40);
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        use std::os::unix::fs::PermissionsExt;
        let manifest =
            serde_json::to_string(&session::Manifest::new(None, None, "", false)).unwrap();
        let shut = damaged(root, 42, Some(&manifest), true);
        let mode = |mode| {
            std::fs::set_permissions(
                shut.join("session.json"),
                std::fs::Permissions::from_mode(mode),
            )
            .unwrap()
        };
        mode(0o000);
        set_old(&shut, 42);
        let selected = kept_audio(root, cutoff).len();
        let deleted = expire_kept_audio(root, cutoff, &HashSet::new());
        mode(0o600);
        assert_eq!((selected, deleted), (0, 0));
        assert!(fresh.exists() && started_now.exists() && touched.exists() && shut.exists());
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
