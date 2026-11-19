use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use leo_core::store::Store;
use serde::{Deserialize, Serialize};

use crate::AppState;

const PENDING: &str = "deleting";
const HOLDING: &str = "holding";
const HELD: &str = "held";
const KEPT_FILES: &str = "kept-files";
const QUEUED: &str = "deleting-shown";

#[derive(Clone, Default, Serialize, Deserialize)]
struct Queued {
    #[serde(default)]
    folders: BTreeMap<String, leo_core::paths::FileDigest>,
    #[serde(default)]
    held: Option<leo_core::paths::FileDigest>,
}
use leo_core::paths::own::{HELD as HELD_DIR, ORIGINALS, SOURCES};

const MOST_PICTURE_CHANGES: usize = 256;
const HELD_FOLDER_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    Asked,
    Expired,
    Swept,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Held {
    pub(crate) entered: i64,
    pub(crate) at: DateTime<Utc>,
    #[serde(default)]
    owner: Option<DateTime<Utc>>,
    #[serde(default)]
    owner_digest: Option<leo_core::paths::FileDigest>,
    #[serde(default)]
    files: Option<Vec<DirMove>>,
    #[serde(default)]
    sessions: Vec<crate::record::KeptSession>,
    #[serde(default)]
    pending: bool,
    #[serde(default)]
    protected: bool,
    #[serde(default)]
    swept: bool,
    #[serde(default)]
    row_ticket: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DirMove {
    kind: String,
    identity: DirIdentity,
    #[serde(default)]
    digest: Option<leo_core::paths::FileDigest>,
    #[serde(default)]
    restoring_to: Option<String>,
    #[serde(default)]
    returned: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DirIdentity {
    modified: std::time::SystemTime,
    #[cfg(unix)]
    inode: (u64, u64),
    #[cfg(not(unix))]
    created: std::time::SystemTime,
}

pub(crate) struct Owners {
    held: HashMap<String, HashSet<DateTime<Utc>>>,
    sure: bool,
}

impl Owners {
    pub(crate) fn of(store: &Store) -> Owners {
        let trash = store.trash_snapshot();
        let sure = store.unreadable.is_empty()
            && store.trash_complete()
            && trash.iter().all(|t| t.head.is_some());
        let mut held: HashMap<_, HashSet<_>> = HashMap::new();
        for note in &store.notes {
            held.entry(note.id.to_ascii_lowercase())
                .or_default()
                .insert(note.created_at);
        }
        for head in trash.into_iter().filter_map(|t| t.head) {
            held.entry(head.id.to_ascii_lowercase())
                .or_default()
                .extend(head.created_at);
        }
        Owners { held, sure }
    }

    fn owns(&self, id: &str) -> bool {
        self.held.contains_key(&id.to_ascii_lowercase())
    }

    fn same(&self, id: &str, owner: Option<DateTime<Utc>>) -> bool {
        self.held
            .get(&id.to_ascii_lowercase())
            .is_some_and(|dates| {
                dates.len() == 1 && owner.is_some_and(|owner| dates.contains(&owner))
            })
    }
}

fn refused() -> std::io::Error {
    std::io::Error::other("leo could not verify the file or its folder")
}

fn dir_identity(folder: &Path) -> std::io::Result<Option<DirIdentity>> {
    let meta = match std::fs::symlink_metadata(folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        meta => meta?,
    };
    if !meta.is_dir() {
        return Err(refused());
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(Some(DirIdentity {
        modified: meta.modified()?,
        #[cfg(unix)]
        inode: (meta.dev(), meta.ino()),
        #[cfg(not(unix))]
        created: meta.created()?,
    }))
}

fn locations(notes: &Path, kind: &str, id: &str) -> std::io::Result<(PathBuf, PathBuf)> {
    if ![ORIGINALS, SOURCES].contains(&kind) || !leo_core::recording::valid_id(id) {
        return Err(refused());
    }
    let data = notes.parent().unwrap_or(notes);
    if std::fs::symlink_metadata(data).is_ok_and(|meta| !meta.is_dir()) {
        return Err(refused());
    }
    let relative = Path::new(kind);
    let safe =
        |path: &Path| leo_core::paths::contained_path(data, path).map_err(std::io::Error::other);
    Ok((
        safe(relative)?.join(id),
        safe(&Path::new(HELD_DIR).join(relative))?.join(id),
    ))
}

fn recording(state: &AppState) -> bool {
    state.capture_activity.busy() || crate::record::live(&state.recording)
}

pub(crate) fn held_rows(chats: &Path) -> BTreeMap<String, Held> {
    crate::margin::state_of(chats, HELD).unwrap_or_default()
}

fn free_home(home: &Path, substitute: bool) -> std::io::Result<PathBuf> {
    const MOST_NAMES: usize = 100_000;
    let parent = home.parent().ok_or_else(refused)?;
    std::fs::create_dir_all(parent)?;
    let mut names = HashSet::new();
    for (n, entry) in std::fs::read_dir(parent)?.enumerate() {
        if n >= MOST_NAMES {
            return Err(refused());
        }
        names.insert(entry?.file_name().to_string_lossy().to_ascii_lowercase());
    }
    let name = home
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(refused)?;
    if !substitute && !names.contains(&name.to_ascii_lowercase()) {
        return Ok(home.to_path_buf());
    }
    let other = format!("{name}-restored-{}", uuid::Uuid::new_v4());
    if names.contains(&other.to_ascii_lowercase()) {
        return Err(refused());
    }
    Ok(parent.join(other))
}

type Hashed = HashMap<PathBuf, (leo_core::paths::FileDigest, leo_core::paths::FolderStamps)>;

fn hashed_now<'a>(
    hashed: &'a Hashed,
    folder: &Path,
    before: &Path,
) -> Option<&'a leo_core::paths::FileDigest> {
    let (digest, stamps) = hashed.get(folder).or_else(|| hashed.get(before))?;
    (leo_core::paths::folder_stamps(folder, &[]).ok().as_ref() == Some(stamps)).then_some(digest)
}

fn held_folders(
    notes: &Path,
    id: &str,
    from_home: bool,
    hashed: &Hashed,
) -> std::io::Result<Vec<DirMove>> {
    let mut files = Vec::new();
    for kind in [ORIGINALS, SOURCES] {
        let (home, held) = locations(notes, kind, id)?;
        let path = if from_home { &home } else { &held };
        if let Some(identity) = dir_identity(path)? {
            let digest = if from_home {
                Some(*hashed_now(hashed, path, path).ok_or_else(refused)?)
            } else {
                None
            };
            files.push(DirMove {
                kind: kind.to_string(),
                identity,
                digest,
                restoring_to: None,
                returned: false,
            });
        }
    }
    Ok(files)
}

fn same_folder(folder: &Path, file: &DirMove, hashed: &Hashed, before: &Path) -> bool {
    dir_identity(folder).is_ok_and(|found| found.as_ref() == Some(&file.identity))
        && file
            .digest
            .is_some_and(|expected| hashed_now(hashed, folder, before) == Some(&expected))
}

fn hash_into(hashed: &mut Hashed, folder: PathBuf) {
    if hashed.contains_key(&folder) {
        return;
    }
    if let Ok(found) = leo_core::paths::digest_directory(&folder, HELD_FOLDER_BYTES, &[]) {
        hashed.insert(folder, found);
    }
}

fn hold_files(notes: &Path, id: &str, row: &Held, hashed: &Hashed) -> std::io::Result<()> {
    let mut failed = None;
    for file in row.files.as_deref().ok_or_else(refused)? {
        let moved = (|| {
            let (home, held) = locations(notes, &file.kind, id)?;
            if dir_identity(&held)?.as_ref() == Some(&file.identity) {
                return Ok(());
            }
            std::fs::create_dir_all(held.parent().ok_or_else(refused)?)?;
            if dir_identity(&held)?.is_some()
                || dir_identity(&home)?.as_ref() != Some(&file.identity)
                || std::fs::symlink_metadata(&home)?.permissions().readonly()
                || !same_folder(&home, file, hashed, &home)
            {
                return Err(refused());
            }
            leo_core::paths::move_new(&home, &held)
        })();
        if let Err(error) = moved {
            failed = Some(error);
        }
    }
    failed.map_or(Ok(()), Err)
}

fn restore_dir(
    state: &AppState,
    id: &str,
    held: &mut BTreeMap<String, Held>,
    at: usize,
    substitute: bool,
) -> std::io::Result<()> {
    let file = &held[id].files.as_ref().unwrap()[at];
    if file.returned {
        return Ok(());
    }
    let identity = file.identity.clone();
    let (home, bin) = locations(&state.notes_dir, &file.kind, id)?;
    let in_bin = dir_identity(&bin)?;
    let planned = file
        .restoring_to
        .as_deref()
        .filter(|name| {
            *name == id
                || name
                    .strip_prefix(&format!("{id}-restored-"))
                    .is_some_and(|suffix| uuid::Uuid::parse_str(suffix).is_ok())
        })
        .map(|name| home.with_file_name(name));
    if let Some(found) = &in_bin {
        if found != &identity {
            return Err(refused());
        }
        let to = free_home(&home, substitute)?;
        held.get_mut(id).unwrap().files.as_mut().unwrap()[at].restoring_to = to
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        if !crate::margin::set_state(&state.chats, HELD, held) {
            return Err(refused());
        }
        if dir_identity(&bin)?.as_ref() != Some(&identity) || dir_identity(&to)?.is_some() {
            return Err(refused());
        }
        leo_core::paths::move_new(&bin, &to)?;
    } else if dir_identity(planned.as_ref().unwrap_or(&home))?.as_ref() != Some(&identity) {
        return Err(refused());
    }
    held.get_mut(id).unwrap().files.as_mut().unwrap()[at].returned = true;
    Ok(())
}

fn give_back(
    state: &AppState,
    id: &str,
    held: &mut BTreeMap<String, Held>,
    manual: bool,
    substitute: bool,
) -> bool {
    if manual && held[id].files.is_none() {
        let Ok(files) = held_folders(&state.notes_dir, id, false, &HashMap::new()) else {
            return false;
        };
        held.get_mut(id).unwrap().files = Some(files);
    }
    let Some(count) = held[id].files.as_ref().map(Vec::len) else {
        return false;
    };
    let mut complete = true;
    for i in 0..count {
        complete &= restore_dir(state, id, held, i, substitute).is_ok();
    }
    crate::margin::set_state(&state.chats, HELD, held) && complete
}

pub(crate) fn recover_holds(state: &AppState) {
    const MOST_HELD_FOLDERS: usize = 100_000;
    let _clearing = state.clearing.lock().unwrap_or_else(|e| e.into_inner());
    let mut held = held_rows(&state.chats);
    let before = held.len();
    let data = state.notes_dir.parent().unwrap_or(&state.notes_dir);
    for kind in [ORIGINALS, SOURCES] {
        let Ok(root) = leo_core::paths::contained_path(data, &Path::new(HELD_DIR).join(kind))
        else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.take(MOST_HELD_FOLDERS).flatten() {
            let Some(id) = entry
                .file_name()
                .to_str()
                .filter(|id| leo_core::recording::valid_id(id))
                .map(str::to_string)
            else {
                continue;
            };
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                held.entry(id).or_insert_with(|| Held {
                    entered: 0,
                    at: entry
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .map_or_else(|_| Utc::now(), DateTime::from),
                    owner: None,
                    owner_digest: None,
                    files: None,
                    sessions: Vec::new(),
                    pending: false,
                    protected: true,
                    swept: false,
                    row_ticket: None,
                });
            }
        }
    }
    if before != held.len() {
        crate::margin::set_state(&state.chats, HELD, &held);
    }
}

