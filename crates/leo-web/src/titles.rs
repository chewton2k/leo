use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use leo_core::notes::Note;
use leo_core::store::Store;

pub(crate) type Table = Arc<HashMap<String, Vec<usize>>>;

#[derive(Default)]
pub(crate) struct Titles(Mutex<Option<(u64, usize, Table)>>);

impl Titles {
    pub(crate) fn of(&self, store: &Store) -> Table {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((fingerprint, count, table)) = kept.as_ref() {
            if *fingerprint == store.fingerprint() && *count == store.notes.len() {
                return Arc::clone(table);
            }
        }
        let table = Arc::new(table_of(store));
        *kept = Some((store.fingerprint(), store.notes.len(), Arc::clone(&table)));
        table
    }

    fn rebuilt(&self, store: &Store) -> Table {
        let table = Arc::new(table_of(store));
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((store.fingerprint(), store.notes.len(), Arc::clone(&table)));
        table
    }

    pub(crate) fn exact<'a>(&self, store: &'a Store, title: &str) -> Option<&'a Note> {
        let lower = title.to_lowercase();
        exact_in(store, &self.of(store), &lower)
            .unwrap_or_else(|| exact_in(store, &self.rebuilt(store), &lower).flatten())
    }
}

fn table_of(store: &Store) -> HashMap<String, Vec<usize>> {
    let mut table: HashMap<String, Vec<usize>> = HashMap::with_capacity(store.notes.len());
    for (i, note) in store.notes.iter().enumerate() {
        table.entry(note.title.to_lowercase()).or_default().push(i);
    }
    table
}

fn at<'a>(store: &'a Store, i: usize, lower: &str) -> Option<&'a Note> {
    store
        .notes
        .get(i)
        .filter(|note| note.title.to_lowercase() == lower)
}

pub(crate) fn exact_in<'a>(
    store: &'a Store,
    table: &HashMap<String, Vec<usize>>,
    lower: &str,
) -> Option<Option<&'a Note>> {
    match table.get(lower).and_then(|places| places.first()) {
        None => Some(None),
        Some(&i) => at(store, i, lower).map(Some),
    }
}

pub(crate) fn containing<'a>(
    store: &'a Store,
    table: &HashMap<String, Vec<usize>>,
    lower: &str,
) -> Option<Vec<&'a Note>> {
    let mut found: Vec<(usize, &str)> = table
        .iter()
        .filter(|(title, _)| title.contains(lower))
        .flat_map(|(title, places)| places.iter().map(move |&i| (i, title.as_str())))
        .collect();
    found.sort_unstable_by_key(|(i, _)| *i);
    found
        .into_iter()
        .map(|(i, title)| at(store, i, title))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_note_is_found_by_its_new_title_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let id = store.create_note("Graphs", "", "").unwrap().id.clone();
        store.create_note("Trees", "", "").unwrap();
        store.save().unwrap();
        let titles = Titles::default();
        assert_eq!(titles.exact(&store, "graphs").unwrap().id, id);
        store.find_note_mut(&id).unwrap().title = "Networks".into();
        store.save().unwrap();
        assert!(titles.exact(&store, "Graphs").is_none());
        assert_eq!(titles.exact(&store, "NETWORKS").unwrap().id, id);
    }

    #[test]
    fn two_notes_with_one_title_give_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        store.create_note("Week one", "a", "A").unwrap();
        store.create_note("week ONE", "b", "B").unwrap();
        store.save().unwrap();
        let scanned = store
            .notes
            .iter()
            .find(|n| n.title.to_lowercase() == "week one")
            .unwrap()
            .id
            .clone();
        assert_eq!(
            Titles::default().exact(&store, "Week One").unwrap().id,
            scanned
        );
    }

    #[test]
    fn a_table_out_of_step_with_the_notes_is_never_believed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let id = store.create_note("Old", "", "").unwrap().id.clone();
        store.save().unwrap();
        let titles = Titles::default();
        let table = titles.of(&store);
        store.find_note_mut(&id).unwrap().title = "New".into();
        assert!(exact_in(&store, &table, "old").is_none());
        assert!(containing(&store, &table, "ol").is_none());
        assert!(titles.exact(&store, "old").is_none());
    }
}
