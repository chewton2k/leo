use super::*;
use std::time::{Duration, Instant};

const WORDS: &[&str] = &[
    "the", "graph", "search", "queue", "stack", "heap", "tree", "node", "edge", "visit", "order",
    "breadth", "depth", "first", "lecture", "exam", "proof", "lemma", "theorem", "plant", "light",
    "sugar", "cell", "energy", "market", "price", "supply", "demand", "history", "empire", "river",
    "trade", "language", "grammar", "verb", "noun", "memory", "cache", "process", "thread", "lock",
    "network", "packet", "route", "signal", "matrix", "vector", "integral", "limit", "series",
    "function", "variable", "loop", "array", "list", "hash", "table", "sort", "merge", "split",
    "and", "of", "to", "in", "is", "that", "with", "for", "as", "on", "by", "from", "this",
];

fn body_for(seed: usize) -> String {
    let mut state = (seed as u64).wrapping_mul(6_364_136_223_846_793_005) ^ 0x9e37_79b9;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize
    };
    let mut body = String::new();
    while body.len() < 5000 {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        let mut paragraph = String::new();
        while paragraph.len() < 480 {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            let word = if seed.is_multiple_of(2) || next() % 3 != 0 {
                WORDS[next() % WORDS.len()]
            } else {
                WORDS[next() % 3 + 4]
            };
            paragraph.push_str(if word == "queue" && seed % 2 == 1 {
                "stack"
            } else {
                word
            });
        }
        paragraph.push('.');
        body.push_str(&paragraph);
    }
    body
}

fn big_body() -> String {
    "word ".repeat(4 * 1024 * 1024)
}

fn library(n: usize, big: bool) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let notes_dir = dir.path().join("notes");
    let mut store = Store::load_from(&notes_dir).unwrap();
    for i in 0..n {
        let directory = format!("Course {}", i % 20);
        store
            .create_note(
                format!("Lecture {i} {}", WORDS[i % WORDS.len()]),
                body_for(i),
                vec![],
                &directory,
            )
            .unwrap();
    }
    if big {
        store
            .create_note("Big", big_body(), vec![], "Course 0")
            .unwrap();
    }
    store.save().unwrap();
    let store = Store::load_from(&notes_dir).unwrap();
    assert_eq!(store.notes.len(), n + usize::from(big));
    (dir, store)
}

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

fn state_on(dir: &tempfile::TempDir, store: Store) -> (AppState, tempfile::TempDir) {
    let (mut state, other, _) = state_with(&[]);
    let notes_dir = store.notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(&notes_dir, None));
    state.vectors = Arc::new(vectors::Vectors::for_notes(&notes_dir));
    state.chats = dir.path().join("chats");
    state.store = Arc::new(Mutex::new(Storage {
        changes: Some(store_refresh::Changes::new(&notes_dir)),
        store,
        reload: false,
    }));
    (state, other)
}

fn modified_since(root: &std::path::Path, since: std::time::SystemTime) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md")
                && meta.modified().unwrap() > since
            {
                count += 1;
            }
        }
    }
    count
}