pub(crate) fn keep_held(state: &AppState, id: &str) -> bool {
    let _clearing = state.clearing.lock().unwrap_or_else(|e| e.into_inner());
    let mut held = held_rows(&state.chats);
    let Some(row) = held.get(id) else {
        return false;
    };
    let substitute = crate::store_now(state, |store| {
        let owners = Owners::of(store);
        Ok(!owners.sure || (owners.owns(id) && !owners.same(id, row.owner)))
    })
    .unwrap_or(true);
    if !give_back(state, id, &mut held, true, substitute) {
        return false;
    }
    let mut kept: BTreeSet<String> =
        crate::margin::state_of(&state.chats, KEPT_FILES).unwrap_or_default();
    kept.insert(id.to_string());
    if !crate::margin::set_state(&state.chats, KEPT_FILES, &kept) {
        return false;
    }
    if let Some(ticket) = &held[id].row_ticket {
        if !crate::margin::finish_held_rows(&state.chats, id, ticket, false) {
            return false;
        }
    }
    held.remove(id);
    crate::margin::set_state(&state.chats, HELD, &held)
}

pub(crate) fn held_revision(row: &Held) -> leo_core::paths::FileDigest {
    let files: Option<Vec<_>> = row
        .files
        .as_ref()
        .map(|files| files.iter().map(|file| (&file.kind, file.digest)).collect());
    let sessions: Vec<_> = row.sessions.iter().map(|session| session.digest).collect();
    let recorded = (files, sessions, row.owner, row.owner_digest);
    leo_core::paths::digest(&serde_json::to_vec(&recorded).unwrap_or_default())
}

