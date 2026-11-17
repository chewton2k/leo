use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::routes::uploads::{originals_dir, safe_file_name};
use crate::{export, AppState};

const MOST_ZIPS: usize = 2;
pub(crate) fn zip_slots() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(MOST_ZIPS))
}

struct ZipStream {
    file: tokio::fs::File,
    _path: tempfile::TempPath,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

async fn zip_download(
    slots: Arc<Semaphore>,
    build: impl FnOnce(&std::fs::File) -> anyhow::Result<()> + Send + 'static,
    file_name: String,
) -> Response {
    zip_download_with_slots(slots, tempfile::NamedTempFile::new, build, file_name).await
}

async fn zip_download_with_slots(
    slots: Arc<Semaphore>,
    temporary: impl FnOnce() -> std::io::Result<tempfile::NamedTempFile> + Send + 'static,
    build: impl FnOnce(&std::fs::File) -> anyhow::Result<()> + Send + 'static,
    file_name: String,
) -> Response {
    let Ok(permit) = slots.try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "Two downloads are still being prepared or sent. Finish or close one, then try again."})),
        ).into_response();
    };
    let made = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        use std::io::Seek;
        let tmp = temporary()?;
        build(tmp.as_file())?;
        let (mut file, path) = tmp.into_parts();
        file.rewind()?;
        let size = file.metadata()?.len();
        Ok((file, path, size, permit))
    })
    .await;
    let Ok(Ok((file, path, size, permit))) = made else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "The zip could not be made. Export fewer parts, check folder permissions and free disk space, then try again. Each export allows up to 10 GiB, 100,000 items and 64 folder levels." })),
        )
            .into_response();
    };
    let held = ZipStream {
        file: tokio::fs::File::from_std(file),
        _path: path,
        _permit: permit,
    };
    let stream = futures_util::stream::unfold(Some(held), |held| async move {
        use tokio::io::AsyncReadExt;
        let mut held = held?;
        let mut chunk = vec![0u8; 64 * 1024];
        match held.file.read(&mut chunk).await {
            Ok(0) => None,
            Ok(n) => {
                chunk.truncate(n);
                Some((Ok::<_, std::io::Error>(chunk), Some(held)))
            }
            Err(e) => Some((Err(e), None)),
        }
    });
    let mut response = axum::body::Body::from_stream(stream).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    if let Ok(value) = HeaderValue::from_str(&attachment_header(&file_name)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

pub(crate) fn attachment_header(file_name: &str) -> String {
    let plain: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = file_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

pub(crate) async fn export_zip(
    State(state): State<AppState>,
    Query(parts): Query<export::Parts>,
) -> Response {
    let notes_dir = state.notes_dir.clone();
    let chats = state.chats.clone();
    zip_download(
        Arc::clone(&state.zip_slots),
        move |file| export::write_zip(file, &notes_dir, &chats, parts).map(|_| ()),
        format!("leo-export-{}.zip", chrono::Local::now().format("%Y-%m-%d")),
    )
    .await
}

pub(crate) async fn originals_zip(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let found = state
        .read_store(move |store| {
            let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
            let folder = originals_dir(&store.notes_dir, &note.id).ok_or(StatusCode::NOT_FOUND)?;
            Ok((
                folder,
                note.title.clone(),
                store.notes_dir.clone(),
                note.id.clone(),
            ))
        })
        .await;
    let (folder, title, notes_dir, note) = match found {
        Ok(found) if found.0.is_dir() => found,
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(code) => return code.into_response(),
    };
    let base = safe_file_name(&title);
    let name = format!("{} originals.zip", base.trim_end_matches(".zip"));
    zip_download(
        Arc::clone(&state.zip_slots),
        move |file| {
            let checked = originals_dir(&notes_dir, &note).ok_or_else(|| {
                anyhow::anyhow!("Originals must stay inside the attachments folder")
            })?;
            if checked != folder {
                anyhow::bail!("The originals folder changed; try again");
            }
            export::write_folder_from(
                file,
                notes_dir.parent().unwrap_or(&notes_dir),
                &checked,
                &base,
            )
            .map(|_| ())
        },
        name,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn example_download(slots: Arc<Semaphore>, folder: std::path::PathBuf) -> Response {
        zip_download_with_slots(
            slots,
            move || tempfile::NamedTempFile::new_in(folder),
            |mut file| {
                use std::io::Write;
                file.write_all(b"example")?;
                Ok(())
            },
            "example.zip".into(),
        )
        .await
    }

    #[tokio::test]
    async fn zip_slots_and_temporary_files_last_until_stream_completion_or_drop() {
        let folder = tempfile::tempdir().unwrap();
        let slots = Arc::new(Semaphore::new(MOST_ZIPS));
        let first = example_download(Arc::clone(&slots), folder.path().to_path_buf()).await;
        let second = example_download(Arc::clone(&slots), folder.path().to_path_buf()).await;
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(slots.available_permits(), 0);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 2);
        let denied = zip_download_with_slots(
            Arc::clone(&slots),
            || panic!("a refused download must not make a file"),
            |_| panic!("a refused download must not start a builder"),
            "example.zip".into(),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
        drop(first);
        assert_eq!(slots.available_permits(), 1);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
        let bytes = axum::body::to_bytes(second.into_body(), 16).await.unwrap();
        assert_eq!(&bytes[..], b"example");
        assert_eq!(slots.available_permits(), MOST_ZIPS);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);

        let temporary = folder.path().to_path_buf();
        let failed = zip_download_with_slots(
            Arc::clone(&slots),
            move || tempfile::NamedTempFile::new_in(temporary),
            |_| anyhow::bail!("Example builder failure"),
            "example.zip".into(),
        )
        .await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(slots.available_permits(), MOST_ZIPS);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
        let failed = zip_download_with_slots(
            Arc::clone(&slots),
            || Err(std::io::Error::other("Example temporary file failure")),
            |_| panic!("a failed temporary file must not start a builder"),
            "example.zip".into(),
        )
        .await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(slots.available_permits(), MOST_ZIPS);
    }

    #[tokio::test]
    async fn a_cancelled_request_keeps_its_slot_until_the_blocking_builder_finishes() {
        let folder = tempfile::tempdir().unwrap();
        let slots = Arc::new(Semaphore::new(1));
        let temporary = folder.path().to_path_buf();
        let (started, began) = tokio::sync::oneshot::channel();
        let (release, waiting) = std::sync::mpsc::channel();
        let request = tokio::spawn(zip_download_with_slots(
            Arc::clone(&slots),
            move || tempfile::NamedTempFile::new_in(temporary),
            move |_| {
                started.send(()).unwrap();
                waiting.recv_timeout(std::time::Duration::from_secs(10))?;
                Ok(())
            },
            "example.zip".into(),
        ));
        began.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert_eq!(slots.available_permits(), 0);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while slots.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(slots.available_permits(), 1);
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }
}
