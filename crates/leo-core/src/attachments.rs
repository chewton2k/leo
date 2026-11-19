use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::Digest as _;

pub const DIR: &str = "attachments";
pub const MOST_BYTES: usize = 20 * 1024 * 1024;
pub const HEAD_BYTES: usize = 16;

pub fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

pub fn mime_of(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 3)?;
    Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
}

pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match kind_of(bytes)? {
        "png" => Some((
            u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
            u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
        )),
        "gif" => Some((le16(bytes, 6)?, le16(bytes, 8)?)),
        "webp" => match bytes.get(12..16)? {
            b"VP8 " => Some((le16(bytes, 26)? & 0x3fff, le16(bytes, 28)? & 0x3fff)),
            b"VP8L" => {
                let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
            _ => None,
        },
        _ => {
            let mut at = 2;
            while at + 9 < bytes.len() {
                if bytes[at] != 0xff {
                    return None;
                }
                let marker = bytes[at + 1];
                if marker == 0xff {
                    at += 1;
                    continue;
                }
                let length = be16(bytes, at + 2)? as usize;
                let frame = matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
                if frame {
                    return Some((be16(bytes, at + 7)?, be16(bytes, at + 5)?));
                }
                at += 2 + length;
            }
            None
        }
    }
}

fn stem_of(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "image".to_string()
    } else {
        out
    }
}

pub fn save(notes_dir: &Path, name: &str, bytes: &[u8]) -> Result<String> {
    let (head, rest) = bytes.split_at(bytes.len().min(HEAD_BYTES));
    save_from(notes_dir, name, head, rest)
}

pub fn save_from(notes_dir: &Path, name: &str, head: &[u8], mut rest: impl Read) -> Result<String> {
    let Some(extension) = kind_of(head) else {
        bail!("{name} is not a picture leo can show: use PNG, JPEG, GIF or WebP");
    };
    let dir = crate::paths::contained_path(notes_dir, Path::new(DIR))?;
    std::fs::create_dir_all(&dir).with_context(|| format!("could not make {}", dir.display()))?;
    let mut temp = tempfile::NamedTempFile::new_in(&dir)
        .with_context(|| format!("could not write in {}", dir.display()))?;
    let mut hash = sha2::Sha256::new();
    hash.update(head);
    temp.write_all(head)
        .and_then(|()| {
            let mut chunk = vec![0; 64 * 1024];
            loop {
                let read = rest.read(&mut chunk)?;
                if read == 0 {
                    return Ok(());
                }
                hash.update(&chunk[..read]);
                temp.write_all(&chunk[..read])?;
            }
        })
        .and_then(|()| temp.as_file().sync_all())
        .with_context(|| format!("could not write {name}"))?;
    let digest: crate::paths::FileDigest = hash.finalize().into();
    let dir = crate::paths::contained_path(notes_dir, Path::new(DIR))?;
    let base = format!(
        "{}-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        stem_of(name)
    );
    for n in 1..1000 {
        let file = if n == 1 {
            format!("{base}.{extension}")
        } else {
            format!("{base}-{n}.{extension}")
        };
        let path = dir.join(&file);
        let saved = match temp.persist_noclobber(&path) {
            Ok(saved) => saved,
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temp = e.file;
                continue;
            }
            Err(e) => return Err(e.error).with_context(|| format!("could not write {name}")),
        };
        let recorded = saved
            .metadata()
            .map_err(anyhow::Error::from)
            .and_then(|meta| record_made(notes_dir, &file, &meta, digest));
        if let Err(e) = recorded {
            crate::diag::warn(format!("leo could not note that it saved {file}: {e:#}"));
        }
        return Ok(format!("{DIR}/{file}"));
    }
    bail!("could not find a free name for {name}")
}