pub(crate) fn clear(state: &AppState, ids: Vec<String>, why: Why) {
    clear_shown(state, ids, why, None);
}

pub(crate) fn clear_shown(
    state: &AppState,
    ids: Vec<String>,
    why: Why,
    shown: Option<&HashMap<String, leo_core::paths::FileDigest>>,
) {
    let (chats, notes) = (&state.chats, &state.notes_dir);
    let _clearing = state.clearing.lock().unwrap_or_else(|e| e.into_inner());
    crate::routes::combine::retry_retargets(state);
    let mut asked: BTreeSet<String> = crate::margin::state_of(chats, PENDING).unwrap_or_default();
    let mut holding: BTreeMap<String, bool> =
        crate::margin::state_of(chats, HOLDING).unwrap_or_default();
    let mut held = held_rows(chats);
    let kept: BTreeSet<String> = crate::margin::state_of(chats, KEPT_FILES).unwrap_or_default();
    let mut hashed = Hashed::new();
    let mut queued: BTreeMap<String, Queued> =
        crate::margin::state_of(chats, QUEUED).unwrap_or_default();
    match why {
        Why::Asked => {
            let newly: Vec<String> = ids
                .into_iter()
                .filter(|id| {
                    shown.is_none_or(|shown| {
                        held.get(id).is_none_or(|row| {
                            shown.get(&format!("held:{id}")) == Some(&held_revision(row))
                        })
                    })
                })
                .collect();
            for id in &newly {
                if queued.contains_key(id) {
                    continue;
                }
                let mut seen = Queued::default();
                match held.get(id) {
                    Some(row) => seen.held = Some(held_revision(row)),
                    None => {
                        for kind in [ORIGINALS, SOURCES] {
                            let Ok((home, _)) = locations(notes, kind, id) else {
                                continue;
                            };
                            hash_into(&mut hashed, home.clone());
                            if let Some((digest, _)) = hashed.get(&home) {
                                seen.folders.insert(kind.to_string(), *digest);
                            }
                        }
                    }
                }
                queued.insert(id.clone(), seen);
            }
            asked.extend(newly);
            queued.retain(|id, _| asked.contains(id));
            crate::margin::set_state(chats, QUEUED, &queued);
        }
        Why::Expired => holding.extend(
            ids.into_iter()
                .chain(crate::lock_storage(&state.store).expired_notes())
                .map(|id| (id, false)),
        ),
        Why::Swept => holding.extend(ids.into_iter().map(|id| (id, true))),
    }
    let awaiting: HashSet<String> = asked
        .iter()
        .chain(holding.keys())
        .filter(|id| !held.contains_key(*id) && !kept.contains(*id))
        .cloned()
        .collect();
    const MOST_HOLDS: usize = 32;
    let before_lock = crate::lock_storage(&state.store).sample_clock();
    let automatic: Vec<String> = held
        .keys()
        .chain(holding.keys())
        .filter(|id| !asked.contains(*id))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let start = usize::try_from(before_lock.unsigned_abs()).unwrap_or(0) % automatic.len().max(1);
    let work: Vec<String> = asked
        .iter()
        .cloned()
        .chain(
            automatic[start..]
                .iter()
                .chain(&automatic[..start])
                .take(MOST_HOLDS)
                .cloned(),
        )
        .collect();
    let awaiting: HashSet<String> = work
        .iter()
        .filter(|id| awaiting.contains(*id))
        .cloned()
        .collect();
    let inventory = if awaiting.is_empty() || recording(state) {
        Ok(HashMap::new())
    } else {
        state.housekeeper.as_ref().map_or_else(
            || Ok(awaiting.iter().map(|id| (id.clone(), Vec::new())).collect()),
            |h| h.kept_sessions_for(&awaiting),
        )
    };
    let period = leo_core::store::trash_days_now(notes)
        .flatten()
        .map(|days| i64::from(days) * 86_400);
    let mut audio_checked: HashMap<String, Vec<Option<leo_core::paths::FolderStamps>>> =
        HashMap::new();
    let mut check_audio = |id: &str, sessions: &[crate::record::KeptSession]| {
        let checked = match &state.housekeeper {
            Some(h) => h.verify_held_audio(id, sessions),
            None => Ok(vec![None; sessions.len()]),
        };
        if let Ok(checked) = checked {
            audio_checked.insert(id.to_string(), checked);
        }
    };
    if !recording(state) {
        for id in work.iter().filter(|id| awaiting.contains(*id)) {
            if recording(state) {
                break;
            }
            for kind in [ORIGINALS, SOURCES] {
                if let Ok((home, _)) = locations(notes, kind, id) {
                    hash_into(&mut hashed, home);
                }
            }
            if let Some(sessions) = inventory
                .as_ref()
                .ok()
                .and_then(|found| found.get(id))
                .filter(|_| asked.contains(id))
            {
                check_audio(id, sessions);
            }
        }
        let due = |id: &str, row: &Held| {
            asked.contains(id)
                || period.is_some_and(|period| before_lock.saturating_sub(row.entered) >= period)
        };
        for (id, row) in work
            .iter()
            .filter_map(|id| Some((id, held.get(id)?)))
            .filter(|(id, row)| row.pending || (!row.protected && due(id, row)))
        {
            if recording(state) {
                break;
            }
            for file in row.files.iter().flatten() {
                if let Ok((home, bin)) = locations(notes, &file.kind, id) {
                    hash_into(&mut hashed, if row.pending { home } else { bin });
                }
            }
            if !row.protected && due(id, row) {
                check_audio(id, &row.sessions);
            }
        }
    }
    let _ = crate::store_now(state, |store| {
        let owners = Owners::of(store);
        store.observe_generations();
        asked.retain(|id| !owners.owns(id) && !kept.contains(id));
        holding.retain(|id, _| !kept.contains(id));
        if !crate::margin::set_state(chats, PENDING, &asked)
            || !crate::margin::set_state(chats, HOLDING, &holding)
            || recording(state)
            || !owners.sure
        {
            return Ok(());
        }
        let Some(_audio) = crate::routes::combine::try_audio(state) else {
            return Ok(());
        };
        let waiting = crate::routes::combine::pending_retargets(chats);
        let confirmed =
            if holding.values().any(|&swept| swept) || held.values().any(|row| row.swept) {
                store.confirmed_gone()
            } else {
                HashSet::new()
            };
        let clock = store.sample_clock();
        let trashed = store.trashed_ids();
        let unchanged = std::cell::OnceCell::new();
        let unchanged = |store: &Store| {
            *unchanged.get_or_init(|| {
                #[cfg(test)]
                before_removing(state);
                !store.changed_on_disk() && store.trashed_ids() == trashed
            })
        };
        let blocked: HashSet<_> = waiting
            .iter()
            .flat_map(|(from, target)| [from.as_str(), target.as_str()])
            .collect();
        let mut cleared = Vec::new();
        for id in work {
            if recording(state) {
                break;
            }
            if why == Why::Asked && !asked.contains(&id) {
                continue;
            }
            if kept.contains(&id) {
                held.remove(&id);
                continue;
            }
            if owners.owns(&id) {
                if let Some(row) = held.get_mut(&id) {
                    let already_protected = row.protected;
                    row.protected = true;
                    if !already_protected && !crate::margin::set_state(chats, HELD, &held) {
                        continue;
                    }
                    if owners.same(&id, held[&id].owner)
                        && give_back(state, &id, &mut held, false, false)
                    {
                        if let Some(ticket) = &held[&id].row_ticket {
                            crate::margin::finish_held_rows(chats, &id, ticket, false);
                        }
                        held.remove(&id);
                        holding.remove(&id);
                        cleared.push(id);
                    }
                } else if store.find_note(&id).is_some() {
                    holding.remove(&id);
                    cleared.push(id);
                }
                continue;
            }
            if blocked.contains(id.as_str())
                || held
                    .get(&id)
                    .is_some_and(|row| row.swept && !confirmed.contains(&id))
            {
                continue;
            }
            if !held.contains_key(&id) {
                if holding.get(&id) == Some(&true) && !confirmed.contains(&id) {
                    holding.remove(&id);
                    continue;
                }
                let Some((owner, owner_digest)) = store.removed_note_generation(&id) else {
                    continue;
                };
                let Ok(files) = held_folders(notes, &id, true, &hashed) else {
                    continue;
                };
                let Some(sessions) = inventory
                    .as_ref()
                    .ok()
                    .and_then(|sessions| sessions.get(&id))
                else {
                    continue;
                };
                let Ok(ticket) = crate::margin::record_held_rows(chats, &id) else {
                    continue;
                };
                held.insert(
                    id.clone(),
                    Held {
                        entered: clock,
                        at: Utc::now(),
                        owner: Some(owner),
                        owner_digest: Some(owner_digest),
                        files: Some(files),
                        sessions: sessions.clone(),
                        pending: true,
                        protected: false,
                        swept: holding.get(&id) == Some(&true),
                        row_ticket: Some(ticket),
                    },
                );
                if !crate::margin::set_state(chats, HELD, &held) {
                    held.remove(&id);
                    continue;
                }
            }
            let row = held.get_mut(&id).unwrap();
            if row.pending && !row.protected {
                if !unchanged(store) || hold_files(notes, &id, row, &hashed).is_err() {
                    continue;
                }
                row.entered = store.sample_clock();
                row.at = Utc::now();
                row.pending = false;
                if !crate::margin::set_state(chats, HELD, &held) {
                    continue;
                }
            }
            holding.remove(&id);
            let row = &held[&id];
            if row.protected
                || row.pending
                || row
                    .owner
                    .zip(row.owner_digest)
                    .is_none_or(|generation| store.recorded_generation(&id) != Some(generation))
                || !(asked.contains(&id)
                    || period.is_some_and(|period| clock.saturating_sub(row.entered) >= period))
            {
                continue;
            }
            let urgent = asked.contains(&id)
                && !period.is_some_and(|period| clock.saturating_sub(row.entered) >= period);
            if urgent && queued.get(&id).is_some_and(|seen| seen.held.is_some()) {
                if queued[&id].held != Some(held_revision(row)) {
                    asked.remove(&id);
                    continue;
                }
            } else if urgent {
                let shown = queued.get(&id).map(|seen| &seen.folders);
                let matches = |file: &DirMove| {
                    file.digest.is_some()
                        && shown.and_then(|shown| shown.get(&file.kind)).copied() == file.digest
                };
                if row.files.iter().flatten().any(|file| !matches(file)) {
                    let mut rest = Vec::new();
                    for file in row.files.iter().flatten() {
                        let Ok((home, bin)) = locations(notes, &file.kind, &id) else {
                            rest.push(file.clone());
                            continue;
                        };
                        let gone = matches(file)
                            && !file.returned
                            && unchanged(store)
                            && same_folder(&bin, file, &hashed, &home)
                            && std::fs::remove_dir_all(&bin).is_ok();
                        if !gone {
                            rest.push(file.clone());
                        }
                    }
                    let row = held.get_mut(&id).unwrap();
                    row.files = Some(rest);
                    row.entered = clock;
                    row.at = Utc::now();
                    asked.remove(&id);
                    crate::margin::set_state(chats, HELD, &held);
                    continue;
                }
            }
            let Some(files) = &row.files else { continue };
            let mut verified = Vec::new();
            for file in files {
                let Ok((home, bin)) = locations(notes, &file.kind, &id) else {
                    break;
                };
                if file.returned {
                    break;
                }
                match dir_identity(&bin) {
                    Ok(None) => verified.push(None),
                    Ok(Some(_)) if same_folder(&bin, file, &hashed, &home) => {
                        verified.push(Some((bin, file, home)))
                    }
                    _ => break,
                }
            }
            let Some(audio) = audio_checked.get(&id) else {
                continue;
            };
            if verified.len() != files.len() || !unchanged(store) {
                continue;
            }
            if state
                .housekeeper
                .as_ref()
                .is_some_and(|h| h.discard_held_audio(&id, &row.sessions, audio).is_err())
            {
                continue;
            }
            let removed = verified.into_iter().flatten().all(|(dir, file, home)| {
                same_folder(&dir, file, &hashed, &home) && std::fs::remove_dir_all(dir).is_ok()
            });
            if removed
                && row
                    .row_ticket
                    .as_ref()
                    .is_none_or(|ticket| crate::margin::finish_held_rows(chats, &id, ticket, true))
            {
                if let Some(folder) = crate::routes::uploads::originals_dir(notes, &id) {
                    state.originals.forget_note(&folder);
                }
                asked.remove(&id);
                held.remove(&id);
                cleared.push(id);
            }
        }
        queued.retain(|id, _| asked.contains(id));
        crate::margin::set_state(chats, QUEUED, &queued);
        if crate::margin::set_state(chats, PENDING, &asked)
            && crate::margin::set_state(chats, HOLDING, &holding)
            && store.forget_departed(&cleared)
        {
            crate::margin::set_state(chats, HELD, &held);
        }
        Ok(())
    });
}

