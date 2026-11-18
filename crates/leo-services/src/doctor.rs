//! `leo doctor`: a full health scan of leo, notes, AI and recording.
//!

use std::path::Path;

use crate::config::secret::SecretStore;
use crate::config::Config;
use crate::health::{self, Check, State};

/// One group of checks, headed by what it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub checks: Vec<Check>,
}

/// Run every check.
pub fn scan(
    config: &Config,
    secrets: &dyn SecretStore,
    notes_dir: &Path,
    config_path: &Path,
) -> Vec<Section> {
    vec![
        Section {
            title: "leo",
            checks: leo_checks(config_path),
        },
        Section {
            title: "notes",
            checks: with_recent_problems(notes_checks(notes_dir)),
        },
        Section {
            title: "AI",
            checks: ai_checks(config, secrets),
        },
        Section {
            title: "recording",
            checks: recording_checks(),
        },
    ]
}

fn warn(what: &str, needed_for: &str, note: String) -> Check {
    Check {
        what: what.to_string(),
        needed_for: needed_for.to_string(),
        state: State::Warn { note },
        detail: None,
    }
}

fn with_recent_problems(mut checks: Vec<Check>) -> Vec<Check> {
    let recent = leo_core::diag::recent();
    if !recent.is_empty() {
        checks.push(warn(
            "recent problems",
            "knowing what leo could not do",
            recent.join("\n"),
        ));
    }
    checks
}

fn leo_checks(config_path: &Path) -> Vec<Check> {
    let version = format!("version {}", env!("CARGO_PKG_VERSION"));
    let mut checks = vec![Check::ready("leo", "everything", Some(version))];

    checks.push(if health::on_path("leo") {
        Check::ready("leo on your PATH", "running `leo` from any terminal", None)
    } else {
        Check::missing(
            "leo on your PATH",
            "running `leo` from any terminal",
            if cfg!(windows) {
                "Add %LOCALAPPDATA%\\Leo\\bin to your PATH, then open a new terminal."
            } else {
                "echo 'export PATH=\"$HOME/.local/bin:$PATH\"' >> ~/.zshrc && source ~/.zshrc\n\
                 (bash on a Mac: ~/.bash_profile instead of ~/.zshrc)"
            },
        )
    });

    checks.push(match std::fs::read_to_string(config_path) {
        Err(_) => Check::ready(
            "config file",
            "providers and settings",
            Some("not created yet; using the defaults".to_string()),
        ),
        Ok(text) => match Config::parse(&text) {
            Ok(_) => Check::ready(
                "config file",
                "providers and settings",
                Some(config_path.display().to_string()),
            ),
            Err(e) => {
                let reason = format!("{e:#}").lines().next().unwrap_or("").to_string();
                let mut c = Check::missing(
                    "config file",
                    "providers and settings",
                    &format!(
                        "fix it, or move it aside to start again:\n{}",
                        config_path.display()
                    ),
                );
                c.detail = Some(reason);
                c
            }
        },
    });
    checks
}

fn meaning_check(state: crate::providers::ModelState) -> Check {
    use crate::providers::ModelState;
    let what = "finding notes by meaning";
    let needed_for = "Felix and search in leo";
    match state {
        ModelState::Ready => Check::ready(what, needed_for, Some("bge-small, every file checked".into())),
        ModelState::Missing => warn(
            what,
            needed_for,
            format!(
                "the model ({} MB) downloads the next time leo opens; until then search matches words only",
                crate::meaning::MODEL_MB
            ),
        ),
        ModelState::Damaged => warn(
            what,
            needed_for,
            "the model is damaged: leo downloads the damaged parts again the next time it opens".into(),
        ),
    }
}

