use anyhow::{bail, Result};

pub const BUNDLE_ID: &str = "io.github.chewton2k.leo";

const NOT_INSTALLED: &str = "leo isn't installed as an app yet. Install it from https://github.com/chewton2k/leo-cli/releases/latest";

pub fn open_command() -> Option<std::process::Command> {
    #[cfg(target_os = "macos")]
    {
        let mut command = leo_core::children::quiet_command("open");
        command.args(["-b", BUNDLE_ID]);
        Some(command)
    }
    #[cfg(windows)]
    {
        let app = super::installation::app_dir().ok()?.join("leo-app.exe");
        app.is_file()
            .then(|| leo_core::children::quiet_command(app))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

pub fn open() -> Result<()> {
    let Some(mut command) = open_command() else {
        bail!(NOT_INSTALLED);
    };
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    if cfg!(target_os = "macos") && !child.wait()?.success() {
        bail!(NOT_INSTALLED);
    }
    Ok(())
}