fn own_pictures(
    folder: &Path,
    made: &leo_core::attachments::Made,
) -> Option<(HashMap<String, PathBuf>, HashMap<String, PathBuf>)> {
    let entries = match std::fs::read_dir(folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Some((HashMap::new(), HashMap::new()))
        }
        entries => entries.ok()?,
    };
    let (mut recorded, mut older) = (HashMap::new(), HashMap::new());
    for entry in entries {
        let entry = entry.ok()?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !leo_core::attachments::own_name(&name) || !entry.file_type().ok()?.is_file() {
            continue;
        }
        let meta = entry.metadata().ok()?;
        let into = if leo_core::attachments::still_made(made, &name, &meta) {
            &mut recorded
        } else {
            &mut older
        };
        into.insert(name.to_ascii_lowercase(), entry.path());
    }
    Some((recorded, older))
}

type Stamp = (u64, u128, i64);

fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        meta.ctime()
            .saturating_mul(1_000_000_000)
            .saturating_add(meta.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = 0;
    (meta.len(), modified, changed)
}

const NOT_TEXT: [&str; 12] = [
    "png", "jpg", "jpeg", "gif", "webp", "wav", "mp3", "m4a", "pdf", "docx", "pptx", "zip",
];

const BOOKKEEPING: [&str; 3] = ["pictures.json", "trash.json", leo_core::attachments::MADE];