fn notes_checks(notes_dir: &Path) -> Vec<Check> {
    let mut checks = Vec::new();

    let writable = std::fs::create_dir_all(notes_dir).is_ok()
        && tempfile::NamedTempFile::new_in(notes_dir).is_ok();
    checks.push(if writable {
        Check::ready(
            "notes directory",
            "saving notes",
            Some(notes_dir.display().to_string()),
        )
    } else {
        Check::missing(
            "notes directory",
            "saving notes",
            &format!(
                "leo cannot write to {} — check its permissions",
                notes_dir.display()
            ),
        )
    });
    checks.push(meaning_check(crate::meaning::state()));

    let store = match leo_core::store::Store::load_from(notes_dir) {
        Ok(store) => store,
        Err(e) => {
            checks.push(Check::missing(
                "notes",
                "everything",
                &format!("could not be read: {e}"),
            ));
            return checks;
        }
    };
    let dirs = store.directories.len();
    checks.push(Check::ready(
        "notes",
        "everything",
        Some(format!(
            "{} note{} in {} director{}",
            store.notes.len(),
            if store.notes.len() == 1 { "" } else { "s" },
            dirs,
            if dirs == 1 { "y" } else { "ies" }
        )),
    ));

    if !store.unreadable.is_empty() {
        let list: String = store
            .unreadable
            .iter()
            .map(|(path, why)| format!("{} — {why}\n", path.display()))
            .collect();
        checks.push(Check::missing(
            "unreadable notes",
            "seeing every note",
            &format!(
                "leo skips these (it never deletes them). Open each in your editor and fix \
                 the header between the --- lines:\n{list}"
            ),
        ));
    }

    let copies: Vec<&str> = store
        .notes
        .iter()
        .filter(|n| n.title.ends_with(leo_core::store::CONFLICT_SUFFIX))
        .map(|n| n.title.as_str())
        .collect();
    if !copies.is_empty() {
        checks.push(warn(
            "conflict copies",
            "keeping every edit",
            format!(
                "another program changed a note while leo had it open, so leo kept your edit as a copy. Compare each with its original and delete the one you do not want:\n{}",
                copies.join("\n")
            ),
        ));
    }
    checks
}

fn speech_result(selected: anyhow::Result<crate::speech::library::Selected>) -> Check {
    let what = "speech model";
    let needed_for = "turning speech into text on this computer";
    match selected {
        Ok(selected) => Check::ready(what, needed_for, Some(selected.id)),
        Err(error) => Check::missing(what, needed_for, &error.to_string()),
    }
}

fn ai_checks(config: &Config, secrets: &dyn SecretStore) -> Vec<Check> {
    vec![
        health::chain_check(config, health::Chain::Chat, secrets),
        health::credentials_check(),
        speech_result(crate::speech::library::library().and_then(|l| l.selected())),
    ]
}

