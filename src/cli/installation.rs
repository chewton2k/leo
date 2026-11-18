use anyhow::{ensure, Context, Result};
use std::path::{Path, PathBuf};

pub fn bundle_for(exe: &Path) -> Option<PathBuf> {
    let parent = exe.parent()?;
    if parent.file_name()? == "MacOS" && parent.parent()?.file_name()? == "Contents" {
        let bundle = parent.parent()?.parent()?;
        if bundle.file_name()? == "Leo.app" {
            return Some(bundle.to_path_buf());
        }
    }
    if parent
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("Leo")
        && parent.join("leo-app.exe").is_file()
    {
        return Some(parent.to_path_buf());
    }
    None
}

pub fn cli_dir(exe: &Path, record: &Path, fallback: &Path) -> Result<PathBuf> {
    let resolved = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let Some(bundle) = bundle_for(&resolved) else {
        return Ok(exe
            .parent()
            .context("Leo needs an installation folder.")?
            .to_path_buf());
    };
    let saved = (|| -> Option<PathBuf> {
        let meta = std::fs::symlink_metadata(record).ok()?;
        if !meta.is_file() || meta.len() > 4096 {
            return None;
        }
        let path = PathBuf::from(
            std::fs::read_to_string(record)
                .ok()?
                .trim_end_matches(['\r', '\n']),
        );
        (path.is_absolute()
            && !path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
            && !path.starts_with(&bundle)
            && !path
                .canonicalize()
                .is_ok_and(|resolved| resolved.starts_with(&bundle)))
        .then_some(path)
    })();
    Ok(saved.unwrap_or_else(|| fallback.to_path_buf()))
}

pub fn validate_bundle(bundle: &Path, identifier: &str) -> Result<()> {
    let meta = std::fs::symlink_metadata(bundle)?;
    ensure!(
        meta.is_dir() && !meta.is_symlink(),
        "The Leo app folder is a link. Remove the link yourself; your notes were kept."
    );
    ensure!(
        identifier == super::open_app::BUNDLE_ID,
        "This folder is not the Leo app. It was left untouched."
    );
    Ok(())
}

pub fn app_dir() -> Result<PathBuf> {
    app_dir_in(std::env::var_os("LOCALAPPDATA"))
}

fn app_dir_in(local: Option<std::ffi::OsString>) -> Result<PathBuf> {
    Ok(PathBuf::from(local.context("The Windows app folder could not be found.")?).join("Leo"))
}

pub fn installed_bundle(exe: &Path) -> Result<Option<PathBuf>> {
    let resolved = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    if let Some(bundle) = bundle_for(&resolved) {
        return Ok(Some(bundle));
    }
    if std::env::var_os("LEO_HOME").is_some() {
        return Ok(None);
    }
    #[cfg(target_os = "macos")]
    let path = PathBuf::from("/Applications/Leo.app");
    #[cfg(windows)]
    let path = app_dir()?;
    #[cfg(not(any(target_os = "macos", windows)))]
    return Ok(None);
    #[cfg(any(target_os = "macos", windows))]
    Ok(path.exists().then_some(path))
}