struct Scan {
    roots: Vec<PathBuf>,
    leo: PathBuf,
    stamps: HashMap<PathBuf, Option<Stamp>>,
    found: HashSet<String>,
}

impl Scan {
    fn new(notes: &Path) -> Scan {
        let data = notes.parent().unwrap_or(notes);
        Scan {
            roots: vec![
                notes.to_path_buf(),
                data.join("recordings"),
                data.join("recording-sources"),
                data.join("held/recording-sources"),
            ],
            leo: notes.join(".leo"),
            stamps: HashMap::new(),
            found: HashSet::new(),
        }
    }

    fn skipped(&self, path: &Path) -> bool {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let bookkeeping = path.parent() == Some(self.leo.as_path())
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| BOOKKEEPING.contains(&n));
        bookkeeping || extension.is_some_and(|e| NOT_TEXT.contains(&e.as_str()))
    }

    fn run(&mut self) -> Option<()> {
        const MOST_SCAN_ENTRIES: usize = 1_000_000;
        const MOST_SCAN_DEPTH: usize = 64;
        let mut entries_seen = 0;
        let mut stack: Vec<(PathBuf, usize)> =
            self.roots.iter().map(|root| (root.clone(), 0)).collect();
        while let Some((dir, depth)) = stack.pop() {
            if depth > MOST_SCAN_DEPTH {
                return None;
            }
            match std::fs::symlink_metadata(&dir) {
                Ok(meta) if meta.is_dir() => {
                    self.stamps.insert(dir.clone(), Some(stamp_of(&meta)));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && depth == 0 => {
                    self.stamps.insert(dir, None);
                    continue;
                }
                _ => return None,
            }
            for entry in std::fs::read_dir(dir).ok()? {
                entries_seen += 1;
                if entries_seen > MOST_SCAN_ENTRIES {
                    return None;
                }
                let entry = entry.ok()?;
                let kind = entry.file_type().ok()?;
                let path = entry.path();
                if kind.is_symlink() {
                    return None;
                }
                if kind.is_dir() {
                    stack.push((path, depth + 1));
                } else if !kind.is_file() {
                    return None;
                } else if !self.skipped(&path) {
                    let stamp = stamp_of(&entry.metadata().ok()?);
                    self.read(&path, stamp)?;
                    self.stamps.insert(path, Some(stamp));
                }
            }
        }
        Some(())
    }

    fn unchanged(&self) -> bool {
        self.stamps
            .iter()
            .all(|(path, expected)| match std::fs::symlink_metadata(path) {
                Ok(meta) => !meta.file_type().is_symlink() && Some(stamp_of(&meta)) == *expected,
                Err(e) => expected.is_none() && e.kind() == std::io::ErrorKind::NotFound,
            })
    }

    fn read(&mut self, path: &Path, expected: Stamp) -> Option<()> {
        use std::io::Read;
        const MOST_TEXT_BYTES: u64 = 256 * 1024 * 1024;
        let file = leo_core::paths::open_regular(path).ok()?;
        if stamp_of(&file.metadata().ok()?) != expected || expected.0 > MOST_TEXT_BYTES {
            return None;
        }
        let mut file = file.take(MOST_TEXT_BYTES + 1);
        let mut carry: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 32 * 1024];
        loop {
            let read = file.read(&mut chunk).ok()?;
            if read == 0 {
                return (file.limit() != 0).then_some(());
            }
            carry.extend_from_slice(&chunk[..read]);
            self.found
                .extend(leo_core::attachments::mentioned(&String::from_utf8_lossy(
                    &carry,
                )));
            let keep = carry.len().saturating_sub(512);
            carry.drain(..keep);
        }
    }
}

