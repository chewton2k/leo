use std::collections::VecDeque;
use std::fs::{File, Metadata};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result};

const KEPT: usize = 12;
const KEPT_BYTES: usize = 16_000_000;
pub const SPILL_BYTES: usize = 1_000_000;
const PIECE: usize = 64 * 1024;
const LARGE_BYTES: u64 = 16_000_000;
const COPIES: &str = ".read";
const PART_INDEX: &str = ".parts";
const STAMP_LIMIT: u64 = 128;
const RECORD_BYTES: u64 = 24;

type Key = (PathBuf, u64, Option<SystemTime>);

#[derive(Clone)]
pub enum Lines {
    Held(Arc<String>),
    Spilled(PathBuf),
}

struct Bytes(Arc<String>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

struct Split<R: BufRead> {
    from: R,
    carry: Vec<u8>,
    read: u64,
}

impl<R: BufRead> Split<R> {
    fn new(from: R, read: u64) -> Self {
        Self {
            from,
            carry: Vec::new(),
            read,
        }
    }

    fn offset(&self) -> u64 {
        self.read - self.carry.len() as u64
    }

    fn positioned(&mut self) -> impl Iterator<Item = (u64, String)> + '_ {
        std::iter::from_fn(|| {
            let offset = self.offset();
            self.next().map(|line| (offset, line))
        })
    }
}

impl<R: BufRead> Iterator for Split<R> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let mut line = std::mem::take(&mut self.carry);
        let mut any = !line.is_empty();
        let mut ended = false;
        while line.len() <= PIECE {
            let Ok(buffer) = self.from.fill_buf() else {
                break;
            };
            if buffer.is_empty() {
                break;
            }
            any = true;
            let window = &buffer[..buffer.len().min(PIECE + 1 - line.len())];
            match window.iter().position(|&b| b == b'\n') {
                Some(at) => {
                    line.extend_from_slice(&window[..at]);
                    self.from.consume(at + 1);
                    self.read += (at + 1) as u64;
                    ended = true;
                    break;
                }
                None => {
                    line.extend_from_slice(window);
                    let taken = window.len();
                    self.from.consume(taken);
                    self.read += taken as u64;
                }
            }
        }
        if !any {
            return None;
        }
        if ended && line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.len() > PIECE {
            let cut = cut_of(&line);
            self.carry = line.split_off(cut);
        }
        Some(
            String::from_utf8(line)
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
        )
    }
}

fn cut_of(line: &[u8]) -> usize {
    let valid = match std::str::from_utf8(line) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&line[..error.valid_up_to()]).unwrap_or_default(),
    };
    let edge = valid
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(valid.len()))
        .take_while(|&at| at <= PIECE)
        .last()
        .unwrap_or(0);
    let head = valid.get(..edge).unwrap_or_default();
    match head.rfind(char::is_whitespace) {
        Some(at) if at > 0 => at + head[at..].chars().next().map_or(1, char::len_utf8),
        _ if edge > 0 => edge,
        _ => line.len().min(PIECE),
    }
}

impl Lines {
    pub fn iter(&self) -> Result<Box<dyn Iterator<Item = String>>> {
        Ok(match self {
            Lines::Held(text) => Box::new(Split::new(Cursor::new(Bytes(Arc::clone(text))), 0)),
            Lines::Spilled(path) => {
                let (from, offset) = open_spill(path)?;
                Box::new(Split::new(from, offset))
            }
        })
    }

    pub(crate) fn part(&self, size: usize, wanted: usize) -> Result<(usize, String)> {
        let Lines::Spilled(copy) = self else {
            return Ok(pack_part(
                self.iter()?.map(|line| (0, line)),
                size,
                wanted,
                0,
                |_, _, _| {},
            ));
        };
        let (mut from, body) = open_spill(copy)?;
        let stamp = from
            .get_ref()
            .metadata()
            .ok()
            .and_then(|m| part_stamp(m, size));
        let index = index_of(copy);
        if let Some(stamp) = &stamp {
            if let Some(part) = indexed_part(&index, &mut from, stamp, body, wanted) {
                return Ok(part);
            }
        }
        from.seek(SeekFrom::Start(body))?;
        let mut out = stamp
            .as_ref()
            .and_then(|stamp| start_index(copy, stamp, body));
        let mut split = Split::new(from, body);
        let mut lines = 0;
        let (total, text) = pack_part(
            split.positioned().inspect(|_| lines += 1),
            size,
            wanted,
            0,
            |part, offset, number| {
                if let (Some(out_file), Some(stamp)) = (&mut out, &stamp) {
                    if write_record(out_file, stamp, (part - 1) as u64, [offset, number as u64])
                        .is_err()
                    {
                        out = None;
                    }
                }
            },
        );
        if let (Some(out), Some(stamp)) = (out, &stamp) {
            if split.offset() == stamp.0 {
                let _ = finish_index(copy, &index, out, stamp, total as u64, lines);
            }
        }
        Ok((total, text))
    }
}

fn open_spill(copy: &Path) -> Result<(BufReader<File>, u64)> {
    let file = open_copy_file(copy).context("leo could not read back what it kept")?;
    let mut from = BufReader::new(file);
    let body = from.skip_until(b'\n')?;
    Ok((from, body as u64))
}

