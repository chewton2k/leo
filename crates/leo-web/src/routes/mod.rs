pub(crate) mod activity;
pub(crate) mod assets;
pub(crate) mod auth;
pub(crate) mod boards;
pub(crate) mod combine;
pub(crate) mod downloads;
pub(crate) mod felix;
pub(crate) mod housekeeping;
pub(crate) mod insert;
pub(crate) mod margin;
pub(crate) mod notes;
pub(crate) mod organize;
pub(crate) mod pictures;
pub(crate) mod processes;
pub(crate) mod related;
pub(crate) mod settings;
pub(crate) mod sources;
pub(crate) mod tasks;
pub(crate) mod trash;
pub(crate) mod uploads;

use axum::{
    routing::{get, post},
    Router,
};

use crate::routes::activity::{activity, page_visible};
use crate::routes::assets::{
    app_js, chat_js, doc_js, editing_js, favicon, index, markdown_js, recording_js, saving_js,
};
use crate::routes::auth::{auth_middleware, note_peer, security_headers};
use crate::routes::downloads::{export_zip, originals_zip};
use crate::routes::felix::{
    add_chat_file, apply_suggestion, change_answer_access, chat_reply, delete_chat,
    drop_last_messages, get_chat, get_review, list_chat_files, list_chats, mark_reviewed, put_chat,
    remove_chat_file, steer_answer,
};
use crate::routes::housekeeping::{get_keep, set_keep};
use crate::routes::notes::{
    create_dir, create_note, delete_note, get_note, list_dirs, list_folders, list_notes, move_dir,
    move_note, rename_dir, search_notes, update_note,
};
use crate::routes::pictures::{add_picture, get_picture, PICTURE_BYTES};
use crate::routes::settings::{change_setting, get_settings, test_setting};
use crate::routes::trash::{
    delete_from_trash, list_trash, move_to_trash, restore_many, restore_note,
};
use crate::routes::uploads::{
    get_original, import_status, list_originals, start_import, IMPORT_BYTES,
};
use crate::AppState;

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/notes", get(list_notes).post(create_note))
        .route(
            "/api/notes/{id}",
            get(get_note).patch(update_note).delete(delete_note),
        )
        .route(
            "/api/calendar",
            get(crate::calendar::get)
                .post(crate::calendar::add)
                .patch(crate::calendar::change),
        )
        .route("/api/calendar/refresh", post(crate::calendar::refresh_now))
        .route(
            "/api/calendar/{id}",
            axum::routing::delete(crate::calendar::remove),
        )
        .route(
            "/api/notes/{id}/recording",
            get(sources::sources).put(sources::edit_sources),
        )
        .route("/api/notes/{id}/regenerate", post(sources::regenerate))
        .route("/api/notes/{id}/move", post(move_note))
        .route("/api/notes/{id}/combine", post(combine::start))
        .route(
            "/api/notes/{id}/combined",
            post(combine::keep).layer(axum::extract::DefaultBodyLimit::max(
                crate::combine::MOST_CHARS * 8 + 65_536,
            )),
        )
        .route("/api/notes/{id}/uncombine", post(combine::uncombine))
        .route(
            "/api/combines/{job}",
            get(combine::job).delete(combine::forget),
        )
        .route("/api/notes/{id}/suggestion", post(apply_suggestion))
        .route("/api/felix/act", post(organize::act_route))
        .route("/api/notes/{id}/insert", post(insert::insert_into_note))
        .route(
            "/api/notes/{id}/margin",
            get(margin::note_margin).post(margin::set_note_mode),
        )
        .route(
            "/api/notes/{id}/margin/read",
            post(margin::read).layer(axum::extract::DefaultBodyLimit::max(
                crate::margin::BODY_BYTES,
            )),
        )
        .route(
            "/api/notes/{id}/margin/analyze",
            post(margin::analyze).layer(axum::extract::DefaultBodyLimit::max(
                crate::margin::BODY_BYTES,
            )),
        )
        .route(
            "/api/notes/{id}/margin/threads",
            post(margin::ask).layer(axum::extract::DefaultBodyLimit::max(
                crate::margin::BODY_BYTES,
            )),
        )
        .route(
            "/api/notes/{id}/margin/threads/{thread}",
            post(margin::set_status)
                .patch(margin::pin)
                .delete(margin::forget),
        )
        .route(
            "/api/notes/{id}/margin/threads/{thread}/reply",
            post(margin::reply).layer(axum::extract::DefaultBodyLimit::max(
                crate::margin::BODY_BYTES,
            )),
        )
        .route(
            "/api/notes/{id}/margin/threads/{thread}/weave",
            post(margin::weave).layer(axum::extract::DefaultBodyLimit::max(
                crate::margin::BODY_BYTES,
            )),
        )
        .route(
            "/api/margin",
            get(margin::settings).post(margin::change_settings),
        )
        .route("/api/notes/{id}/related", get(related::related_notes))
        .route(
            "/api/notes/{id}/reading",
            get(related::get_reading).put(related::put_reading),
        )
        .route(
            "/api/tasks",
            get(tasks::list_tasks).post(tasks::create_task),
        )
        .route("/api/tasks/promote", post(tasks::promote_task))
        .route("/api/tasks/import", post(tasks::import_tasks))
        .route(
            "/api/tasks/{id}",
            axum::routing::patch(tasks::change_task).delete(tasks::delete_task),
        )
        .route("/api/tasks/{id}/ics", get(tasks::task_ics))
        .route(
            "/api/boards",
            get(boards::list_boards).post(boards::create_board),
        )
        .route("/api/boards/restore", post(boards::restore_board))
        .route(
            "/api/boards/{id}",
            get(boards::get_board)
                .patch(boards::change_board)
                .delete(boards::delete_board),
        )
        .route("/api/boards/{id}/cards", post(boards::create_card))
        .route(
            "/api/cards/{id}",
            axum::routing::patch(boards::change_card).delete(boards::delete_card),
        )
        .route("/api/cards/{id}/move", post(boards::move_card))
        .route("/api/schedule", get(crate::calendar::schedule))
        .route("/api/search", get(search_notes))
        .route("/api/dirs", get(list_dirs).post(create_dir))
        .route("/api/dirs/move", post(move_dir))
        .route("/api/dirs/rename", post(rename_dir))
        .route("/api/folders", get(list_folders))
        .route("/api/trash", get(list_trash))
        .route("/api/trash/{id}/restore", post(restore_note))
        .route("/api/trash/delete", post(delete_from_trash))
        .route("/api/trash/move", post(move_to_trash))
        .route("/api/trash/restore", post(restore_many))
        .route("/app.js", get(app_js))
        .route("/markdown.js", get(markdown_js))
        .route(assets::MERMAID_PATH, get(assets::mermaid_js))
        .route("/vendor/katex-0.16.11/{*path}", get(assets::katex_file))
        .route("/editing.js", get(editing_js))
        .route("/doc.js", get(doc_js))
        .route("/saving.js", get(saving_js))
        .route("/schedule.js", get(assets::schedule_js))
        .route("/margin.js", get(assets::margin_js))
        .route("/prefs.js", get(crate::prefs::script))
        .route("/theme.js", get(assets::theme_js))
        .route(
            "/api/ui-prefs",
            axum::routing::put(crate::prefs::change).layer(axum::extract::DefaultBodyLimit::max(
                crate::prefs::BODY_BYTES,
            )),
        )
        .route("/chat.js", get(chat_js))
        .route("/pane-resize.js", get(assets::pane_resize_js))
        .route("/api/chat", post(chat_reply))
        .route("/api/quiz/mark", post(felix::mark_quiz))
        .route(
            "/api/chat/compact",
            post(felix::compact_chat).layer(axum::extract::DefaultBodyLimit::max(
                crate::chats::CHAT_BYTES,
            )),
        )
        .route("/api/chat/{answer}/steer", post(steer_answer))
        .route("/api/chat/{answer}/access", post(change_answer_access))
        .route("/api/chats", get(list_chats))
        .route("/api/review", get(get_review).post(mark_reviewed))
        .route(
            "/api/chats/{id}/files",
            get(list_chat_files)
                .post(add_chat_file)
                .layer(axum::extract::DefaultBodyLimit::max(
                    crate::chat_files::UPLOAD_BYTES,
                )),
        )
        .route(
            "/api/chats/{id}/files/{doc}",
            axum::routing::delete(remove_chat_file),
        )
        .route("/api/keep", get(get_keep).post(set_keep))
        .route("/api/keep/preview", post(housekeeping::preview_keep))
        .route("/api/export", get(export_zip))
        .route("/api/chats/{id}/drop", post(drop_last_messages))
        .route(
            "/api/chats/{id}",
            get(get_chat).put(put_chat).delete(delete_chat).layer(
                axum::extract::DefaultBodyLimit::max(crate::chats::CHAT_BYTES),
            ),
        )
        .route("/api/settings", get(get_settings).post(change_setting))
        .route("/api/settings/test", post(test_setting))
        .route("/api/settings/usage", post(settings::refresh_usage))
        .route("/api/writing", get(settings::writing))
        .route("/api/processes", get(processes::list))
        .route(
            "/api/import",
            post(start_import)
                .layer(axum::extract::DefaultBodyLimit::max(IMPORT_BYTES))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    uploads::import_limit,
                )),
        )
        .route("/api/import/{id}", get(import_status))
        .route("/api/activity", get(activity))
        .route("/api/visible", post(page_visible))
        .route("/api/image", get(get_picture))
        .route(
            "/api/images",
            post(add_picture).layer(axum::extract::DefaultBodyLimit::max(PICTURE_BYTES)),
        )
        .route(
            "/api/app",
            get(crate::app_settings::get)
                .post(crate::app_settings::change)
                .layer(axum::extract::DefaultBodyLimit::max(16_384)),
        )
        .route("/api/health", get(crate::app_settings::health))
        .route(
            "/api/dictation",
            get(crate::dictation::get)
                .post(crate::dictation::change)
                .layer(axum::extract::DefaultBodyLimit::max(200_000)),
        )
        .route(
            "/api/speech",
            get(crate::speech::get)
                .post(crate::speech::change)
                .layer(axum::extract::DefaultBodyLimit::max(4096)),
        )
        .route(
            "/api/writing/ollama",
            get(crate::ollama::get)
                .post(crate::ollama::change)
                .layer(axum::extract::DefaultBodyLimit::max(4096)),
        )
        .route(
            "/api/record",
            get(crate::record::overview).post(crate::record::start),
        )
        .route(
            "/api/record/{id}",
            get(crate::record::status).delete(crate::record::discard),
        )
        .route("/api/record/{id}/pause", post(crate::record::pause))
        .route("/api/record/{id}/point", post(crate::record::point))
        .route("/api/record/{id}/wants", post(crate::record::wants))
        .route(
            "/api/record/{id}/snapshot",
            post(crate::record::snapshot).layer(axum::extract::DefaultBodyLimit::max(
                pictures::PICTURE_BYTES,
            )),
        )
        .route("/api/record/{id}/stop", post(crate::record::stop))
        .route(
            "/api/record/{id}/finish",
            post(crate::record::finish_available),
        )
        .route("/api/record/{id}/recover", post(crate::record::recover))
        .route("/recording.js", get(recording_js))
        .route("/api/notes/{id}/originals", get(list_originals))
        .route(
            "/api/notes/{id}/originals/{name}/passage",
            get(felix::original_passage),
        )
        .route("/api/notes/{id}/originals.zip", get(originals_zip))
        .route("/api/notes/{id}/originals/{name}", get(get_original))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::note_activity,
        ))
        .route("/favicon.svg", get(favicon))
        .layer(axum::middleware::from_fn(security_headers))
        .layer(axum::middleware::from_fn(note_peer))
        .with_state(state)
}
