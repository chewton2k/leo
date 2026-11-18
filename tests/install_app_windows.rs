#![cfg(windows)]
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["bin", "config"] {
            std::fs::create_dir(dir.path().join(folder)).unwrap();
        }
        let this = Scratch { dir };
        std::fs::write(this.path("bin/leo.cmd"), "old command").unwrap();
        std::fs::write(this.path("config/app-cli-dir"), "old record").unwrap();
        std::fs::write(this.path("bin/new-command"), "new command").unwrap();
        std::fs::write(this.path("config/new-record"), "new record").unwrap();
        std::fs::write(this.path("old-command"), "old command").unwrap();
        std::fs::write(this.path("old-record"), "old record").unwrap();
        this
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }
    fn run(&self, body: &str) -> Output {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install-app.ps1");
        let harness = self.path("harness.ps1");
        let text = format!(
            "$ErrorActionPreference = 'Stop'\n$env:LEO_INSTALL_FUNCTIONS_ONLY = '1'\n. '{}'\n$linkTemp = '{}'\n$link = '{}'\n$linkOld = '{}'\n$recordTemp = '{}'\n$record = '{}'\n$recordOld = '{}'\n{body}\n",
            script.display(),
            self.path("bin/new-command").display(),
            self.path("bin/leo.cmd").display(),
            self.path("old-command").display(),
            self.path("config/new-record").display(),
            self.path("config/app-cli-dir").display(),
            self.path("old-record").display(),
        );
        std::fs::write(&harness, text).unwrap();
        run_script(&harness)
    }
}

fn run_script(harness: &Path) -> Output {
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(harness)
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout)
}

const THROWS: &str = "$threw = $false\ntry { Publish-Owned { param($from, $to) if ($to.EndsWith('MATCH')) { throw 'refused' }; Move-Item -LiteralPath $from -Destination $to -Force } } catch { $threw = $true }\nif (-not $threw) { exit 3 }";

#[test]
fn a_failed_record_puts_back_the_old_command_and_record() {
    let scratch = Scratch::new();
    let out = scratch.run(&THROWS.replace("MATCH", "app-cli-dir"));
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(scratch.read("bin/leo.cmd"), "old command");
    assert_eq!(scratch.read("config/app-cli-dir"), "old record");
}

#[test]
fn a_failed_command_publication_changes_nothing() {
    let scratch = Scratch::new();
    let out = scratch.run(&THROWS.replace("MATCH", "leo.cmd"));
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(scratch.read("bin/leo.cmd"), "old command");
    assert_eq!(scratch.read("config/app-cli-dir"), "old record");
}

#[test]
fn a_failed_path_step_is_a_warning() {
    let scratch = Scratch::new();
    let out = scratch.run(
        "$said = Add-UserPath 'C:\\leo-test-folder-not-on-path' { param($value) throw 'denied' } 3>&1 | Out-String\nif ($said -notmatch 'could not add') { exit 3 }",
    );
    assert!(out.status.success(), "{}", said(&out));
}