fn pack_part(
    lines: impl Iterator<Item = (u64, String)>,
    size: usize,
    wanted: usize,
    first: usize,
    mut start: impl FnMut(usize, u64, usize),
) -> (usize, String) {
    let (mut total, mut kept, mut chars) = (1, String::new(), 0usize);
    for (i, (offset, line)) in lines.enumerate() {
        let number = first + i;
        let line = format!("{}| {line}", number + 1);
        if chars > 0 && chars + line.chars().count() > size {
            chars = 0;
            total += 1;
            start(total, offset, number);
        }
        if chars > 0 {
            chars += 1;
            if total == wanted {
                kept.push('\n');
            }
        }
        let clipped = crate::chat::clip(&line, size);
        chars += clipped.chars().count();
        if total == wanted {
            kept.push_str(&clipped);
        }
    }
    (total, kept)
}

type PartStamp = (u64, SystemTime, usize);

fn part_stamp(meta: Metadata, size: usize) -> Option<PartStamp> {
    meta.is_file()
        .then_some((meta.len(), meta.modified().ok()?, size))
}

fn record_hash(stamp: &PartStamp, at: u64, data: [u64; 2]) -> u64 {
    let mut hash = DefaultHasher::new();
    (stamp, at, data).hash(&mut hash);
    hash.finish()
}

fn write_record(
    out: &mut impl Write,
    stamp: &PartStamp,
    at: u64,
    data: [u64; 2],
) -> io::Result<()> {
    for word in [data[0], data[1], record_hash(stamp, at, data)] {
        out.write_all(&word.to_le_bytes())?;
    }
    Ok(())
}

fn read_record(from: &mut impl Read, stamp: &PartStamp, at: u64) -> Option<[u64; 2]> {
    let mut word = || {
        let mut bytes = [0; 8];
        from.read_exact(&mut bytes).ok()?;
        Some(u64::from_le_bytes(bytes))
    };
    let data = [word()?, word()?];
    (word()? == record_hash(stamp, at, data)).then_some(data)
}

fn indexed_part(
    index: &Path,
    from: &mut BufReader<File>,
    stamp: &PartStamp,
    body: u64,
    wanted: usize,
) -> Option<(usize, String)> {
    let mut index = BufReader::new(open_copy_file(index).ok()?);
    let end = index.seek(SeekFrom::End(-(RECORD_BYTES as i64))).ok()?;
    let total = end / RECORD_BYTES;
    if total == 0 || end % RECORD_BYTES != 0 || read_record(&mut index, stamp, total)?[0] != stamp.0
    {
        return None;
    }
    let total = usize::try_from(total).ok()?;
    if wanted == 0 || wanted > total {
        return Some((total, String::new()));
    }
    let at = (wanted - 1) as u64;
    index.seek(SeekFrom::Start(at * RECORD_BYTES)).ok()?;
    let [offset, first] = read_record(&mut index, stamp, at)?;
    let [end, next] = read_record(&mut index, stamp, at + 1)?;
    let count = usize::try_from(next.checked_sub(first)?).ok()?;
    let first = usize::try_from(first).ok()?;
    first.checked_add(count)?;
    let size = stamp.2;
    if offset < body || end < offset || end > stamp.0 || count > size.saturating_add(1) {
        return None;
    }
    from.seek(SeekFrom::Start(offset)).ok()?;
    let mut split = Split::new(from, offset);
    let (parts, text) = pack_part(split.positioned().take(count), size, 1, first, |_, _, _| {});
    (parts == 1 && split.offset() == end).then_some((total, text))
}

fn start_index(
    copy: &Path,
    stamp: &PartStamp,
    body: u64,
) -> Option<BufWriter<tempfile::NamedTempFile>> {
    if !copies_folder_is_ours(copy) {
        return None;
    }
    let mut out = BufWriter::new(tempfile::NamedTempFile::new_in(copy.parent()?).ok()?);
    write_record(&mut out, stamp, 0, [body, 0]).ok()?;
    Some(out)
}

fn finish_index(
    copy: &Path,
    index: &Path,
    mut out: BufWriter<tempfile::NamedTempFile>,
    stamp: &PartStamp,
    total: u64,
    lines: u64,
) -> io::Result<()> {
    write_record(&mut out, stamp, total, [stamp.0, lines])?;
    let temp = out.into_inner().map_err(|error| error.into_error())?;
    if copies_folder_is_ours(copy)
        && std::fs::symlink_metadata(copy)
            .ok()
            .and_then(|m| part_stamp(m, stamp.2))
            == Some(*stamp)
    {
        temp.persist(index).map_err(|error| error.error)?;
        if copies_folder_is_ours(copy)
            && !std::fs::symlink_metadata(copy).is_ok_and(|m| m.is_file())
        {
            let _ = std::fs::remove_file(index);
        }
    }
    Ok(())
}

pub fn through(reader: crate::Reader) -> crate::ReaderTo {
    Arc::new(
        move |path: &Path,
              name: &str,
              mime: &str,
              progress: &mut dyn FnMut(&str),
              out: &mut dyn Write| {
            let mut bytes = Vec::new();
            leo_core::paths::open_regular(path)?.read_to_end(&mut bytes)?;
            let text = reader(
                crate::UploadFile {
                    name: name.to_string(),
                    mime: mime.to_string(),
                    bytes,
                },
                progress,
            )?;
            out.write_all(text.as_bytes())?;
            Ok(())
        },
    )
}

