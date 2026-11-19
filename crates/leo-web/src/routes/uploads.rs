use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leo_core::attachments;
use serde::Deserialize;

use crate::routes::notes::{directory, save};
use crate::{read_now, store_now, AppState, Figure, Importer, Made, UploadFile};

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ImportJob {
    pub(crate) state: &'static str,
    pub(crate) label: String,
    pub(crate) dir: String,
    pub(crate) step: String,
    pub(crate) done: usize,
    pub(crate) total: usize,
    pub(crate) note: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) notes: Vec<String>,
    pub(crate) error: Option<String>,
    #[serde(
        rename = "section_chars",
        serialize_with = "count_chars",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) section: Option<String>,
}

fn count_chars<S: serde::Serializer>(section: &Option<String>, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_u64(section.as_deref().map_or(0, |s| s.chars().count() as u64))
}

pub(crate) const IMPORT_JSON_BYTES: usize = 8 * 1024 * 1024;
const ORIGINAL_PIECE: usize = 64 * 1024;
const MOST_IMPORTS: usize = 2;
const KEPT_IMPORTS: usize = 40;
const UPLOAD_IDLE: std::time::Duration = std::time::Duration::from_secs(60);
const INCOMING: &str = leo_core::paths::own::INCOMING;
const INCOMING_KEPT: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

type Task = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Queue {
    waiting: std::collections::VecDeque<Task>,
    running: usize,
}

#[derive(Default)]
pub(crate) struct ImportSlots {
    queue: std::sync::Mutex<Queue>,
}

impl ImportSlots {
    fn submit(
        self: &std::sync::Arc<Self>,
        jobs: &std::sync::Mutex<std::collections::HashMap<String, ImportJob>>,
        id: &str,
        mut job: ImportJob,
        task: Task,
    ) -> std::io::Result<()> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let starts = queue.running < MOST_IMPORTS;
        job.state = if starts { "working" } else { "waiting" };
        reserve_import(
            &mut jobs.lock().unwrap_or_else(|e| e.into_inner()),
            id.to_string(),
            job,
        );
        queue.waiting.push_back(task);
        if starts {
            queue.running += 1;
            let slots = std::sync::Arc::clone(self);
            if let Err(error) = std::thread::Builder::new()
                .name("leo-import".into())
                .spawn(move || slots.work())
            {
                queue.running -= 1;
                queue.waiting.pop_back();
                jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
                return Err(error);
            }
        }
        Ok(())
    }

    fn work(&self) {
        loop {
            let task = {
                let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                match queue.waiting.pop_front() {
                    Some(task) => task,
                    None => {
                        queue.running -= 1;
                        return;
                    }
                }
            };
            task();
        }
    }
}

fn reserve_import(
    jobs: &mut std::collections::HashMap<String, ImportJob>,
    id: String,
    job: ImportJob,
) {
    if jobs.len() >= KEPT_IMPORTS {
        let finished = jobs
            .iter()
            .find(|(_, job)| !matches!(job.state, "waiting" | "working"))
            .map(|(id, _)| id.clone());
        if let Some(id) = finished {
            jobs.remove(&id);
        }
    }
    jobs.insert(id, job);
}

fn incoming_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn incoming_root(notes_dir: &std::path::Path) -> std::path::PathBuf {
    crate::db::beside_of(notes_dir, None).join(INCOMING)
}

fn incoming_dir(notes_dir: &std::path::Path, upload: &str) -> Option<std::path::PathBuf> {
    if !incoming_name(upload) {
        return None;
    }
    let data = crate::db::beside_of(notes_dir, None);
    leo_core::paths::contained_path(data, &std::path::Path::new(INCOMING).join(upload)).ok()
}

struct Incoming(Option<std::path::PathBuf>);