#[cfg(target_os = "macos")]
fn capture(program: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output> {
    let mut command = leo_core::children::quiet_command(program);
    command.args(args);
    capture_command(command)
}

#[cfg(any(target_os = "macos", windows))]
fn capture_command(mut command: std::process::Command) -> Result<std::process::Output> {
    use std::{
        io::Read,
        process::Stdio,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("A system check took too long. Reopen Leo and try again.");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        pipe.take(16_384).read_to_end(&mut stdout)?;
    }
    if let Some(pipe) = child.stderr.take() {
        pipe.take(16_384).read_to_end(&mut stderr)?;
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

pub fn verify_installed_bundle(bundle: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        let plist = bundle.join("Contents/Info.plist");
        let output = capture(
            "/usr/libexec/PlistBuddy",
            &[
                OsStr::new("-c"),
                OsStr::new("Print :CFBundleIdentifier"),
                plist.as_os_str(),
            ],
        )?;
        ensure!(
            output.status.success(),
            "The Leo app could not be identified. It was left untouched."
        );
        validate_bundle(bundle, String::from_utf8_lossy(&output.stdout).trim())?;
        let pattern = bundle.join("Contents/MacOS/leo-app");
        let running = capture("/usr/bin/pgrep", &[OsStr::new("-f"), pattern.as_os_str()])?;
        ensure!(
            running.status.code() == Some(1),
            "Quit Leo before uninstalling. Finish any recording or dictation first."
        );
    }
    #[cfg(windows)]
    {
        let expected = app_dir()?;
        ensure!(bundle == expected, "This app is outside the Leo installation folder. Use its Windows uninstaller; your notes were kept.");
        validate_bundle(bundle, super::open_app::BUNDLE_ID)?;
        ensure!(
            bundle.join("uninstall.exe").is_file() && bundle.join("leo-app.exe").is_file(),
            "The app installation is incomplete. Reinstall Leo before uninstalling."
        );
        let mut command = leo_core::children::quiet_command("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; $exe=Join-Path $env:LEO_UNINSTALL_APP 'leo-app.exe'; if(Get-Process -Name 'leo-app' -ErrorAction SilentlyContinue | Where-Object {$_.Path -and $_.Path -eq $exe}){exit 2}"])
            .env("LEO_UNINSTALL_APP", bundle);
        ensure!(
            capture_command(command)?.status.success(),
            "Quit Leo before uninstalling. Finish any recording or dictation first."
        );
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    validate_bundle(bundle, super::open_app::BUNDLE_ID)?;
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn remove_login_file(
    file: &Path,
    bundle: &Path,
    read_program: impl FnOnce() -> Result<PathBuf>,
    unload: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    ensure!(
        std::fs::symlink_metadata(file)?.is_file(),
        "The login item is a link. Remove Leo from Login Items yourself before uninstalling."
    );
    let program = read_program()?;
    ensure!(
        program == bundle || program == bundle.join("Contents/MacOS/leo-app"),
        "This login item belongs to another app. It was left untouched."
    );
    unload(file)?;
    std::fs::remove_file(file)?;
    Ok(())
}

pub fn remove_login_item(home: &Path, bundle: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        let item = home.join("Library/LaunchAgents/Leo.plist");
        if !item.exists() {
            return Ok(());
        }
        remove_login_file(
            &item,
            bundle,
            || {
                let output = capture(
                    "/usr/libexec/PlistBuddy",
                    &[
                        OsStr::new("-c"),
                        OsStr::new("Print :ProgramArguments:0"),
                        item.as_os_str(),
                    ],
                )?;
                ensure!(
                    output.status.success(),
                    "The login item could not be identified. Remove Leo from Login Items first."
                );
                Ok(PathBuf::from(
                    String::from_utf8_lossy(&output.stdout).trim(),
                ))
            },
            |file| {
                let uid = capture("/usr/bin/id", &[OsStr::new("-u")])?;
                let uid: u32 = String::from_utf8_lossy(&uid.stdout).trim().parse()?;
                let _ = capture(
                    "/bin/launchctl",
                    &[
                        OsStr::new("bootout"),
                        OsStr::new(&format!("gui/{uid}")),
                        file.as_os_str(),
                    ],
                );
                Ok(())
            },
        )?;
    }
    #[cfg(windows)]
    {
        let mut command = leo_core::children::quiet_command("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; $key='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run'; $value=(Get-ItemProperty -LiteralPath $key -Name 'Leo' -ErrorAction SilentlyContinue).Leo; if($value){$exe=Join-Path $env:LEO_UNINSTALL_APP 'leo-app.exe'; $quoted='\"'+$exe+'\"'; if($value -ne $quoted -and $value -ne ($quoted+' --hidden') -and $value -ne ($exe+' --hidden')){throw 'The Leo login entry belongs to another app. Remove it from Startup apps first.'}; Remove-ItemProperty -LiteralPath $key -Name 'Leo'}"])
            .env("LEO_UNINSTALL_APP", bundle);
        ensure!(capture_command(command)?.status.success(), "The Leo startup entry could not be removed. Remove Leo from Startup apps before uninstalling.");
        let _ = home;
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (home, bundle);
    Ok(())
}

pub fn remove_bundle(bundle: &Path) -> Result<()> {
    #[cfg(not(windows))]
    std::fs::remove_dir_all(bundle)
        .context("The app could not be removed. Check its folder permissions and try again.")?;
    #[cfg(windows)]
    {
        let script = "$ErrorActionPreference='Stop'; Wait-Process -Id ([int]$env:LEO_UNINSTALL_PARENT_PID) -Timeout 30 -ErrorAction SilentlyContinue; if(Get-Process -Id ([int]$env:LEO_UNINSTALL_PARENT_PID) -ErrorAction SilentlyContinue){exit 1}; $uninstaller=Join-Path $env:LEO_UNINSTALL_APP 'uninstall.exe'; $p=Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru; if(-not $p.WaitForExit(120000) -or $p.ExitCode -ne 0){exit 1}; $bin=Join-Path $env:LEO_UNINSTALL_APP 'bin'; if((Test-Path -LiteralPath $bin -PathType Container) -and -not (Get-ChildItem -LiteralPath $bin -Force)){Remove-Item -LiteralPath $bin}; if((Test-Path -LiteralPath $env:LEO_UNINSTALL_APP -PathType Container) -and -not (Get-ChildItem -LiteralPath $env:LEO_UNINSTALL_APP -Force)){Remove-Item -LiteralPath $env:LEO_UNINSTALL_APP}";
        leo_core::children::quiet_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("LEO_UNINSTALL_PARENT_PID", std::process::id().to_string())
            .env("LEO_UNINSTALL_APP", bundle)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("The Windows uninstaller could not start. Remove Leo from Installed apps.")?;
    }
    Ok(())
}

pub fn is_cli_link(link: &Path, bundle: &Path) -> bool {
    if let Ok(target) = std::fs::read_link(link) {
        return target == bundle.join("Contents/MacOS/leo");
    }
    if cfg!(windows) && link.extension().is_some_and(|e| e == "cmd") {
        let Ok(meta) = std::fs::symlink_metadata(link) else {
            return false;
        };
        if !meta.is_file() || meta.len() > 8192 {
            return false;
        }
        let expected = format!(
            "@\"{}\" %*",
            bundle
                .join("leo.exe")
                .display()
                .to_string()
                .replace('%', "%%")
        );
        return std::fs::read_to_string(link)
            .ok()
            .is_some_and(|text| text.lines().any(|line| line == expected));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_unset_local_app_data_is_an_error_not_a_relative_folder() {
        assert!(app_dir_in(None).is_err());
        assert_eq!(
            app_dir_in(Some("data".into())).unwrap(),
            PathBuf::from("data").join("Leo")
        );
    }
    #[test]
    fn terminal_link_removal_keeps_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Leo.app");
        let link = dir.path().join("leo");
        std::fs::write(&link, "some other command").unwrap();
        assert!(!super::is_cli_link(&link, &app));
        #[cfg(unix)]
        {
            std::fs::remove_file(&link).unwrap();
            std::os::unix::fs::symlink(app.join("Contents/MacOS/leo"), &link).unwrap();
            assert!(super::is_cli_link(&link, &app));
        }
    }
    #[test]
    fn removing_a_login_item_requires_matching_program_and_keeps_other_items() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Leo.plist");
        let bundle = dir.path().join("Leo.app");
        std::fs::write(&file, "private login item").unwrap();
        assert!(super::remove_login_file(
            &file,
            &bundle,
            || Ok(dir.path().join("Other.app")),
            |_| panic!("must not unload another app")
        )
        .is_err());
        assert!(file.exists());
        let called = std::cell::Cell::new(false);
        super::remove_login_file(
            &file,
            &bundle,
            || Ok(bundle.join("Contents/MacOS/leo-app")),
            |_| {
                called.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(called.get());
        assert!(!file.exists());
    }
    #[test]
    fn uninstall_uses_the_saved_terminal_folder_and_never_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Leo.app/Contents/MacOS/leo");
        let record = dir.path().join("app-cli-dir");
        let fallback = dir.path().join("default-bin");
        let custom = dir.path().join("custom bin ");
        std::fs::write(&record, custom.to_str().unwrap()).unwrap();
        assert_eq!(cli_dir(&exe, &record, &fallback).unwrap(), custom);
        std::fs::write(&record, exe.parent().unwrap().to_str().unwrap()).unwrap();
        assert_eq!(cli_dir(&exe, &record, &fallback).unwrap(), fallback);
        std::fs::write(
            &record,
            dir.path()
                .join("Else/../Leo.app/Contents/MacOS")
                .to_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(cli_dir(&exe, &record, &fallback).unwrap(), fallback);
        std::fs::write(&record, "relative").unwrap();
        assert_eq!(cli_dir(&exe, &record, &fallback).unwrap(), fallback);
        assert_eq!(
            cli_dir(&dir.path().join("old-bin/leo"), &record, &fallback).unwrap(),
            dir.path().join("old-bin")
        );
    }
    #[test]
    fn app_removal_rejects_other_apps_links_and_ambiguous_executables() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Leo.app");
        std::fs::create_dir(&app).unwrap();
        assert!(validate_bundle(&app, "another.app").is_err());
        assert!(validate_bundle(&app, super::super::open_app::BUNDLE_ID).is_ok());
        assert!(bundle_for(&dir.path().join("Other.app/Contents/MacOS/leo")).is_none());
        assert!(bundle_for(&dir.path().join("random/leo")).is_none());
        #[cfg(unix)]
        {
            let link = dir.path().join("link.app");
            std::os::unix::fs::symlink(&app, &link).unwrap();
            assert!(validate_bundle(&link, super::super::open_app::BUNDLE_ID).is_err());
        }
    }
}
