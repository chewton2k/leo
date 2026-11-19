//! Credentials in a file only this user can read.
//!
//! This is the default because the OS keychain, on macOS, cannot be made quiet
//! for a tool like this. A keychain item records *which binary* created it and
//! asks for permission whenever a different one reads it — and every
//! `cargo install` produces a different binary, so the prompt came back after
//! every upgrade, once per provider, with no way to answer it permanently.
//!
//! What this gives up, stated plainly: the file is not encrypted, so it is
//! readable by anything running as this user, and by anyone who can read the
//! disk while it is unlocked. What it keeps: mode `0600` in a `0700` directory,
//! so no other account on the machine can read it, and on macOS FileVault still
//! encrypts it at rest. That is the same arrangement as `~/.aws/credentials`,
//! `~/.npmrc`, and `gh`'s token file.
//!
//! Anyone who wants nothing on disk at all can set the provider's env var, which
//! takes precedence over this file, or opt into the keychain with
//! `LEO_USE_KEYCHAIN=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, OnceLock};

use anyhow::{Context, Result};
use zeroize::Zeroizing;

use super::secret::{Secret, SecretStore};

/// provider name -> key. `BTreeMap` so the file is stable and diffable.
type Bundle = BTreeMap<String, String>;

/// Permissions for the file: readable and writable by the owner only.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;
/// Permissions for the directory holding it.
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;
const MOST_CREDENTIAL_BYTES: u64 = 1024 * 1024;
const QUEUED_WRITES: usize = 32;
const WRITE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

struct Change {
    path: PathBuf,
    account: String,
    secret: Option<Zeroizing<String>>,
    reply: mpsc::Sender<Result<()>>,
}

fn writer() -> Result<&'static mpsc::SyncSender<Change>> {
    static WRITER: OnceLock<std::result::Result<mpsc::SyncSender<Change>, String>> =
        OnceLock::new();
    WRITER
        .get_or_init(|| {
            let (to, from) = mpsc::sync_channel::<Change>(QUEUED_WRITES);
            std::thread::Builder::new()
                .name("leo-credentials".into())
                .spawn(move || {
                    for change in from {
                        let store = FileStore { path: change.path };
                        let result = (|| -> Result<()> {
                            let parent = store
                                .path
                                .parent()
                                .context("The credentials file needs a private folder")?;
                            let _lock = crate::private_file::lock(
                                parent,
                                leo_core::paths::own::CREDENTIALS_LOCK,
                                std::time::Duration::from_secs(5),
                            )?;
                            let mut bundle = store.read()?;
                            let changed = if let Some(secret) = change.secret {
                                bundle.insert(change.account, secret.to_string());
                                true
                            } else {
                                bundle.remove(&change.account).is_some()
                            };
                            if changed {
                                store.write(&bundle)
                            } else {
                                Ok(())
                            }
                        })();
                        let _ = change.reply.send(result);
                    }
                })
                .map(|_| to)
                .map_err(|_| {
                    "the credentials writer could not start; restart leo and try again".into()
                })
        })
        .as_ref()
        .map_err(|message| anyhow::anyhow!(message.clone()))
}

pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// The default location, beside `config.toml`.
    pub fn new() -> Result<Self> {
        let path = super::Config::config_path()?.with_file_name(leo_core::paths::own::CREDENTIALS);
        Ok(Self { path })
    }

    /// For tests, and for anyone who wants it somewhere else.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn open(&self) -> std::io::Result<std::fs::File> {
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(std::io::Error::other(
                "credentials must be a regular private file, not a symbolic link",
            ));
        }
        let file = leo_core::paths::open_regular(&self.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::Other {
                std::io::Error::other("credentials must be a regular private file")
            } else {
                error
            }
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if file.metadata()?.nlink() != 1 {
                return Err(std::io::Error::other(
                    "credentials must have no hard links; restore a private copy",
                ));
            }
        }
        Ok(file)
    }

    fn read(&self) -> Result<Bundle> {
        use std::io::Read;
        let file = match self.open() {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Bundle::new()),
            Err(error) => return Err(error).context("credentials could not be read safely; check that credentials.json is a private regular file"),
        };
        anyhow::ensure!(
            file.metadata()?.len() <= MOST_CREDENTIAL_BYTES,
            "the credentials file is too large; restore a valid credentials.json backup"
        );
        let mut text = String::new();
        file.take(MOST_CREDENTIAL_BYTES + 1)
            .read_to_string(&mut text)
            .context("credentials could not be read; check credentials.json and try again")?;
        anyhow::ensure!(
            text.len() as u64 <= MOST_CREDENTIAL_BYTES,
            "the credentials file is too large; restore a valid credentials.json backup"
        );
        serde_json::from_str::<Bundle>(&text).map_err(|_| {
            anyhow::anyhow!(
                "credentials.json is damaged; restore a valid backup before changing credentials"
            )
        })
    }

    fn write(&self, bundle: &Bundle) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            anyhow::ensure!(
                !std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()),
                "the credentials folder is a symbolic link; choose a private folder instead"
            );
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
            Self::restrict_dir(parent)
                .with_context(|| format!("could not make {} private", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(bundle)?;
        anyhow::ensure!(
            json.len() as u64 <= MOST_CREDENTIAL_BYTES,
            "the credentials file is too large; remove an unused credential before saving another"
        );

        use std::io::Write;
        let parent = self
            .path
            .parent()
            .context("the credentials file needs a private folder")?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".leo-credentials-")
            .tempfile_in(parent)
            .with_context(|| format!("could not write privately in {}", parent.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(FILE_MODE))?;
        }
        tmp.write_all(json.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(&self.path)
            .with_context(|| format!("could not replace {}", self.path.display()))?;
        Ok(())
    }

    fn update(&self, account: &str, secret: Option<&str>) -> Result<()> {
        anyhow::ensure!(
            account.len().saturating_add(secret.map_or(0, str::len)) as u64
                <= MOST_CREDENTIAL_BYTES,
            "that credential is too large; paste only the API key and try again"
        );
        let (reply, answer) = mpsc::channel();
        writer()?
            .try_send(Change {
                path: self.path.clone(),
                account: account.to_string(),
                secret: secret.map(|s| Zeroizing::new(s.to_string())),
                reply,
            })
            .map_err(|_| {
                anyhow::anyhow!("the credentials writer is busy or unavailable; try again shortly")
            })?;
        answer.recv_timeout(WRITE_WAIT).map_err(|_| {
            anyhow::anyhow!("the credential change was not confirmed; check the stored credential before retrying")
        })?
    }

    #[cfg(unix)]
    fn restrict_dir(dir: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(DIR_MODE))
    }

    #[cfg(not(unix))]
    fn restrict_dir(_dir: &Path) -> std::io::Result<()> {
        Ok(())
    }

    /// Whether the file is readable by anyone but its owner.
    ///
    /// Checked rather than assumed: the file may predate this code, or have been
    /// copied in by hand.
    #[cfg(unix)]
    pub fn is_private(&self) -> bool {
        use std::os::unix::fs::PermissionsExt;
        match self.open() {
            Ok(file) => file
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o077 == 0),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        }
    }

    #[cfg(not(unix))]
    pub fn is_private(&self) -> bool {
        true
    }

    /// Tighten permissions on a file that is more open than it should be.
    #[cfg(unix)]
    pub fn make_private(&self) -> bool {
        use std::os::unix::fs::PermissionsExt;
        let Ok(file) = self.open() else {
            return false;
        };
        if file
            .metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o077 == 0)
        {
            return false;
        }
        file.set_permissions(std::fs::Permissions::from_mode(FILE_MODE))
            .is_ok()
    }

    #[cfg(not(unix))]
    pub fn make_private(&self) -> bool {
        false
    }
}