impl Drop for Incoming {
    fn drop(&mut self) {
        if let Some(folder) = self.0.take() {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

pub(crate) fn sweep_incoming(notes_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(incoming_root(notes_dir)) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|at| at.elapsed().is_ok_and(|age| age > INCOMING_KEPT));
        if old && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn refused(code: StatusCode, error: impl Into<String>) -> Response {
    (code, Json(serde_json::json!({ "error": error.into() }))).into_response()
}

#[derive(Deserialize, Default)]
pub(crate) struct Named {
    #[serde(default)]
    pub(crate) name: String,
}

pub(crate) async fn put_file(
    State(state): State<AppState>,
    Path((upload, item)): Path<(String, String)>,
    Query(named): Query<Named>,
    body: axum::body::Body,
) -> Response {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let name: String = named.name.chars().take(MOST_NAME_CHARS).collect();
    let name = if name.is_empty() {
        "the file".to_string()
    } else {
        name
    };
    let (Some(folder), true) = (
        incoming_dir(&state.notes_dir, &upload),
        incoming_name(&item),
    ) else {
        return refused(StatusCode::BAD_REQUEST, "Choose the files again.");
    };
    let target = folder.join(&item);
    let started = {
        let (folder, target) = (folder.clone(), target.clone());
        tokio::task::spawn_blocking(
            move || -> std::io::Result<Option<tempfile::NamedTempFile>> {
                if target.exists() {
                    return Ok(None);
                }
                std::fs::create_dir_all(&folder)?;
                tempfile::NamedTempFile::new_in(&folder).map(Some)
            },
        )
        .await
    };
    let temp = match started {
        Ok(Ok(Some(temp))) => temp,
        Ok(Ok(None)) => {
            return refused(StatusCode::CONFLICT, format!("{name} was already sent."));
        }
        Ok(Err(error)) => return write_refused(&error, &name),
        Err(_) => return refused(StatusCode::INTERNAL_SERVER_ERROR, "leo hit a problem."),
    };
    let (std_file, path) = temp.into_parts();
    let mut file = tokio::fs::File::from_std(std_file);
    let mut stream = body.into_data_stream();
    loop {
        let piece = match tokio::time::timeout(UPLOAD_IDLE, stream.next()).await {
            Ok(piece) => piece,
            Err(_) => {
                return refused(
                    StatusCode::REQUEST_TIMEOUT,
                    format!("{name} stopped arriving. Check your connection and try again."),
                );
            }
        };
        match piece {
            None => break,
            Some(Ok(piece)) => {
                if let Err(error) = file.write_all(&piece).await {
                    return write_refused(&error, &name);
                }
            }
            Some(Err(_)) => {
                return refused(
                    StatusCode::BAD_REQUEST,
                    format!("{name} did not arrive intact; try again."),
                );
            }
        }
    }
    if let Err(error) = file.sync_all().await {
        return write_refused(&error, &name);
    }
    drop(file);
    match tokio::task::spawn_blocking(move || path.persist_noclobber(&target)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            refused(StatusCode::CONFLICT, format!("{name} was already sent."))
        }
        Ok(Err(error)) => write_refused(&error.error, &name),
        Err(_) => refused(StatusCode::INTERNAL_SERVER_ERROR, "leo hit a problem."),
    }
}

fn write_refused(error: &std::io::Error, name: &str) -> Response {
    if error.kind() == std::io::ErrorKind::StorageFull {
        return refused(
            StatusCode::INSUFFICIENT_STORAGE,
            format!("There is not enough room on this disk for {name}."),
        );
    }
    refused(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("leo could not save {name}: {error}"),
    )
}

#[derive(Deserialize)]
pub(crate) struct SentFile {
    pub(crate) item: String,
    pub(crate) name: String,
    #[serde(default, rename = "type")]
    pub(crate) mime: String,
}

#[derive(Deserialize)]
pub(crate) struct ImportBody {
    pub(crate) upload: String,
    #[serde(default)]
    pub(crate) directory: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) wants: String,
    pub(crate) files: Vec<SentFile>,
    #[serde(default)]
    pub(crate) each: bool,
    /// A note to add the written section to, instead of making a new note
    /// (files dropped on an open note, 2026-11-03).
    #[serde(default)]
    pub(crate) into: Option<String>,
}

pub(crate) const MOST_WANTS: usize = 2_000;
const MOST_NAME_CHARS: usize = 120;
const MOST_NAME_BYTES: usize = 200;
const MOST_EXTENSION_CHARS: usize = 10;

fn cut(stem: &str, chars: usize, bytes: usize) -> String {
    let mut out = String::new();
    for c in stem.chars().take(chars) {
        if out.len() + c.len_utf8() > bytes {
            break;
        }
        out.push(c);
    }
    out
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at)
            if at > 0
                && (2..=MOST_EXTENSION_CHARS + 1).contains(&(name.len() - at))
                && name[at + 1..].bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            name.split_at(at)
        }
        _ => (name, ""),
    }
}

