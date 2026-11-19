use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::record::{discard_all_unless_in_use, Housekeeper, Recordings};
use crate::AppState;

const DISCARD_BATCH: usize = 256;

pub fn data_dir(notes_dir: &Path) -> PathBuf {
    crate::db::beside_of(notes_dir, None).to_path_buf()
}

fn older(path: &Path, cutoff: SystemTime) -> bool {
    std::fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|modified| modified < cutoff)
}

fn cutoff(days: Option<u32>, now: DateTime<Utc>) -> Option<SystemTime> {
    let days = days?;
    SystemTime::from(now).checked_sub(std::time::Duration::from_secs(u64::from(days) * 86_400))
}

fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn child_dirs(root: &Path, valid: fn(&str) -> bool) -> impl Iterator<Item = PathBuf> + '_ {
    real_dir(root)
        .then(|| std::fs::read_dir(root).ok())
        .flatten()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(move |e| e.file_name().to_str().is_some_and(valid))
        .filter_map(move |e| leo_core::paths::contained_path(root, Path::new(&e.file_name())).ok())
}

fn upload_note_id(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

const GIVE_WAY_EVERY: usize = 1_000;

fn stop_asked() -> bool {
    leo_core::children::current_stop().is_some_and(|s| s.asked())
}

fn each_old_upload(notes_dir: &Path, cutoff: SystemTime, visit: &mut dyn FnMut(&Path)) {
    let Ok(base) = leo_core::paths::contained_path(
        &data_dir(notes_dir),
        Path::new(leo_core::paths::own::ORIGINALS),
    ) else {
        return;
    };
    let mut met = 0usize;
    let mut give_way = || {
        met += 1;
        met.is_multiple_of(GIVE_WAY_EVERY) && stop_asked()
    };
    for dir in child_dirs(&base, upload_note_id) {
        if give_way() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if give_way() {
                return;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_file()) && older(&path, cutoff) {
                visit(&path);
            }
        }
    }
}

fn old_upload_count(notes_dir: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(cutoff) = cutoff(days, now) else {
        return 0;
    };
    let mut count = 0;
    each_old_upload(notes_dir, cutoff, &mut |_| count += 1);
    count
}

pub fn expire_uploads(notes_dir: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(cutoff) = cutoff(days, now) else {
        return 0;
    };
    let mut removed = 0;
    each_old_upload(notes_dir, cutoff, &mut |path| {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
            crate::originals::drop_copy_of(path);
        }
    });
    removed
}

fn old_transcripts(
    notes_dir: &Path,
    days: Option<u32>,
    now: DateTime<Utc>,
    active: Option<&str>,
) -> Vec<PathBuf> {
    let Some(cutoff) = cutoff(days, now) else {
        return Vec::new();
    };
    let root = leo_core::recording::root(notes_dir);
    child_dirs(&root, leo_core::recording::valid_id)
        .filter(|dir| active.is_none() || dir.file_name().and_then(|n| n.to_str()) != active)
        .filter(|dir| older(dir, cutoff))
        .collect()
}

pub fn expire_transcripts(
    notes_dir: &Path,
    days: Option<u32>,
    now: DateTime<Utc>,
    active: Option<&str>,
) -> usize {
    old_transcripts(notes_dir, days, now, active)
        .into_iter()
        .filter(|dir| std::fs::remove_dir_all(dir).is_ok())
        .count()
}

const BACKUP_DAYS: u64 = 30;

fn set_aside_at(meta: &std::fs::Metadata) -> Option<SystemTime> {
    let modified = meta.modified().ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let changed = u64::try_from(meta.ctime()).ok().map(|secs| {
            SystemTime::UNIX_EPOCH
                + std::time::Duration::new(secs, u32::try_from(meta.ctime_nsec()).unwrap_or(0))
        });
        Some(changed.map_or(modified, |changed| changed.max(modified)))
    }
    #[cfg(not(unix))]
    Some(modified)
}