fn db_revision(c: &rusqlite::Connection) -> rusqlite::Result<(u64, i64)> {
    let version = c.query_row("PRAGMA data_version", [], |r| r.get(0))?;
    Ok((c.total_changes(), version))
}

fn database_naming(db: &crate::db::Db) -> Option<((u64, i64), HashSet<String>)> {
    let maybe = |column: &str| {
        format!(
            "instr({column}, '%') > 0 OR {column} LIKE '%.png%' OR {column} LIKE '%.jpg%' OR {column} LIKE '%.gif%' OR {column} LIKE '%.webp%'"
        )
    };
    db.with(|c| {
        let tx = c.transaction()?;
        let revision = db_revision(&tx)?;
        let mut found = HashSet::new();
        for (table, columns) in [
            ("chat_messages", &["message"][..]),
            ("chats", &["title", "refs", "about", "memory", "messages"]),
            ("chats_text", &["title", "body"]),
            ("chat_docs", &["name", "text", "excerpt"]),
            (
                "margin_threads",
                &["messages", "quote", "prefix", "suffix", "source"],
            ),
            ("margin_instructions", &["text"]),
            ("note_tasks", &["text", "before", "after", "description"]),
            ("tasks", &["data"]),
            ("dictations", &["text", "original"]),
            ("captions", &["caption"]),
            ("reading", &["anchor"]),
            ("graph_notes", &["data"]),
            ("graph_links", &["data"]),
            ("graph_meta", &["data"]),
            ("board_undos", &["data"]),
            ("interviews", &["data"]),
            ("interview_keys", &["data"]),
            ("interview_problem_undo", &["data"]),
            ("margin_state", &["data"]),
        ] {
            for column in columns {
                let mut rows = tx.prepare(&format!(
                    "SELECT {column} FROM {table} WHERE {}",
                    maybe(column)
                ))?;
                let texts = rows.query_map([], |r| r.get::<_, String>(0))?;
                for text in texts {
                    found.extend(leo_core::attachments::mentioned(&text?));
                }
            }
        }
        tx.commit()?;
        Ok((revision, found))
    })
    .ok()
}