pub fn safe_file_name(name: &str) -> String {
    let cleaned = stored_file_name(name);
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        "upload".to_string()
    } else if leo_core::filename::reserved_on_windows(cleaned) {
        let (stem, rest) = cleaned.split_at(cleaned.find('.').unwrap_or(cleaned.len()));
        format!("{stem}-{rest}")
    } else {
        cleaned.to_string()
    }
}

fn stored_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    let (stem, extension) = split_extension(&cleaned);
    let room = MOST_NAME_CHARS.saturating_sub(extension.len());
    let cleaned = format!(
        "{}{extension}",
        cut(stem, room, MOST_NAME_BYTES.saturating_sub(extension.len()))
    );
    if cleaned.is_empty() {
        "upload".to_string()
    } else {
        cleaned
    }
}

pub(crate) fn originals_dir(notes_dir: &std::path::Path, note: &str) -> Option<std::path::PathBuf> {
    if note.is_empty() || !note.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let data = crate::db::beside_of(notes_dir, None);
    leo_core::paths::contained_path(data, &std::path::Path::new("attachments").join(note)).ok()
}

pub(crate) fn original_path(
    notes_dir: &std::path::Path,
    note: &str,
    name: &str,
) -> Option<std::path::PathBuf> {
    if name.is_empty() || name != stored_file_name(name) {
        return None;
    }
    originals_dir(notes_dir, note)?;
    let relative = std::path::Path::new("attachments").join(note).join(name);
    leo_core::paths::contained_path(crate::db::beside_of(notes_dir, None), &relative).ok()
}

fn stage_originals(files: &[UploadFile]) -> std::io::Result<Vec<(String, std::path::PathBuf)>> {
    files
        .iter()
        .map(|file| {
            let path = file.path.clone().ok_or_else(|| {
                std::io::Error::other("An uploaded file must wait on disk until its note is made")
            })?;
            Ok((safe_file_name(&file.name), path))
        })
        .collect()
}

