use std::collections::HashSet;
use std::io::{Read, Seek, Write};
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Parts {
    #[serde(default)]
    pub uploads: bool,
    #[serde(default)]
    pub chats: bool,
    #[serde(default)]
    pub trash: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Packed {
    pub files: usize,
    pub bytes: u64,
}

const README: &str = "Everything leo exported.

notes/    your notes as Markdown files, in their folders. Any Markdown
          editor opens them; leo reads them back if you copy them into its
          notes folder.
uploads/  the files you uploaded, in a folder per note (named by the note's id,
          which is the id: line at the top of the note).
chats/    your conversations with Felix, one JSON file each.
tasks.json the tasks you added on the Calendar page. Tasks from notes are
          in the notes themselves.
boards.json your boards and their cards.
margin.json Felix's margin notes and their replies, included with chats.
trash/    notes in the trash, as Markdown.
recording-sources/ retained transcripts, original points and diagnostics,
          included with uploads. Audio recovery and calendar links
          stay on your computer.

Settings and API keys are never included.
";

const MOST_DEPTH: usize = 64;
const PIECE: usize = 64 * 1024;
const LARGE_FROM: u64 = 1 << 30;
const IDS_AT_A_TIME: i64 = 500;
const DOCUMENT_CHARS: usize = 1 << 20;

#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(not(unix))]
type Identity = std::path::PathBuf;

#[cfg(unix)]
fn identity(_: &Path, meta: &std::fs::Metadata) -> Option<Identity> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn identity(path: &Path, _: &std::fs::Metadata) -> Option<Identity> {
    std::fs::canonicalize(path).ok()
}

#[derive(Default)]
struct Walk {
    skipped: usize,
    entered: HashSet<Identity>,
    packed: Packed,
}

fn stopped() -> Result<()> {
    ensure!(
        !leo_core::children::current_stop().is_some_and(|stop| stop.asked()),
        "The export was stopped."
    );
    Ok(())
}

fn copy_through(from: &mut impl Read, to: &mut impl Write) -> Result<u64> {
    let mut piece = vec![0u8; PIECE];
    let mut total = 0u64;
    loop {
        stopped()?;
        let read = match from.read(&mut piece) {
            Ok(0) => return Ok(total),
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        to.write_all(&piece[..read])?;
        total += read as u64;
    }
}

fn plain() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true)
}

fn checked_path(root: &Path, path: &Path) -> Result<std::path::PathBuf> {
    match std::fs::symlink_metadata(root) {
        Ok(meta) => ensure!(meta.is_dir(), "Export folders cannot use symbolic links; replace the link with a real folder and try again."),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("The export folder could not be checked; check its permissions and try again"),
    }
    let relative = path
        .strip_prefix(root)
        .context("Export files must stay inside Leo's data folder")?;
    if relative.as_os_str().is_empty() {
        return Ok(root.to_path_buf());
    }
    leo_core::paths::contained_path(root, relative)
        .context("Export folders cannot use symbolic links; replace the link with a real folder and try again")
}

fn add_dir<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    from: &Path,
    under: &str,
    skip_hidden: bool,
    depth: usize,
    walk: &mut Walk,
) -> Result<()> {
    stopped()?;
    if depth > MOST_DEPTH {
        leo_core::diag::warn(format!(
            "An export skipped {}: it lies deeper than leo reads.",
            from.display()
        ));
        return Ok(());
    }
    let from = checked_path(root, from)?;
    let entries = match std::fs::read_dir(&from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).context(
                "The export folder could not be read; check its permissions and try again",
            )
        }
    };
    if let Some(known) = std::fs::metadata(&from)
        .ok()
        .and_then(|meta| identity(&from, &meta))
    {
        if !walk.entered.insert(known) {
            leo_core::diag::warn(format!(
                "An export skipped {}: it had already been entered.",
                from.display()
            ));
            return Ok(());
        }
    }
    for entry in entries {
        stopped()?;
        let entry = entry
            .context("An export item could not be read; check folder permissions and try again")?;
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_hidden && name.starts_with('.') {
            continue;
        }
        if name.contains(['/', '\\']) || name.chars().any(char::is_control) {
            walk.skipped += 1;
            continue;
        }
        let kind = entry
            .file_type()
            .context("An export item could not be checked; check its permissions and try again")?;
        let inside = format!("{under}/{name}");
        if kind.is_dir() {
            add_dir(
                zip,
                root,
                &entry.path(),
                &inside,
                skip_hidden,
                depth + 1,
                walk,
            )?;
        } else if kind.is_file() {
            let path = checked_path(root, &entry.path())?;
            let mut file = match leo_core::paths::open_regular(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::Other => bail!(
                    "Only regular files can be exported; remove the special file and try again"
                ),
                opened => opened.context(
                    "An export file could not be opened safely; check its permissions and try again",
                )?,
            };
            let large = file.metadata()?.len() >= LARGE_FROM;
            zip.start_file(&inside, plain().large_file(large))?;
            walk.packed.bytes += copy_through(&mut file, zip)?;
            walk.packed.files += 1;
        }
    }
    Ok(())
}