impl SecretStore for FileStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        Ok(self
            .read()?
            .get(account)
            .map(|s| Secret::new(Zeroizing::new(s.clone()))))
    }

    fn has(&self, account: &str) -> bool {
        self.read().is_ok_and(|bundle| bundle.contains_key(account))
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.update(account, Some(secret))
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.update(account, None)
    }

    fn available(&self) -> bool {
        // A directory leo can write to is all this needs, and it makes one.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (FileStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::at(dir.path().join("credentials.json"));
        (store, dir)
    }

    #[test]
    fn a_key_round_trips() {
        let (store, _d) = store();
        assert!(store.get("groq").unwrap().is_none());

        store.set("groq", "gsk-secret").unwrap();
        assert_eq!(store.get("groq").unwrap().unwrap().as_str(), "gsk-secret");
        assert!(store.has("groq"));

        store.delete("groq").unwrap();
        assert!(store.get("groq").unwrap().is_none());
        assert!(!store.has("groq"));
    }

    #[test]
    fn several_providers_share_the_file() {
        let (store, _d) = store();
        store.set("groq", "a").unwrap();
        store.set("openrouter", "b").unwrap();
        store.set("hf", "c").unwrap();

        assert_eq!(store.get("groq").unwrap().unwrap().as_str(), "a");
        assert_eq!(store.get("openrouter").unwrap().unwrap().as_str(), "b");
        assert_eq!(store.get("hf").unwrap().unwrap().as_str(), "c");

        // Removing one leaves the others.
        store.delete("openrouter").unwrap();
        assert!(store.has("groq"));
        assert!(!store.has("openrouter"));
        assert!(store.has("hf"));
    }

    /// The one protection this design relies on. If the mode is wrong, the
    /// trade-off that justified a file instead of the keychain does not hold.
    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _d) = store();
        store.set("groq", "gsk-secret").unwrap();

        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode is {:o}", mode & 0o777);
        assert!(store.is_private());

        let dir_mode = std::fs::metadata(store.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            dir_mode & 0o777,
            0o700,
            "dir mode is {:o}",
            dir_mode & 0o777
        );
    }

    /// A file left open by an older version, or copied in by hand, must be
    /// noticed and fixable.
    #[cfg(unix)]
    #[test]
    fn an_over_permissive_file_is_detected_and_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _d) = store();
        store.set("groq", "gsk-secret").unwrap();

        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!store.is_private(), "0644 reported as private");

        assert!(store.make_private());
        assert!(store.is_private());
        // And the value survived the permission change.
        assert_eq!(store.get("groq").unwrap().unwrap().as_str(), "gsk-secret");
    }

    /// Writing must never leave a half-written file behind on the real path.
    #[test]
    fn a_write_leaves_no_temporary_file() {
        let (store, _d) = store();
        store.set("groq", "a").unwrap();
        assert!(!store.path().with_extension("json.new").exists());
    }

    #[cfg(unix)]
    #[test]
    fn stale_temporary_links_are_never_followed_or_reused() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (store, dir) = store();
        let outside = dir.path().join("public-fixture");
        std::fs::write(&outside, "keep this fixture").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644)).unwrap();
        let old = store.path().with_extension("json.new");
        symlink(&outside, &old).unwrap();
        store.set("fixture", "dummy-secret").unwrap();
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "keep this fixture"
        );
        assert!(std::fs::symlink_metadata(&old)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!std::fs::symlink_metadata(store.path())
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_credentials_link_is_not_read_or_written_through() {
        use std::os::unix::fs::symlink;
        let (store, dir) = store();
        let outside = dir.path().join("outside-fixture");
        let original = r#"{"fixture":"outside-secret"}"#;
        std::fs::write(&outside, original).unwrap();
        symlink(&outside, store.path()).unwrap();
        assert!(store.get("fixture").is_err());
        assert!(store.set("fixture", "dummy-secret").is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), original);
        assert!(!store.is_private());
        assert!(!store.make_private());
    }

    #[test]
    fn concurrent_key_updates_keep_every_account() {
        let (store, _dir) = store();
        std::thread::scope(|threads| {
            for index in 0..4 {
                let store = &store;
                threads.spawn(move || {
                    store
                        .set(&format!("fixture-{index}"), "dummy-secret")
                        .unwrap()
                });
            }
        });
        for index in 0..4 {
            assert!(store.has(&format!("fixture-{index}")));
        }
    }

    #[test]
    fn a_corrupt_file_is_reported_rather_than_silently_replaced() {
        let (store, _d) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "not json at all").unwrap();

        assert!(store.get("groq").is_err());
        assert!(!store.has("groq"));
        assert!(store.set("new", "dummy-secret").is_err());
        assert_eq!(
            std::fs::read_to_string(store.path()).unwrap(),
            "not json at all"
        );
    }

    #[test]
    fn a_missing_file_is_simply_empty() {
        let (store, _d) = store();
        assert!(!store.path().exists());
        assert!(store.get("anything").unwrap().is_none());
        assert!(store.available());
    }
}