fn decoded(link: &str) -> Option<String> {
    let link = link.trim();
    let link = link
        .strip_prefix('<')
        .and_then(|l| l.strip_suffix('>'))
        .unwrap_or(link);
    let link = link.split(['?', '#']).next().unwrap_or("");
    let bytes = link.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn inside(notes_dir: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || crate::paths::validate_directory(relative).is_err() {
        return None;
    }
    let path = crate::paths::contained_path(notes_dir, Path::new(relative)).ok()?;
    let extension = path.extension()?.to_str()?;
    (mime_of(extension).is_some() && path.is_file()).then_some(path)
}

pub fn resolve(notes_dir: &Path, from_dir: &str, link: &str) -> Option<PathBuf> {
    let link = decoded(link)?;
    let link = link.strip_prefix("./").unwrap_or(&link);
    let from_dir = from_dir.trim_matches('/');
    if crate::paths::validate_directory(from_dir).is_err() {
        return None;
    }
    let mut tries = Vec::new();
    if !from_dir.is_empty() {
        tries.push(format!("{from_dir}/{link}"));
    }
    tries.push(link.to_string());
    if !link.contains('/') {
        tries.push(format!("{DIR}/{link}"));
    }
    tries.iter().find_map(|t| inside(notes_dir, t))
}

pub fn own_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let Some((base, extension)) = name.rsplit_once('.') else {
        return false;
    };
    let bytes = base.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    let stem = base.get(16..).unwrap_or("");
    matches!(extension, "png" | "jpg" | "gif" | "webp")
        && digits(0..8)
        && bytes.get(8) == Some(&b'-')
        && digits(9..15)
        && bytes.get(15) == Some(&b'-')
        && !stem.is_empty()
        && !stem.starts_with('-')
        && !stem.ends_with('-')
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn mentioned(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut plain = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                plain.push(byte.to_ascii_lowercase());
                i += 3;
            }
            None => {
                plain.push(bytes[i].to_ascii_lowercase());
                i += 1;
            }
        }
    }
    let in_name = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    let mut names = Vec::new();
    for (dot, _) in plain.iter().enumerate().filter(|(_, b)| **b == b'.') {
        let Some(extension) = ["png", "jpg", "gif", "webp"].into_iter().find(|ext| {
            plain.get(dot + 1..dot + 1 + ext.len()) == Some(ext.as_bytes())
                && !plain
                    .get(dot + 1 + ext.len())
                    .is_some_and(|b| b.is_ascii_alphanumeric())
        }) else {
            continue;
        };
        let stem_end = if dot > 0 && plain[dot - 1] == b'\\' {
            dot - 1
        } else {
            dot
        };
        let mut start = stem_end;
        while start > 0 && in_name(plain[start - 1]) {
            start -= 1;
        }
        for from in start..stem_end {
            if !plain[from].is_ascii_digit() || (from > start && plain[from - 1].is_ascii_digit()) {
                continue;
            }
            let Ok(stem) = std::str::from_utf8(&plain[from..stem_end]) else {
                continue;
            };
            let name = format!("{stem}.{extension}");
            if own_name(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

pub const MADE: &str = "pictures-made";

static LEDGER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ledger(notes_dir: &Path) -> Result<PathBuf> {
    crate::paths::contained_path(notes_dir, &Path::new(".leo").join(MADE))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedPicture {
    pub stamp: (u64, u128),
    pub digest: Option<crate::paths::FileDigest>,
}

pub type Made = std::collections::HashMap<String, SavedPicture>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SeenPicture {
    pub stamp: (u64, u128),
    pub digest: crate::paths::FileDigest,
}

impl SavedPicture {
    pub fn seen(&self) -> Option<SeenPicture> {
        self.digest.map(|digest| SeenPicture {
            stamp: self.stamp,
            digest,
        })
    }
}

pub fn same_bytes(path: &Path, seen: &SeenPicture) -> Option<crate::paths::EntryStamp> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || written(&meta) != seen.stamp {
        return None;
    }
    let (digest, stamp) = crate::paths::digest_stamped(path, MOST_BYTES as u64).ok()?;
    (digest == seen.digest).then_some(stamp)
}

pub fn seen_now(path: &Path, before: Option<&SeenPicture>) -> Option<SeenPicture> {
    let meta = std::fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_file())?;
    let stamp = written(&meta);
    if let Some(before) = before.filter(|before| before.stamp == stamp) {
        return Some(*before);
    }
    let digest = crate::paths::digest_file(path, MOST_BYTES as u64).ok()?;
    Some(SeenPicture { stamp, digest })
}

pub fn written(meta: &std::fs::Metadata) -> (u64, u128) {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    (meta.len(), modified)
}

pub fn still_made(made: &Made, name: &str, meta: &std::fs::Metadata) -> bool {
    meta.is_file()
        && made
            .get(name)
            .is_some_and(|record| record.digest.is_some() && record.stamp == written(meta))
}

fn record_made(
    notes_dir: &Path,
    file: &str,
    meta: &std::fs::Metadata,
    digest: crate::paths::FileDigest,
) -> Result<()> {
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let root = notes_dir.canonicalize()?;
    let folder = crate::paths::contained_path(&root, Path::new(".leo"))?;
    if !folder.starts_with(&root) {
        bail!("leo could not verify the picture ledger directory");
    }
    match std::fs::create_dir(&folder) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let parent = folder.canonicalize()?;
    if !parent.starts_with(&root)
        || parent.parent() != Some(root.as_path())
        || !parent
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(".leo"))
    {
        bail!("leo could not verify the picture ledger directory");
    }
    let path = crate::paths::contained_path(&parent, Path::new(MADE))?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    let mut out = crate::paths::open_regular_with(&path, options)?;
    let (len, modified) = written(meta);
    let digest = crate::paths::digest_text(&digest);
    out.write_all(format!("{file}\t{len}\t{modified}\t{digest}\n").as_bytes())?;
    out.sync_all()?;
    Ok(())
}

fn parsed(line: &str) -> Option<(String, SavedPicture)> {
    let mut parts = line.split('\t');
    let name = parts.next().filter(|name| own_name(name))?;
    let len = parts.next()?.parse().ok()?;
    let modified = parts.next()?.parse().ok()?;
    Some((
        name.to_string(),
        SavedPicture {
            stamp: (len, modified),
            digest: parts.next().and_then(crate::paths::parse_digest),
        },
    ))
}

pub fn made(notes_dir: &Path) -> Option<Made> {
    use std::io::Read;
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let mut text = String::new();
    match crate::paths::open_regular(&ledger(notes_dir).ok()?) {
        Ok(mut file) => {
            file.read_to_string(&mut text).ok()?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Default::default()),
        Err(_) => return None,
    }
    Some(text.lines().filter_map(parsed).collect())
}

pub fn forget_made(notes_dir: &Path, files: &[String]) {
    let Some(mut known) = made(notes_dir) else {
        return;
    };
    let _held = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    known.retain(|name, _| !files.contains(name));
    if files.is_empty() {
        return;
    }
    let mut text: Vec<_> = known.iter().collect();
    text.sort_by_key(|(name, _)| *name);
    let text: String = text
        .into_iter()
        .map(|(name, record)| {
            let (len, modified) = record.stamp;
            let digest = record
                .digest
                .map_or_else(String::new, |digest| crate::paths::digest_text(&digest));
            format!("{name}\t{len}\t{modified}\t{digest}\n")
        })
        .collect();
    if let Err(e) = ledger(notes_dir).and_then(|path| crate::paths::write_atomic(&path, text)) {
        crate::diag::warn(format!(
            "leo could not update its list of saved pictures: {e:#}"
        ));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub alt: String,
    pub target: String,
}

pub fn pictures_in(line: &str) -> Vec<(std::ops::Range<usize>, Shown)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = line[from..].find('!').map(|i| from + i) {
        let rest = &line[start..];
        if let Some(inner) = rest.strip_prefix("![[") {
            if let Some(end) = inner.find("]]") {
                let name = inner[..end].split('|').next().unwrap_or("").trim();
                let len = 3 + end + 2;
                if !name.is_empty() {
                    out.push((
                        start..start + len,
                        Shown {
                            alt: name.to_string(),
                            target: name.to_string(),
                        },
                    ));
                }
                from = start + len;
                continue;
            }
        } else if let Some(inner) = rest.strip_prefix("![") {
            if let Some(close) = inner.find("](") {
                let after = &inner[close + 2..];
                if let Some(end) = after.find(')') {
                    let target = after[..end].split_whitespace().next().unwrap_or("");
                    let len = 2 + close + 2 + end + 1;
                    if !target.is_empty() {
                        out.push((
                            start..start + len,
                            Shown {
                                alt: inner[..close].trim().to_string(),
                                target: target.to_string(),
                            },
                        ));
                    }
                    from = start + len;
                    continue;
                }
            }
        }
        from = start + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
        0, 2, 128, 0, 0, 1, 224, 8, 2, 0, 0, 0,
    ];

    #[test]
    fn pictures_are_known_by_their_bytes_not_their_names() {
        assert_eq!(kind_of(PNG), Some("png"));
        assert_eq!(dimensions(PNG), Some((640, 480)));
        assert_eq!(kind_of(b"GIF89a\x10\x00\x20\x00"), Some("gif"));
        assert_eq!(dimensions(b"GIF89a\x10\x00\x20\x00"), Some((16, 32)));
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0, 90, 0, 120, 3, 1, 1,
        ];
        assert_eq!(kind_of(&jpeg), Some("jpg"));
        assert_eq!(dimensions(&jpeg), Some((120, 90)));
        assert_eq!(kind_of(b"<svg onload=alert(1)>"), None);
        assert_eq!(kind_of(b""), None);
        assert_eq!(dimensions(&[0xff, 0xd8, 0xff]), None);
    }

    #[test]
    fn a_saved_picture_gets_its_own_name_in_the_attachments_folder() {
        let dir = tempfile::tempdir().unwrap();
        let first = save(dir.path(), "Board Photo!.PNG", PNG).unwrap();
        let second = save(dir.path(), "Board Photo!.PNG", PNG).unwrap();
        assert!(
            first.starts_with("attachments/") && first.ends_with("-board-photo.png"),
            "{first}"
        );
        assert_ne!(first, second);
        assert!(dir.path().join(&second).is_file());
        assert!(save(dir.path(), "x.svg", b"<svg/>").is_err());
    }

    struct Chunks {
        left: usize,
        asked: usize,
        fail_at: Option<usize>,
    }

    impl Read for Chunks {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.fail_at.is_some_and(|at| self.asked >= at) {
                return Err(std::io::Error::other("the connection dropped"));
            }
            let n = self.left.min(4096).min(buf.len());
            buf[..n].fill(7);
            self.left -= n;
            self.asked += n;
            Ok(n)
        }
    }

    fn files_in(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir.join(DIR))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_large_picture_is_written_as_it_arrives() {
        let dir = tempfile::tempdir().unwrap();
        let size = 30 * 1024 * 1024;
        let rest = Chunks {
            left: size - PNG.len(),
            asked: 0,
            fail_at: None,
        };
        let saved = save_from(dir.path(), "scan.png", PNG, rest).unwrap();
        let written = std::fs::read(dir.path().join(&saved)).unwrap();
        assert_eq!(written.len(), size);
        assert_eq!(&written[..PNG.len()], PNG);
        assert!(written[PNG.len()..].iter().all(|b| *b == 7));
        assert_eq!(files_in(dir.path()).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_picture_save_refuses_a_folder_linked_during_upload() {
        let library = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let attachments = library.path().join(DIR);
        let moved = outside.path().join(DIR);
        struct Finishing<F>(Option<F>);

        impl<F: FnOnce()> Read for Finishing<F> {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                if let Some(finish) = self.0.take() {
                    finish();
                }
                Ok(0)
            }
        }

        let changed = Finishing(Some(|| {
            std::fs::rename(&attachments, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &attachments).unwrap();
        }));
        let saved = save_from(library.path(), "picture.png", PNG, changed);
        assert!(saved.is_err(), "leo published a picture through a link");
        assert!(std::fs::read_dir(&moved).unwrap().all(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp")
        }));
    }

    #[test]
    fn a_file_that_is_not_a_picture_is_refused_after_its_first_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut rest = Chunks {
            left: usize::MAX,
            asked: 0,
            fail_at: Some(0),
        };
        let refused = save_from(dir.path(), "x.svg", b"<svg onload=alert(1)>", &mut rest);
        assert!(refused.is_err());
        assert_eq!(rest.asked, 0, "nothing past the first bytes was read");
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_picture_that_stops_arriving_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let rest = Chunks {
            left: 1 << 20,
            asked: 0,
            fail_at: Some(100_000),
        };
        assert!(save_from(dir.path(), "x.png", PNG, rest).is_err());
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_link_is_found_beside_the_note_then_from_the_top_and_never_outside() {
        let dir = tempfile::tempdir().unwrap();
        let saved = save(dir.path(), "a.png", PNG).unwrap();
        let file = saved.rsplit('/').next().unwrap().to_string();
        std::fs::create_dir_all(dir.path().join("cs130")).unwrap();
        std::fs::write(dir.path().join("cs130/local.png"), PNG).unwrap();
        std::fs::write(dir.path().join("notes.md"), "x").unwrap();
        let at = |from: &str, link: &str| resolve(dir.path(), from, link);
        assert_eq!(
            at("cs130", "local.png"),
            Some(dir.path().join("cs130/local.png"))
        );
        assert_eq!(at("cs130", &saved), Some(dir.path().join(&saved)));
        assert_eq!(at("", &file), Some(dir.path().join(&saved)));
        assert_eq!(at("", &format!("<{saved}>")), Some(dir.path().join(&saved)));
        assert_eq!(
            at("", &saved.replace('-', "%2D")),
            Some(dir.path().join(&saved))
        );
        assert_eq!(
            at("../..", &saved),
            None,
            "a bad folder is refused, not skipped"
        );
        for bad in [
            "../secret.png",
            "/etc/passwd",
            "notes.md",
            ".git/x.png",
            "https://x/y.png",
            "cs130/../a.png",
            "missing.png",
            "%zz",
        ] {
            assert_eq!(at("cs130", bad), None, "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_with_a_control_character_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("attachments")).unwrap();
        std::fs::write(dir.path().join("attachments/a.png"), PNG).unwrap();
        std::fs::write(dir.path().join("attachments/a\nb.png"), PNG).unwrap();
        assert_eq!(resolve(dir.path(), "", "attachments/a%0Ab.png"), None);
        assert_eq!(
            resolve(dir.path(), "", "attachments/a.png"),
            Some(dir.path().join("attachments/a.png"))
        );
    }

    #[test]
    fn text_without_pictures_has_none() {
        assert_eq!(pictures_in("no pictures! here").len(), 0);
    }

    #[test]
    fn leos_own_picture_names_are_known_and_others_are_not() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Board Photo!.PNG", "Board Photo!.PNG", "x.gif", "...webp"] {
            let saved = save(dir.path(), name, PNG).unwrap();
            let file = saved.rsplit('/').next().unwrap();
            assert!(own_name(file), "{file}");
            assert!(own_name(&file.to_ascii_uppercase()), "{file}");
        }
        assert!(own_name("20261117-101500-board-photo-2.png"));
        for name in [
            "photo.png",
            "../x.png",
            "20261117-101500-a.png.txt",
            "20261117-101500-.png",
            "20261117-101500-a b.png",
            "20261117-1015-a.png",
            "20261117-101500-a.svg",
        ] {
            assert!(!own_name(name), "{name}");
        }
    }

    #[test]
    fn every_way_a_text_can_name_a_picture_counts() {
        let own = "20261117-101500-slide.png";
        for text in [
            format!("![a](attachments/{own})"),
            format!("![[{own}|400]]"),
            format!("[plain link](attachments/{own})"),
            format!("[ref]: attachments/{own}\n\nSee [it][ref]"),
            format!("<img src=\"attachments/{own}\" width=200>"),
            format!("cover: attachments/{own}"),
            "![a](attachments/20261117%2D101500%2Dslide.png)".to_string(),
            "![a](Attachments/20261117-101500-SLIDE.PNG)".to_string(),
            "![a](cs130/attachments/20261117-101500-slide.png)".to_string(),
            "![a](attachments/20261117-101500-slide\\.png)".to_string(),
        ] {
            assert_eq!(mentioned(&text), [own], "{text}");
        }
        assert!(mentioned("photo.png and 2026-11-17.png").is_empty());
    }

    #[test]
    fn leo_remembers_each_picture_it_saved() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(made(dir.path()), Some(Default::default()));
        let saved = save(dir.path(), "a.png", PNG).unwrap();
        let file = saved.rsplit('/').next().unwrap().to_string();
        assert!(made(dir.path()).unwrap().contains_key(&file));
        let path = dir.path().join(&saved);
        let meta = std::fs::metadata(&path).unwrap();
        assert!(still_made(&made(dir.path()).unwrap(), &file, &meta));
        std::fs::write(&path, b"another file with the same name").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(
            !still_made(&made(dir.path()).unwrap(), &file, &meta),
            "a replaced file is not leo's"
        );
        forget_made(dir.path(), std::slice::from_ref(&file));
        assert!(!made(dir.path()).unwrap().contains_key(&file));
        std::fs::write(dir.path().join(".leo").join(MADE), [0xff, 0xfe]).unwrap();
        assert_eq!(
            made(dir.path()),
            None,
            "a ledger leo cannot read deletes nothing"
        );
    }
}
