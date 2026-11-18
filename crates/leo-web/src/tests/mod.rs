mod audit_bench;
mod auth;
mod boards;
mod calendar;
mod combine;
mod combine_originals;
mod dictation;
mod errors;
mod felix;
mod housekeeping;
mod margin;
mod notes;
mod ollama;
mod passages;
mod pictures;
mod recording;
mod related;
mod sources;
mod speech;
mod start;
mod study_compat;
mod study_context;
mod tasks;
mod trash;
mod uploads;
mod usage;

use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::Response;
use axum::Json;
use leo_core::store::Store;

use crate::routes::activity::activity_tasks;
use crate::routes::downloads::attachment_header;
use crate::routes::felix::{
    add_chat_file, apply_suggestion, chat_reply, list_chat_files, remove_chat_file, steer_answer,
    Suggestion,
};
use crate::routes::housekeeping::{get_keep, set_keep};
use crate::routes::notes::{
    create_dir, create_note, delete_note, excerpt, get_note, list_folders, list_notes, move_dir,
    move_note, tasks_in, update_note, CreateBody, CreateDirBody, ListParams, MoveBody, MoveDirBody,
    NoteResponse, UpdateBody, EXCERPT_CHARS,
};
use crate::routes::pictures::{add_picture, get_picture, NewPicture, PictureAt};
use crate::routes::trash::{
    list_trash, move_to_trash, restore_many, restore_note, TrashChoice, TrashMove,
};
use crate::routes::uploads::{
    get_original, list_originals, safe_file_name, settle_figures, start_import, upload_label,
    viewable, ImportBody, ImportFileBody, ImportJob, OriginalView,
};
use crate::*;

fn state_with(notes: &[(&str, &str)]) -> (AppState, tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    let ids = notes
        .iter()
        .map(|(title, d)| store.create_note(*title, "", vec![], d).unwrap().id.clone())
        .collect();
    store.save().unwrap();
    let graphs = Arc::new(graph::Graphs::for_notes(&store.notes_dir, None));
    let notes_dir = store.notes_dir.clone();
    let state = AppState {
        powers: Powers {
            lighter: Some(Arc::new(|| true)),
            ..Default::default()
        },
        margin: Default::default(),
        visible: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        shown: Default::default(),
        covered: Default::default(),
        page_seen: Default::default(),
        refresh_requested: Default::default(),
        store: Arc::new(Mutex::new(Storage {
            changes: None,
            store,
            reload: false,
        })),
        gate: Arc::new(sessions::Gate::new(
            "0123456789abcdef0123456789abcdef".into(),
            sessions::Sessions::in_memory(),
        )),
        graphs,
        progress: Default::default(),
        warm: Default::default(),
        work: Default::default(),
        ollama_pulls: Default::default(),
        usage_refreshes: Arc::new(tokio::sync::Semaphore::new(1)),
        marking: Arc::new(tokio::sync::Semaphore::new(2)),
        regenerating: Arc::new(tokio::sync::Semaphore::new(2)),
        setting_tests: Arc::new(tokio::sync::Semaphore::new(1)),
        imports: Default::default(),
        import_slots: Default::default(),
        zip_slots: routes::downloads::zip_slots(),
        combines: Default::default(),
        recording: Default::default(),
        quitting: Default::default(),
        capture_activity: leo_core::activity::Gate::default(),
        discarded_recordings: Default::default(),
        calendar: Default::default(),
        source_writing: Default::default(),
        chats: dir.path().join("chats"),
        notes_dir,
        captions: Arc::new(captions::Captions::at(dir.path().join("captions.json"))),
        originals: Default::default(),
        activity: Default::default(),
        steering: Default::default(),
        titles: Default::default(),
        generation: Default::default(),
        listed: Default::default(),
        vectors: Arc::new(vectors::Vectors::at(dir.path().join("meaning.json"))),
        searched: Default::default(),
    };
    (state, dir, ids)
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}

fn wait_for(state: &AppState, id: &str) -> ImportJob {
    let started = std::time::Instant::now();
    loop {
        let job = state.imports.lock().unwrap().get(id).cloned().unwrap();
        if job.state != "working" {
            return job;
        }
        assert!(
            started.elapsed().as_secs() < 10,
            "the import never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn json_of(response: Response) -> serde_json::Value {
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}