fn copy_of(original: &Path) -> Option<PathBuf> {
    let mut name = original.file_name()?.to_os_string();
    name.push(".txt");
    Some(original.parent()?.join(COPIES).join(name))
}

fn index_of(copy: &Path) -> PathBuf {
    let mut path = copy.as_os_str().to_os_string();
    path.push(PART_INDEX);
    PathBuf::from(path)
}

pub(crate) fn forget_copy(original: &Path) {
    if let Some(copy) = copy_of(original) {
        drop_copy(&copy);
    }
}

fn copies_folder_is_ours(copy: &Path) -> bool {
    copy.parent()
        .and_then(|folder| std::fs::symlink_metadata(folder).ok())
        .is_some_and(|m| m.file_type().is_dir())
}

fn open_copy_file(path: &Path) -> io::Result<File> {
    if !copies_folder_is_ours(path) || !std::fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::other("the saved read is not a regular file"));
    }
    leo_core::paths::open_regular(path)
}

pub fn drop_copy(copy: &Path) {
    if copies_folder_is_ours(copy) {
        for path in [copy, &index_of(copy)] {
            if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

pub fn drop_copy_of(original: &Path) {
    if let Some(copy) = copy_of(original) {
        drop_copy(&copy);
    }
}

fn stamp_of(key: &Key) -> String {
    let nanos = key
        .2
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("leo-read {} {nanos}\n", key.1)
}

fn copy_matches(copy: &Path, stamp: &str) -> bool {
    let Ok(file) = open_copy_file(copy) else {
        return false;
    };
    let mut first = String::new();
    BufReader::new(file.take(STAMP_LIMIT))
        .read_line(&mut first)
        .is_ok()
        && first == stamp
}

struct Spooler {
    head: Vec<u8>,
    stamp: String,
    folder: PathBuf,
    copy: PathBuf,
    file: Option<std::io::BufWriter<tempfile::NamedTempFile>>,
}

impl Spooler {
    fn start(&mut self) -> std::io::Result<()> {
        match std::fs::create_dir(&self.folder) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            other => other?,
        }
        if !copies_folder_is_ours(&self.copy) {
            return Err(std::io::Error::other(
                "the read copies folder is not a folder",
            ));
        }
        let mut file = std::io::BufWriter::with_capacity(
            PIECE,
            tempfile::NamedTempFile::new_in(&self.folder)?,
        );
        file.write_all(self.stamp.as_bytes())?;
        file.write_all(&self.head)?;
        self.head = Vec::new();
        self.file = Some(file);
        Ok(())
    }

    fn finish(self) -> Result<Lines> {
        match self.file {
            None => Ok(Lines::Held(Arc::new(
                String::from_utf8(self.head).context("leo read text that is not text")?,
            ))),
            Some(file) => {
                let temp = file.into_inner().map_err(|e| e.into_error())?;
                temp.as_file().sync_all()?;
                temp.persist(&self.copy).map_err(|e| e.error)?;
                Ok(Lines::Spilled(self.copy))
            }
        }
    }
}

impl Write for Spooler {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.file.is_none() && self.head.len() + bytes.len() > SPILL_BYTES {
            self.start()?;
        }
        match &mut self.file {
            Some(file) => file.write_all(bytes)?,
            None => self.head.extend_from_slice(bytes),
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.file {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

#[derive(Default)]
pub struct Originals {
    read: Mutex<VecDeque<(Key, Lines)>>,
    large: Mutex<()>,
}

pub fn files(notes_dir: &Path, note: &str) -> Vec<(String, u64)> {
    let Some(folder) = crate::routes::uploads::originals_dir(notes_dir, note) else {
        return Vec::new();
    };
    let mut found: Vec<(String, u64)> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| path_of(notes_dir, note, name).is_some())
                })
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().to_string(),
                        e.metadata().map(|m| m.len()).unwrap_or(0),
                    )
                })
                .filter(|(name, _)| !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

pub fn path_of(notes_dir: &Path, note: &str, name: &str) -> Option<PathBuf> {
    let path = crate::routes::uploads::original_path(notes_dir, note, name)?;
    std::fs::symlink_metadata(&path)
        .is_ok_and(|m| m.is_file())
        .then_some(path)
}

fn mime_of(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("md" | "markdown") => "text/markdown",
        _ => "text/plain",
    }
}

