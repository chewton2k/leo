use anyhow::Result;
use zeroize::Zeroizing;

/// Service name under which all leo credentials are filed in the OS keychain.
pub const SERVICE: &str = "leo";

/// A place secrets can be persisted. Behind a trait so tests never touch the
/// real OS keychain.
pub trait SecretStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
    /// Whether a backend is usable at all (e.g. Secret Service running).
    fn available(&self) -> bool;
}

/// Render a secret for display. Only the last four characters survive, so this
/// is safe to print, log, or show in the UI.
pub fn redact(secret: &str) -> String {
    let tail: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

/// Resolution order: env var first, then the store. Env-first lets an operator
/// override a stored credential for one invocation without mutating the
/// keychain. A store error degrades to `None` — a missing key is a skipped
/// provider, never a crash.
pub fn resolve(
    provider: &str,
    key_env: Option<&str>,
    store: &dyn SecretStore,
) -> Option<Zeroizing<String>> {
    if let Some(var) = key_env {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                return Some(Zeroizing::new(value));
            }
        }
    }
    store.get(provider).ok().flatten()
}

/// The real OS keychain: macOS Keychain, Windows Credential Manager, or Linux
/// Secret Service, selected by the `keyring` crate's default feature.
///
/// The underlying `keyring` crate lazily initializes the platform-specific
/// credential store the first time an `Entry` is created (see `v1::Entry::new`
/// in the `keyring` crate source); no explicit setup call is needed here.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).map_err(Into::into)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>> {
        match Self::entry(account)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        Self::entry(account)?.set_password(secret)?;
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn available(&self) -> bool {
        // A probe read against a name we never write. NoEntry means the
        // backend answered, which is what we are testing for.
        matches!(
            Self::entry("__leo_probe__").and_then(|e| {
                match e.get_password() {
                    Ok(_) => Ok(()),
                    Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(e.into()),
                }
            }),
            Ok(())
        )
    }
}

/// In-memory store for tests. Never touches the OS keychain.
#[derive(Default)]
pub struct MemoryStore {
    items: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .get(account)
            .map(|s| Zeroizing::new(s.clone())))
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.items
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.items.lock().unwrap().remove(account);
        Ok(())
    }

    fn available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn redact_shows_only_last_four() {
        assert_eq!(redact("sk-or-v1-abcdefgh1234"), "…1234");
        assert_eq!(redact("abc"), "…abc");
        assert_eq!(redact(""), "…");
    }

    #[test]
    fn redact_never_contains_the_original_secret() {
        let secret = "sk-or-v1-supersecretvalue";
        let shown = redact(secret);
        assert!(!shown.contains("supersecret"));
        assert!(!shown.contains(secret));
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::default();
        assert!(store.get("openrouter").unwrap().is_none());
        store.set("openrouter", "key-1").unwrap();
        assert_eq!(&**store.get("openrouter").unwrap().unwrap(), "key-1");
        store.delete("openrouter").unwrap();
        assert!(store.get("openrouter").unwrap().is_none());
    }

    #[test]
    fn env_var_wins_over_stored_secret() {
        let _guard = ENV_LOCK.lock().unwrap();
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain").unwrap();
        std::env::set_var("LEO_TEST_KEY_A", "from-env");

        let got = resolve("openrouter", Some("LEO_TEST_KEY_A"), &store).unwrap();
        std::env::remove_var("LEO_TEST_KEY_A");

        assert_eq!(&*got, "from-env");
    }

    #[test]
    fn falls_back_to_store_when_env_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_KEY_B");
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain").unwrap();

        let got = resolve("openrouter", Some("LEO_TEST_KEY_B"), &store).unwrap();
        assert_eq!(&*got, "from-keychain");
    }

    #[test]
    fn empty_env_var_is_treated_as_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_KEY_C", "   ");
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain").unwrap();

        let got = resolve("openrouter", Some("LEO_TEST_KEY_C"), &store).unwrap();
        std::env::remove_var("LEO_TEST_KEY_C");

        assert_eq!(&*got, "from-keychain");
    }

    #[test]
    fn resolve_returns_none_when_nothing_configured() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_KEY_D");
        let store = MemoryStore::default();
        assert!(resolve("openrouter", Some("LEO_TEST_KEY_D"), &store).is_none());
        assert!(resolve("openrouter", None, &store).is_none());
    }

    #[test]
    fn unavailable_store_degrades_instead_of_failing() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_KEY_E", "from-env");
        let store = BrokenStore;
        // Env still resolves even though the backend is dead.
        let got = resolve("openrouter", Some("LEO_TEST_KEY_E"), &store).unwrap();
        std::env::remove_var("LEO_TEST_KEY_E");
        assert_eq!(&*got, "from-env");

        // And a store error is swallowed into None, not propagated as a panic.
        assert!(resolve("openrouter", None, &store).is_none());
    }

    /// A store whose backend is missing, like headless Linux with no
    /// Secret Service.
    struct BrokenStore;

    impl SecretStore for BrokenStore {
        fn get(&self, _account: &str) -> Result<Option<Zeroizing<String>>> {
            anyhow::bail!("no keychain backend available")
        }
        fn set(&self, _account: &str, _secret: &str) -> Result<()> {
            anyhow::bail!("no keychain backend available")
        }
        fn delete(&self, _account: &str) -> Result<()> {
            anyhow::bail!("no keychain backend available")
        }
        fn available(&self) -> bool {
            false
        }
    }
}
