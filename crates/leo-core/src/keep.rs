use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CHOICES: [Option<u32>; 5] = [Some(7), Some(30), Some(90), Some(365), None];
pub const DEFAULT_TRASH_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keep {
    #[serde(default = "default_trash_days")]
    pub trash_days: Option<u32>,
    #[serde(default)]
    pub chat_days: Option<u32>,
    #[serde(default)]
    pub upload_days: Option<u32>,
    #[serde(default)]
    pub transcript_days: Option<u32>,
    #[serde(default)]
    pub audio_days: Option<u32>,
}

fn default_trash_days() -> Option<u32> {
    Some(DEFAULT_TRASH_DAYS)
}

impl Default for Keep {
    fn default() -> Self {
        Keep {
            trash_days: default_trash_days(),
            chat_days: None,
            upload_days: None,
            transcript_days: None,
            audio_days: None,
        }
    }
}

pub fn path_for(notes_dir: &Path) -> PathBuf {
    notes_dir
        .parent()
        .unwrap_or(notes_dir)
        .join(crate::paths::own::KEEP)
}

pub fn load(notes_dir: &Path) -> Keep {
    let read: serde_json::Value = std::fs::read_to_string(path_for(notes_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let offered = |field: &str, otherwise: Option<u32>| match read.get(field) {
        Some(serde_json::Value::Null) => None,
        Some(days) => days
            .as_u64()
            .and_then(|days| u32::try_from(days).ok())
            .map(Some)
            .filter(|days| CHOICES.contains(days))
            .unwrap_or(otherwise),
        None => otherwise,
    };
    Keep {
        trash_days: match read.get("trash_days").and_then(serde_json::Value::as_u64) {
            Some(days) => CHOICES
                .into_iter()
                .find(|choice| {
                    choice.is_none_or(|c| {
                        u64::from(c) == days || u64::from(c) >= days.max(DEFAULT_TRASH_DAYS.into())
                    })
                })
                .flatten(),
            None => offered("trash_days", default_trash_days()),
        },
        chat_days: offered("chat_days", None),
        upload_days: offered("upload_days", None),
        transcript_days: offered("transcript_days", None),
        audio_days: offered("audio_days", None),
    }
}

pub fn save(notes_dir: &Path, keep: &Keep) -> Result<()> {
    let days = [
        keep.trash_days,
        keep.chat_days,
        keep.upload_days,
        keep.transcript_days,
        keep.audio_days,
    ];
    if !days.iter().all(|d| CHOICES.contains(d)) {
        anyhow::bail!("that is not one of the choices");
    }
    let path = path_for(notes_dir);
    let dir = path.parent().unwrap_or(notes_dir);
    std::fs::create_dir_all(dir)?;
    crate::paths::write_atomic(&path, serde_json::to_string_pretty(keep)?)
        .with_context(|| format!("could not write {}", path.display()))
}

pub fn describe(days: Option<u32>) -> String {
    match days {
        None => "forever".to_string(),
        Some(365) => "a year".to_string(),
        Some(1) => "a day".to_string(),
        Some(n) => format!("{n} days"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_edited_period_outside_the_choices_never_shortens_what_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(
            path_for(&notes),
            r#"{"trash_days":0,"chat_days":1,"upload_days":0,"transcript_days":3,"audio_days":4000000000}"#,
        )
        .unwrap();
        assert_eq!(load(&notes), Keep::default());
        std::fs::write(path_for(&notes), r#"{"upload_days":90,"audio_days":7}"#).unwrap();
        let kept = load(&notes);
        assert_eq!((kept.upload_days, kept.audio_days), (Some(90), Some(7)));
    }

    #[test]
    fn nothing_saved_means_thirty_days_of_trash_and_chats_kept_forever() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        assert_eq!(load(&notes), Keep::default());
        assert_eq!(Keep::default().trash_days, Some(30));
        assert_eq!(Keep::default().chat_days, None);
        std::fs::write(path_for(&notes), "{}").unwrap();
        assert_eq!(
            load(&notes),
            Keep::default(),
            "missing keys take the defaults"
        );
        std::fs::write(path_for(&notes), "not json").unwrap();
        assert_eq!(load(&notes), Keep::default());
    }

    #[test]
    fn a_choice_is_saved_beside_the_notes_folder_and_odd_values_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        let keep = Keep {
            trash_days: None,
            chat_days: Some(90),
            ..Keep::default()
        };
        save(&notes, &keep).unwrap();
        assert_eq!(load(&notes), keep);
        assert_eq!(path_for(&notes), tmp.path().join("keep.json"));
        let odd = Some(3);
        for bad in [
            Keep {
                trash_days: odd,
                ..Keep::default()
            },
            Keep {
                chat_days: odd,
                ..Keep::default()
            },
            Keep {
                upload_days: odd,
                ..Keep::default()
            },
            Keep {
                transcript_days: odd,
                ..Keep::default()
            },
            Keep {
                audio_days: odd,
                ..Keep::default()
            },
        ] {
            assert!(save(&notes, &bad).is_err());
        }
        assert_eq!(load(&notes), keep);
    }

    #[test]
    fn a_file_with_only_the_older_keys_loads_and_the_new_ones_keep_forever() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::write(path_for(&notes), r#"{"trash_days": 90, "chat_days": 7}"#).unwrap();
        assert_eq!(
            load(&notes),
            Keep {
                trash_days: Some(90),
                chat_days: Some(7),
                ..Keep::default()
            }
        );
        assert_eq!(load(&notes).upload_days, None);
        assert_eq!(load(&notes).transcript_days, None);
        assert_eq!(load(&notes).audio_days, None);
    }

    #[test]
    fn one_unreadable_period_leaves_the_others_as_they_were_saved() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        std::fs::write(
            path_for(&notes),
            r#"{"trash_days":null,"chat_days":90,"upload_days":"30","audio_days":-1}"#,
        )
        .unwrap();
        let keep = load(&notes);
        assert_eq!(keep.trash_days, None);
        assert_eq!(keep.chat_days, Some(90));
        assert_eq!(keep.upload_days, None);
        assert_eq!(keep.transcript_days, None);
        assert_eq!(keep.audio_days, None);
    }

    #[test]
    fn a_trash_period_outside_the_choices_rounds_up_to_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        for (stored, kept) in [
            ("180", Some(365)),
            ("1", Some(30)),
            ("45", Some(90)),
            ("7", Some(7)),
            ("400", None),
            ("4000000000", None),
            ("\"180\"", Some(30)),
        ] {
            std::fs::write(path_for(&notes), format!(r#"{{"trash_days":{stored}}}"#)).unwrap();
            assert_eq!(load(&notes).trash_days, kept, "{stored}");
        }
    }

    #[test]
    fn choices_read_like_speech() {
        assert_eq!(describe(None), "forever");
        assert_eq!(describe(Some(365)), "a year");
        assert_eq!(describe(Some(30)), "30 days");
    }
}
