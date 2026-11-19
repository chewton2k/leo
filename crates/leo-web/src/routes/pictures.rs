use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leo_core::attachments;
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct PictureAt {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) from: String,
}

const PICTURE_PIECE: usize = 64 * 1024;
const PICTURE_IDLE: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) async fn get_picture(
    State(state): State<AppState>,
    Query(at): Query<PictureAt>,
) -> Response {
    use std::io::{Read, Seek};
    let notes_dir = state.notes_dir.clone();
    let found = tokio::task::spawn_blocking(move || {
        let path = attachments::resolve(&notes_dir, &at.from, &at.path)?;
        let mut file = leo_core::paths::open_regular(&path).ok()?;
        let size = file.metadata().ok()?.len();
        let mut head = [0u8; attachments::HEAD_BYTES];
        let mut got = 0;
        while got < head.len() {
            match file.read(&mut head[got..]).ok()? {
                0 => break,
                n => got += n,
            }
        }
        let kind = attachments::kind_of(&head[..got])?;
        file.rewind().ok()?;
        Some((attachments::mime_of(kind)?, file, size))
    })
    .await;
    match found {
        Ok(Some((mime, file, size))) => {
            let stream = futures_util::stream::try_unfold(
                (tokio::fs::File::from_std(file), vec![0u8; PICTURE_PIECE]),
                |(mut file, mut piece)| async move {
                    use tokio::io::AsyncReadExt;
                    let read = file.read(&mut piece).await?;
                    Ok::<_, std::io::Error>(
                        (read > 0).then(|| (piece[..read].to_vec(), (file, piece))),
                    )
                },
            );
            let mut response = axum::body::Body::from_stream(stream).into_response();
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=3600"),
            );
            response
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

struct Arriving {
    pieces: tokio::sync::mpsc::Receiver<std::io::Result<axum::body::Bytes>>,
    held: axum::body::Bytes,
}

impl std::io::Read for Arriving {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.held.is_empty() {
            match self.pieces.blocking_recv() {
                Some(piece) => self.held = piece?,
                None => return Ok(0),
            }
        }
        let n = self.held.len().min(out.len());
        out[..n].copy_from_slice(&self.held.split_to(n));
        Ok(n)
    }
}

fn said(status: StatusCode, error: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": error.into() }))).into_response()
}

fn not_a_picture() -> Response {
    said(
        StatusCode::BAD_REQUEST,
        "leo can show PNG, JPEG, GIF and WebP pictures.",
    )
}

fn storage_full(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::StorageFull)
    })
}

pub(crate) async fn put_picture(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: axum::body::Body,
) -> Response {
    use futures_util::StreamExt;
    let mut stream = body.into_data_stream();
    let mut head: Vec<u8> = Vec::new();
    let mut first = axum::body::Bytes::new();
    while head.len() < attachments::HEAD_BYTES && first.is_empty() {
        match tokio::time::timeout(PICTURE_IDLE, stream.next()).await {
            Ok(Some(Ok(mut piece))) => {
                let take = piece.len().min(attachments::HEAD_BYTES - head.len());
                head.extend_from_slice(&piece.split_to(take));
                first = piece;
            }
            Ok(None) => break,
            Ok(Some(Err(_))) | Err(_) => {
                return said(
                    StatusCode::BAD_REQUEST,
                    "That picture did not arrive whole.",
                )
            }
        }
    }
    if attachments::kind_of(&head).is_none() {
        return not_a_picture();
    }
    let notes_dir = state.notes_dir.clone();
    let (send, pieces) = tokio::sync::mpsc::channel(4);
    let saving = tokio::task::spawn_blocking(move || {
        attachments::save_from(
            &notes_dir,
            &name,
            &head,
            Arriving {
                pieces,
                held: axum::body::Bytes::new(),
            },
        )
    });
    let mut feeding = true;
    if !first.is_empty() {
        feeding = send.send(Ok(first)).await.is_ok();
    }
    while feeding {
        let piece = match tokio::time::timeout(PICTURE_IDLE, stream.next()).await {
            Ok(Some(Ok(piece))) => Ok(piece),
            Ok(None) => break,
            Ok(Some(Err(error))) => Err(std::io::Error::other(error)),
            Err(_) => Err(std::io::ErrorKind::TimedOut.into()),
        };
        let failed = piece.is_err();
        feeding = send.send(piece).await.is_ok() && !failed;
    }
    drop(send);
    match saving.await {
        Ok(Ok(path)) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "path": path })),
        )
            .into_response(),
        Ok(Err(e)) if storage_full(&e) => said(
            StatusCode::INSUFFICIENT_STORAGE,
            "There is not enough room on this disk for that picture.",
        ),
        Ok(Err(e)) => said(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("leo could not save the picture: {e:#}"),
        ),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
