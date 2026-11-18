use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

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
    temp.write_all(head)
        .and_then(|()| std::io::copy(&mut rest, &mut temp).map(drop))
        .and_then(|()| temp.as_file().sync_all())
        .with_context(|| format!("could not write {name}"))?;
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
        match temp.persist_noclobber(dir.join(&file)) {
            Ok(_) => return Ok(format!("{DIR}/{file}")),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temp = e.file,
            Err(e) => return Err(e.error).with_context(|| format!("could not write {name}")),
        }
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
}