fn save_original(
    notes_dir: &std::path::Path,
    note: &str,
    name: &str,
    source: &std::path::Path,
) -> std::io::Result<()> {
    let invalid =
        || std::io::Error::other("Original files must stay inside the attachments folder");
    let folder = originals_dir(notes_dir, note).ok_or_else(invalid)?;
    std::fs::create_dir_all(&folder)?;
    let path = original_path(notes_dir, note, name).ok_or_else(invalid)?;
    if std::fs::hard_link(source, &path).is_ok() {
        return Ok(());
    }
    let mut out = tempfile::NamedTempFile::new_in(&folder)?;
    std::io::copy(&mut leo_core::paths::open_regular(source)?, &mut out)?;
    out.as_file().sync_all()?;
    original_path(notes_dir, note, name).ok_or_else(invalid)?;
    out.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

pub(crate) fn share_originals(
    notes_dir: &std::path::Path,
    from: &str,
    into: &str,
) -> (Vec<String>, std::io::Result<()>) {
    let mut written = Vec::new();
    let shared = crate::originals::files(notes_dir, from)
        .into_iter()
        .try_for_each(|(name, _)| {
            let name = share_original(notes_dir, from, into, &name, &written)?;
            written.push(name);
            Ok(())
        });
    (written, shared)
}

fn share_original(
    notes_dir: &std::path::Path,
    from: &str,
    into: &str,
    name: &str,
    taken: &[String],
) -> std::io::Result<String> {
    let invalid =
        || std::io::Error::other("Original files must stay inside the attachments folder");
    let source = original_path(notes_dir, from, name).ok_or_else(invalid)?;
    let free = free_original_name(notes_dir, into, name, taken);
    let folder = originals_dir(notes_dir, into).ok_or_else(invalid)?;
    std::fs::create_dir_all(&folder)?;
    let target = original_path(notes_dir, into, &free).ok_or_else(invalid)?;
    if std::fs::hard_link(&source, &target).is_err() {
        let mut out = tempfile::NamedTempFile::new_in(&folder)?;
        std::io::copy(&mut leo_core::paths::open_regular(&source)?, &mut out)?;
        out.as_file().sync_all()?;
        original_path(notes_dir, into, &free).ok_or_else(invalid)?;
        out.persist_noclobber(&target)
            .map_err(|error| error.error)?;
    }
    Ok(free)
}

fn open_original(
    notes_dir: &std::path::Path,
    note: &str,
    name: &str,
) -> std::io::Result<(std::fs::File, u64)> {
    let path = original_path(notes_dir, note, name).ok_or_else(|| {
        std::io::Error::other("Original files must stay inside the attachments folder")
    })?;
    let file = leo_core::paths::open_regular(&path)?;
    let size = file.metadata()?.len();
    Ok((file, size))
}

fn set_job(state: &AppState, id: &str, change: impl FnOnce(&mut ImportJob)) {
    if let Some(job) = state
        .imports
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(id)
    {
        change(job);
    }
}

fn figure_number(target: &str) -> Option<usize> {
    target.strip_prefix("figure:")?.parse().ok()
}

pub(crate) fn settle_figures(
    body: &str,
    figures: &[Figure],
    save: &mut dyn FnMut(&Figure) -> Option<String>,
) -> String {
    let mut saved: Vec<Option<Option<String>>> = vec![None; figures.len()];
    let mut lines = Vec::new();
    for line in body.lines() {
        let found = attachments::pictures_in(line);
        if !found.iter().any(|(_, s)| s.target.starts_with("figure:")) {
            lines.push(line.to_string());
            continue;
        }
        let mut out = String::new();
        let mut at = 0;
        for (range, shown) in found {
            out.push_str(&line[at..range.start]);
            at = range.end;
            let Some(n) = figure_number(&shown.target) else {
                out.push_str(&line[range]);
                continue;
            };
            let Some(figure) = n.checked_sub(1).and_then(|i| figures.get(i)) else {
                continue;
            };
            let slot = &mut saved[n - 1];
            if slot.is_some() || figure.photo {
                continue;
            }
            let path = save(figure);
            if let Some(path) = &path {
                out.push_str(&format!("![{}]({path})", shown.alt));
            }
            *slot = Some(path);
        }
        out.push_str(&line[at..]);
        if !out.trim().is_empty() || line.trim().is_empty() {
            lines.push(out.trim_end().to_string());
        }
    }
    let mut body = lines.join("\n");
    let mut section = |heading: &str, which: &dyn Fn(&Figure) -> bool| {
        let shown: Vec<String> = figures
            .iter()
            .enumerate()
            .filter(|(i, f)| which(f) && saved[*i].is_none())
            .filter_map(|(_, f)| {
                let caption = if f.photo {
                    f.place.clone()
                } else {
                    format!("Figure from {}", f.place)
                };
                save(f).map(|path| format!("![{caption}]({path})"))
            })
            .collect();
        if !shown.is_empty() {
            body.push_str(&format!("\n\n## {heading}\n\n{}", shown.join("\n\n")));
        }
    };
    section("More figures", &|f| !f.photo);
    section("Photos", &|f| f.photo);
    body
}

struct Missed {
    note: Option<String>,
    error: String,
}

/// What an upload wrote: the note it went to, and for an upload added to an
/// open note, the section the page puts into that note (`into`).
struct Wrote {
    note: String,
    section: Option<String>,
    warning: Option<String>,
}

/// A name for an original that does not replace one the note already keeps:
/// `slides.pdf`, then `slides (2).pdf`, `slides (3).pdf`...
fn free_original_name(
    notes_dir: &std::path::Path,
    note: &str,
    name: &str,
    taken: &[String],
) -> String {
    let (stem, ext) = split_extension(name);
    let used = |candidate: &str| {
        taken.iter().any(|t| t == candidate)
            || original_path(notes_dir, note, candidate).is_some_and(|p| p.exists())
    };
    if !used(name) {
        return name.to_string();
    }
    (2..1000)
        .map(|n| {
            let number = format!(" ({n})");
            let room = MOST_NAME_CHARS.saturating_sub(ext.len() + number.len());
            let bytes = MOST_NAME_BYTES.saturating_sub(ext.len() + number.len());
            format!("{}{number}{ext}", cut(stem, room, bytes))
        })
        .find(|candidate| !used(candidate))
        .unwrap_or_else(|| name.to_string())
}

#[allow(clippy::too_many_arguments)]
fn write_upload(
    state: &AppState,
    importer: &Importer,
    dir: &str,
    into: Option<&str>,
    title: Option<String>,
    wants: &str,
    files: Vec<UploadFile>,
    step: &mut dyn FnMut(&str, usize, usize),
) -> Result<Wrote, Missed> {
    let missed = |note: Option<String>, error: &str| Missed {
        note,
        error: error.to_string(),
    };
    let gone = "The note to add to is gone. Open another note, or make a new note from the file.";
    let mut originals = stage_originals(&files).map_err(|_| {
        missed(
            None,
            "The originals could not be kept. Check that there is free disk space and try again.",
        )
    })?;
    let Made {
        title: made_title,
        body,
        figures,
        scratch,
    } = importer(files, wants, step).map_err(|e| missed(None, &e.to_string()))?;
    // Originals take free names, so a file of the same name in this upload or
    // an earlier one (and its citations) stays as it was.
    let (notes_dir, id) = match into {
        Some(id) => {
            let notes_dir = store_now(state, |store| {
                if store.notes.iter().any(|n| n.id == id) {
                    Ok(store.notes_dir.clone())
                } else {
                    Err(StatusCode::NOT_FOUND)
                }
            })
            .map_err(|_| missed(None, gone))?;
            (notes_dir, id)
        }
        None => (std::path::PathBuf::new(), ""),
    };
    let mut taken = Vec::new();
    for (name, _) in originals.iter_mut() {
        let free = free_original_name(&notes_dir, id, name, &taken);
        taken.push(free.clone());
        *name = free;
    }
    let names: Vec<String> = originals.iter().map(|(name, _)| name.clone()).collect();
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| (!made_title.trim().is_empty()).then(|| made_title.trim().to_string()))
        .unwrap_or_else(|| names.first().cloned().unwrap_or_else(|| "Upload".into()));
    let footer = format!(
        "\n\n---\n*From {}, uploaded {}.*",
        names.join(", "),
        chrono::Local::now().format("%b %-d, %Y")
    );
    let stem = names
        .first()
        .and_then(|n| n.rsplit_once('.').map(|(s, _)| s.to_string()))
        .unwrap_or_else(|| "upload".into());
    let pictures_dir = read_now(state, |store| Ok(store.notes_dir.clone()))
        .map_err(|_| missed(None, "The note could not be saved."))?;
    let body = settle_figures(&body, &figures, &mut |figure| {
        use std::io::Read as _;
        let mut file = leo_core::paths::open_regular(&figure.path).ok()?;
        let mut head = Vec::new();
        (&mut file)
            .take(attachments::HEAD_BYTES as u64)
            .read_to_end(&mut head)
            .ok()?;
        let kind = attachments::kind_of(&head)?;
        let name = if figure.photo {
            let named = figure
                .place
                .rsplit_once('.')
                .map_or(figure.place.as_str(), |(s, _)| s);
            format!("{named}.{kind}")
        } else {
            format!("{stem} {}.{kind}", figure.place)
        };
        attachments::save_from(&pictures_dir, &name, &head, file).ok()
    });
    drop(scratch);
    let made = store_now(state, |store| {
        if into.is_none() && !store.dir_exists(dir) {
            store.create_dir(dir);
        }
        let written = format!("{}{footer}", body.trim());
        let wrote = match into {
            // Added to an open note: the page puts the section in through the
            // version-checked insert, as the note's editor knows its version.
            Some(id) => {
                if !store.notes.iter().any(|n| n.id == id) {
                    return Err(StatusCode::NOT_FOUND);
                }
                let heading = title.split(['\r', '\n']).collect::<Vec<_>>().join(" ");
                Wrote {
                    note: id.to_string(),
                    section: Some(format!("## {}\n\n{written}", heading.trim())),
                    warning: None,
                }
            }
            None => {
                let note = store
                    .create_note(title, written, vec![], dir)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .id
                    .clone();
                save(store)?;
                Wrote {
                    note,
                    section: None,
                    warning: None,
                }
            }
        };
        Ok((wrote, store.notes_dir.clone()))
    });
    let (mut wrote, notes_dir) = made.map_err(|code| {
        missed(
            None,
            if code == StatusCode::NOT_FOUND {
                gone
            } else {
                "The note could not be saved."
            },
        )
    })?;
    if originals
        .iter_mut()
        .any(|(name, source)| save_original(&notes_dir, &wrote.note, name, source).is_err())
    {
        if wrote.section.is_some() {
            // Nothing is in the note yet, so the section still goes in; only
            // the originals are missing, and the job says so.
            wrote.warning = Some("The section is ready, but the original files could not be kept beside the note. Check the attachments folder permissions.".into());
            return Ok(wrote);
        }
        return Err(missed(Some(wrote.note), "The note was saved, but its originals could not be kept. Check the attachments folder permissions and try again."));
    }
    Ok(wrote)
}

