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
fn command(dir: &Path) -> Command {
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
    cmd
}
fn install(dir: &Path, shell: Option<&str>, skip: bool) -> Output {
    let mut cmd = command(dir);
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

fn tool(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let tools = dir.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let path = tools.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
fn mv_refusing(dir: &Path, test: &str) {
    tool(
        dir,
        "mv",
        &format!(
            "for last; do :; done\ncase \"$1|$last\" in {test}) exit 1;; esac\nexec /bin/mv \"$@\""
        ),
    );
}
fn said(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout)
}
fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

#[test]
fn a_running_app_stops_the_install_before_anything_is_unpacked() {
    let dir = fixture();
    let app = dir.path().join("Applications with spaces/Leo.app");
    tool(
        dir.path(),
        "ps",
        &format!("echo '{}/Contents/MacOS/leo-app'", app.display()),
    );
    tool(dir.path(), "pgrep", "exit 0");
    let marker = dir.path().join("unpacked");
    tool(
        dir.path(),
        "tar",
        &format!("touch '{}'\nexit 1", marker.display()),
    );
    let out = install(dir.path(), Some("/bin/zsh"), true);
    assert!(!out.status.success());
    assert!(said(&out).contains("Quit Leo"), "{}", said(&out));
    assert!(!marker.exists(), "the archive was opened before the check");
}

#[test]
fn a_saved_terminal_folder_that_is_a_link_leaves_the_installed_app_alone() {
    let dir = fixture();
    assert!(install(dir.path(), None, true).status.success());
    let applications = dir.path().join("Applications with spaces");
    std::fs::write(applications.join("Leo.app/obsolete-file"), "old").unwrap();
    let record = dir.path().join("config/app-cli-dir");
    std::fs::remove_file(&record).unwrap();
    std::os::unix::fs::symlink(dir.path().join("elsewhere"), &record).unwrap();
    let out = install(dir.path(), None, true);
    assert!(!out.status.success());
    assert!(applications.join("Leo.app/obsolete-file").is_file());
    assert_eq!(names(&applications), ["Leo.app"]);
}

#[test]
fn a_failed_record_leaves_no_new_terminal_command_on_a_first_install() {
    let dir = fixture();
    mv_refusing(dir.path(), "*'|'*/app-cli-dir");
    let out = install(dir.path(), None, true);
    assert!(!out.status.success());
    assert!(
        said(&out).contains("Nothing was installed."),
        "{}",
        said(&out)
    );
    assert!(names(&dir.path().join("bin with 'quote' and $dollar")).is_empty());
    assert!(names(&dir.path().join("Applications with spaces")).is_empty());
    assert!(names(&dir.path().join("config")).is_empty());
}

#[test]
fn a_failed_record_restores_the_old_link_and_record_on_a_reinstall() {
    let dir = fixture();
    assert!(install(dir.path(), None, true).status.success());
    let applications = dir.path().join("Applications with spaces");
    std::fs::write(applications.join("Leo.app/obsolete-file"), "old").unwrap();
    let record = dir.path().join("config/app-cli-dir");
    let old_record = std::fs::read_to_string(&record).unwrap();
    let other = dir.path().join("other bin");
    std::fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink("/old/target", other.join("leo")).unwrap();
    mv_refusing(dir.path(), "*'|'*/app-cli-dir");
    let out = command(dir.path())
        .env("LEO_INSTALL_DIR", &other)
        .env("LEO_INSTALL_SKIP_PATH", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(said(&out).contains("left as it was"), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(&record).unwrap(), old_record);
    assert_eq!(
        std::fs::read_link(other.join("leo")).unwrap(),
        Path::new("/old/target")
    );
    assert!(applications.join("Leo.app/obsolete-file").is_file());
    assert_eq!(names(&other), ["leo"]);
    assert_eq!(names(&applications), ["Leo.app"]);
}

#[test]
fn a_failed_swap_restores_the_app_the_link_and_the_record() {
    let dir = fixture();
    assert!(install(dir.path(), None, true).status.success());
    let applications = dir.path().join("Applications with spaces");
    std::fs::write(applications.join("Leo.app/obsolete-file"), "old").unwrap();
    let record = dir.path().join("config/app-cli-dir");
    let old_record = std::fs::read_to_string(&record).unwrap();
    let bin = dir.path().join("bin with 'quote' and $dollar");
    let old_link = std::fs::read_link(bin.join("leo")).unwrap();
    let other = dir.path().join("other bin");
    mv_refusing(dir.path(), "*/.leo-install.*/Leo.app'|'*/Leo.app");
    let out = command(dir.path())
        .env("LEO_INSTALL_DIR", &other)
        .env("LEO_INSTALL_SKIP_PATH", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(said(&out).contains("left as it was"), "{}", said(&out));
    assert!(applications.join("Leo.app/obsolete-file").is_file());
    assert_eq!(std::fs::read_to_string(&record).unwrap(), old_record);
    assert_eq!(std::fs::read_link(bin.join("leo")).unwrap(), old_link);
    assert!(!other.join("leo").exists());
    assert_eq!(names(&applications), ["Leo.app"]);
}

#[test]
fn a_failed_profile_line_is_a_warning_after_a_complete_install() {
    let dir = fixture();
    mv_refusing(dir.path(), "*'|'*/.zshrc");
    let out = install(dir.path(), Some("/bin/zsh"), false);
    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("could not add"), "{}", said(&out));
    assert!(said(&out).contains("bin with 'quote' and $dollar"));
    assert!(dir.path().join("Applications with spaces/Leo.app").is_dir());
    assert!(dir
        .path()
        .join("bin with 'quote' and $dollar/leo")
        .is_symlink());
}

fn uninstall(dir: &Path, cli: &Path) -> Output {
    Command::new(cli)
        .args(["uninstall", "--yes"])
        .env("LEO_HOME", dir.join("config"))
        .env("HOME", dir.join("home"))
        .env_remove("LEO_USE_KEYCHAIN")
        .env("LEO_NO_UPDATE_CHECK", "1")
        .env("LEO_INSTALL_NO_MODEL", "1")
        .env("LEO_NO_MICROPHONE", "1")
        .output()
        .unwrap()
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn kept_notes(dir: &Path) -> PathBuf {
    let notes = dir.join("config/notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("Keep.md"), "My own notes").unwrap();
    notes.join("Keep.md")
}

#[test]
fn uninstall_keeps_the_app_when_a_step_fails_and_a_second_run_finishes() {
    let dir = fixture();
    assert!(install(dir.path(), Some("/bin/zsh"), false)
        .status
        .success());
    let note = kept_notes(dir.path());
    let bin = dir.path().join("bin with 'quote' and $dollar");
    let cli = bin.join("leo");
    let app = dir.path().join("Applications with spaces/Leo.app");
    let rc = dir.path().join("home/.zshrc");
    set_mode(&bin, 0o555);
    let out = uninstall(dir.path(), &cli);
    set_mode(&bin, 0o755);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("terminal command"), "{}", said(&out));
    assert!(app.is_dir(), "the app went while a step failed");
    assert!(cli.is_symlink());
    assert!(!std::fs::read_to_string(&rc)
        .unwrap()
        .contains("Added by the leo installer"));
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "My own notes");

    let out = uninstall(dir.path(), &cli);
    assert!(out.status.success(), "{}", said(&out));
    assert!(!app.exists());
    assert!(!cli.is_symlink());
    assert_eq!(
        names(&dir.path().join("Applications with spaces")),
        Vec::<String>::new()
    );
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "My own notes");
}

#[test]
fn uninstall_keeps_the_command_when_a_later_step_fails() {
    let dir = fixture();
    assert!(install(dir.path(), None, true).status.success());
    let note = kept_notes(dir.path());
    let config = dir.path().join("config");
    std::fs::write(config.join("config.toml"), "").unwrap();
    let record = config.join("app-cli-dir");
    let cli = dir.path().join("bin with 'quote' and $dollar/leo");
    let app = dir.path().join("Applications with spaces/Leo.app");
    set_mode(&config, 0o555);
    let out = uninstall(dir.path(), &cli);
    set_mode(&config, 0o755);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(
        cli.is_symlink(),
        "the terminal command went while a step failed"
    );
    assert!(record.is_file(), "the record of the terminal folder went");
    assert!(app.is_dir());

    let out = uninstall(dir.path(), &cli);
    assert!(out.status.success(), "{}", said(&out));
    assert!(!cli.is_symlink());
    assert!(!app.exists());
    assert_eq!(names(&config), ["notes"]);
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "My own notes");
}