#[test]
#[ignore]
fn audit_bench_c1_autosave() {
    let n: usize = std::env::var("LEO_BENCH_NOTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let big = std::env::var("LEO_BENCH_BIG").is_ok_and(|v| v == "1");
    {
        let (dir, store) = library(n, big);
        let notes_dir = store.notes_dir.clone();
        let id = if big {
            store
                .notes
                .iter()
                .find(|note| note.title == "Big")
                .unwrap()
                .id
                .clone()
        } else {
            store.notes[n / 2].id.clone()
        };
        let base = if big { big_body() } else { body_for(n / 2) };
        let (state, _other) = state_on(&dir, store);
        let mut pre = Vec::new();
        let mut save_edit = Vec::new();
        let mut save_none = Vec::new();
        let mut post = Vec::new();
        let mut waited = Vec::new();
        for round in 0..7 {
            let mut storage = state.store.lock().unwrap();
            while storage.changes.as_mut().unwrap().due() {}
            let start = Instant::now();
            assert!(!storage.changed_on_disk());
            pre.push(start.elapsed());
            let note = storage.find_note_mut(&id).unwrap();
            note.body = format!("{} edit {round}", base);
            note.updated_at = chrono::Utc::now();
            let before = std::time::SystemTime::now();
            std::thread::sleep(Duration::from_millis(5));
            let start = Instant::now();
            storage.save().unwrap();
            save_edit.push(start.elapsed());
            if round == 0 {
                assert_eq!(modified_since(&notes_dir, before), 1, "one file rewritten");
            }
            let start = Instant::now();
            storage.save().unwrap();
            save_none.push(start.elapsed());
            let start = Instant::now();
            let mut seen = false;
            while start.elapsed() < Duration::from_secs(5) {
                if storage.changes.as_mut().unwrap().due() {
                    seen = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            waited.push(start.elapsed());
            assert!(seen, "the watcher reports leo's own write");
            let start = Instant::now();
            assert!(!storage.changed_on_disk());
            post.push(start.elapsed());
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut patch = Vec::new();
        let mut read = Vec::new();
        let mut quiet = Vec::new();
        let mut reloads = 0;
        let mut margin_ms = Vec::new();
        let mut bare = Vec::new();
        let (mut hot_patch, mut hot_read, mut hot_entered, mut hot_save, mut hot_after) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let ptr = |state: &AppState| state.store.lock().unwrap().notes.as_ptr() as usize;
        for round in 0..7 {
            let at = ptr(&state);
            std::thread::sleep(Duration::from_millis(300));
            let body: UpdateBody = serde_json::from_value(
                serde_json::json!({"body": format!("{} again {round}", base)}),
            )
            .unwrap();
            let start = Instant::now();
            let _ = rt
                .block_on(update_note(
                    State(state.clone()),
                    Path(id.clone()),
                    Json(body),
                ))
                .unwrap();
            patch.push(start.elapsed());
            std::thread::sleep(Duration::from_millis(300));
            let start = Instant::now();
            let _ = rt
                .block_on(get_note(State(state.clone()), Path(id.clone())))
                .unwrap();
            read.push(start.elapsed());
            let start = Instant::now();
            let _ = rt
                .block_on(get_note(State(state.clone()), Path(id.clone())))
                .unwrap();
            quiet.push(start.elapsed());
            if ptr(&state) != at {
                reloads += 1;
            }
            std::thread::sleep(Duration::from_millis(300));
            let text = format!("{} plain {round}", base);
            let target = id.clone();
            let start = Instant::now();
            rt.block_on(state.with_store(move |store| {
                let note = store.find_note_mut(&target).unwrap();
                note.body = text;
                note.updated_at = chrono::Utc::now();
                let resp = NoteResponse::from_note(note);
                crate::routes::notes::save(store)?;
                Ok(resp.version)
            }))
            .unwrap();
            bare.push(start.elapsed());
            let mut split = Vec::new();
            for hot in 0..3 {
                let text = format!("{} hot {round} {hot}", base);
                let target = id.clone();
                let start = Instant::now();
                let (entered, saving) = rt
                    .block_on(state.with_store(move |store| {
                        let entered = start.elapsed();
                        let note = store.find_note_mut(&target).unwrap();
                        note.body = text;
                        note.updated_at = chrono::Utc::now();
                        let at = Instant::now();
                        crate::routes::notes::save(store)?;
                        Ok((entered, at.elapsed()))
                    }))
                    .unwrap();
                let total = start.elapsed();
                split.push((entered, saving, total));
                let body: UpdateBody = serde_json::from_value(
                    serde_json::json!({"body": format!("{} hotpatch {round} {hot}", base)}),
                )
                .unwrap();
                let start = Instant::now();
                let _ = rt
                    .block_on(update_note(
                        State(state.clone()),
                        Path(id.clone()),
                        Json(body),
                    ))
                    .unwrap();
                hot_patch.push(start.elapsed());
                let start = Instant::now();
                let _ = rt
                    .block_on(get_note(State(state.clone()), Path(id.clone())))
                    .unwrap();
                hot_read.push(start.elapsed());
            }
            for (entered, saving, total) in split {
                hot_entered.push(entered);
                hot_save.push(saving);
                hot_after.push(total.saturating_sub(entered + saving));
            }
            let start = Instant::now();
            state.margin.edited(&id, "v");
            margin_ms.push(start.elapsed());
        }
        println!(
            "C1 notes={n} pre_check_ms={:.2} save_with_edit_ms={:.2} save_nothing_changed_ms={:.2} watcher_event_ms={:.1} next_read_rescan_ms={:.2} update_note_handler_ms={:.2} next_get_note_handler_ms={:.2} second_get_note_ms={:.2} margin_edited_ms={:.3} store_reloads_in_7_rounds={reloads} with_store_edit_save_ms={:.2} | back_to_back: update_note_ms={:.2} next_get_note_ms={:.2} with_store_before_closure_ms={:.2} with_store_save_ms={:.2} with_store_after_ms={:.2}",
            median(pre), median(save_edit), median(save_none), median(waited), median(post), median(patch), median(read), median(quiet), median(margin_ms), median(bare), median(hot_patch), median(hot_read), median(hot_entered), median(hot_save), median(hot_after)
        );
    }
}