#[allow(clippy::too_many_arguments)]
fn run_import(
    state: AppState,
    id: String,
    importer: Importer,
    dir: String,
    into: Option<String>,
    title: Option<String>,
    wants: String,
    files: Vec<UploadFile>,
    incoming: Incoming,
) {
    let mut step = |text: &str, done: usize, total: usize| {
        set_job(&state, &id, |job| {
            job.step = text.to_string();
            job.done = done;
            job.total = total;
        })
    };
    let made = write_upload(
        &state,
        &importer,
        &dir,
        into.as_deref(),
        title,
        &wants,
        files,
        &mut step,
    );
    drop(incoming);
    set_job(&state, &id, |job| match made {
        Ok(wrote) => {
            job.state = "done";
            job.done = job.total.max(1);
            job.notes = vec![wrote.note.clone()];
            job.note = Some(wrote.note);
            job.section = wrote.section;
            job.error = wrote.warning;
        }
        Err(missed) => {
            job.state = "failed";
            job.note = missed.note;
            job.error = Some(missed.error);
        }
    });
}

fn run_each(
    state: AppState,
    id: String,
    importer: Importer,
    dir: String,
    wants: String,
    files: Vec<UploadFile>,
    incoming: Incoming,
) {
    let count = files.len();
    let mut notes = Vec::new();
    let mut problems = Vec::new();
    for (i, file) in files.into_iter().enumerate() {
        let name = file.name.clone();
        let mut step = |text: &str, _: usize, _: usize| {
            set_job(&state, &id, |job| {
                job.step = format!("{text} · {name} ({} of {count})", i + 1);
                job.done = i;
                job.total = count;
            })
        };
        match write_upload(
            &state,
            &importer,
            &dir,
            None,
            None,
            &wants,
            vec![file],
            &mut step,
        ) {
            Ok(wrote) => notes.push(wrote.note),
            Err(missed) => {
                notes.extend(missed.note);
                problems.push(format!("{name}: {}", missed.error));
            }
        }
    }
    drop(incoming);
    set_job(&state, &id, |job| {
        job.done = count;
        job.total = count;
        job.state = if notes.is_empty() { "failed" } else { "done" };
        job.note = notes.first().cloned();
        job.notes = notes;
        job.error = (!problems.is_empty()).then(|| problems.join("\n"));
    });
}