fn past_backup_days(path: &Path, now: SystemTime) -> bool {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_file())
        .as_ref()
        .and_then(set_aside_at)
        .and_then(|at| now.duration_since(at).ok())
        .is_some_and(|age| age.as_secs() > BACKUP_DAYS * 86_400)
}

fn table_of(name: &str) -> Option<&'static str> {
    use leo_core::paths::own::{BEFORE, CAPTIONS, GRAPH, MEANING};
    let kept = name.strip_suffix(BEFORE)?;
    [
        (CAPTIONS, "captions"),
        (GRAPH, "graph_notes"),
        (MEANING, "vectors"),
    ]
    .into_iter()
    .find(|(file, _)| *file == kept)
    .map(|(_, table)| table)
}

fn proven_backups(data: &Path) -> Vec<PathBuf> {
    let database = data.join(leo_core::paths::own::DATABASE);
    if !std::fs::symlink_metadata(&database).is_ok_and(|meta| meta.is_file()) {
        return Vec::new();
    }
    let Ok(db) = crate::db::try_at(&database) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(data) else {
        return Vec::new();
    };
    let damaged = format!("{}.damaged-", leo_core::paths::own::DATABASE);
    let mut found = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return Vec::new();
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&damaged) {
            return Vec::new();
        }
        if let Some(table) = table_of(name) {
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                found.push((entry.path(), table));
            }
        }
    }
    found
        .into_iter()
        .filter(|(_, table)| {
            db.with(|c| {
                c.query_row(&format!("SELECT EXISTS (SELECT 1 FROM {table})"), [], |r| {
                    r.get::<_, bool>(0)
                })
            })
            .unwrap_or(false)
        })
        .map(|(path, _)| path)
        .collect()
}

pub(crate) fn expire_backups(data: &Path, notes: &Path, now: SystemTime) -> usize {
    let mut removed = 0;
    let mut remove = |path: &Path| match std::fs::remove_file(path) {
        Ok(()) => removed += 1,
        Err(e) => leo_core::diag::warn(format!(
            "could not remove the old backup {}: {e}",
            path.display()
        )),
    };
    let list = notes.join("directories.json");
    let list_reads = match std::fs::read(&list) {
        Ok(raw) => serde_json::from_slice::<Vec<String>>(&raw).is_ok(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    };
    let corrupt = list.with_extension("json.corrupt");
    if list_reads && past_backup_days(&corrupt, now) {
        remove(&corrupt);
    }
    for backup in proven_backups(data) {
        if past_backup_days(&backup, now) {
            remove(&backup);
        }
    }
    removed
}

pub(crate) async fn would_delete(state: &AppState, field: &str, days: Option<u32>) -> usize {
    let Some(days) = days else {
        return 0;
    };
    let now = Utc::now();
    let before = now - chrono::Duration::days(i64::from(days));
    let chats_dir = state.chats.clone();
    let field = field.to_string();
    let housekeeper = state.housekeeper.clone();
    let active = state
        .recording
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|job| job.session_id());
    let notes_dir = state.notes_dir.clone();
    if field == "trash_days" {
        return state
            .read_store(move |store| Ok(store.would_expire(days)))
            .await
            .unwrap_or(0);
    }
    let (_stop_on_drop, stop) = crate::stop_when_dropped();
    tokio::task::spawn_blocking(move || match field.as_str() {
        "chat_days" => crate::chats::list(&chats_dir)
            .iter()
            .filter(|c| c.updated_at < before)
            .count(),
        "upload_days" => {
            leo_core::children::with_stop(stop, || old_upload_count(&notes_dir, Some(days), now))
        }
        "transcript_days" => old_transcripts(&notes_dir, Some(days), now, active.as_deref()).len(),
        "audio_days" => housekeeper.map_or(0, |h| audio_count(&*h, before, active.as_deref())),
        _ => 0,
    })
    .await
    .unwrap_or(0)
}

fn audio_count(
    housekeeper: &dyn Housekeeper,
    before: DateTime<Utc>,
    active: Option<&str>,
) -> usize {
    let pending = housekeeper.old_recordings(before).map_or(0, |old| {
        old.iter().filter(|p| active != Some(p.id.as_str())).count()
    });
    pending + housekeeper.kept_audio_count(before)
}

