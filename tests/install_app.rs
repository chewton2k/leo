#![cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let app = dir.path().join("stage/Leo.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    for name in ["leo", "leo-app"] {
        std::fs::copy(
            env!("CARGO_BIN_EXE_leo"),
            app.join("Contents/MacOS").join(name),
        )
        .unwrap();
    }
    std::fs::write(app.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>io.github.chewton2k.leo</string><key>CFBundleExecutable</key><string>leo-app</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#).unwrap();
    assert!(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("tar")
        .arg("-czf")
        .arg(dir.path().join("app.tar.gz"))
        .arg("-C")
        .arg(dir.path().join("stage"))
        .arg("Leo.app")
        .status()
        .unwrap()
        .success());
    dir
}
fn install(dir: &Path, shell: Option<&str>, skip: bool) -> Output {
    let mut cmd = Command::new("sh");
    cmd.arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install-app.sh"))
        .env_clear()
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                dir.join("tools").display()
            ),
        )
        .env("HOME", dir.join("home"))
        .env("LEO_HOME", dir.join("config"))
        .env("LEO_APPLICATIONS_DIR", dir.join("Applications with spaces"))
        .env("LEO_INSTALL_DIR", dir.join("bin with 'quote' and $dollar"))
        .env("LEO_INSTALL_ARCHIVE", dir.join("app.tar.gz"));
    if let Some(shell) = shell {
        cmd.env("SHELL", shell);
    }
    if skip {
        cmd.env("LEO_INSTALL_SKIP_PATH", "1");
    }
    cmd.output().unwrap()
}
#[test]
fn installs_a_bundle_and_live_link_with_quoted_path_exactly_once() {
    let dir = fixture();
    let out = install(dir.path(), Some("/bin/zsh"), false);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bin = dir.path().join("bin with 'quote' and $dollar");
    let link = bin.join("leo");
    assert!(link.is_symlink());
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        dir.path()
            .join("Applications with spaces/Leo.app/Contents/MacOS/leo")
    );
    assert!(Command::new(&link)
        .arg("--version")
        .status()
        .unwrap()
        .success());
    let rc = dir.path().join("home/.zshrc");
    let text = std::fs::read_to_string(&rc).unwrap();
    assert!(install(dir.path(), Some("/bin/zsh"), false)
        .status
        .success());
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), text);
    let evaluated = Command::new("sh")
        .arg("-c")
        .arg(". \"$1\"; command -v leo")
        .arg("sh")
        .arg(&rc)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(evaluated.status.success());
    assert_eq!(
        String::from_utf8_lossy(&evaluated.stdout).trim(),
        link.to_str().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("config/app-cli-dir"))
            .unwrap()
            .trim(),
        bin.to_str().unwrap()
    );
}
#[test]
fn updating_replaces_the_bundle_and_cleans_only_the_previous_installation() {
    let dir = fixture();
    assert!(install(dir.path(), None, true).status.success());
    let applications = dir.path().join("Applications with spaces");
    let app = applications.join("Leo.app");
    std::fs::write(app.join("obsolete-file"), "old version").unwrap();
    let unrelated = applications.join("Other.app");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.join("keep"), "unrelated").unwrap();
    assert!(install(dir.path(), None, true).status.success());
    assert!(!app.join("obsolete-file").exists());
    assert!(app.join("Contents/MacOS/leo-app").is_file());
    assert_eq!(
        std::fs::read_to_string(unrelated.join("keep")).unwrap(),
        "unrelated"
    );
    assert_eq!(std::fs::read_dir(&applications).unwrap().count(), 2);
}

#[test]
fn missing_shell_and_skip_path_do_not_break_installation() {
    let dir = fixture();
    let out = install(dir.path(), None, false);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let other = fixture();
    let out = install(other.path(), Some("/bin/zsh"), true);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!other.path().join("home/.zshrc").exists());
}
#[test]
fn unsafe_archives_and_unrelated_applications_are_preserved() {
    let dir = fixture();
    let app = dir.path().join("Applications with spaces/Leo.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), "another app").unwrap();
    assert!(!install(dir.path(), Some("/bin/zsh"), false)
        .status
        .success());
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
        "another app"
    );
    std::fs::remove_dir_all(app).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("stage/Leo.app/link")).unwrap();
    assert!(Command::new("tar")
        .arg("-czf")
        .arg(dir.path().join("app.tar.gz"))
        .arg("-C")
        .arg(dir.path().join("stage"))
        .arg("Leo.app")
        .status()
        .unwrap()
        .success());
    assert!(!install(dir.path(), Some("/bin/zsh"), false)
        .status
        .success());
    assert!(!dir.path().join("Applications with spaces/Leo.app").exists());
}

#[test]
fn uninstall_removes_the_app_and_cli_link_but_preserves_notes() {
    let dir = fixture();
    assert!(install(dir.path(), Some("/bin/zsh"), false)
        .status
        .success());
    let notes = dir.path().join("config/notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("Keep.md"), "My own notes").unwrap();
    let cli = dir.path().join("bin with 'quote' and $dollar/leo");
    let out = Command::new(&cli)
        .args(["uninstall", "--yes"])
        .env("LEO_HOME", dir.path().join("config"))
        .env("HOME", dir.path().join("home"))
        .env("LEO_NO_UPDATE_CHECK", "1")
        .env("LEO_INSTALL_NO_MODEL", "1")
        .env("LEO_NO_MICROPHONE", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.path().join("Applications with spaces/Leo.app").exists());
    assert!(!cli.is_symlink());
    assert_eq!(
        std::fs::read_to_string(notes.join("Keep.md")).unwrap(),
        "My own notes"
    );
    assert!(!std::fs::read_to_string(dir.path().join("home/.zshrc"))
        .unwrap()
        .contains("Added by the leo installer"));
}

#[test]
fn an_archive_listing_is_stopped_when_it_exceeds_the_entry_limit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture();
    let tools = dir.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let marker = dir.path().join("listing-consumed-entirely");
    let tar = tools.join("tar");
    std::fs::write(&tar, format!("#!/bin/sh\ntrap 'exit 0' PIPE\nn=0\nwhile [ \"$n\" -lt 20200 ]; do printf 'Leo.app/Contents/a-long-enough-path-to-exceed-the-pipe-buffer-when-listing-spills\\n' || exit 0; n=$((n+1)); done\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&tar, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = install(dir.path(), Some("/bin/zsh"), true);
    assert!(!out.status.success());
    assert!(
        !marker.exists(),
        "the entire oversized listing was consumed before checking the bound"
    );
}