struct Counted<'a, W> {
    out: &'a mut W,
    bytes: u64,
}

impl<W: Write> Write for Counted<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        stopped().map_err(std::io::Error::other)?;
        let written = self.out.write(bytes)?;
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

fn add_json<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    inside: &str,
    value: &impl Serialize,
    walk: &mut Walk,
) -> Result<()> {
    stopped()?;
    zip.start_file(inside, plain())?;
    let mut counted = Counted {
        out: &mut *zip,
        bytes: 0,
    };
    serde_json::to_writer_pretty(&mut counted, value)?;
    walk.packed.bytes += counted.bytes;
    walk.packed.files += 1;
    Ok(())
}

struct Cards<'a> {
    chats: &'a Path,
    boards: &'a [crate::boards::Board],
}

impl Serialize for Cards<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use crate::boards::Seat;
        use serde::ser::{Error, SerializeSeq};
        let mut seq = serializer.serialize_seq(None)?;
        for board in self.boards {
            for place in crate::boards::places_of(self.chats, board) {
                stopped().map_err(S::Error::custom)?;
                let Some(mut card) = crate::boards::card(self.chats, &place.id) else {
                    continue;
                };
                card.seat_move(place.seat_column().to_string());
                seq.serialize_element(&card)?;
            }
        }
        seq.end()
    }
}

struct Messages<'a> {
    chats: &'a Path,
    id: &'a str,
    count: usize,
}

impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeSeq};
        let mut seq = serializer.serialize_seq(Some(self.count))?;
        let mut from = 0;
        while from < self.count {
            stopped().map_err(S::Error::custom)?;
            let to = (from + crate::chats::PAGE).min(self.count);
            let Some(page) = crate::chats::page(self.chats, self.id, Some(to), to - from) else {
                return Err(S::Error::custom("A chat changed while it was exported"));
            };
            for message in &page.chat.messages {
                seq.serialize_element(message)?;
            }
            from = to;
        }
        seq.end()
    }
}

struct WholeChat<'a> {
    rest: serde_json::Map<String, serde_json::Value>,
    messages: Messages<'a>,
}

impl Serialize for WholeChat<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        for (key, value) in &self.rest {
            map.serialize_entry(key, value)?;
        }
        map.serialize_entry("messages", &self.messages)?;
        map.end()
    }
}