pub(crate) async fn start_import(
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> Response {
    let Some(folder) = incoming_dir(&state.notes_dir, &body.upload) else {
        return refused(StatusCode::BAD_REQUEST, "Choose the files again.");
    };
    let incoming = Incoming(Some(folder.clone()));
    let Some(importer) = state.importer.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": crate::no_writer() })),
        )
            .into_response();
    };
    if body.files.is_empty() {
        return refused(StatusCode::BAD_REQUEST, "Choose a file first.");
    }
    if let Some(note) = body.into.clone() {
        let found = state
            .read_store(move |store| Ok(store.notes.iter().any(|n| n.id == note)))
            .await
            .unwrap_or(false);
        if !found {
            return refused(
                StatusCode::NOT_FOUND,
                "The note to add to is gone. Open another note, or make a new note from the file.",
            );
        }
    }
    if body.directory.len() > 1_024
        || body
            .files
            .iter()
            .any(|file| file.name.len() > 512 || file.mime.len() > 128)
    {
        return refused(StatusCode::PAYLOAD_TOO_LARGE, "Use short file names.");
    }
    let dir = body.directory.trim().to_string();
    let checked = state.read_store({
        let dir = dir.clone();
        move |store| directory(store, &dir)
    });
    if checked.await.is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let uploads = body.files;
    let found = tokio::task::spawn_blocking(move || -> Result<Vec<UploadFile>, String> {
        let mut seen = std::collections::HashSet::new();
        uploads
            .into_iter()
            .map(|file| {
                let path = folder.join(&file.item);
                let arrived = incoming_name(&file.item)
                    && seen.insert(file.item.clone())
                    && std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
                if arrived {
                    Ok(UploadFile {
                        name: safe_file_name(&file.name),
                        mime: file.mime,
                        bytes: Vec::new(),
                        path: Some(path),
                    })
                } else {
                    Err(format!("{} did not arrive intact; try again.", file.name))
                }
            })
            .collect()
    })
    .await
    .unwrap_or_else(|_| Err("leo hit a problem.".into()));
    let files = match found {
        Ok(files) => files,
        Err(error) => return refused(StatusCode::BAD_REQUEST, error),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let into = body.into.clone();
    let each = body.each && files.len() > 1 && into.is_none();
    let label = if into.is_some() {
        upload_label(&files).replacen("Making a note from", "Adding to a note from", 1)
    } else if each {
        format!("Making {} notes, one from each file", files.len())
    } else {
        upload_label(&files)
    };
    let job = ImportJob {
        state: "waiting",
        label,
        dir: dir.clone(),
        step: "Uploading".into(),
        done: 0,
        total: 1,
        note: None,
        notes: Vec::new(),
        error: None,
        section: None,
    };
    let worker = state.clone();
    let job_id = id.clone();
    let wants: String = body.wants.chars().take(MOST_WANTS).collect();
    let title = body.title;
    let task: Task = Box::new(move || {
        set_job(&worker, &job_id, |job| job.state = "working");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if each {
                run_each(
                    worker.clone(),
                    job_id.clone(),
                    importer,
                    dir,
                    wants,
                    files,
                    incoming,
                )
            } else {
                run_import(
                    worker.clone(),
                    job_id.clone(),
                    importer,
                    dir,
                    into,
                    title,
                    wants,
                    files,
                    incoming,
                )
            }
        }));
        if outcome.is_err() {
            set_job(&worker, &job_id, |job| {
                job.state = "failed";
                job.error =
                    Some("The upload stopped unexpectedly. Try again with fewer files.".into());
            });
        }
    });
    if state
        .import_slots
        .submit(&state.imports, &id, job, task)
        .is_err()
    {
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "The upload could not start. Close another busy task and try again.",
        );
    }
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "id": id }))).into_response()
}

