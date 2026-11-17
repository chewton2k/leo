use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::token;

pub const COOKIE: &str = "leo_session";
pub const KEEP_DAYS: i64 = 30;
const TOUCH_EVERY_SECS: i64 = 300;
const MOST: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub secret: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

fn off_worker<R>(work: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(work)
        }
        _ => work(),
    }
}

pub struct Sessions {
    path: Option<PathBuf>,
    list: Mutex<Vec<Session>>,
}

impl Sessions {
    pub fn in_memory() -> Sessions {
        Sessions {
            path: None,
            list: Mutex::new(Vec::new()),
        }
    }

    pub fn load(path: &Path) -> Result<Sessions> {
        let mut list: Vec<Session> = match token::read_private(path).with_context(|| {
            format!(
                "could not safely read {}; check its permissions",
                path.display()
            )
        })? {
            Some(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                leo_core::diag::warn(format!(
                    "{} is damaged ({e}); signing in again",
                    path.display()
                ));
                Vec::new()
            }),
            None => Vec::new(),
        };
        list.retain(|s| Utc::now() - s.last_seen < Duration::days(KEEP_DAYS));
        list.sort_by_key(|s| std::cmp::Reverse(s.last_seen));
        list.truncate(MOST);
        Ok(Sessions {
            path: Some(path.to_path_buf()),
            list: Mutex::new(list),
        })
    }

    fn save(&self, list: &[Session]) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(list)?;
        token::write_private(path, &text).with_context(|| {
            format!(
                "could not safely save {}; check its permissions",
                path.display()
            )
        })
    }

    fn change<R>(&self, work: impl FnOnce(&mut Vec<Session>) -> (R, bool)) -> Result<R> {
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut updated = list.clone();
        let (out, changed) = work(&mut updated);
        if changed {
            off_worker(|| self.save(&updated))?;
            *list = updated;
        }
        Ok(out)
    }

    pub fn start(&self, now: DateTime<Utc>) -> Result<String> {
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let session = Session {
            secret: secret.clone(),
            created_at: now,
            last_seen: now,
        };
        self.change(|list| {
            list.retain(|s| now - s.last_seen < Duration::days(KEEP_DAYS));
            list.push(session);
            list.sort_by_key(|s| std::cmp::Reverse(s.last_seen));
            list.truncate(MOST);
            ((), true)
        })?;
        Ok(secret)
    }

    pub fn check(&self, secret: &str, now: DateTime<Utc>) -> bool {
        self.change(|list| {
            let Some(found) = list.iter_mut().find(|s| token::same(&s.secret, secret)) else {
                return (false, false);
            };
            if now - found.last_seen >= Duration::days(KEEP_DAYS) {
                list.retain(|s| !token::same(&s.secret, secret));
                return (false, true);
            }
            let stale = (now - found.last_seen).num_seconds() >= TOUCH_EVERY_SECS;
            if stale {
                found.last_seen = now;
            }
            (true, stale)
        })
        .unwrap_or(false)
    }
}

pub struct Gate {
    token: std::sync::RwLock<String>,
    pub sessions: Sessions,
}

impl Gate {
    pub fn new(token: String, sessions: Sessions) -> Gate {
        Gate {
            token: std::sync::RwLock::new(token),
            sessions,
        }
    }

    pub fn token_matches(&self, given: &str) -> bool {
        let token = self.token.read().unwrap_or_else(|e| e.into_inner());
        token::same(given, &token)
    }
}