fn each_chat(chats: &Path, mut visit: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let mut after: Option<(String, String)> = None;
    loop {
        let (stamp, id) = after.clone().unwrap_or_default();
        let page: Vec<(String, String)> = crate::chats::db_of(chats).with(|connection| {
            let mut query = connection.prepare(
                "SELECT id, updated_at FROM chats
                 WHERE ?1 = '' OR updated_at < ?1 OR (updated_at = ?1 AND id > ?2)
                 ORDER BY updated_at DESC, id LIMIT ?3",
            )?;
            let rows = query.query_map(rusqlite::params![stamp, id, IDS_AT_A_TIME], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
            rows.collect()
        })?;
        for (id, _) in &page {
            visit(id)?;
        }
        match page.last() {
            Some((id, stamp)) if page.len() as i64 == IDS_AT_A_TIME => {
                after = Some((stamp.clone(), id.clone()));
            }
            _ => return Ok(()),
        }
    }
}

fn each_document(
    chats: &Path,
    chat: &str,
    mut visit: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut after: Option<(String, String)> = None;
    loop {
        let (stamp, id) = after.clone().unwrap_or_default();
        let page: Vec<(String, String)> = crate::chats::db_of(chats).with(|connection| {
            let mut query = connection.prepare(
                "SELECT id, added_at FROM chat_docs
                 WHERE chat = ?1 AND (?2 = '' OR added_at > ?2 OR (added_at = ?2 AND id > ?3))
                 ORDER BY added_at, id LIMIT ?4",
            )?;
            let rows = query
                .query_map(rusqlite::params![chat, stamp, id, IDS_AT_A_TIME], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;
            rows.collect()
        })?;
        for (id, _) in &page {
            visit(id)?;
        }
        match page.last() {
            Some((id, stamp)) if page.len() as i64 == IDS_AT_A_TIME => {
                after = Some((stamp.clone(), id.clone()));
            }
            _ => return Ok(()),
        }
    }
}

fn add_chat<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    chats: &Path,
    id: &str,
    walk: &mut Walk,
) -> Result<()> {
    let Some(head) = crate::chats::page(chats, id, Some(0), 1) else {
        return Ok(());
    };
    let serde_json::Value::Object(rest) = serde_json::to_value(&head.chat)? else {
        bail!("A chat could not be written; try the export again");
    };
    let rest = rest
        .into_iter()
        .filter(|(key, _)| key != "messages")
        .collect();
    let chat = WholeChat {
        rest,
        messages: Messages {
            chats,
            id: &head.chat.id,
            count: head.count,
        },
    };
    add_json(
        zip,
        &format!("leo/chats/{}.json", head.chat.id),
        &chat,
        walk,
    )?;
    each_document(chats, &head.chat.id, |doc| {
        ensure!(
            doc.len() == 32 && doc.chars().all(|c| c.is_ascii_hexdigit()),
            "A saved document has an invalid id; remove it from the chat and try again"
        );
        zip.start_file(
            format!("leo/chats/{}.files/{doc}.txt", head.chat.id),
            plain(),
        )?;
        let mut from = 1;
        loop {
            stopped()?;
            let piece = crate::chat_files::part(chats, &head.chat.id, doc, from, DOCUMENT_CHARS)
                .context("A saved document could not be read; try the export again")?;
            if piece.is_empty() {
                break;
            }
            zip.write_all(piece.as_bytes())?;
            walk.packed.bytes += piece.len() as u64;
            from += piece.chars().count();
        }
        walk.packed.files += 1;
        Ok(())
    })
}

pub fn write_zip<W: Write + Seek>(
    out: W,
    notes_dir: &Path,
    chats_dir: &Path,
    parts: Parts,
) -> Result<Packed> {
    let mut zip = zip::ZipWriter::new(out);
    let mut walk = Walk::default();
    zip.start_file("leo/README.txt", SimpleFileOptions::default())?;
    zip.write_all(README.as_bytes())?;
    let data = crate::keeping::data_dir(notes_dir);
    add_dir(&mut zip, &data, notes_dir, "leo/notes", true, 0, &mut walk)?;
    #[derive(Serialize)]
    struct TaskExport<'a> {
        version: u8,
        tasks: &'a [crate::tasks::Task],
    }
    let tasks = crate::tasks::load_all(chats_dir);
    if !tasks.is_empty() {
        add_json(
            &mut zip,
            "leo/tasks.json",
            &TaskExport {
                version: 1,
                tasks: &tasks,
            },
            &mut walk,
        )?;
    }
    drop(tasks);
    #[derive(Serialize)]
    struct BoardExport<'a> {
        version: u8,
        boards: &'a [crate::boards::Board],
        cards: Cards<'a>,
    }
    let boards = crate::boards::boards(chats_dir);
    if !boards.is_empty() {
        add_json(
            &mut zip,
            "leo/boards.json",
            &BoardExport {
                version: 1,
                boards: &boards,
                cards: Cards {
                    chats: chats_dir,
                    boards: &boards,
                },
            },
            &mut walk,
        )?;
    }
    #[derive(Serialize)]
    struct MarginExport<'a> {
        version: u8,
        threads: &'a [crate::margin::Thread],
    }
    let trashed = leo_core::store::trashed_ids(notes_dir);
    let threads: Vec<crate::margin::Thread> = if parts.chats {
        crate::margin::every(chats_dir)
            .into_iter()
            .filter(|t| parts.trash || !trashed.contains(&t.note))
            .collect()
    } else {
        Vec::new()
    };
    if !threads.is_empty() {
        add_json(
            &mut zip,
            "leo/margin.json",
            &MarginExport {
                version: 1,
                threads: &threads,
            },
            &mut walk,
        )?;
    }
    drop(threads);
    if parts.trash {
        add_dir(
            &mut zip,
            &data,
            &notes_dir.join(".trash"),
            "leo/trash",
            false,
            0,
            &mut walk,
        )?;
    }
    if parts.uploads {
        let uploads = data.join("attachments");
        add_dir(&mut zip, &data, &uploads, "leo/uploads", true, 0, &mut walk)?;
        add_dir(
            &mut zip,
            &data,
            &leo_core::recording::root(notes_dir),
            "leo/recording-sources",
            true,
            0,
            &mut walk,
        )?;
    }
    if parts.trash && parts.uploads {
        for kind in [
            leo_core::paths::own::ORIGINALS,
            leo_core::paths::own::SOURCES,
        ] {
            add_dir(
                &mut zip,
                &data,
                &data.join(leo_core::paths::own::HELD).join(kind),
                &format!("leo/trash/held/{kind}"),
                true,
                0,
                &mut walk,
            )?;
        }
    }
    if parts.chats {
        each_chat(chats_dir, |id| add_chat(&mut zip, chats_dir, id, &mut walk))?;
    }
    if walk.skipped > 0 {
        let note = format!(
            "{} file or folder name(s) with control characters could not be exported and were skipped.\n",
            walk.skipped
        );
        zip.start_file("leo/SKIPPED.txt", SimpleFileOptions::default())?;
        zip.write_all(note.as_bytes())?;
    }
    zip.finish()?;
    Ok(walk.packed)
}