impl Originals {
    pub fn forget_missing(&self) {
        let mut missing: Vec<_> = self
            .read
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        missing.retain(|(path, _, _)| !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()));
        if !missing.is_empty() {
            self.forget(|key| missing.contains(key));
        }
    }

    pub fn forget_note(&self, folder: &Path) {
        self.forget(|(path, _, _)| path.starts_with(folder));
    }

    fn forget(&self, remove: impl Fn(&Key) -> bool) {
        let mut dropped = Vec::new();
        self.read
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(key, lines)| {
                let gone = remove(key);
                if let (true, Lines::Spilled(copy)) = (gone, lines) {
                    dropped.push(copy.clone());
                }
                !gone
            });
        for copy in dropped {
            drop_copy(&copy);
        }
    }

    fn cached(&self, key: &Key) -> Option<Lines> {
        let found = self
            .read
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, lines)| lines.clone())?;
        match &found {
            Lines::Spilled(copy) if !std::fs::symlink_metadata(copy).is_ok_and(|m| m.is_file()) => {
                None
            }
            _ => Some(found),
        }
    }

    fn keep(&self, key: Key, lines: &Lines) {
        let mut kept = self.read.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(k, _)| k.0 != key.0);
        kept.push_back((key, lines.clone()));
        let held = |kept: &VecDeque<(Key, Lines)>| -> usize {
            kept.iter()
                .map(|(_, l)| match l {
                    Lines::Held(text) => text.len(),
                    Lines::Spilled(_) => 0,
                })
                .sum()
        };
        while kept.len() > KEPT || (kept.len() > 1 && held(&kept) > KEPT_BYTES) {
            kept.pop_front();
        }
    }

    pub fn lines(&self, reader: &crate::ReaderTo, path: &Path, name: &str) -> Result<Lines> {
        let meta = leo_core::paths::open_regular(path)?.metadata()?;
        let key: Key = (path.to_path_buf(), meta.len(), meta.modified().ok());
        self.forget_missing();
        if let Some(found) = self.cached(&key) {
            return Ok(found);
        }
        let copy = copy_of(path).context("leo could not place what it reads")?;
        let stamp = stamp_of(&key);
        if copy_matches(&copy, &stamp) {
            let found = Lines::Spilled(copy);
            self.keep(key, &found);
            return Ok(found);
        }
        let _one_at_a_time = (meta.len() > LARGE_BYTES)
            .then(|| self.large.lock().unwrap_or_else(|e| e.into_inner()));
        let mut spooler = Spooler {
            head: Vec::new(),
            stamp,
            folder: copy
                .parent()
                .map(Path::to_path_buf)
                .context("leo could not place what it reads")?,
            copy,
            file: None,
        };
        reader(path, name, mime_of(name), &mut |_| {}, &mut spooler)?;
        let found = spooler
            .finish()
            .with_context(|| format!("leo could not keep what it read from {name}"))?;
        if let (Lines::Spilled(copy), Some(folder)) = (&found, path.parent()) {
            forget_orphans(folder, copy);
        }
        self.keep(key, &found);
        Ok(found)
    }
}

fn forget_orphans(folder: &Path, made: &Path) {
    if !copies_folder_is_ours(made) {
        return;
    }
    let Some(copies) = made.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(copies) else {
        return;
    };
    for entry in entries.flatten() {
        let mut copy = entry.path();
        if copy.extension().and_then(|ext| ext.to_str()) == PART_INDEX.strip_prefix('.') {
            copy.set_extension("");
        }
        let Some(original) = copy
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".txt"))
            .filter(|o| !o.is_empty())
        else {
            continue;
        };
        if copy != made
            && !std::fs::symlink_metadata(folder.join(original)).is_ok_and(|m| m.is_file())
        {
            drop_copy(&copy);
        }
    }
}

pub fn place_of(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(n) = line
        .strip_prefix("[page ")
        .and_then(|r| r.strip_suffix(']'))
    {
        return n.parse::<u32>().ok().map(|n| format!("page {n}"));
    }
    line.strip_prefix("## Slide ")
        .and_then(|n| n.trim().parse::<u32>().ok())
        .map(|n| format!("slide {n}"))
}