pub(crate) fn expire_audio(
    housekeeper: &dyn Housekeeper,
    days: Option<u32>,
    now: DateTime<Utc>,
    recording: &Recordings,
    protected: &HashSet<String>,
) -> usize {
    let Some(days) = days else {
        return 0;
    };
    let Some(cutoff) = now.checked_sub_signed(chrono::Duration::days(i64::from(days))) else {
        return 0;
    };
    let blocked: HashSet<String> = if protected.is_empty() {
        HashSet::new()
    } else {
        let Ok(sessions) = housekeeper.kept_sessions_for(protected) else {
            return 0;
        };
        sessions
            .into_values()
            .flatten()
            .map(|recording| {
                let id = recording.session;
                id.strip_suffix("-you")
                    .or_else(|| id.strip_suffix("-others"))
                    .unwrap_or(&id)
                    .to_string()
            })
            .collect()
    };
    let kept = housekeeper.expire_kept_audio(cutoff, protected);
    let Ok(old) = housekeeper.old_recordings(cutoff) else {
        return kept;
    };
    let ids: Vec<String> = old
        .into_iter()
        .map(|p| p.id)
        .filter(|id| !blocked.contains(id))
        .collect();
    kept + ids
        .chunks(DISCARD_BATCH)
        .map(|batch| discard_all_unless_in_use(recording, housekeeper, batch.to_vec()))
        .sum::<usize>()
}

pub(crate) fn expire_dependents(state: &AppState) {
    let Ok(expired) = crate::store_now(state, |store| Ok(store.tidy_trash_now())) else {
        return;
    };
    crate::dependents::clear(state, expired, crate::dependents::Why::Expired);
    crate::dependents::pictures(state, Utc::now());
}

pub(crate) async fn expire_choices(state: &AppState) {
    let state = state.clone();
    let _ = tokio::task::spawn_blocking(move || apply_choices(&state)).await;
}

#[cfg(test)]
pub(crate) async fn expire_now(state: &AppState) {
    let state = state.clone();
    let _ = tokio::task::spawn_blocking(move || {
        apply_choices(&state);
        expire_dependents(&state);
    })
    .await;
}

