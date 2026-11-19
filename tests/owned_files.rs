mod common;

use common::shipped_part;
use leo_core::paths::own;
use std::path::{Path, PathBuf};

const ELSEWHERE: &[(&str, &str)] = &[
    (",", "separator"),
    (".", "folder"),
    (";", "separator"),
    ("--cpu-only", "argument"),
    ("--hidden", "argument"),
    ("--leo-speech-worker", "argument"),
    (".before-database", "suffix"),
    (".codex", "codex"),
    (".leo", "home"),
    (".leo-download-", "models"),
    (".trash", "notes"),
    ("directories.json", "notes"),
    ("trash.json", "keeping"),
    ("pictures.json", "keeping"),
    ("pictures-made", "notes"),
    ("picture:", "label"),
    ("Downloads", "home"),
    ("LEO_TRANSCRIBE_WORKER_LOG", "variable"),
    ("Leo", "app"),
    ("bin", "app"),
    ("leo-app.exe", "app"),
    ("leo.exe", "app"),
    ("uninstall.exe", "app"),
    ("io.github.chewton2k.leo", "bundle"),
    ("Microphone", "label"),
    ("leo-quit", "label"),
    ("leo-file:", "label"),
    (".read", "attachments"),
    (".parts", "attachments"),
    ("main", "window"),
    ("medium", "effort"),
    ("transcribe_worker", "log"),
    ("__LEO_PATH__", "marker"),
    ("leo", "folder"),
    ("leo_session", "cookie"),
    ("credentials", "keychain"),
    ("combined:", "database"),
    ("deleting", "database"),
    ("deleting-shown", "database"),
    ("retarget", "database"),
    ("holding", "database"),
    ("kept-files", "database"),
    ("held", "data"),
    ("chats", "anchor"),
    ("leo-test-vad", "temp"),
    ("leo-uninstall.log", "temp"),
    ("bge-small-en-v1.5", "models"),
    ("config.json", "models"),
    ("model.safetensors", "models"),
    ("tokenizer.json", "models"),
    ("speech", "models"),
    ("vad", "models"),
    ("silero-vad-v6.2-F32.gguf", "models"),
    ("parakeet-tdt-0.6b-v3", "models"),
    ("diar_streaming_sortformer_4spk-v2.1", "models"),
    ("session.json", "recording"),
    ("drafts.json", "recording"),
    ("prepared.json", "recording"),
    ("speakers.json", "recording"),
    ("points.jsonl", "recording"),
    ("trace.jsonl", "recording"),
    ("lock", "recording"),
];

const LEGACY_ON_2026_11_17: &[&str] = &[
    "serve-sessions.json",
    "config.toml.new",
    "update-check.json",
    "recent.json",
    ".manual-installed",
    ".tour-completed",
    "notes.json",
    "notes.json.bak",
    "leo-recording",
    "leo-live-",
    "leo-mic-probe-",
];

const WRITTEN_ELSEWHERE: &[(&str, &str)] = &[(own::CLI_DIR, "the install scripts")];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn production_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !matches!(name.as_str(), "tests" | "target" | "node_modules" | "gen") {
                production_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            out.push(path);
        }
    }
}

fn without_test_module(text: &str) -> &str {
    let mut at = 0;
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("#[cfg(")
            && line.contains("test")
            && lines
                .peek()
                .is_some_and(|next| next.starts_with("mod ") && next.trim_end().ends_with('{'))
        {
            return text.get(..at).unwrap_or(text);
        }
        at += line.len();
    }
    text
}

fn sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    production_files(&root().join("src"), &mut files);
    for entry in std::fs::read_dir(root().join("crates")).unwrap().flatten() {
        production_files(&entry.path().join("src"), &mut files);
    }
    files.sort();
    files
        .into_iter()
        .map(|file| {
            let text = shipped_part(without_test_module(
                &std::fs::read_to_string(&file).unwrap(),
            ));
            let shown = file
                .strip_prefix(root())
                .unwrap_or(&file)
                .display()
                .to_string();
            (shown, text)
        })
        .collect()
}

fn literal_after(text: &str, at: usize) -> Option<&str> {
    let rest = text[at..].trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn names_in(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for opener in [".join(", "Path::new("] {
        for (at, _) in text.match_indices(opener) {
            if let Some(name) = literal_after(text, at + opener.len()) {
                out.push((at, name));
            }
        }
    }
    for (at, _) in text.match_indices("const ") {
        let line = &text[at..text[at..].find('\n').map_or(text.len(), |e| at + e)];
        if let Some(eq) = line.find(": &str =") {
            if let Some(name) = literal_after(text, at + eq + ": &str =".len()) {
                out.push((at, name));
            }
        }
    }
    out.retain(|(_, name)| !name.is_empty() && !name.contains(['/', ' ', '{', '\\']));
    out
}

#[test]
fn every_name_leo_writes_in_its_folders_is_listed() {
    let mut missing = Vec::new();
    for (file, text) in sources() {
        for (at, name) in names_in(&text) {
            if own::listed(name)
                || own::KEPT_WITH_NOTES.contains(&name)
                || name == "notes"
                || ELSEWHERE.iter().any(|(n, _)| *n == name)
            {
                continue;
            }
            missing.push(format!(
                "{file}:{} {name}",
                text[..at].matches('\n').count() + 1
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "add each name to leo_core::paths::own (uninstall then removes it) or to ELSEWHERE with the folder it lives in:\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_legacy_list_only_shrinks() {
    for name in own::LEGACY.iter().chain(own::LEGACY_TEMP) {
        assert!(
            LEGACY_ON_2026_11_17.contains(name),
            "{name} joined the legacy list; a name today's code writes belongs in own::ALL"
        );
    }
}

fn constant_named(paths: &str, value: &str) -> Option<String> {
    paths.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("pub const ")?;
        let (name, literal) = rest.split_once(": &str = ")?;
        (literal == format!("\"{value}\";")).then(|| name.to_string())
    })
}

#[test]
fn every_listed_name_is_still_written() {
    let paths = std::fs::read_to_string(root().join("crates/leo-core/src/paths.rs")).unwrap();
    let others: Vec<(String, String)> = sources()
        .into_iter()
        .filter(|(file, _)| !file.ends_with("leo-core/src/paths.rs"))
        .collect();
    let mut unwritten = Vec::new();
    for name in own::ALL
        .iter()
        .chain(own::MIGRATED)
        .chain(own::KEPT_WITH_NOTES)
    {
        if WRITTEN_ELSEWHERE.iter().any(|(n, _)| n == name) {
            continue;
        }
        let quoted = format!("\"{name}\"");
        let constant = constant_named(&paths, name).map(|c| format!("own::{c}"));
        let written = others.iter().any(|(_, text)| {
            text.contains(&quoted) || constant.as_ref().is_some_and(|c| text.contains(c.as_str()))
        });
        if !written {
            unwritten.push(*name);
        }
    }
    assert!(
        unwritten.is_empty(),
        "no production code writes these any more; move them to own::LEGACY or drop them: {unwritten:?}"
    );
}