fn trash_period(notes: &Path) -> Option<Option<chrono::Duration>> {
    leo_core::store::trash_days_now(notes)
        .map(|days| days.map(|days| chrono::Duration::days(i64::from(days))))
}

fn remove_picture(
    folder: &Path,
    path: &Path,
    stamp: &leo_core::paths::EntryStamp,
) -> std::io::Result<()> {
    if path.parent() != Some(folder) || std::fs::symlink_metadata(folder)?.file_type().is_symlink()
    {
        return Err(refused());
    }
    if !leo_core::paths::same_stamp(path, stamp) {
        return Err(refused());
    }
    std::fs::remove_file(path)
}

#[derive(Default)]
pub(crate) struct Asked {
    pub(crate) binned: Vec<String>,
    pub(crate) unused: Vec<String>,
    pub(crate) explicit: bool,
    pub(crate) shown: HashMap<String, leo_core::paths::FileDigest>,
}

pub(crate) fn pictures(state: &AppState, now: DateTime<Utc>) {
    pictures_asked(state, now, &Asked::default());
}

pub(crate) fn pictures_asked(state: &AppState, now: DateTime<Utc>, asked: &Asked) -> usize {
    let refused_all = asked.binned.len() + asked.unused.len();
    let _one = state
        .picture_cleanup
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    #[cfg(test)]
    let started = std::time::Instant::now();
    let notes = state.notes_dir.clone();
    if std::fs::read_dir(&notes).is_err() {
        return refused_all;
    }
    let Ok(home) = leo_core::paths::contained_path(&notes, Path::new(leo_core::attachments::DIR))
    else {
        return refused_all;
    };
    let ledger = notes.join(".leo").join(leo_core::attachments::MADE);
    let ledger_at = || match std::fs::symlink_metadata(&ledger) {
        Ok(meta) if meta.is_file() => Some(Some(stamp_of(&meta))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(None),
        _ => None,
    };
    let Some(ledger_stamp) = ledger_at() else {
        return refused_all;
    };
    let Some(made) = leo_core::attachments::made(&notes) else {
        return refused_all;
    };
    let Some((recorded, older)) = own_pictures(&home, &made) else {
        return refused_all;
    };
    let Some(period) = trash_period(&notes) else {
        return refused_all;
    };
    let (owed, known_unused) = {
        let store = crate::lock_storage(&state.store);
        if asked.explicit
            && asked.binned.is_empty()
            && asked.unused.is_empty()
            && !store
                .released_pictures()
                .iter()
                .any(|name| store.released_by_user(name))
        {
            return 0;
        }
        (store.trash_reads_owed(), store.unused_files())
    };
    let older_seen: BTreeMap<String, leo_core::attachments::SeenPicture> = older
        .iter()
        .filter_map(|(name, path)| {
            leo_core::attachments::seen_now(path, known_unused.get(name))
                .map(|seen| (name.clone(), seen))
        })
        .collect();
    let read: Vec<_> = owed
        .into_iter()
        .filter_map(|owed| owed.read().map(|pictures| (owed, pictures)))
        .collect();
    let mut scan = Scan::new(&notes);
    let Ok(db) = crate::db::try_for_chats(&state.chats) else {
        return refused_all;
    };
    let Some((revision, encoded)) = database_naming(&db) else {
        return refused_all;
    };
    if scan.run().is_none() {
        return refused_all;
    }
    scan.found.extend(encoded);
    #[cfg(test)]
    let outside = started.elapsed();
    #[cfg(test)]
    {
        let mut hooks = AFTER_SCAN.lock().unwrap();
        let hook = hooks
            .iter()
            .position(|(path, _)| path == &state.notes_dir)
            .map(|at| hooks.remove(at).1);
        drop(hooks);
        if let Some(hook) = hook {
            hook(state);
        }
    }
    let _boards = crate::boards::writing(&state.chats);
    let _tasks = crate::tasks::writing(&state.chats);
    #[cfg(test)]
    let locked = std::time::Instant::now();
    let answer = crate::store_now(state, |store| {
        if !store.unreadable.is_empty()
            || !store.revalidate_trash()
            || !scan.unchanged()
            || ledger_at() != Some(ledger_stamp)
            || recording(state)
            || state
                .source_writing
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(refused_all);
        }
        store.settle_owed(read);
        let busy = state.capture_activity.busy();
        if busy {
            return Ok(refused_all);
        }
        let mut removed = Vec::new();
        let result = db
            .with(|c| {
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                if db_revision(&tx)? != revision {
                    return Ok(refused_all);
                }
                let names: Vec<String> = recorded.keys().cloned().collect();
                let mut failed = 0;
                store.picture_batch(|| {
                    let Some(mut due) = store.picture_pass(&names, now, period) else {
                        failed = refused_all;
                        return;
                    };
                    let named =
                        |name: &str| scan.found.contains(name) || store.picture_owners(name) != 0;
                    for name in store.binned_picture_names() {
                        if named(&name) {
                            store.unbin_picture(&name);
                        }
                    }
                    for name in store.released_pictures() {
                        if named(&name) {
                            store.picture_gone(&name);
                        }
                    }
                    let mut unlink = |name: &str| {
                        if recording(state) {
                            return false;
                        }
                        #[cfg(test)]
                        held_while_deleting(state);
                        if store.delete_binned_picture(name).is_ok() {
                            store.picture_gone(name);
                            removed.push(name.to_string());
                            true
                        } else {
                            false
                        }
                    };
                    for name in &asked.binned {
                        let shown = asked.shown.get(&format!("picture:{name}"));
                        if named(name)
                            || shown.is_none()
                            || shown != store.binned_digest(name).as_ref()
                            || !unlink(name)
                        {
                            failed += 1;
                        }
                    }
                    if !asked.explicit {
                        for name in store
                            .ripe_binned(period)
                            .into_iter()
                            .take(MOST_PICTURE_CHANGES)
                        {
                            if !named(&name) {
                                unlink(&name);
                            }
                        }
                    }

                    due.sort_unstable();
                    let mut moves = Vec::new();
                    for name in due
                        .into_iter()
                        .filter(|name| {
                            !named(name) && (!asked.explicit || store.released_by_user(name))
                        })
                        .take(MOST_PICTURE_CHANGES)
                    {
                        if recording(state) {
                            break;
                        }
                        let Some(path) = recorded.get(&name) else {
                            continue;
                        };
                        if !store.picture_still_due(&name, period) {
                            continue;
                        }
                        if store.released_by_user(&name) {
                            #[cfg(test)]
                            held_while_deleting(state);
                            if made
                                .get(&name)
                                .and_then(leo_core::attachments::SavedPicture::seen)
                                .and_then(|seen| leo_core::attachments::same_bytes(path, &seen))
                                .is_some_and(|stamp| remove_picture(&home, path, &stamp).is_ok())
                            {
                                store.picture_gone(&name);
                                removed.push(name);
                            }
                        } else if let Some(stamp) = made.get(&name) {
                            moves.push((name, path.clone(), *stamp));
                        }
                    }
                    let moved = if recording(state) {
                        Ok(())
                    } else {
                        store.bin_pictures(&moves)
                    };
                    if let Err(e) = moved {
                        leo_core::diag::warn(format!(
                            "could not move pictures into the trash: {e}"
                        ));
                    }
                    let mut unused: BTreeMap<String, leo_core::attachments::SeenPicture> =
                        older_seen
                            .iter()
                            .filter(|(name, _)| !named(name))
                            .map(|(name, seen)| (name.clone(), *seen))
                            .collect();
                    for name in &asked.unused {
                        let recorded = known_unused.get(name).filter(|seen| {
                            asked.shown.get(&format!("unused:{name}")) == Some(&seen.digest)
                        });
                        let gone = unused.contains_key(name)
                            && older.get(name).is_some_and(|path| {
                                recorded
                                    .and_then(|seen| leo_core::attachments::same_bytes(path, seen))
                                    .is_some_and(|stamp| {
                                        remove_picture(&home, path, &stamp).is_ok()
                                    })
                            });
                        if gone {
                            unused.remove(name);
                        } else {
                            failed += 1;
                        }
                    }
                    store.set_unused(unused);
                });
                Ok(failed)
            })
            .unwrap_or(refused_all);
        leo_core::attachments::forget_made(&notes, &removed);
        Ok(result)
    })
    .unwrap_or(refused_all);
    #[cfg(test)]
    TIMED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((state.notes_dir.clone(), (outside, locked.elapsed())));
    answer
}

