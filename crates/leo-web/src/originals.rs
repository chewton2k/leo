use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{bail, Result};

const KEPT: usize = 12;
const KEPT_BYTES: usize = 16_000_000;
const MOST_BYTES: u64 = 60_000_000;

type Key = (PathBuf, u64, Option<SystemTime>);

#[derive(Default)]
pub struct Originals {
    read: Mutex<VecDeque<(Key, Arc<String>)>>,
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
    pub fn text(&self, reader: &crate::Reader, path: &Path, name: &str) -> Result<Arc<String>> {
        use std::io::Read;
        let file = match leo_core::paths::open_regular(path) {
            Err(error) if error.kind() == std::io::ErrorKind::Other => {
                bail!("{name} is too big for leo to read")
            }
            opened => opened?,
        };
        let meta = file.metadata()?;
        if meta.len() > MOST_BYTES {
            bail!("{name} is too big for leo to read");
        }
        let key: Key = (path.to_path_buf(), meta.len(), meta.modified().ok());
        let cached = self
            .read
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(k, _)| *k == key)
            .cloned();
        if let Some((_, text)) = cached {
            return Ok(text);
        }
        let mut bytes = Vec::new();
        file.take(MOST_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MOST_BYTES {
            bail!("{name} is too big for leo to read");
        }
        let text = Arc::new(reader(
            crate::UploadFile {
                name: name.to_string(),
                mime: mime_of(name).to_string(),
                bytes,
            },
            &mut |_| {},
        )?);
        let mut kept = self.read.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(k, _)| k.0 != key.0);
        kept.push_back((key, Arc::clone(&text)));
        while kept.len() > KEPT
            || (kept.len() > 1 && kept.iter().map(|(_, t)| t.len()).sum::<usize>() > KEPT_BYTES)
        {
            kept.pop_front();
        }
        Ok(text)
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

pub fn passage(text: &str, at: &str) -> Option<(usize, usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let at = at.trim();
    if let Some(want) = at
        .strip_prefix('p')
        .or_else(|| at.strip_prefix('s'))
        .and_then(|n| n.parse::<u32>().ok())
    {
        let start = lines.iter().position(|l| {
            place_of(l).is_some_and(|p| p == format!("page {want}") || p == format!("slide {want}"))
        })?;
        let end = lines[start + 1..]
            .iter()
            .position(|l| place_of(l).is_some())
            .map_or(lines.len(), |i| start + 1 + i);
        let label = place_of(lines[start]).unwrap_or_default();
        return Some((start + 1, end, label));
    }
    let range = at.strip_prefix('L')?;
    let (from, to) = match range.split_once('-') {
        Some((a, b)) => (a.parse::<usize>().ok()?, b.parse::<usize>().ok()?),
        None => {
            let n = range.parse::<usize>().ok()?;
            (n, n)
        }
    };
    if from == 0 || from > lines.len() || to < from {
        return None;
    }
    let to = to.min(lines.len()).min(from + 200);
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

    const SLIDES: &str = "## Slide 1\nIntro\n\n## Slide 2\nDijkstra keeps a heap\nIt never revisits a node\n\n## Slide 3\nEnd";

    #[test]
    fn a_page_or_slide_or_lines_can_be_found_again() {
        assert_eq!(passage(SLIDES, "p2"), Some((4, 7, "slide 2".into())));
        assert_eq!(passage(SLIDES, "s3"), Some((8, 9, "slide 3".into())));
        assert_eq!(
            passage("[page 1]\na\n[page 2]\nb", "p2"),
            Some((3, 4, "page 2".into()))
        );
        assert_eq!(passage(SLIDES, "L5-6"), Some((5, 6, "lines 5–6".into())));
        assert_eq!(passage(SLIDES, "L5"), Some((5, 5, "line 5".into())));
        assert_eq!(passage(SLIDES, "p9"), None);
        assert_eq!(passage(SLIDES, "L99"), None);
        assert_eq!(passage(SLIDES, "nonsense"), None);
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
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let reader: crate::Reader =
            Arc::new(move |file: crate::UploadFile, _: &mut dyn FnMut(&str)| {
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(String::from_utf8_lossy(&file.bytes).into_owned())
            });
        let originals = Originals::default();
        assert_eq!(*originals.text(&reader, &path, "a.txt").unwrap(), "hello");
        assert_eq!(*originals.text(&reader, &path, "a.txt").unwrap(), "hello");
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[cfg(unix)]
    #[test]
    fn original_readers_refuse_links_directories_and_oversized_files_before_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let large = dir.path().join("large.txt");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MOST_BYTES + 1)
            .unwrap();
        let target = dir.path().join("example.txt");
        std::fs::write(&target, "EXAMPLE PRIVATE CONTENT").unwrap();
        let linked = dir.path().join("linked.txt");
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        let reader: crate::Reader = Arc::new(|_: crate::UploadFile, _: &mut dyn FnMut(&str)| {
            panic!("unsafe files must not reach the converter")
        });
        let originals = Originals::default();
        for path in [large.as_path(), linked.as_path(), dir.path()] {
            assert!(originals.text(&reader, path, "example.txt").is_err());
        }
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "EXAMPLE PRIVATE CONTENT"
        );
    }

    fn held(originals: &Originals) -> usize {
        let kept = originals.read.lock().unwrap();
        kept.iter().map(|(_, t)| t.len()).sum()
    }

    #[test]
    fn large_texts_are_held_up_to_a_size() {
        let dir = tempfile::tempdir().unwrap();
        let reader: crate::Reader = Arc::new(|file: crate::UploadFile, _: &mut dyn FnMut(&str)| {
            let size = if file.name == "big.txt" {
                20_000_000
            } else {
                1_000_000
            };
            Ok(" ".repeat(size))
        });
        let originals = Originals::default();
        for n in 0..13 {
            let name = format!("f{n:02}.txt");
            let path = dir.path().join(&name);
            std::fs::write(&path, b"tiny").unwrap();
            originals.text(&reader, &path, &name).unwrap();
        }
        assert!(held(&originals) <= KEPT_BYTES);
        let path = dir.path().join("big.txt");
        std::fs::write(&path, b"tiny").unwrap();
        let big = originals.text(&reader, &path, "big.txt").unwrap();
        assert_eq!(big.len(), 20_000_000);
        assert_eq!(originals.read.lock().unwrap().len(), 1);
        assert_eq!(held(&originals), 20_000_000);
    }
}