pub fn passage(lines: impl Iterator<Item = String>, at: &str) -> Option<(usize, usize, String)> {
    let at = at.trim();
    if let Some(want) = at
        .strip_prefix('p')
        .or_else(|| at.strip_prefix('s'))
        .and_then(|n| n.parse::<u32>().ok())
    {
        let (mut start, mut label, mut count) = (None, String::new(), 0);
        for (i, line) in lines.enumerate() {
            count = i + 1;
            let place = place_of(&line);
            if start.is_some() {
                if place.is_some() {
                    return start.map(|s| (s + 1, i, label));
                }
            } else if place
                .as_ref()
                .is_some_and(|p| *p == format!("page {want}") || *p == format!("slide {want}"))
            {
                start = Some(i);
                label = place.unwrap_or_default();
            }
        }
        return start.map(|s| (s + 1, count, label));
    }
    let range = at.strip_prefix('L')?;
    let (from, to) = match range.split_once('-') {
        Some((a, b)) => (a.parse::<usize>().ok()?, b.parse::<usize>().ok()?),
        None => {
            let n = range.parse::<usize>().ok()?;
            (n, n)
        }
    };
    if from == 0 || to < from {
        return None;
    }
    let to = to.min(from + 200);
    let reached = lines.take(to).count();
    if from > reached {
        return None;
    }
    let to = to.min(reached);
    let label = if from == to {
        format!("line {from}")
    } else {
        format!("lines {from}–{to}")
    };
    Some((from, to, label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SLIDES: &str = "## Slide 1\nIntro\n\n## Slide 2\nDijkstra keeps a heap\nIt never revisits a node\n\n## Slide 3\nEnd";

    fn found(text: &str, at: &str) -> Option<(usize, usize, String)> {
        passage(text.lines().map(String::from), at)
    }

    #[test]
    fn a_page_or_slide_or_lines_can_be_found_again() {
        assert_eq!(found(SLIDES, "p2"), Some((4, 7, "slide 2".into())));
        assert_eq!(found(SLIDES, "s3"), Some((8, 9, "slide 3".into())));
        assert_eq!(
            found("[page 1]\na\n[page 2]\nb", "p2"),
            Some((3, 4, "page 2".into()))
        );
        assert_eq!(found(SLIDES, "L5-6"), Some((5, 6, "lines 5–6".into())));
        assert_eq!(found(SLIDES, "L5"), Some((5, 5, "line 5".into())));
        assert_eq!(found(SLIDES, "L8-99"), Some((8, 9, "lines 8–9".into())));
        assert_eq!(found(SLIDES, "p9"), None);
        assert_eq!(found(SLIDES, "L99"), None);
        assert_eq!(found(SLIDES, "nonsense"), None);
    }

    fn plain() -> crate::ReaderTo {
        through(Arc::new(
            |file: crate::UploadFile, _: &mut dyn FnMut(&str)| {
                Ok(String::from_utf8_lossy(&file.bytes).into_owned())
            },
        ))
    }

    fn copying(count: &Arc<AtomicUsize>) -> crate::ReaderTo {
        let count = Arc::clone(count);
        Arc::new(
            move |path: &Path, _: &str, _: &str, _: &mut dyn FnMut(&str), out: &mut dyn Write| {
                count.fetch_add(1, Ordering::Relaxed);
                std::io::copy(&mut std::fs::File::open(path)?, out)?;
                Ok(())
            },
        )
    }

    fn entries(originals: &Originals) -> usize {
        originals.read.lock().unwrap().len()
    }

    fn all(lines: &Lines) -> Vec<String> {
        lines.iter().unwrap().collect()
    }

    fn held(lines: &Lines) -> String {
        match lines {
            Lines::Held(text) => text.to_string(),
            Lines::Spilled(_) => panic!("spilled"),
        }
    }

    #[test]
    fn a_deleted_files_text_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = (dir.path().join("one.txt"), dir.path().join("two.txt"));
        std::fs::write(&one, "first").unwrap();
        std::fs::write(&two, "second").unwrap();
        let originals = Originals::default();
        originals.lines(&plain(), &one, "one.txt").unwrap();
        originals.lines(&plain(), &two, "two.txt").unwrap();
        assert_eq!(entries(&originals), 2);
        std::fs::remove_file(&one).unwrap();
        assert_eq!(
            held(&originals.lines(&plain(), &two, "two.txt").unwrap()),
            "second"
        );
        assert_eq!(entries(&originals), 1);
    }

    #[test]
    fn a_notes_texts_are_dropped_with_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (gone, kept) = (dir.path().join("note-gone"), dir.path().join("note-kept"));
        for folder in [&gone, &kept] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("a.txt"), "words").unwrap();
        }
        let originals = Originals::default();
        originals
            .lines(&plain(), &gone.join("a.txt"), "a.txt")
            .unwrap();
        originals
            .lines(&plain(), &kept.join("a.txt"), "a.txt")
            .unwrap();
        originals.forget_note(&gone);
        assert_eq!(entries(&originals), 1);
        assert_eq!(originals.read.lock().unwrap()[0].0 .0, kept.join("a.txt"));
    }

    #[test]
    fn names_that_leave_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let folder = dir.path().join("attachments/note-1234");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("slides.pdf"), b"x").unwrap();
        assert!(path_of(&notes, "note-1234", "slides.pdf").is_some());
        for bad in ["../x", "a/b", ".hidden", ""] {
            assert!(path_of(&notes, "note-1234", bad).is_none(), "{bad}");
        }
        assert!(path_of(&notes, "../other", "slides.pdf").is_none());
        assert_eq!(files(&notes, "note-1234"), [("slides.pdf".to_string(), 1)]);
    }

    #[test]
    fn a_file_is_read_once_and_then_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello").unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = copying(&reads);
        let originals = Originals::default();
        assert_eq!(
            held(&originals.lines(&reader, &path, "a.txt").unwrap()),
            "hello"
        );
        assert_eq!(
            held(&originals.lines(&reader, &path, "a.txt").unwrap()),
            "hello"
        );
        assert_eq!(reads.load(Ordering::Relaxed), 1);
    }

    #[cfg(unix)]
    #[test]
    fn original_readers_refuse_links_and_directories_before_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("example.txt");
        std::fs::write(&target, "EXAMPLE PRIVATE CONTENT").unwrap();
        let linked = dir.path().join("linked.txt");
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        let reader: crate::ReaderTo = Arc::new(
            |_: &Path, _: &str, _: &str, _: &mut dyn FnMut(&str), _: &mut dyn Write| {
                panic!("unsafe files must not reach the converter")
            },
        );
        let originals = Originals::default();
        for path in [linked.as_path(), dir.path()] {
            assert!(originals.lines(&reader, path, "example.txt").is_err());
        }
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "EXAMPLE PRIVATE CONTENT"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_over_sixty_megabytes_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let large = dir.path().join("large.txt");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(60_000_001)
            .unwrap();
        let reader: crate::ReaderTo = Arc::new(
            |_: &Path, _: &str, _: &str, _: &mut dyn FnMut(&str), out: &mut dyn Write| {
                out.write_all(b"read anyway")?;
                Ok(())
            },
        );
        let read = Originals::default()
            .lines(&reader, &large, "large.txt")
            .unwrap();
        assert_eq!(held(&read), "read anyway");
    }

    fn big_text(bytes: usize) -> String {
        let mut text = "a".repeat(PIECE - 1);
        text.push('é');
        text.push('\n');
        let mut n = 0;
        while text.len() < bytes {
            text.push_str(&format!("line {n} of 漢字 words go here\n"));
            n += 1;
        }
        text
    }

    #[test]
    fn a_large_text_original_is_read_in_pieces_and_spilled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let text = big_text(5_000_000);
        std::fs::write(&path, &text).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let originals = Originals::default();
        let read = originals.lines(&copying(&reads), &path, "big.txt").unwrap();
        let Lines::Spilled(copy) = &read else {
            panic!("a 5 MB text is held")
        };
        assert_eq!(*copy, dir.path().join(".read/big.txt.txt"));
        assert!(copy.is_file());
        let pieces = all(&read);
        assert!(pieces.iter().all(|p| p.len() <= PIECE));
        assert_eq!(pieces.concat(), text.replace('\n', ""));
        assert_eq!(pieces[0].chars().count(), PIECE - 1);
        assert!(pieces[1].starts_with('é'));
        assert_eq!(files(dir.path(), "x"), Vec::<(String, u64)>::new());
        let again = Originals::default()
            .lines(&copying(&reads), &path, "big.txt")
            .unwrap();
        assert_eq!(all(&again).len(), all(&read).len());
        assert_eq!(reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_writer_never_holds_more_than_one_megabyte() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(COPIES);
        let mut spooler = Spooler {
            head: Vec::new(),
            stamp: "leo-read 1 1\n".into(),
            copy: folder.join("a.txt.txt"),
            folder,
            file: None,
        };
        let piece = vec![b'x'; 300_000];
        for _ in 0..40 {
            spooler.write_all(&piece).unwrap();
            assert!(spooler.head.len() <= SPILL_BYTES);
        }
        assert!(spooler.file.is_some());
        let Lines::Spilled(copy) = spooler.finish().unwrap() else {
            panic!("12 MB was held")
        };
        assert_eq!(std::fs::metadata(copy).unwrap().len(), 12_000_000 + 13);
    }

    #[test]
    fn a_changed_original_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        std::fs::write(&path, big_text(1_500_000)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let originals = Originals::default();
        let first = originals.lines(&copying(&reads), &path, "big.txt").unwrap();
        let before = all(&first).len();
        std::fs::write(&path, format!("{}extra\n", big_text(1_500_000))).unwrap();
        let second = originals.lines(&copying(&reads), &path, "big.txt").unwrap();
        assert_eq!(all(&second).len(), before + 1);
        assert_eq!(reads.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_large_original_of_one_line_is_read_in_pieces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flat.txt");
        let words = "word é 漢字 ".repeat(1_800_000);
        assert!(words.len() > 20_000_000);
        std::fs::write(&path, &words).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let read = Originals::default()
            .lines(&copying(&reads), &path, "flat.txt")
            .unwrap();
        let mut joined = String::new();
        for piece in read.iter().unwrap() {
            assert!(piece.len() <= PIECE, "{}", piece.len());
            joined.push_str(&piece);
        }
        assert_eq!(joined, words);
        let mut solid = String::new();
        solid.push_str(&"漢".repeat(PIECE));
        std::fs::write(&path, &solid).unwrap();
        let read = Originals::default()
            .lines(&copying(&reads), &path, "flat.txt")
            .unwrap();
        let pieces: Vec<String> = read.iter().unwrap().collect();
        assert!(pieces.iter().all(|p| p.len() <= PIECE));
        assert_eq!(pieces.concat(), solid);
    }

    #[test]
    fn lines_are_numbered_the_same_held_or_spilled() {
        let text = "one\r\ntwo\n\nthree\r\nlast";
        let held = Lines::Held(Arc::new(text.to_string()));
        assert_eq!(all(&held), text.lines().collect::<Vec<_>>());
        assert_eq!(all(&Lines::Held(Arc::new(String::new()))).len(), 0);
        assert_eq!(all(&Lines::Held(Arc::new("a\n".into()))), ["a"]);
    }

    #[test]
    fn a_part_is_picked_the_way_packing_would() {
        let lines: Vec<String> = (0..400)
            .map(|n| format!("{n}| {}", "w".repeat(n % 37)))
            .chain(std::iter::once("x".repeat(500)))
            .chain(std::iter::once(String::new()))
            .collect();
        let read = Lines::Held(Arc::new(format!("{}\n", lines.join("\n"))));
        for size in [60, 200, 1_000] {
            let numbered = lines
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{}| {line}", i + 1));
            let parts = crate::tools::packed(numbered, size);
            for want in [1, 2, parts.len(), parts.len() + 1] {
                let (total, text) = read.part(size, want).unwrap();
                assert_eq!(total, parts.len());
                assert_eq!(text, parts.get(want - 1).cloned().unwrap_or_default());
            }
        }
    }

    #[test]
    fn spilled_parts_match_packing_after_reuse_and_index_damage() {
        let dir = tempfile::tempdir().unwrap();
        let copies = dir.path().join(COPIES);
        std::fs::create_dir(&copies).unwrap();
        let copy = copies.join("notes.txt.txt");
        let index = copies.join("notes.txt.txt.parts");
        let mut text: String = (0..80)
            .map(|n| format!("{n} é 漢字 {}\r\n", "w".repeat(n % 37)))
            .collect();
        text.push_str(&format!("a {}漢字\n\nlast é", "é".repeat(PIECE)));
        std::fs::write(&copy, format!("leo-read 1 1\n{text}")).unwrap();
        let read = Lines::Spilled(copy.clone());
        let numbered = || {
            read.iter()
                .unwrap()
                .enumerate()
                .map(|(i, line)| format!("{}| {line}", i + 1))
        };
        let pick = |size, wanted| read.part(size, wanted).unwrap();
        let marker = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        for size in [60, 200, 1_000] {
            let parts = crate::tools::packed(numbered(), size);
            assert_eq!(pick(size, 1), (parts.len(), parts[0].clone()));
            let file = std::fs::File::options().write(true).open(&index).unwrap();
            file.set_modified(marker).unwrap();
            drop(file);
            for (i, part) in parts.iter().enumerate() {
                assert_eq!(pick(size, i + 1), (parts.len(), part.clone()));
            }
            assert_eq!(pick(size, parts.len() + 1), (parts.len(), String::new()));
            assert_eq!(
                std::fs::metadata(&index).unwrap().modified().unwrap(),
                marker
            );
            let intact = std::fs::read(&index).unwrap();
            let mut damaged = intact.clone();
            damaged[RECORD_BYTES as usize] ^= 1;
            std::fs::write(&index, damaged).unwrap();
            assert_eq!(pick(size, 2), (parts.len(), parts[1].clone()));
            assert_eq!(std::fs::read(&index).unwrap(), intact);
        }
        let intact = std::fs::read(&index).unwrap();
        std::fs::write(&index, &intact[..intact.len() - 1]).unwrap();
        let parts = crate::tools::packed(numbered(), 1_000);
        assert_eq!(pick(1_000, 2), (parts.len(), parts[1].clone()));
        assert_eq!(std::fs::read(&index).unwrap(), intact);

        std::fs::remove_file(&index).unwrap();
        std::fs::create_dir(&index).unwrap();
        assert_eq!(pick(1_000, 2), (parts.len(), parts[1].clone()));
        assert!(index.is_dir());
        std::fs::remove_dir(&index).unwrap();
        assert_eq!(pick(1_000, 2), (parts.len(), parts[1].clone()));
        assert_eq!(std::fs::read(&index).unwrap(), intact);

        let before = std::fs::metadata(&copy).unwrap();
        std::fs::write(&copy, format!("leo-read 1 1\n{}", text.replace('w', "z"))).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&copy)
            .unwrap()
            .set_modified(marker)
            .unwrap();
        let after = std::fs::metadata(&copy).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(after.modified().unwrap(), marker);
        assert_ne!(after.modified().unwrap(), before.modified().unwrap());
        let parts = crate::tools::packed(numbered(), 1_000);
        assert_eq!(pick(1_000, 2), (parts.len(), parts[1].clone()));
        assert_ne!(std::fs::read(&index).unwrap(), intact);
    }

    #[test]
    fn the_spill_file_goes_with_its_original() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = (dir.path().join("one.txt"), dir.path().join("two.txt"));
        std::fs::write(&one, big_text(1_500_000)).unwrap();
        std::fs::write(&two, big_text(1_500_000)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let originals = Originals::default();
        for (path, name) in [(&one, "one.txt"), (&two, "two.txt")] {
            let lines = originals.lines(&copying(&reads), path, name).unwrap();
            lines.part(6_000, 2).unwrap();
        }
        let (copy_one, copy_two) = (
            dir.path().join(".read/one.txt.txt"),
            dir.path().join(".read/two.txt.txt"),
        );
        let (index_one, index_two) = (index_of(&copy_one), index_of(&copy_two));
        assert!(copy_one.is_file() && copy_two.is_file());
        assert!(index_one.is_file() && index_two.is_file());
        std::fs::remove_file(&one).unwrap();
        originals.forget_missing();
        assert!(!copy_one.exists() && copy_two.is_file());
        assert!(!index_one.exists() && index_two.is_file());
        originals.forget_note(dir.path());
        assert!(!copy_two.exists() && !index_two.exists());
    }

    #[test]
    fn a_copy_left_by_a_vanished_original_is_swept_with_the_next_read() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = (dir.path().join("one.txt"), dir.path().join("two.txt"));
        std::fs::write(&one, big_text(1_500_000)).unwrap();
        std::fs::write(&two, big_text(1_500_000)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        Originals::default()
            .lines(&copying(&reads), &one, "one.txt")
            .unwrap();
        std::fs::remove_file(&one).unwrap();
        Originals::default()
            .lines(&copying(&reads), &two, "two.txt")
            .unwrap();
        assert!(!dir.path().join(".read/one.txt.txt").exists());
        assert!(dir.path().join(".read/two.txt.txt").is_file());
    }

    #[test]
    fn a_failed_read_leaves_no_copy_to_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        std::fs::write(&path, "x").unwrap();
        let failing: crate::ReaderTo = Arc::new(
            |_: &Path, _: &str, _: &str, _: &mut dyn FnMut(&str), out: &mut dyn Write| {
                out.write_all(&vec![b'x'; 2_000_000])?;
                anyhow::bail!("the file ended badly")
            },
        );
        assert!(Originals::default()
            .lines(&failing, &path, "big.txt")
            .is_err());
        let left: Vec<_> = std::fs::read_dir(dir.path().join(".read"))
            .unwrap()
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let reads = Arc::new(AtomicUsize::new(0));
        let read = Originals::default()
            .lines(&copying(&reads), &path, "big.txt")
            .unwrap();
        assert_eq!(held(&read), "x");
    }

    #[test]
    fn a_stale_header_is_not_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        std::fs::write(&path, big_text(1_500_000)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        Originals::default()
            .lines(&copying(&reads), &path, "big.txt")
            .unwrap();
        let copy = dir.path().join(".read/big.txt.txt");
        let body = std::fs::read(&copy).unwrap();
        let mut forged = b"leo-read 1 1\n".to_vec();
        forged.extend_from_slice(&body[body.iter().position(|&b| b == b'\n').unwrap() + 1..]);
        std::fs::write(&copy, forged).unwrap();
        Originals::default()
            .lines(&copying(&reads), &path, "big.txt")
            .unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_copies_folder_is_never_written_through_or_deleted_from() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("big.txt.txt"), "the person's file").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(COPIES)).unwrap();
        let path = dir.path().join("big.txt");
        std::fs::write(&path, big_text(1_500_000)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        assert!(Originals::default()
            .lines(&copying(&reads), &path, "big.txt")
            .is_err());
        assert!(Lines::Spilled(copy_of(&path).unwrap()).part(60, 1).is_err());
        drop_copy_of(&path);
        assert_eq!(
            std::fs::read_to_string(outside.path().join("big.txt.txt")).unwrap(),
            "the person's file"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn an_index_symlink_is_rebuilt_without_touching_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let copy = copy_of(&dir.path().join("original.txt")).unwrap();
        std::fs::create_dir(copy.parent().unwrap()).unwrap();
        std::fs::write(&copy, "leo-read 1 1\none é\ntwo 漢字\n").unwrap();
        let index = index_of(&copy);
        let outside = dir.path().join("outside");
        let read = Lines::Spilled(copy);
        let expected = read.part(8, 2).unwrap();
        let intact = std::fs::read(&index).unwrap();
        std::fs::rename(&index, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &index).unwrap();
        assert_eq!(read.part(8, 2).unwrap(), expected);
        assert!(std::fs::symlink_metadata(&index).unwrap().is_file());
        assert_eq!(std::fs::read(&outside).unwrap(), intact);
    }

    #[test]
    fn small_texts_are_held_and_large_ones_spilled_within_a_size() {
        let dir = tempfile::tempdir().unwrap();
        let writes = |size: usize| -> crate::ReaderTo {
            Arc::new(
                move |_: &Path, _: &str, _: &str, _: &mut dyn FnMut(&str), out: &mut dyn Write| {
                    out.write_all(" ".repeat(size).as_bytes())?;
                    Ok(())
                },
            )
        };
        let originals = Originals::default();
        for n in 0..13 {
            let name = format!("f{n:02}.txt");
            let path = dir.path().join(&name);
            std::fs::write(&path, b"tiny").unwrap();
            originals.lines(&writes(900_000), &path, &name).unwrap();
        }
        assert_eq!(entries(&originals), KEPT);
        let path = dir.path().join("big.txt");
        std::fs::write(&path, b"tiny").unwrap();
        let big = originals
            .lines(&writes(20_000_000), &path, "big.txt")
            .unwrap();
        assert!(matches!(big, Lines::Spilled(_)));
    }

    #[test]
    #[ignore = "benchmark"]
    fn audit_bench_large_original() {
        let dir = tempfile::tempdir().unwrap();
        for (megabytes, one_line) in [(20u64, false), (200, false), (1024, false), (1024, true)] {
            let path = dir.path().join("bench.txt");
            {
                let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
                let row = if one_line {
                    "word é 漢字 ".to_string()
                } else {
                    "Dijkstra keeps a heap of the nearest nodes é 漢字\n".to_string()
                };
                let mut written = 0u64;
                while written < megabytes * 1_000_000 {
                    file.write_all(row.as_bytes()).unwrap();
                    written += row.len() as u64;
                }
            }
            let reads = Arc::new(AtomicUsize::new(0));
            let started = std::time::Instant::now();
            let read = Originals::default()
                .lines(&copying(&reads), &path, "bench.txt")
                .unwrap();
            let made = started.elapsed();
            let started = std::time::Instant::now();
            let (mut count, mut most) = (0u64, 0usize);
            for piece in read.iter().unwrap() {
                count += 1;
                most = most.max(piece.len());
            }
            let walked = started.elapsed();
            let started = std::time::Instant::now();
            let first = read.part(6_000, 2).unwrap();
            let packed = started.elapsed();
            let started = std::time::Instant::now();
            let (total, part) = read.part(6_000, 2).unwrap();
            let indexed = started.elapsed();
            assert_eq!(first, (total, part.clone()));
            println!(
                "{megabytes} MB one_line={one_line}: read {made:?}, walk {walked:?} ({count} lines, longest {most}), part 2 of {total} ({} bytes), first {packed:?}, indexed {indexed:?}",
                part.len()
            );
        }
    }

    #[test]
    fn removing_an_original_takes_its_read_copy_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let copies = dir.path().join(COPIES);
        std::fs::create_dir(&copies).unwrap();
        for name in [
            "gone.pdf.txt",
            "gone.pdf.txt.parts",
            "kept.pdf.txt",
            "kept.pdf.txt.parts",
        ] {
            std::fs::write(copies.join(name), "text").unwrap();
        }
        forget_copy(&dir.path().join("gone.pdf"));
        assert!(!copies.join("gone.pdf.txt").exists());
        assert!(!copies.join("gone.pdf.txt.parts").exists());
        assert!(copies.join("kept.pdf.txt").is_file());
        assert!(copies.join("kept.pdf.txt.parts").is_file());
    }
}