#[cfg(test)]
pub(crate) type AfterScan = (PathBuf, Box<dyn FnOnce(&AppState) + Send>);

#[cfg(test)]
pub(crate) static AFTER_SCAN: Mutex<Vec<AfterScan>> = Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) type Timed = (PathBuf, (std::time::Duration, std::time::Duration));

#[cfg(test)]
pub(crate) static TIMED: Mutex<Vec<Timed>> = Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) static WATCH: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(test)]
pub(crate) static BEFORE_REMOVING: Mutex<Vec<AfterScan>> = Mutex::new(Vec::new());

#[cfg(test)]
fn before_removing(state: &AppState) {
    let hook = {
        let mut hooks = BEFORE_REMOVING.lock().unwrap();
        hooks
            .iter()
            .position(|(path, _)| path == &state.notes_dir)
            .map(|at| hooks.remove(at).1)
    };
    if let Some(hook) = hook {
        hook(state);
    }
}

#[cfg(test)]
pub(crate) static HELD_LOCKS: Mutex<Vec<[bool; 3]>> = Mutex::new(Vec::new());

#[cfg(test)]
fn held_while_deleting(state: &AppState) {
    if WATCH.lock().unwrap().as_deref() != Some(&state.notes_dir) {
        return;
    }
    let blocked = |wait: Box<dyn FnOnce() + Send>| {
        let (sent, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            wait();
            let _ = sent.send(());
        });
        got.recv_timeout(std::time::Duration::from_millis(200))
            .is_err()
    };
    let held = [
        blocked(Box::new({
            let chats = state.chats.clone();
            move || drop(crate::boards::writing(&chats))
        })),
        blocked(Box::new({
            let chats = state.chats.clone();
            move || drop(crate::tasks::writing(&chats))
        })),
        state.store.try_lock().is_err(),
    ];
    HELD_LOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(held);
}