pub fn write_folder<W: Write + Seek>(out: W, folder: &Path, under: &str) -> Result<Packed> {
    write_folder_from(out, folder.parent().unwrap_or(folder), folder, under)
}

pub(crate) fn write_folder_from<W: Write + Seek>(
    out: W,
    root: &Path,
    folder: &Path,
    under: &str,
) -> Result<Packed> {
    let mut zip = zip::ZipWriter::new(out);
    let mut walk = Walk::default();
    add_dir(&mut zip, root, folder, under, true, 0, &mut walk)?;
    zip.finish()?;
    Ok(walk.packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(bytes: Vec<u8>) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut listed: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        listed.sort();
        listed
    }

    #[test]
    fn held_files_require_both_trash_and_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let chats = dir.path().join("chats");
        std::fs::create_dir(&notes).unwrap();
        for kind in ["attachments", "recording-sources"] {
            let held = dir.path().join("held").join(kind).join("note");
            std::fs::create_dir_all(&held).unwrap();
            std::fs::write(held.join("source.txt"), kind).unwrap();
        }
        for (trash, uploads) in [(true, true), (false, true), (true, false)] {
            let mut out = std::io::Cursor::new(Vec::new());
            write_zip(
                &mut out,
                &notes,
                &chats,
                Parts {
                    trash,
                    uploads,
                    chats: false,
                },
            )
            .unwrap();
            let zip = zip::ZipArchive::new(out).unwrap();
            assert_eq!(
                zip.file_names()
                    .filter_map(Result::ok)
                    .filter(|name| name.starts_with("leo/trash/held/"))
                    .count(),
                if trash && uploads { 2 } else { 0 }
            );
        }
    }

    fn folder_zipped(folder: &Path) -> Result<Packed> {
        write_folder_from(std::io::Cursor::new(Vec::new()), folder, folder, "files")
    }

    #[cfg(unix)]
    #[test]
    fn a_file_with_a_control_character_in_its_name_is_skipped_not_fatal() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("Icon\r"), "").unwrap();
        std::fs::write(folder.path().join("Notes.md"), "ab").unwrap();
        assert_eq!(
            folder_zipped(folder.path()).unwrap(),
            Packed { files: 1, bytes: 2 }
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_roots_refuse_symlinks_and_nested_links_are_omitted() {
        for root in ["notes", "notes/.trash", "attachments", "recording-sources"] {
            let home = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(
                outside.path().join("example.txt"),
                "EXAMPLE PRIVATE CONTENT",
            )
            .unwrap();
            if root != "notes" {
                std::fs::create_dir(home.path().join("notes")).unwrap();
            }
            std::os::unix::fs::symlink(outside.path(), home.path().join(root)).unwrap();
            let result = write_zip(
                std::io::Cursor::new(Vec::new()),
                &home.path().join("notes"),
                &home.path().join("chats"),
                Parts {
                    uploads: true,
                    trash: true,
                    chats: false,
                },
            );
            assert!(result.is_err(), "{root}");
        }
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("example.txt"),
            "EXAMPLE PRIVATE CONTENT",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("linked-folder")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("example.txt"),
            home.path().join("linked-file"),
        )
        .unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        let packed = write_folder(&mut out, home.path(), "files").unwrap();
        assert_eq!(packed, Packed::default());
        assert!(names(out.into_inner()).is_empty());
        assert!(write_folder_from(
            std::io::Cursor::new(Vec::new()),
            home.path(),
            &home.path().join("linked-folder/nested"),
            "files"
        )
        .is_err());
    }

    #[test]
    #[cfg(unix)]
    fn names_skipped_for_control_characters_are_counted_in_the_export() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("Fine.md"), "ok").unwrap();
        std::fs::write(notes.join("Bad\u{1}.md"), "no").unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        write_zip(
            &mut out,
            &notes,
            &tmp.path().join("chats"),
            Parts::default(),
        )
        .unwrap();
        let listed = names(out.into_inner());
        assert!(listed.iter().any(|n| n == "leo/SKIPPED.txt"), "{listed:?}");
        assert!(!listed.iter().any(|n| n.contains("Bad")), "{listed:?}");
    }

    #[test]
    fn the_zip_holds_the_notes_in_their_folders_and_only_the_parts_asked_for() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(notes.join("cs130")).unwrap();
        std::fs::create_dir_all(notes.join(".git")).unwrap();
        std::fs::create_dir_all(notes.join(".trash")).unwrap();
        std::fs::write(notes.join("Top.md"), "top").unwrap();
        std::fs::write(notes.join("cs130/Graphs.md"), "graphs").unwrap();
        std::fs::write(notes.join(".git/config"), "secret-ish").unwrap();
        std::fs::write(notes.join(".trash/Old.md"), "old").unwrap();
        std::fs::create_dir_all(tmp.path().join("attachments/abc")).unwrap();
        std::fs::write(tmp.path().join("attachments/abc/slides.pdf"), "pdf").unwrap();
        let chats = tmp.path().join("chats");
        crate::chats::save(
            &chats,
            "chat-1234",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": [{ "role": "user", "text": "hi" }] })).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        crate::chat_files::add(&chats, "chat-1234", "w.pdf", "week 3", chrono::Utc::now()).unwrap();

        let legacy = crate::db::for_chats(&chats);
        legacy.with(|c| {
            c.execute("INSERT INTO interviews VALUES ('old-practice', '2000-01-01T00:00:00Z', '{\"private\":true}')", [])?;
            Ok(())
        }).unwrap();

        let mut only_notes = std::io::Cursor::new(Vec::new());
        let packed = write_zip(&mut only_notes, &notes, &chats, Parts::default()).unwrap();
        assert_eq!(packed, Packed { files: 2, bytes: 9 });
        assert_eq!(
            names(only_notes.into_inner()),
            [
                "leo/README.txt",
                "leo/notes/Top.md",
                "leo/notes/cs130/Graphs.md"
            ]
        );

        let mut all = std::io::Cursor::new(Vec::new());
        let parts = Parts {
            uploads: true,
            chats: true,
            trash: true,
        };
        write_zip(&mut all, &notes, &chats, parts).unwrap();
        let listed = names(all.into_inner());
        for want in [
            "leo/trash/Old.md",
            "leo/uploads/abc/slides.pdf",
            "leo/chats/chat-1234.json",
        ] {
            assert!(listed.iter().any(|n| n == want), "{want} in {listed:?}");
        }
        assert!(!listed.iter().any(|n| n.contains(".git")), "{listed:?}");
        assert!(!listed.iter().any(|n| n.starts_with("leo/interviews/")));
        assert_eq!(
            legacy
                .with(|c| c.query_row(
                    "SELECT data FROM interviews WHERE id = 'old-practice'",
                    [],
                    |r| r.get::<_, String>(0)
                ))
                .unwrap(),
            "{\"private\":true}"
        );
        assert!(
            listed
                .iter()
                .any(|n| n.starts_with("leo/chats/chat-1234.files/") && n.ends_with(".txt")),
            "{listed:?}"
        );
    }

    #[test]
    fn a_notes_uploads_are_zipped_together_in_one_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("slides.pdf"), "pdf").unwrap();
        std::fs::write(tmp.path().join("board.jpg"), "jpg").unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        let packed = write_folder(&mut out, tmp.path(), "Lecture 4").unwrap();
        assert_eq!(packed.files, 2);
        assert_eq!(
            names(out.into_inner()),
            ["Lecture 4/board.jpg", "Lecture 4/slides.pdf"]
        );
    }

    fn library_of(count: usize) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        for n in 0..count {
            std::fs::write(notes.join(format!("n{n}.md")), "").unwrap();
        }
        (tmp, notes)
    }

    #[test]
    fn an_export_has_no_item_or_size_limit() {
        let (tmp, notes) = library_of(300);
        let packed = write_zip(
            std::io::Cursor::new(Vec::new()),
            &notes,
            &tmp.path().join("chats"),
            Parts::default(),
        )
        .unwrap();
        assert_eq!(packed.files, 300);
    }

    #[test]
    #[ignore]
    fn an_export_of_a_very_large_library_completes() {
        let count: usize = std::env::var("LEO_EXPORT_NOTES")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(100_500);
        let (tmp, notes) = library_of(count);
        if let Some(gib) = std::env::var("LEO_EXPORT_SPARSE_GIB")
            .ok()
            .and_then(|n| n.parse::<u64>().ok())
        {
            let dir = tmp.path().join("attachments/abc");
            std::fs::create_dir_all(&dir).unwrap();
            let file = std::fs::File::create(dir.join("big.bin")).unwrap();
            file.set_len(gib << 30).unwrap();
        }
        let out = tempfile::tempfile().unwrap();
        let started = std::time::Instant::now();
        let packed = write_zip(
            out,
            &notes,
            &tmp.path().join("chats"),
            Parts {
                uploads: true,
                ..Parts::default()
            },
        )
        .unwrap();
        eprintln!("exported {} files in {:?}", packed.files, started.elapsed());
        assert!(packed.files >= count);
    }

    #[test]
    #[ignore]
    fn a_file_over_four_gigabytes_is_written_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let length: u64 = (4 << 30) + 12345;
        let file = std::fs::File::create(tmp.path().join("big.bin")).unwrap();
        file.set_len(length).unwrap();
        let mut out = tempfile::tempfile().unwrap();
        let packed = write_folder_from(&mut out, tmp.path(), tmp.path(), "files").unwrap();
        assert_eq!(
            packed,
            Packed {
                files: 1,
                bytes: length
            }
        );
        let mut archive = zip::ZipArchive::new(out).unwrap();
        assert_eq!(archive.by_name("files/big.bin").unwrap().size(), length);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_loop_is_skipped_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("a")).unwrap();
        std::fs::write(tmp.path().join("a/Note.md"), "ab").unwrap();
        std::os::unix::fs::symlink(tmp.path(), tmp.path().join("a/up")).unwrap();
        assert_eq!(
            folder_zipped(tmp.path()).unwrap(),
            Packed { files: 1, bytes: 2 }
        );
    }

    #[test]
    fn a_folder_met_twice_is_skipped_and_the_rest_goes_on() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("a")).unwrap();
        std::fs::write(tmp.path().join("a/Note.md"), "ab").unwrap();
        std::fs::write(tmp.path().join("Top.md"), "c").unwrap();
        let mut walk = Walk::default();
        let seen = std::fs::metadata(tmp.path().join("a")).unwrap();
        walk.entered
            .insert(identity(&tmp.path().join("a"), &seen).unwrap());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        add_dir(
            &mut zip,
            tmp.path(),
            tmp.path(),
            "files",
            true,
            0,
            &mut walk,
        )
        .unwrap();
        assert_eq!(walk.packed, Packed { files: 1, bytes: 1 });
    }

    #[test]
    fn folders_deeper_than_leo_reads_are_left_out_without_failing_the_export() {
        let tmp = tempfile::tempdir().unwrap();
        let mut deep = tmp.path().to_path_buf();
        for level in 0..=MOST_DEPTH + 1 {
            deep.push(format!("d{level}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("Lost.md"), "x").unwrap();
        std::fs::write(tmp.path().join("d0/Kept.md"), "y").unwrap();
        assert_eq!(
            folder_zipped(tmp.path()).unwrap(),
            Packed { files: 1, bytes: 1 }
        );
    }

    #[test]
    fn a_long_chat_and_a_long_document_are_written_whole_and_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        let chats = tmp.path().join("chats");
        let now = chrono::Utc::now();
        let messages: Vec<_> = (0..450)
            .map(|n| serde_json::json!({ "role": "user", "text": format!("message {n}") }))
            .collect();
        crate::chats::save(
            &chats,
            "chat-long-1234",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": messages }))
                .unwrap(),
            now,
        )
        .unwrap();
        let text = "é".repeat(DOCUMENT_CHARS * 2 + 7);
        let doc = crate::chat_files::add(&chats, "chat-long-1234", "long.txt", &text, now).unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        write_zip(
            &mut out,
            &notes,
            &chats,
            Parts {
                chats: true,
                ..Parts::default()
            },
        )
        .unwrap();
        out.set_position(0);
        let mut archive = zip::ZipArchive::new(out).unwrap();
        let chat: serde_json::Value =
            serde_json::from_reader(archive.by_name("leo/chats/chat-long-1234.json").unwrap())
                .unwrap();
        let said: Vec<&str> = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["text"].as_str().unwrap())
            .collect();
        assert_eq!(said.len(), 450);
        assert!(said
            .iter()
            .enumerate()
            .all(|(n, t)| *t == format!("message {n}")));
        let mut body = String::new();
        archive
            .by_name(&format!("leo/chats/chat-long-1234.files/{}.txt", doc.id))
            .unwrap()
            .read_to_string(&mut body)
            .unwrap();
        assert!(body == text);
    }

    #[test]
    fn the_zip_holds_exactly_what_it_held_before_and_what_later_lanes_added() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let notes = data.join("notes");
        let write = |path: &str, text: &str| {
            let path = data.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("notes/Top.md", "top");
        write("notes/cs/Graphs.md", "graphs");
        write("notes/.hidden/Secret.md", "no");
        write("notes/.trash/Old.md", "old");
        write("attachments/abc/slides.pdf", "pdf");
        write("attachments/abc/.read/slides.pdf.txt", "text copy");
        write("recording-sources/abc/r1.json", "{}");
        write("recording-sources/abc/r1.passages", "p");
        write("recording-sources/abc/r1.points", "q");
        write("calendar/feed.1234.ics", "BEGIN:VCALENDAR");
        write("incoming/up/item", "half");
        write("ui-prefs.json", "{}");
        write("keep.json", "{}");
        write("calendars.json", "{}");
        write("graph.json.before-database", "{}");
        let chats = data.join("chats");
        let now = chrono::Utc::now();
        crate::chats::save(
            &chats,
            "chat-1234",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": [{ "role": "user", "text": "hi" }] })).unwrap(),
            now,
        )
        .unwrap();
        let doc = crate::chat_files::add(&chats, "chat-1234", "w.pdf", "week 3", now).unwrap();
        let task = serde_json::from_value(serde_json::json!({"id":"texport-1234", "title":"Example task", "created_at":now, "updated_at":now})).unwrap();
        crate::tasks::put(&chats, &task).unwrap();
        let board = crate::boards::new_board("Plan", now);
        crate::boards::put_board(&chats, &board).unwrap();
        let card = serde_json::from_value(serde_json::json!({"id":"c1234567", "board": board.id, "column": board.columns[0].id, "title": "Card", "created_at":now, "updated_at":now})).unwrap();
        crate::boards::put_card(&chats, &card).unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        write_zip(
            &mut out,
            &notes,
            &chats,
            Parts {
                uploads: true,
                chats: true,
                trash: true,
            },
        )
        .unwrap();
        let mut want = vec![
            "leo/README.txt".to_string(),
            "leo/notes/Top.md".into(),
            "leo/notes/cs/Graphs.md".into(),
            "leo/trash/Old.md".into(),
            "leo/uploads/abc/slides.pdf".into(),
            "leo/recording-sources/abc/r1.json".into(),
            "leo/recording-sources/abc/r1.passages".into(),
            "leo/recording-sources/abc/r1.points".into(),
            "leo/tasks.json".into(),
            "leo/boards.json".into(),
            "leo/chats/chat-1234.json".into(),
            format!("leo/chats/chat-1234.files/{}.txt", doc.id),
        ];
        want.sort();
        assert_eq!(names(out.into_inner()), want);
    }
}
