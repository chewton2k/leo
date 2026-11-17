use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MARK: &str = "__LEO_PATH__";
const SAVED: &str = "login-path";
const MOST_SAVED: u64 = 16 * 1024;

pub fn cached(config: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(config.join(SAVED))
        .ok()?
        .take(MOST_SAVED + 1)
        .read_to_string(&mut text)
        .ok()?;
    let line = text.trim_end_matches(['\n', '\r']);
    (text.len() as u64 <= MOST_SAVED && !line.is_empty() && !line.contains('\n'))
        .then(|| line.to_string())
}

pub fn remember(config: &Path, path: &str) -> std::io::Result<bool> {
    if path.is_empty() || path.len() as u64 > MOST_SAVED || path.contains('\n') {
        return Ok(false);
    }
    if cached(config).as_deref() == Some(path) {
        return Ok(false);
    }
    std::fs::create_dir_all(config)?;
    leo_core::paths::write_atomic(&config.join(SAVED), path).map_err(std::io::Error::other)?;
    Ok(true)
}

pub fn login_path(shell: &Path, timeout: Duration) -> Option<String> {
    let mut command = leo_core::children::quiet_command(shell);
    command
        .args([
            "-l",
            "-i",
            "-c",
            &format!("printf '{MARK}%s{MARK}' \"$PATH\""),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    text.extend_from_slice(&chunk[..read]);
                    if String::from_utf8_lossy(&text).matches(MARK).count() >= 2 {
                        break;
                    }
                }
            }
        }
        let _ = sender.send(String::from_utf8_lossy(&text).into_owned());
    });
    let text = match received.recv_timeout(timeout) {
        Ok(text) => text,
        Err(_) => {
            leo_core::children::stop_tree(&mut child);
            let _ = child.wait();
            return None;
        }
    };
    leo_core::children::stop_tree(&mut child);
    let _ = child.wait();
    let (_, rest) = text.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    Some(path.to_string()).filter(|p| !p.is_empty())
}

pub fn merged(current: &str, login: Option<&str>, extra: &[PathBuf]) -> String {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut seen = Vec::<String>::new();
    let extras = extra.iter().map(|p| p.to_string_lossy().into_owned());
    let entries = login
        .unwrap_or("")
        .split(separator)
        .chain(current.split(separator))
        .map(str::to_string)
        .chain(extras);
    for entry in entries {
        if !entry.is_empty() && !seen.contains(&entry) {
            seen.push(entry);
        }
    }
    seen.join(&separator.to_string())
}

pub fn usual_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home {
        folders.push(home.join(".local/bin"));
        folders.push(home.join(".cargo/bin"));
    }
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::Instant;

    #[cfg(unix)]
    fn fake_shell(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_noisy_login_shell_still_gives_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "echo 'Welcome to oh-my-zsh'; printf '__LEO_PATH__/opt/homebrew/bin:/usr/bin__LEO_PATH__'; echo bye",
        );
        assert_eq!(
            login_path(&shell, Duration::from_secs(3)).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_login_shell_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "sleep 30");
        let began = Instant::now();
        assert_eq!(login_path(&shell, Duration::from_millis(300)), None);
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn a_background_process_left_by_the_shell_does_not_hide_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "printf '__LEO_PATH__/opt/homebrew/bin__LEO_PATH__'; sleep 30 &",
        );
        let began = Instant::now();
        assert_eq!(
            login_path(&shell, Duration::from_secs(3)).as_deref(),
            Some("/opt/homebrew/bin")
        );
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_saved_login_path_is_read_back_and_rewritten_only_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cached(dir.path()), None);
        assert!(remember(dir.path(), "/opt/homebrew/bin:/usr/bin").unwrap());
        assert_eq!(
            cached(dir.path()).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert!(!remember(dir.path(), "/opt/homebrew/bin:/usr/bin").unwrap());
        assert!(remember(dir.path(), "/usr/bin").unwrap());
        assert_eq!(cached(dir.path()).as_deref(), Some("/usr/bin"));
        std::fs::write(dir.path().join("login-path"), "a".repeat(17 * 1024)).unwrap();
        assert_eq!(cached(dir.path()), None);
        std::fs::write(dir.path().join("login-path"), "/usr/bin\n/bin").unwrap();
        assert_eq!(cached(dir.path()), None);
        assert!(remember(dir.path(), "/bin").unwrap());
        assert_eq!(cached(dir.path()).as_deref(), Some("/bin"));
    }

    #[test]
    fn merged_keeps_login_first_without_duplicates() {
        let path = |items: &[&str]| {
            std::env::join_paths(items)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        let got = merged(
            &path(&["system", "base"]),
            Some(&path(&["tools", "system"])),
            &[PathBuf::from("tools"), PathBuf::from("extra")],
        );
        assert_eq!(got, path(&["tools", "system", "base", "extra"]));
        assert_eq!(
            merged(&path(&["system", "base"]), None, &[]),
            path(&["system", "base"])
        );
    }
}
