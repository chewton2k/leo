use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Mutex;

use leo_core::store::Store;

const PENDING: &str = "deleting";

static CLEARING: Mutex<()> = Mutex::new(());

pub(crate) struct Owners {
    held: HashSet<String>,
    sure: bool,
}

impl Owners {
    pub(crate) fn of(store: &Store) -> Owners {
        let trashed = store.trashed();
        Owners {
            sure: store.unreadable.is_empty() && trashed.iter().all(|t| t.readable),
            held: store
                .notes
                .iter()
                .map(|n| n.id.clone())
                .chain(trashed.into_iter().filter(|t| t.readable).map(|t| t.id))
                .collect(),
        }
    }
}

pub(crate) fn clear(
    chats: &Path,
    notes: &Path,
    ids: Vec<String>,
    active: Option<String>,
    owners: &Owners,
) {
    let _clearing = CLEARING.lock().unwrap_or_else(|e| e.into_inner());
    let mut pending: BTreeSet<String> = crate::margin::state_of(chats, PENDING).unwrap_or_default();
    pending.extend(ids);
    pending.retain(|id| !owners.held.contains(id));
    crate::margin::set_state(chats, PENDING, &pending);
    if !owners.sure || pending.is_empty() {
        return;
    }
    let all: Vec<String> = pending.iter().cloned().collect();
    crate::margin::forget_notes(chats, &all);
    pending.retain(|id| {
        let folders = [
            leo_core::recording::folder(notes, id).ok(),
            crate::routes::uploads::originals_dir(notes, id),
        ];
        if folders.iter().all(Option::is_none) {
            return false;
        }
        if active.as_deref() == Some(id.as_str()) {
            return true;
        }
        let mut left = false;
        for folder in folders.into_iter().flatten() {
            match std::fs::remove_dir_all(&folder) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    leo_core::diag::warn(format!(
                        "could not remove {} of a deleted note: {error}",
                        folder.display()
                    ));
                    left = true;
                }
                _ => {}
            }
        }
        left
    });
    crate::margin::set_state(chats, PENDING, &pending);
}
