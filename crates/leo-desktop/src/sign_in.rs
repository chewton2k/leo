use anyhow::{Context, Result};
use leo_services::ai::provider::agent_cli::{self, Agent, AgentCli};
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

const CODEX_WAIT: Duration = Duration::from_secs(600);
static CODEX_LOGIN: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

pub fn start(provider: &str) -> Result<()> {
    let cfg = leo_services::config::Config::load();
    let pc = matches!(provider, "claude_code" | "codex")
        .then(|| cfg.provider(provider))
        .flatten()
        .context("Choose Claude Code or Codex to sign in.")?;
    let agent = Agent::of(pc).context("Choose Claude Code or Codex to sign in.")?;
    let cli = AgentCli::new(provider.into(), agent, pc);
    let bin = agent_cli::locate(cli.bin()).with_context(|| {
        format!(
            "{} is not installed. Install it, then press Sign in again.",
            agent.kind()
        )
    })?;
    match agent {
        Agent::Codex => codex_login(&bin),
        Agent::ClaudeCode => claude_terminal(&bin),
    }
}

pub fn stop() {
    let mut slot = CODEX_LOGIN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut child) = slot.take() {
        leo_core::children::stop_tree(&mut child);
        let _ = child.wait();
    }
}

fn codex_arguments() -> [&'static str; 1] {
    ["login"]
}

fn codex_login(bin: &Path) -> Result<()> {
    let mut slot = CODEX_LOGIN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut old) = slot.take() {
        leo_core::children::stop_tree(&mut old);
        let _ = old.wait();
    }
    let mut command = leo_core::children::quiet_command(bin);
    command
        .args(codex_arguments())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command
        .spawn()
        .context("Codex could not start its sign-in. Try again in a moment.")?;
    let id = child.id();
    *slot = Some(child);
    drop(slot);
    std::thread::spawn(move || {
        let until = Instant::now() + CODEX_WAIT;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let mut slot = CODEX_LOGIN.lock().unwrap_or_else(|e| e.into_inner());
            let Some(child) = slot.as_mut().filter(|child| child.id() == id) else {
                return;
            };
            let ended = !matches!(child.try_wait(), Ok(None));
            if ended || Instant::now() >= until {
                let mut child = slot.take().unwrap();
                if !ended {
                    leo_core::children::stop_tree(&mut child);
                }
                let _ = child.wait();
                return;
            }
        }
    });
    Ok(())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_script(bin: &Path) -> String {
    let quoted = bin.to_string_lossy().replace('\'', r"'\''");
    format!("#!/bin/zsh -l\nrm -f -- \"$0\"\nexec '{quoted}' auth login\n")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn cmd_misreads(bin: &Path) -> bool {
    bin.to_string_lossy()
        .contains(['"', '%', '!', '^', '&', '|', '<', '>', '(', ')'])
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_folder(bin: &Path) -> std::path::PathBuf {
    bin.parent()
        .filter(|_| cmd_misreads(bin))
        .map_or_else(std::env::temp_dir, Path::to_path_buf)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_line(bin: &Path) -> String {
    let program = if cmd_misreads(bin) {
        "claude".into()
    } else {
        format!("\"{}\"", bin.display())
    };
    format!("/c \"start \"\" cmd /k {program} auth login\"")
}

#[cfg(target_os = "macos")]
fn claude_terminal(bin: &Path) -> Result<()> {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let mut file = tempfile::Builder::new()
        .prefix("leo-sign-in-")
        .suffix(".command")
        .tempfile()?;
    file.write_all(mac_script(bin).as_bytes())?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))?;
    let script: std::path::PathBuf = file.into_temp_path().keep()?;
    let opened = leo_core::children::quiet_command("/usr/bin/open")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !opened.is_ok_and(|status| status.success()) {
        let _ = std::fs::remove_file(&script);
        anyhow::bail!("Terminal could not be opened. Try Sign in again in a moment.");
    }
    Ok(())
}

#[cfg(windows)]
fn claude_terminal(bin: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    leo_core::children::quiet_command("cmd")
        .raw_arg(windows_line(bin))
        .current_dir(windows_folder(bin))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("A command window could not be opened. Try Sign in again in a moment.")?
        .wait()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn claude_terminal(_bin: &Path) -> Result<()> {
    anyhow::bail!("Open a terminal, run `claude` and sign in.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_signs_in_with_login_alone() {
        assert_eq!(codex_arguments(), ["login"]);
    }
    #[test]
    fn the_mac_script_runs_the_resolved_claude_in_a_login_shell_and_removes_itself() {
        assert_eq!(
            mac_script(Path::new("/Users/me/.local/bin/claude")),
            "#!/bin/zsh -l\nrm -f -- \"$0\"\nexec '/Users/me/.local/bin/claude' auth login\n"
        );
        assert_eq!(
            mac_script(Path::new(
                "/Users/me/Library/Application Support/Claude/claude-code/2.1.330/4262a1273231/claude.app/Contents/MacOS/claude"
            )),
            "#!/bin/zsh -l\nrm -f -- \"$0\"\nexec '/Users/me/Library/Application Support/Claude/claude-code/2.1.330/4262a1273231/claude.app/Contents/MacOS/claude' auth login\n"
        );
        assert_eq!(
            mac_script(Path::new("/Users/o'neil/$(touch x)/claude")),
            "#!/bin/zsh -l\nrm -f -- \"$0\"\nexec '/Users/o'\\''neil/$(touch x)/claude' auth login\n"
        );
    }
    #[test]
    fn only_a_path_cmd_misreads_is_run_from_its_own_folder() {
        let odd = Path::new("/a&b/claude");
        assert_eq!(windows_folder(odd), Path::new("/a&b"));
        let plain = Path::new("/plain/claude");
        assert_eq!(windows_folder(plain), std::env::temp_dir());
    }

    #[test]
    fn the_windows_line_opens_one_visible_console_running_claude() {
        assert_eq!(
            windows_line(Path::new(r"C:\Users\me\AppData\Roaming\npm\claude.cmd")),
            r#"/c "start "" cmd /k "C:\Users\me\AppData\Roaming\npm\claude.cmd" auth login""#
        );
        assert_eq!(
            windows_line(Path::new(r"C:\Program Files (x86)\a&b\claude.exe")),
            r#"/c "start "" cmd /k claude auth login""#
        );
    }
}