pub(crate) fn upload_label(files: &[UploadFile]) -> String {
    match files {
        [] => "Making a note".to_string(),
        [one] => format!("Making a note from {}", one.name),
        [first, rest @ ..] => format!("Making a note from {} and {} more", first.name, rest.len()),
    }
}

pub(crate) async fn import_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let job = state
        .imports
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
        .cloned();
    match job {
        Some(job) => Json(job).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn list_originals(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let notes_dir = state.notes_dir.clone();
    let Some(folder) = originals_dir(&notes_dir, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let files = tokio::task::spawn_blocking(move || {
        let mut files: Vec<serde_json::Value> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|kind| kind.is_file()))
                    .filter(|e| {
                        e.file_name()
                            .to_str()
                            .is_some_and(|name| original_path(&notes_dir, &id, name).is_some())
                    })
                    .map(|e| {
                        serde_json::json!({
                            "name": e.file_name().to_string_lossy(),
                            "size": e.metadata().map(|m| m.len()).unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        files
    })
    .await
    .unwrap_or_default();
    Json(files).into_response()
}

fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

#[derive(Deserialize, Default)]
pub(crate) struct OriginalView {
    #[serde(default)]
    pub(crate) view: Option<String>,
}

pub(crate) fn viewable(name: &str) -> bool {
    let mime = mime_for(name);
    mime == "application/pdf" || mime.starts_with("image/")
}

pub(crate) async fn get_original(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    Query(how): Query<OriginalView>,
) -> Response {
    let notes_dir = state.notes_dir.clone();
    let opened = {
        let (id, name) = (id.clone(), name.clone());
        tokio::task::spawn_blocking(move || open_original(&notes_dir, &id, &name)).await
    };
    match opened.unwrap_or_else(|_| Err(std::io::Error::other("The original could not be read"))) {
        Ok((file, size)) => {
            let ascii: String = name
                .chars()
                .map(|c| {
                    if c.is_ascii_graphic() || c == ' ' {
                        c
                    } else {
                        '_'
                    }
                })
                .filter(|c| *c != '"')
                .collect();
            let stream = futures_util::stream::try_unfold(
                (tokio::fs::File::from_std(file), vec![0u8; ORIGINAL_PIECE]),
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
            if let Ok(value) = HeaderValue::from_str(mime_for(&name)) {
                headers.insert(header::CONTENT_TYPE, value);
            }
            let inline = how.view.is_some() && viewable(&name);
            let disposition = if inline { "inline" } else { "attachment" };
            if let Ok(value) =
                HeaderValue::from_str(&format!("{disposition}; filename=\"{ascii}\""))
            {
                headers.insert(header::CONTENT_DISPOSITION, value);
            }
            if inline {
                headers.insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));
                headers.insert(
                    "content-security-policy",
                    HeaderValue::from_static("default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; frame-ancestors 'self'"),
                );
            }
            response
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    fn job(state: &'static str) -> ImportJob {
        ImportJob {
            state,
            label: String::new(),
            dir: String::new(),
            step: String::new(),
            done: 0,
            total: 1,
            note: None,
            notes: Vec::new(),
            error: None,
            section: None,
        }
    }

    #[test]
    fn import_admission_keeps_every_unfinished_job_and_caps_the_finished_ones() {
        let mut jobs = std::collections::HashMap::new();
        for n in 0..KEPT_IMPORTS * 2 {
            reserve_import(&mut jobs, format!("running-{n}"), job("working"));
        }
        assert_eq!(jobs.len(), KEPT_IMPORTS * 2);
        jobs.clear();
        reserve_import(&mut jobs, "a".into(), job("working"));
        reserve_import(&mut jobs, "b".into(), job("waiting"));
        for n in 0..KEPT_IMPORTS * 3 {
            reserve_import(&mut jobs, format!("done-{n}"), job("done"));
        }
        assert!(jobs.len() <= KEPT_IMPORTS);
        assert_eq!(jobs.get("a").unwrap().state, "working");
        assert_eq!(jobs.get("b").unwrap().state, "waiting");
    }
}