pub fn cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        static START: std::sync::OnceLock<DateTime<Utc>> = std::sync::OnceLock::new();
        *START.get_or_init(Utc::now) + Duration::minutes(minutes)
    }

    #[test]
    fn each_sign_in_gets_its_own_session_that_survives_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        let sessions = Sessions::load(&path).unwrap();
        let first = sessions.start(at(0)).unwrap();
        let second = sessions.start(at(1)).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.len(), 64);
        assert!(sessions.check(&first, at(2)));
        assert!(!sessions.check("nope", at(2)));

        let again = Sessions::load(&path).unwrap();
        assert!(again.check(&second, at(5)), "sessions survive a restart");
    }

    #[test]
    fn a_session_file_with_unknown_fields_still_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        let when = at(0).to_rfc3339();
        std::fs::write(
            &path,
            format!(
                r#"[{{"secret":"abc","created_at":"{when}","last_seen":"{when}","extra":"ignored"}}]"#
            ),
        )
        .unwrap();
        assert!(Sessions::load(&path).unwrap().check("abc", at(1)));
    }

    #[test]
    fn old_sessions_expire() {
        let sessions = Sessions::in_memory();
        let mine = sessions.start(at(0)).unwrap();
        assert!(sessions.check(&mine, at(1)));
        let month = KEEP_DAYS * 24 * 60;
        assert!(
            !sessions.check(&mine, at(month + 10)),
            "a session unused for a month expires"
        );
    }

    #[test]
    fn last_seen_is_written_at_most_every_few_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.json");
        let sessions = Sessions::load(&path).unwrap();
        let id = sessions.start(at(0)).unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(sessions.check(&id, at(1)));
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
        assert!(sessions.check(&id, at(6)));
        assert_ne!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
    }

    #[test]
    fn a_touch_that_writes_works_inside_either_kind_of_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        for multi in [false, true] {
            let sessions = Sessions::load(&tmp.path().join(format!("{multi}.json"))).unwrap();
            let id = sessions.start(at(0)).unwrap();
            let mut builder = if multi {
                tokio::runtime::Builder::new_multi_thread()
            } else {
                tokio::runtime::Builder::new_current_thread()
            };
            let runtime = builder.build().unwrap();
            assert!(runtime.block_on(async { sessions.check(&id, at(6)) }));
            let again = Sessions::load(&tmp.path().join(format!("{multi}.json"))).unwrap();
            assert!(again.check(&id, at(7)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn existing_session_files_are_private_on_load_and_after_a_change() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        std::fs::write(&path, "[]").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let sessions = Sessions::load(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let secret = sessions.start(at(0)).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(Sessions::load(&path).unwrap().check(&secret, at(1)));
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_session_paths_cannot_load_start_or_refresh_a_sign_in() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        let sessions = Sessions::load(&path).unwrap();
        let secret = sessions.start(at(0)).unwrap();
        let target = tmp.path().join("example-file");
        std::fs::write(&target, "[]").unwrap();
        std::fs::remove_file(&path).unwrap();
        symlink(&target, &path).unwrap();
        assert!(Sessions::load(&path).is_err());
        assert!(sessions.start(at(1)).is_err());
        assert!(!sessions.check(&secret, at(6)));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "[]");
    }

    #[test]
    fn oversized_session_files_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        std::fs::write(&path, " ".repeat(token::AUTH_BYTES as usize + 1)).unwrap();
        assert!(Sessions::load(&path).is_err());
    }

    #[test]
    fn a_damaged_session_file_loads_as_no_sessions_and_is_rewritten() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        for text in ["broken", ""] {
            std::fs::write(&path, text).unwrap();
            let sessions = Sessions::load(&path).unwrap();
            assert!(!sessions.check("broken", Utc::now()));
            let mine = sessions.start(Utc::now()).unwrap();
            assert!(Sessions::load(&path).unwrap().check(&mine, Utc::now()));
        }
    }

    #[test]
    fn expired_sessions_are_dropped_on_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app-sessions.json");
        let old = Utc::now() - Duration::days(KEEP_DAYS + 1);
        let fresh = Utc::now();
        let list = [("old", old), ("fresh", fresh)].map(|(secret, seen)| Session {
            secret: secret.into(),
            created_at: seen,
            last_seen: seen,
        });
        std::fs::write(&path, serde_json::to_string(&list).unwrap()).unwrap();
        let sessions = Sessions::load(&path).unwrap();
        let held = sessions.list.lock().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].secret, "fresh");
    }

    #[tokio::test]
    async fn a_damaged_sessions_file_does_not_stop_the_engine() {
        for content in ["x", ""] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("app-sessions.json");
            std::fs::write(&path, content).unwrap();
            let running = crate::start_in(
                crate::Places {
                    store: leo_core::store::Store::load_from(&dir.path().join("notes")).unwrap(),
                    sessions: path,
                },
                crate::StartOptions {
                    port: 0,
                    token: "t".into(),
                },
                crate::Powers::default(),
            )
            .await
            .unwrap();
            running.stop().await;
        }
    }

    #[test]
    fn cookies_are_read_by_exact_name() {
        let header = "leo_session=abc; other=1";
        assert_eq!(cookie_value(header, COOKIE), Some("abc"));
        assert_eq!(cookie_value("xleo_session=bad", COOKIE), None);
    }
}
