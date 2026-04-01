use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use colored::Colorize;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::store::Store;

type AppState = Arc<Mutex<Store>>;

const HTML: &str = include_str!("web_ui.html");

pub async fn serve(port: u16) -> Result<()> {
    let store = Store::load()?;
    let state: AppState = Arc::new(Mutex::new(store));

    let app = Router::new()
        .route("/", get(index))
        .route("/api/notes", get(list_notes).post(create_note))
        .route(
            "/api/notes/{id}",
            get(get_note).patch(update_note).delete(delete_note),
        )
        .route("/api/notes/{id}/toggle", post(toggle_checkbox))
        .route("/api/search", get(search_notes))
        .route("/api/tags", get(list_tags))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let network_url = format!("http://{local_ip}:{port}");

    println!(
        "\n  {} {}\n  {} {}\n",
        "Local:".bold(),
        format!("http://localhost:{port}").cyan().underline(),
        "Network:".bold(),
        network_url.cyan().underline(),
    );

    // Print QR code for easy phone scanning
    if let Ok(code) = qrcode::QrCode::new(&network_url) {
        use qrcode::render::unicode::Dense1x2;
        let qr = code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build();
        println!("{qr}\n");
    }

    println!("  {}", "Scan the QR code or open the URL on your phone".dimmed());
    println!("  {}\n", "Press Ctrl+C to stop".dimmed());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(HTML)
}

// ── Query params ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListParams {
    tag: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    full_text: Option<bool>,
}

#[derive(Deserialize)]
struct ToggleParams {
    checkbox: usize,
}

// ── Request bodies ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateBody {
    title: String,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct UpdateBody {
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

// ── Response types ────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct NoteResponse {
    id: String,
    title: String,
    body: String,
    created_at: String,
    updated_at: String,
    tags: Vec<String>,
}

impl NoteResponse {
    fn from_note(n: &crate::notes::Note) -> Self {
        NoteResponse {
            id: n.id.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            created_at: n.created_at.to_rfc3339(),
            updated_at: n.updated_at.to_rfc3339(),
            tags: n.tags.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct TagResponse {
    tag: String,
    count: usize,
}

// ── Handlers ──────────────────────────────────────────────────────────────

async fn list_notes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Json<Vec<NoteResponse>> {
    let store = state.lock().unwrap();
    let limit = params.limit.unwrap_or(100);
    let notes = store.list_notes(params.tag.as_deref(), limit);
    Json(notes.iter().map(|n| NoteResponse::from_note(n)).collect())
}

async fn get_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let store = state.lock().unwrap();
    match store.find_note(&id) {
        Some(n) => Ok(Json(NoteResponse::from_note(n))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn create_note(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut store = state.lock().unwrap();
    let tags = body.tags.unwrap_or_default();
    let note_body = body.body.unwrap_or_default();
    match store.create_note(body.title, note_body, tags, "") {
        Ok(n) => {
            let resp = NoteResponse::from_note(n);
            let _ = store.save();
            Ok((StatusCode::CREATED, Json(resp)))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.lock().unwrap();
    let note = store.find_note_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    if let Some(title) = body.title {
        note.title = title;
    }
    if let Some(new_body) = body.body {
        note.body = new_body;
    }
    if let Some(tags) = body.tags {
        note.tags = tags;
    }
    note.updated_at = chrono::Utc::now();

    let resp = NoteResponse::from_note(note);
    let _ = store.save();
    Ok(Json(resp))
}

async fn delete_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut store = state.lock().unwrap();
    if store.delete_note(&id) {
        let _ = store.save();
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn toggle_checkbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ToggleParams>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.lock().unwrap();
    store
        .toggle_checkbox(&id, params.checkbox)
        .ok_or(StatusCode::NOT_FOUND)?;
    let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
    let resp = NoteResponse::from_note(note);
    let _ = store.save();
    Ok(Json(resp))
}

async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<Vec<NoteResponse>> {
    let store = state.lock().unwrap();
    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Json(vec![]);
    }
    let full_text = params.full_text.unwrap_or(false);
    let results = store.search(&q, full_text);
    Json(results.iter().map(|n| NoteResponse::from_note(n)).collect())
}

async fn list_tags(State(state): State<AppState>) -> Json<Vec<TagResponse>> {
    let store = state.lock().unwrap();
    Json(
        store
            .tags()
            .into_iter()
            .map(|(tag, count)| TagResponse { tag, count })
            .collect(),
    )
}