pub(crate) fn apply_choices(state: &AppState) {
    let notes_dir = &state.notes_dir;
    let keep = leo_core::keep::load(notes_dir);
    let chats = &state.chats;
    let stop = leo_core::children::Stop {
        flag: state.quitting.clone(),
        deadline: None,
    };
    let active = state
        .recording
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|job| job.session_id());
    let now = Utc::now();
    {
        let _writing = crate::tasks::writing(chats);
        crate::tasks::forget_deleted(chats, keep.trash_days, now);
    }
    {
        let _writing = crate::boards::writing(chats);
        crate::boards::finish_deletes(chats);
        crate::boards::forget_deleted(chats, keep.trash_days, now);
        crate::boards::forget_changes(chats, keep.trash_days, now);
    }
    crate::chats::tidy(chats, keep.chat_days, now);
    crate::chat_files::tidy_orphans(chats, now);
    crate::routes::combine::forget_old_combines(chats, now - chrono::Duration::days(1));
    leo_core::children::with_stop(stop.clone(), || {
        expire_uploads(notes_dir, keep.upload_days, now);
    });
    if stop.asked() {
        return;
    }
    state.originals.forget_missing();
    expire_transcripts(notes_dir, keep.transcript_days, now, active.as_deref());
    if let Some(housekeeper) = &state.housekeeper {
        let _audio = crate::routes::combine::audio(state);
        let protected = crate::routes::combine::pending_retargets(chats)
            .into_iter()
            .flat_map(|(from, target)| [from, target])
            .collect();
        expire_audio(
            &**housekeeper,
            keep.audio_days,
            now,
            &state.recording,
            &protected,
        );
    }
    expire_backups(&data_dir(notes_dir), notes_dir, SystemTime::from(now));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Pending, Source};
    use std::sync::Mutex;

    fn age(path: &Path, days: u64, extra_secs: u64) {
        let when = SystemTime::now() - std::time::Duration::from_secs(days * 86_400 + extra_secs);
        let mut open = std::fs::File::options();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.write(true).custom_flags(0x0200_0000);
        }
        #[cfg(not(windows))]
        open.read(true);
        open.open(path).unwrap().set_modified(when).unwrap();
    }

    fn data() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        (tmp, notes)
    }

    fn upload(notes: &Path, note: &str, name: &str, days: u64, extra: u64) -> PathBuf {
        let dir = data_dir(notes).join("attachments").join(note);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, "x").unwrap();
        age(&file, days, extra);
        file
    }

    fn copy_of_upload(file: &Path) -> PathBuf {
        let folder = file.parent().unwrap().join(".read");
        std::fs::create_dir_all(&folder).unwrap();
        let copy = folder.join(format!(
            "{}.txt",
            file.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&copy, "read copy").unwrap();
        copy
    }

    #[test]
    fn the_read_copy_is_not_counted_expired_alone_or_exported() {
        let (tmp, notes) = data();
        let old = upload(&notes, "n1", "old.pdf", 40, 0);
        let fresh = upload(&notes, "n1", "fresh.pdf", 1, 0);
        let (old_copy, fresh_copy) = (copy_of_upload(&old), copy_of_upload(&fresh));
        age(&old_copy, 90, 0);
        age(&fresh_copy, 90, 0);
        let now = Utc::now();
        assert_eq!(old_upload_count(&notes, Some(30), now), 1);
        let mut zip = std::io::Cursor::new(Vec::new());
        let parts = crate::export::Parts {
            uploads: true,
            ..Default::default()
        };
        crate::export::write_zip(&mut zip, &notes, &tmp.path().join("chats"), parts).unwrap();
        let mut archive = zip::ZipArchive::new(zip).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("fresh.pdf")), "{names:?}");
        assert!(names.iter().all(|n| !n.contains(".read")), "{names:?}");
        assert_eq!(expire_uploads(&notes, Some(30), now), 1);
        assert!(!old.exists() && !old_copy.exists());
        assert!(fresh.exists() && fresh_copy.exists());
        assert_eq!(expire_uploads(&notes, Some(30), now), 0);
        assert!(fresh_copy.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_expired_upload_never_takes_a_file_through_a_linked_copies_folder() {
        let (tmp, notes) = data();
        let old = upload(&notes, "n1", "old.pdf", 40, 0);
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("old.pdf.txt"), "the person's file").unwrap();
        std::os::unix::fs::symlink(&outside, old.parent().unwrap().join(".read")).unwrap();
        assert_eq!(expire_uploads(&notes, Some(30), Utc::now()), 1);
        assert!(!old.exists());
        assert!(outside.join("old.pdf.txt").is_file());
    }

    fn transcript(notes: &Path, id: &str, days: u64, extra: u64) -> PathBuf {
        let dir = leo_core::recording::root(notes).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("1.json"), "{}").unwrap();
        age(&dir, days, extra);
        dir
    }

    fn uploads_left(notes: &Path) -> usize {
        std::fs::read_dir(data_dir(notes).join("attachments"))
            .unwrap()
            .flatten()
            .map(|dir| std::fs::read_dir(dir.path()).unwrap().count())
            .sum()
    }

    #[test]
    fn forever_deletes_nothing() {
        let (_tmp, notes) = data();
        let file = upload(&notes, "note-one", "a.pdf", 4000, 0);
        let archive = transcript(&notes, "note-one-aaa", 4000, 0);
        let now = Utc::now();
        assert_eq!(expire_uploads(&notes, None, now), 0);
        assert_eq!(expire_transcripts(&notes, None, now, None), 0);
        assert!(file.exists() && archive.exists());
    }

    #[test]
    fn an_upload_older_than_the_period_goes_and_a_newer_one_stays() {
        let (_tmp, notes) = data();
        let old = upload(&notes, "note-one", "old.pdf", 8, 0);
        let new = upload(&notes, "note-one", "new.pdf", 6, 0);
        let other = upload(&notes, "note-two", "gone.pdf", 30, 0);
        assert_eq!(expire_uploads(&notes, Some(7), Utc::now()), 2);
        assert!(!old.exists() && new.exists() && !other.exists());
        assert!(new.parent().unwrap().exists());
    }

    #[test]
    fn an_upload_just_inside_the_period_stays_and_just_outside_goes() {
        let (_tmp, notes) = data();
        let inside = upload(&notes, "note-one", "in.pdf", 7, 0);
        age(&inside, 6, 86_000);
        let outside = upload(&notes, "note-one", "out.pdf", 7, 120);
        assert_eq!(expire_uploads(&notes, Some(7), Utc::now()), 1);
        assert!(inside.exists() && !outside.exists());
    }

    #[test]
    #[ignore]
    fn every_old_upload_is_expired_and_counted() {
        let now = Utc::now();
        let (_many_folders, notes) = data();
        for n in 0..100_200 {
            upload(&notes, &format!("note-{n}"), "a.pdf", 40, 0);
        }
        assert_eq!(old_upload_count(&notes, Some(30), now), 100_200);
        assert_eq!(expire_uploads(&notes, Some(30), now), 100_200);
        assert_eq!(uploads_left(&notes), 0);
        let (_one_folder, notes) = data();
        for n in 0..100_200 {
            upload(&notes, "note-one", &format!("{n}.pdf"), 40, 0);
        }
        assert_eq!(old_upload_count(&notes, Some(30), now), 100_200);
        assert_eq!(expire_uploads(&notes, Some(30), now), 100_200);
        assert_eq!(uploads_left(&notes), 0);
    }

    #[test]
    fn old_uploads_are_handled_as_they_are_met() {
        let (_tmp, notes) = data();
        for n in 0..5_000 {
            upload(
                &notes,
                &format!("note-{}", n % 50),
                &format!("{n}.pdf"),
                40,
                0,
            );
        }
        let attachments = data_dir(&notes).join("attachments");
        let cutoff = cutoff(Some(30), Utc::now()).unwrap();
        let mut visits = 0;
        each_old_upload(&notes, cutoff, &mut |_| {
            if visits == 0 {
                for dir in std::fs::read_dir(&attachments).unwrap().flatten() {
                    for file in std::fs::read_dir(dir.path()).unwrap().flatten() {
                        std::fs::remove_file(file.path()).unwrap();
                    }
                }
            }
            visits += 1;
        });
        assert!(visits < 10, "{visits}");
    }

    #[test]
    fn a_stopped_preview_gives_way() {
        let (_tmp, notes) = data();
        for n in 0..2_500 {
            upload(&notes, "note-one", &format!("{n}.pdf"), 40, 0);
        }
        let stop = leo_core::children::Stop::default();
        stop.flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let counted =
            leo_core::children::with_stop(stop, || old_upload_count(&notes, Some(30), Utc::now()));
        assert!(counted < GIVE_WAY_EVERY, "{counted}");
        assert_eq!(old_upload_count(&notes, Some(30), Utc::now()), 2_500);
    }

    #[tokio::test]
    async fn upload_expiry_gives_way_on_quit_and_can_run_again() {
        let (tmp, notes) = data();
        let running = crate::start_in(
            crate::Places {
                store: leo_core::store::Store::load_from(&notes).unwrap(),
                sessions: tmp.path().join("sessions.json"),
            },
            crate::StartOptions {
                port: 0,
                token: crate::fresh_token(),
            },
            Default::default(),
        )
        .await
        .unwrap();
        let state = running.state.clone();
        running.stop().await;

        leo_core::keep::save(
            &notes,
            &leo_core::keep::Keep {
                upload_days: Some(30),
                ..Default::default()
            },
        )
        .unwrap();
        let old = 5_000;
        for n in 0..old {
            upload(&notes, "note-one", &format!("{n}.pdf"), 40, 0);
        }
        let fresh = upload(&notes, "note-one", "fresh.pdf", 1, 0);

        expire_now(&state).await;
        assert!(fresh.is_file());
        let remaining = uploads_left(&notes) - 1;
        assert!(
            (old - GIVE_WAY_EVERY + 1..=old).contains(&remaining),
            "{remaining} of {old} old uploads remain"
        );
        let state = AppState {
            quitting: Default::default(),
            ..state
        };
        expire_now(&state).await;
        assert_eq!(uploads_left(&notes), 1);
        assert!(fresh.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_upload_is_not_followed() {
        let (tmp, notes) = data();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "x").unwrap();
        age(&secret, 400, 0);
        let attachments = data_dir(&notes).join("attachments");
        std::fs::create_dir_all(&attachments).unwrap();
        std::os::unix::fs::symlink(&outside, attachments.join("linked")).unwrap();
        let real = attachments.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&secret, real.join("link.txt")).unwrap();
        assert_eq!(expire_uploads(&notes, Some(7), Utc::now()), 0);
        assert!(secret.exists());
        assert!(real.join("link.txt").symlink_metadata().is_ok());
    }

    fn set_aside_captions(data: &Path) -> PathBuf {
        std::fs::write(data.join("captions.json"), r#"{"k": "a heap drawing"}"#).unwrap();
        crate::captions::Captions::at(data.join("captions.json"));
        let backup = data.join("captions.json.before-database");
        assert!(backup.is_file());
        backup
    }

    const DAY: std::time::Duration = std::time::Duration::from_secs(86_400);

    #[test]
    fn old_backups_go_after_thirty_days_and_new_ones_stay() {
        let (tmp, notes) = data();
        let data = tmp.path();
        let captions = set_aside_captions(data);
        std::fs::write(
            data.join("meaning.json"),
            r#"{"notes": {"n1": {"hash": "h", "pieces": [[0, "AACAPwAAAAA="]]}}}"#,
        )
        .unwrap();
        crate::vectors::Vectors::at(data.join("meaning.json"));
        let meaning = data.join("meaning.json.before-database");
        std::fs::write(
            data.join("graph.json"),
            serde_json::json!({ "notes": { "n1": { "hash": "h", "summary": "heaps", "concepts": ["heap"] } }, "links": [], "built_at": "2026-10-01" }).to_string(),
        )
        .unwrap();
        crate::graph::Graphs::for_notes(&notes, None).load();
        let graph = data.join("graph.json.before-database");
        let corrupt = notes.join("directories.json.corrupt");
        std::fs::write(&corrupt, "[\"old folder\"").unwrap();
        let theirs = [
            data.join("mine.json.before-database"),
            notes.join("mine.json.corrupt"),
            notes.join("mine.md.before-database"),
        ];
        for file in &theirs {
            std::fs::write(file, "kept").unwrap();
        }
        let now = SystemTime::now();
        assert_eq!(expire_backups(data, &notes, now), 0);
        assert_eq!(expire_backups(data, &notes, now + DAY * 29), 0);
        assert!(captions.is_file() && meaning.is_file() && graph.is_file() && corrupt.is_file());
        assert_eq!(expire_backups(data, &notes, now + DAY * 31), 4);
        assert!(!captions.exists() && !meaning.exists() && !graph.exists() && !corrupt.exists());
        assert!(theirs.iter().all(|file| file.is_file()));
        assert!(data.join("leo.db").is_file());
    }

    #[test]
    fn a_backup_whose_content_is_not_proven_stays() {
        let (tmp, notes) = data();
        let data = tmp.path();
        let captions = set_aside_captions(data);
        let graph = data.join("graph.json.before-database");
        std::fs::write(&graph, "{}").unwrap();
        let damaged = data.join("leo.db.damaged-20260101T000000Z");
        std::fs::write(&damaged, "damaged").unwrap();
        let corrupt = notes.join("directories.json.corrupt");
        std::fs::write(&corrupt, "older").unwrap();
        std::fs::write(notes.join("directories.json"), "{not json").unwrap();
        let later = SystemTime::now() + DAY * 31;
        assert_eq!(expire_backups(data, &notes, later), 0);
        assert!(captions.is_file() && graph.is_file() && corrupt.is_file());
        std::fs::remove_file(&damaged).unwrap();
        std::fs::write(notes.join("directories.json"), "[]").unwrap();
        assert_eq!(expire_backups(data, &notes, later), 2);
        assert!(!captions.exists() && !corrupt.exists());
        assert!(graph.is_file(), "an empty table proves nothing");
    }

    #[test]
    fn backups_stay_when_the_database_is_gone() {
        let (tmp, notes) = data();
        let data = tmp.path();
        let captions = data.join("captions.json.before-database");
        std::fs::write(&captions, r#"{"k": "a heap drawing"}"#).unwrap();
        assert_eq!(
            expire_backups(data, &notes, SystemTime::now() + DAY * 31),
            0
        );
        assert!(captions.is_file());
        assert!(!data.join("leo.db").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_backup_renamed_lately_is_dated_by_its_change_time() {
        let (tmp, notes) = data();
        let data = tmp.path();
        let captions = set_aside_captions(data);
        age(&captions, 400, 0);
        let corrupt = notes.join("directories.json.corrupt");
        std::fs::write(&corrupt, "older").unwrap();
        age(&corrupt, 400, 0);
        assert_eq!(expire_backups(data, &notes, SystemTime::now()), 0);
        assert!(captions.is_file() && corrupt.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_backup_is_never_followed() {
        let (tmp, notes) = data();
        let data = tmp.path();
        let captions = set_aside_captions(data);
        std::fs::remove_file(&captions).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (one, two) = (outside.path().join("one"), outside.path().join("two"));
        std::fs::write(&one, "theirs").unwrap();
        std::fs::write(&two, "theirs").unwrap();
        std::os::unix::fs::symlink(&one, &captions).unwrap();
        let corrupt = notes.join("directories.json.corrupt");
        std::os::unix::fs::symlink(&two, &corrupt).unwrap();
        assert_eq!(
            expire_backups(data, &notes, SystemTime::now() + DAY * 31),
            0
        );
        assert!(captions.is_symlink() && corrupt.is_symlink());
        assert!(one.is_file() && two.is_file());
    }

    #[test]
    fn a_transcript_older_than_the_period_goes_and_a_newer_one_stays() {
        let (_tmp, notes) = data();
        let old = transcript(&notes, "note-one-aaa", 31, 0);
        let new = transcript(&notes, "note-two-bbb", 29, 0);
        assert_eq!(expire_transcripts(&notes, Some(30), Utc::now(), None), 1);
        assert!(!old.exists() && new.exists());
    }

    #[test]
    fn a_transcript_at_the_boundary_is_judged_by_the_second() {
        let (_tmp, notes) = data();
        let inside = transcript(&notes, "note-one-aaa", 29, 86_000);
        let outside = transcript(&notes, "note-two-bbb", 30, 120);
        assert_eq!(expire_transcripts(&notes, Some(30), Utc::now(), None), 1);
        assert!(inside.exists() && !outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_transcript_is_not_followed() {
        let (tmp, notes) = data();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.json"), "{}").unwrap();
        age(&outside, 400, 0);
        let root = leo_core::recording::root(&notes);
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("note-one-link")).unwrap();
        assert_eq!(expire_transcripts(&notes, Some(7), Utc::now(), None), 0);
        assert!(outside.join("keep.json").exists());
    }

    #[derive(Default)]
    struct Fake {
        pending: Vec<Pending>,
        discarded: Mutex<Vec<String>>,
    }

    impl Housekeeper for Fake {
        fn pending_recordings(&self) -> anyhow::Result<Vec<Pending>> {
            Ok(self.pending.clone())
        }
        fn discard_recording(&self, id: &str) -> anyhow::Result<()> {
            self.discarded.lock().unwrap().push(id.to_string());
            Ok(())
        }
    }

    fn pending(id: &str, days: Option<i64>, now: DateTime<Utc>) -> Pending {
        Pending {
            id: id.into(),
            title: None,
            directory: String::new(),
            source: Source::Microphone,
            profile: Default::default(),
            started: days.map(|d| now - chrono::Duration::days(d) - chrono::Duration::seconds(5)),
            note: None,
        }
    }

    #[test]
    fn unfinished_audio_follows_its_age() {
        let now = Utc::now();
        let fake = Fake {
            pending: vec![
                pending("old-recording", Some(31), now),
                pending("new-recording", Some(29), now),
                pending("unknown-age", None, now),
            ],
            ..Default::default()
        };
        assert_eq!(
            expire_audio(&fake, None, now, &Default::default(), &Default::default()),
            0
        );
        assert!(fake.discarded.lock().unwrap().is_empty());
        assert_eq!(
            expire_audio(
                &fake,
                Some(30),
                now,
                &Default::default(),
                &Default::default()
            ),
            1
        );
        assert_eq!(*fake.discarded.lock().unwrap(), ["old-recording"]);
    }

    #[test]
    fn expiry_reaches_every_old_recording_in_one_pass() {
        struct Many {
            old: Vec<Pending>,
            batches: Mutex<Vec<usize>>,
        }
        impl Housekeeper for Many {
            fn old_recordings(&self, before: DateTime<Utc>) -> anyhow::Result<Vec<Pending>> {
                Ok(self
                    .old
                    .iter()
                    .filter(|p| p.started.is_some_and(|s| s < before))
                    .cloned()
                    .collect())
            }
            fn discard_recordings(&self, ids: &[String]) -> usize {
                self.batches.lock().unwrap().push(ids.len());
                ids.len()
            }
        }
        let now = Utc::now();
        let many = Many {
            old: (0..600)
                .map(|n| pending(&format!("old-{n}"), Some(40), now))
                .collect(),
            batches: Default::default(),
        };
        let cutoff = now - chrono::Duration::days(30);
        assert_eq!(many.old_recordings(cutoff).unwrap().len(), 600);
        assert_eq!(
            expire_audio(
                &many,
                Some(30),
                now,
                &Default::default(),
                &Default::default()
            ),
            600
        );
        assert_eq!(*many.batches.lock().unwrap(), [256, 256, 88]);
    }

    #[test]
    fn the_audio_preview_adds_kept_audio() {
        struct Kept(Vec<Pending>);
        impl Housekeeper for Kept {
            fn pending_recordings(&self) -> anyhow::Result<Vec<Pending>> {
                Ok(self.0.clone())
            }
            fn kept_audio_count(&self, _before: DateTime<Utc>) -> usize {
                3
            }
        }
        let now = Utc::now();
        let kept = Kept(vec![
            pending("old-recording", Some(40), now),
            pending("in-use", Some(40), now),
            pending("new-recording", Some(2), now),
        ]);
        let before = now - chrono::Duration::days(30);
        assert_eq!(audio_count(&kept, before, Some("in-use")), 4);
    }

    #[test]
    fn a_failed_discard_does_not_stop_the_rest() {
        struct Stubborn(Mutex<Vec<String>>);
        impl Housekeeper for Stubborn {
            fn pending_recordings(&self) -> anyhow::Result<Vec<Pending>> {
                let now = Utc::now();
                Ok(vec![
                    pending("first-recording", Some(60), now),
                    pending("second-recording", Some(60), now),
                ])
            }
            fn discard_recording(&self, id: &str) -> anyhow::Result<()> {
                self.0.lock().unwrap().push(id.to_string());
                anyhow::ensure!(id != "first-recording", "locked");
                Ok(())
            }
        }
        let stubborn = Stubborn(Mutex::new(Vec::new()));
        assert_eq!(
            expire_audio(
                &stubborn,
                Some(30),
                Utc::now(),
                &Default::default(),
                &Default::default()
            ),
            1
        );
        assert_eq!(stubborn.0.lock().unwrap().len(), 2);
    }
}
