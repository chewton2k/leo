use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

pub fn is_initialized(notes_dir: &Path) -> bool {
    notes_dir.join(".git").exists()
}

pub fn init(notes_dir: &Path) -> Result<()> {
    if is_initialized(notes_dir) {
        println!("Notes repo already initialized.");
        return Ok(());
    }

    fs::create_dir_all(notes_dir)?;

    run_git(notes_dir, &["init", "-b", "main"])
        .context("git init failed — is git installed?")?;

    let gitignore = notes_dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "*.wav\n*.bak\n")?;
    }

    // Commit any existing files (e.g. migrated notes)
    run_git(notes_dir, &["add", "."])?;
    // Suppress error if there is nothing to commit (empty repo)
    let _ = run_git(notes_dir, &["commit", "-m", "init: initialize leo notes repo"]);

    println!("Initialized notes repo in {}", notes_dir.display());
    Ok(())
}

pub fn connect(notes_dir: &Path, url: &str) -> Result<()> {
    if !is_initialized(notes_dir) {
        anyhow::bail!("Run 'leo sync init' first.");
    }
    run_git(notes_dir, &["remote", "add", "origin", url])?;
    println!("Connected to {url}");
    Ok(())
}

pub fn push(notes_dir: &Path) -> Result<()> {
    run_git(notes_dir, &["push", "-u", "origin", "main"])
}

pub fn pull(notes_dir: &Path) -> Result<()> {
    run_git(notes_dir, &["pull", "origin", "main"])
}

pub fn status(notes_dir: &Path) -> Result<()> {
    run_git(notes_dir, &["status"])
}

pub fn auto_commit(notes_dir: &Path) -> Result<()> {
    run_git(notes_dir, &["add", "."])?;

    // Only create a commit if there are staged changes
    let has_changes = !Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(notes_dir)
        .status()
        .context("failed to run git diff --cached")?
        .success();

    if has_changes {
        run_git(notes_dir, &["commit", "-m", "update notes"])?;
    }
    Ok(())
}

fn run_git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .context("failed to run git — is git installed?")?;
    if !status.success() {
        anyhow::bail!("git {} exited with {}", args.join(" "), status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_initialized_false_before_init() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_initialized(tmp.path()));
    }

    #[test]
    fn test_init_creates_git_repo_and_gitignore() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        init(&notes_dir).unwrap();

        assert!(is_initialized(&notes_dir), ".git dir should exist");
        let gitignore = std::fs::read_to_string(notes_dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("*.wav"));
        assert!(gitignore.contains("*.bak"));
    }

    #[test]
    fn test_connect_before_init_returns_error() {
        let tmp = TempDir::new().unwrap();
        let err = connect(tmp.path(), "https://github.com/user/repo.git").unwrap_err();
        assert!(err.to_string().contains("leo sync init"));
    }

    #[test]
    fn test_auto_commit_no_op_when_nothing_staged() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        init(&notes_dir).unwrap();

        auto_commit(&notes_dir).unwrap();
    }

    #[test]
    fn test_auto_commit_commits_new_file() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        init(&notes_dir).unwrap();

        std::fs::write(notes_dir.join("new.md"), "content").unwrap();
        auto_commit(&notes_dir).unwrap();

        let log = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&notes_dir)
            .output()
            .unwrap();
        let log_str = String::from_utf8(log.stdout).unwrap();
        assert!(log_str.contains("update notes"), "expected commit, got: {log_str}");
    }
}