fn recording_checks() -> Vec<Check> {
    vec![health::microphone_device()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_speech_model_has_a_settings_fix_without_a_microphone_probe() {
        let dir = TempDir::new().unwrap();
        let library = crate::speech::library::Library::new(
            dir.path().join("models"),
            dir.path().join("config"),
        );
        let check = speech_result(library.selected());
        assert!(!check.state.is_ready());
        assert!(format!("{check:?}").contains("Settings → Speech models"));
    }

    use crate::config::secret::MemoryStore;
    use tempfile::TempDir;

    fn section<'a>(sections: &'a [Section], title: &str) -> &'a Section {
        sections
            .iter()
            .find(|s| s.title == title)
            .unwrap_or_else(|| panic!("no {title} section"))
    }

    fn check<'a>(section: &'a Section, what: &str) -> &'a Check {
        section
            .checks
            .iter()
            .find(|c| c.what == what)
            .unwrap_or_else(|| {
                panic!(
                    "no {what:?} check in {}: {:?}",
                    section.title, section.checks
                )
            })
    }

    fn setup() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let notes = tmp.path().join("notes");
        let config = tmp.path().join("config.toml");
        (tmp, notes, config)
    }

    #[test]
    fn every_missing_check_carries_a_fix() {
        let (_tmp, notes, config) = setup();
        for section in scan(&Config::default(), &MemoryStore::default(), &notes, &config) {
            for check in section.checks {
                if let State::Missing { fix } = &check.state {
                    assert!(
                        !fix.trim().is_empty(),
                        "{} is missing with no fix",
                        check.what
                    );
                }
            }
        }
    }

    #[test]
    fn every_part_of_leo_has_a_section_in_order() {
        let (_tmp, notes, config) = setup();
        let sections = scan(&Config::default(), &MemoryStore::default(), &notes, &config);
        let titles: Vec<&str> = sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["leo", "notes", "AI", "recording"]);
        let speech = section(&sections, "AI")
            .checks
            .iter()
            .filter(|c| c.what.contains("speech") || c.what.contains("transcription"))
            .count();
        assert_eq!(speech, 1);
        assert!(matches!(
            meaning_check(crate::providers::ModelState::Missing).state,
            State::Warn { ref note } if note.contains("next time leo opens")
        ));
        for state in [
            crate::providers::ModelState::Missing,
            crate::providers::ModelState::Damaged,
        ] {
            assert!(!format!("{:?}", meaning_check(state)).contains("leo update"));
        }
        assert!(matches!(
            meaning_check(crate::providers::ModelState::Ready).state,
            State::Ready
        ));
    }

    #[test]
    fn the_notes_are_counted_and_an_unreadable_one_is_named() {
        let (_tmp, notes, config) = setup();
        let mut store = leo_core::store::Store::load_from(&notes).unwrap();
        store.create_note("A", "a", vec![], "").unwrap();
        store.create_note("B", "b", vec![], "cs130").unwrap();
        store.create_dir("cs130");
        store.save().unwrap();
        std::fs::write(notes.join("broken.md"), "---\ntitle: [unclosed\n---\nx").unwrap();

        let sections = scan(&Config::default(), &MemoryStore::default(), &notes, &config);
        let notes_section = section(&sections, "notes");
        let count = check(notes_section, "notes");
        assert!(count.state.is_ready());
        assert!(
            count.detail.as_deref().unwrap_or("").contains("2 notes"),
            "{count:?}"
        );

        let unreadable = check(notes_section, "unreadable notes");
        match &unreadable.state {
            State::Missing { fix } => assert!(fix.contains("broken.md"), "{fix}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_config_file_that_does_not_parse_is_a_failure() {
        let (_tmp, notes, config) = setup();
        std::fs::write(&config, "[chat\nchain = [").unwrap();
        let sections = scan(&Config::default(), &MemoryStore::default(), &notes, &config);
        let c = check(section(&sections, "leo"), "config file");
        assert!(matches!(c.state, State::Missing { .. }), "{c:?}");
    }

    #[test]
    fn no_config_file_yet_is_fine() {
        let (_tmp, notes, config) = setup();
        let sections = scan(&Config::default(), &MemoryStore::default(), &notes, &config);
        assert!(check(section(&sections, "leo"), "config file")
            .state
            .is_ready());
    }

    #[test]
    fn a_conflict_copy_is_reported_with_what_to_do() {
        let (_tmp, notes, config) = setup();
        let mut store = leo_core::store::Store::load_from(&notes).unwrap();
        store
            .create_note("Shared (conflict from leo)", "x", vec![], "")
            .unwrap();
        store.save().unwrap();

        let sections = scan(&Config::default(), &MemoryStore::default(), &notes, &config);
        match &check(section(&sections, "notes"), "conflict copies").state {
            State::Warn { note } => {
                assert!(note.contains("Shared (conflict from leo)"), "{note}");
                assert!(note.contains("delete"), "{note}");
            }
            other => panic!("expected a note, got {other:?}"),
        }
    }

    #[test]
    fn a_recent_warning_is_listed_under_recent_problems() {
        let (_tmp, notes, config) = setup();
        leo_core::diag::warn("x test marker");
        let sections = scan(&Config::default(), &MemoryStore::default(), &notes, &config);
        match &check(section(&sections, "notes"), "recent problems").state {
            State::Warn { note } => assert!(note.contains("x test marker"), "{note}"),
            other => panic!("expected a note, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod safe_probe_tests {
    #[test]
    fn a_health_scan_never_truncates_an_existing_probe_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".leo-doctor-probe");
        std::fs::write(&path, b"keep this file").unwrap();
        super::notes_checks(dir.path());
        assert_eq!(std::fs::read(path).unwrap(), b"keep this file");
    }
}
