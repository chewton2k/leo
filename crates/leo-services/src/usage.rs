use std::collections::BTreeMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::provider::ProviderConfig;

pub const STALE_AFTER: Duration = Duration::from_secs(5 * 60);
pub const CODEX_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub seen_at: DateTime<Utc>,
}

pub type Seen = BTreeMap<String, Usage>;

fn window(value: &Value, used_key: &str, scale: f64, resets_key: &str) -> Option<Window> {
    let used = value[used_key].as_f64()? / scale;
    Some(Window {
        used,
        resets_at: value[resets_key].as_i64(),
    })
}

pub fn from_claude(info: &Value, now: DateTime<Utc>) -> Option<Usage> {
    let windows = &info["unifiedWindows"];
    let mut usage = Usage {
        five_hour: window(&windows["five_hour"], "utilization", 1.0, "resetsAt"),
        seven_day: window(&windows["seven_day"], "utilization", 1.0, "resetsAt"),
        seen_at: now,
    };
    if usage.five_hour.is_none() && usage.seven_day.is_none() {
        let only = window(info, "utilization", 1.0, "resetsAt");
        match info["rateLimitType"].as_str() {
            Some("five_hour") => usage.five_hour = only,
            Some("seven_day") => usage.seven_day = only,
            _ => return None,
        }
    }
    Some(usage)
}

pub fn from_codex(result: &Value, now: DateTime<Utc>) -> Option<Usage> {
    let limits = &result["rateLimits"];
    let mut usage = Usage {
        five_hour: None,
        seven_day: None,
        seen_at: now,
    };
    for (slot, fallback_to_week) in [("primary", false), ("secondary", true)] {
        let value = &limits[slot];
        let Some(found) = window(value, "usedPercent", 100.0, "resetsAt") else {
            continue;
        };
        let week = match value["windowDurationMins"].as_i64() {
            Some(minutes) => minutes >= 24 * 60,
            None => fallback_to_week,
        };
        if week {
            usage.seven_day = Some(found);
        } else {
            usage.five_hour = Some(found);
        }
    }
    (usage.five_hour.is_some() || usage.seven_day.is_some()).then_some(usage)
}

fn percent(window: &Window, now: DateTime<Utc>) -> u32 {
    if window.resets_at.is_some_and(|at| at <= now.timestamp()) {
        return 0;
    }
    (window.used * 100.0).round().clamp(0.0, 100.0) as u32
}

fn age(seen_at: DateTime<Utc>, now: DateTime<Utc>) -> Option<String> {
    let secs = (now - seen_at).num_seconds().max(0) as u64;
    if secs < STALE_AFTER.as_secs() {
        return None;
    }
    Some(match secs {
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    })
}

pub fn label(usage: &Usage, now: DateTime<Utc>) -> String {
    let mut parts = Vec::new();
    if let Some(w) = &usage.five_hour {
        parts.push(format!("5h: {}%", percent(w, now)));
    }
    if let Some(w) = &usage.seven_day {
        parts.push(format!("7d: {}%", percent(w, now)));
    }
    if let Some(age) = age(usage.seen_at, now) {
        parts.push(age);
    }
    parts.join(", ")
}

fn file() -> Option<PathBuf> {
    leo_core::paths::config_dir()
        .ok()
        .map(|dir| dir.join(leo_core::paths::own::USAGE))
}

pub fn load_from(path: &Path) -> Seen {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &Path, provider: &str, usage: Usage) {
    static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _saving = SAVING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut all = load_from(path);
    if all.get(provider).is_some_and(|old| {
        old.five_hour == usage.five_hour
            && old.seven_day == usage.seven_day
            && usage.seen_at - old.seen_at < chrono::Duration::seconds(60)
    }) {
        return;
    }
    all.insert(provider.to_string(), usage);
    let Ok(text) = serde_json::to_string_pretty(&all) else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let _ = leo_core::paths::write_atomic(path, text);
}

pub fn load() -> Seen {
    file().map(|path| load_from(&path)).unwrap_or_default()
}

static REPORTED: std::sync::Mutex<std::collections::BTreeMap<String, u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

fn count_report(provider: &str) {
    *REPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(provider.to_string())
        .or_default() += 1;
}

fn reported(provider: &str) -> u64 {
    REPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(provider)
        .copied()
        .unwrap_or(0)
}

pub fn save(provider: &str, usage: Usage) {
    if let Some(path) = file() {
        count_report(provider);
        save_to(&path, provider, usage);
    }
}

pub fn ask_codex(program: &Path) -> Option<Usage> {
    use crate::ai::provider::agent_cli::codex_disables;
    use crate::ai::provider::process::{self, Process, Watchdog, MOST_OUTPUT, QUEUED_MESSAGES};
    let room = tempfile::Builder::new()
        .prefix(crate::ai::USAGE_ROOM)
        .tempdir()
        .ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(room.path(), std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    let mut command = leo_core::children::quiet_command(program);
    crate::ai::provider::scoped_cli::plan_environment(&mut command);
    command.arg("app-server");
    for feature in codex_disables(program).ok()? {
        command.args(["-c", &format!("features.{feature}=false")]);
    }
    command
        .current_dir(room.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take();
    let stdin = child.stdin.take();
    let child = Process::new(child);
    let stdout = stdout?;
    let _watch = Watchdog::start(&child, None, CODEX_WAIT);
    let (tx, rx) = std::sync::mpsc::sync_channel(QUEUED_MESSAGES);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut bytes = 0usize;
        while let Ok(Some(line)) = process::line(&mut reader) {
            bytes = bytes.saturating_add(line.len());
            if bytes > MOST_OUTPUT {
                break;
            }
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message["id"] == 2 {
                let _ = tx.send(message);
                break;
            }
        }
    });
    if let Some(mut stdin) = stdin {
        let version = env!("CARGO_PKG_VERSION");
        let _ = writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"leo","version":"{version}"}}}}}}"#
        );
        let _ = writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"initialized"}}"#);
        let _ = writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read"}}"#
        );
        let answer = rx.recv_timeout(CODEX_WAIT).ok();
        drop(stdin);
        child.stop();
        return from_codex(&answer?["result"], Utc::now());
    }
    child.stop();
    None
}

fn cached_codex(home: &Path, now: DateTime<Utc>) -> Option<Usage> {
    use std::io::{Read, Seek, SeekFrom};
    const DAYS: i64 = 7;
    const MOST_FILES: usize = 512;
    const RECENT_FILES: usize = 8;
    const TAIL_BYTES: u64 = 256 * 1024;
    if !home.is_absolute() || !std::fs::symlink_metadata(home).ok()?.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    let mut remaining = MOST_FILES;
    for day in 0..DAYS {
        let date = now - chrono::Duration::days(day);
        let relative = PathBuf::from(date.format("sessions/%Y/%m/%d").to_string());
        let Ok(folder) = leo_core::paths::contained_path(home, &relative) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.take(remaining).flatten() {
            remaining = remaining.saturating_sub(1);
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_file() {
                if let Ok(modified) = meta.modified() {
                    files.push((modified, entry.path()));
                }
            }
        }
        if remaining == 0 {
            break;
        }
    }
    files.sort_unstable_by_key(|file| std::cmp::Reverse(file.0));
    let mut newest: Option<Usage> = None;
    for (_, path) in files.into_iter().take(RECENT_FILES) {
        let Ok(mut file) = leo_core::paths::open_regular(&path) else {
            continue;
        };
        let Ok(meta) = file.metadata() else {
            continue;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() != 1 {
                continue;
            }
        }
        if file
            .seek(SeekFrom::Start(meta.len().saturating_sub(TAIL_BYTES)))
            .is_err()
        {
            continue;
        }
        let mut bytes = Vec::new();
        if file.take(TAIL_BYTES).read_to_end(&mut bytes).is_err() {
            continue;
        }
        for line in bytes.split(|byte| *byte == b'\n').rev() {
            let Ok(event) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if event["type"] != "event_msg" || event["payload"]["type"] != "token_count" {
                continue;
            }
            let Some(seen_at) = event["timestamp"]
                .as_str()
                .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
                .map(|stamp| stamp.with_timezone(&Utc))
            else {
                continue;
            };
            if seen_at > now || seen_at < now - chrono::Duration::days(DAYS) {
                continue;
            }
            let limits = &event["payload"]["rate_limits"];
            let convert = |slot: &str| serde_json::json!({"usedPercent":limits[slot]["used_percent"],"windowDurationMins":limits[slot]["window_minutes"],"resetsAt":limits[slot]["resets_at"]});
            if let Some(usage) = from_codex(
                &serde_json::json!({"rateLimits":{"primary":convert("primary"),"secondary":convert("secondary")}}),
                seen_at,
            ) {
                if newest
                    .as_ref()
                    .is_none_or(|old| old.seen_at < usage.seen_at)
                {
                    newest = Some(usage);
                }
            }
        }
    }
    newest
}

pub fn refresh_codex(cfg: &crate::config::Config) {
    let Some(pc) = cfg.provider("codex") else {
        return;
    };
    let cli = crate::ai::provider::agent_cli::AgentCli::new(
        "codex".into(),
        crate::ai::provider::agent_cli::Agent::Codex,
        pc,
    );
    let Some(program) = crate::ai::provider::agent_cli::locate(cli.bin()) else {
        return;
    };
    let home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    let usage = ask_codex(&program).or_else(|| {
        home.as_deref()
            .and_then(|home| cached_codex(home, Utc::now()))
    });
    if let Some(usage) = usage {
        if load()
            .get("codex")
            .is_none_or(|old| old.seen_at < usage.seen_at)
        {
            save("codex", usage);
        }
    }
}

fn claude_reader(base: ProviderConfig) -> ProviderConfig {
    let mut reader = base;
    let model = crate::config::choice::WRITING
        .iter()
        .find(|c| c.provider == "claude_code")
        .and_then(|c| c.models.first());
    reader.model = model.map(|m| m.id.to_string());
    reader.effort = model.and_then(|m| {
        crate::config::choice::efforts("claude_code", m.id)
            .first()
            .map(|e| e.to_string())
    });
    reader.no_tools = true;
    reader
}

pub fn refresh_claude(cfg: &crate::config::Config) -> Option<String> {
    use crate::ai::provider::agent_cli::{claude_plan, Agent, AgentCli};
    use crate::ai::provider::{ChatProvider, ChatRequest};
    let base = cfg
        .provider("claude_code")
        .cloned()
        .or_else(|| crate::config::Config::built_in_provider("claude_code"))?;
    let bin = base.bin.clone().filter(|b| !b.trim().is_empty());
    let bin = bin.as_deref().unwrap_or(Agent::ClaudeCode.program());
    if !claude_plan(bin) {
        return Some("Claude limits need Claude Code signed in with your Claude plan.".into());
    }
    let before = reported("claude_code");
    let limit = Duration::from_secs(20);
    let agent = AgentCli::new(
        "claude_code".into(),
        Agent::ClaudeCode,
        &claude_reader(base),
    )
    .with_limits(limit, limit);
    let read = agent.complete(&ChatRequest {
        system: None,
        prompt: "Reply with the single word: ok".into(),
        temperature: 0.0,
        max_tokens: 16,
    });
    if read.is_err() {
        return Some("Claude Code could not be reached. Try Refresh again.".into());
    }
    (reported("claude_code") == before).then(|| "Claude Code reported no limits.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn cached_codex_limits_keep_the_event_time_and_skip_invalid_reports() {
        let dir = tempfile::tempdir().unwrap();
        let now = at(1_800_000_000);
        let folder = dir.path().join(now.format("sessions/%Y/%m/%d").to_string());
        std::fs::create_dir_all(&folder).unwrap();
        let event = serde_json::json!({"timestamp":now.to_rfc3339(),"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":12,"window_minutes":300,"resets_at":1_800_001_000},"secondary":{"used_percent":41,"window_minutes":10080}}}});
        let file = folder.join("rollout-example.jsonl");
        std::fs::write(
            &file,
            format!(
                "{}\n{}\n{}\n",
                serde_json::json!({"type":"response_item","payload":{"text":"private"}}),
                event,
                "{broken"
            ),
        )
        .unwrap();
        let usage = cached_codex(dir.path(), now).unwrap();
        assert_eq!(usage.seen_at, now);
        assert_eq!(label(&usage, now), "5h: 12%, 7d: 41%");
        assert!(cached_codex(dir.path(), now + chrono::Duration::days(8)).is_none());
        let mut later = event.clone();
        later["timestamp"] = serde_json::json!((now + chrono::Duration::seconds(1)).to_rfc3339());
        later["payload"]["rate_limits"]["primary"]["used_percent"] = serde_json::json!(23);
        std::fs::write(&file, format!("{}\n{}\n{}\n", later, event, later)).unwrap();
        assert_eq!(
            cached_codex(dir.path(), now)
                .unwrap()
                .five_hour
                .unwrap()
                .used,
            0.12
        );
        assert_eq!(
            cached_codex(dir.path(), now + chrono::Duration::seconds(1))
                .unwrap()
                .five_hour
                .unwrap()
                .used,
            0.23
        );
        std::fs::write(&file, format!("{}\n{}\n", "x".repeat(300 * 1024), event)).unwrap();
        assert_eq!(cached_codex(dir.path(), now).unwrap().seen_at, now);
    }

    #[cfg(unix)]
    #[test]
    fn cached_codex_limits_reject_symlinked_folders_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let now = at(1_800_000_000);
        std::os::unix::fs::symlink(outside.path(), dir.path().join("sessions")).unwrap();
        assert!(cached_codex(dir.path(), now).is_none());
        std::fs::remove_file(dir.path().join("sessions")).unwrap();
        let folder = dir.path().join(now.format("sessions/%Y/%m/%d").to_string());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(outside.path().join("private"), "{}").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("private"),
            folder.join("rollout-linked.jsonl"),
        )
        .unwrap();
        assert!(cached_codex(dir.path(), now).is_none());
        std::fs::remove_file(folder.join("rollout-linked.jsonl")).unwrap();
        std::fs::hard_link(
            outside.path().join("private"),
            folder.join("rollout-linked.jsonl"),
        )
        .unwrap();
        assert!(cached_codex(dir.path(), now).is_none());
    }

    #[test]
    fn claude_code_reports_both_windows() {
        let info: Value = serde_json::from_str(
            r#"{"status":"allowed_warning","rateLimitType":"seven_day","utilization":0.93,
                "unifiedWindows":{"five_hour":{"utilization":0.09,"resetsAt":2000},
                                  "seven_day":{"utilization":0.93,"resetsAt":9000}}}"#,
        )
        .unwrap();
        let usage = from_claude(&info, at(1000)).unwrap();
        assert_eq!(label(&usage, at(1000)), "5h: 9%, 7d: 93%");
    }

    #[test]
    fn an_older_claude_report_with_one_window_still_counts() {
        let info: Value =
            serde_json::from_str(r#"{"rateLimitType":"five_hour","utilization":0.45}"#).unwrap();
        let usage = from_claude(&info, at(1000)).unwrap();
        assert_eq!(label(&usage, at(1000)), "5h: 45%");
        assert!(from_claude(&serde_json::json!({}), at(1000)).is_none());
    }

    #[test]
    fn codex_windows_are_told_apart_by_their_length() {
        let result: Value = serde_json::from_str(
            r#"{"rateLimits":{"primary":{"usedPercent":2,"windowDurationMins":300,"resetsAt":5000},
                              "secondary":{"usedPercent":47,"windowDurationMins":10080,"resetsAt":9000}}}"#,
        )
        .unwrap();
        let usage = from_codex(&result, at(1000)).unwrap();
        assert_eq!(label(&usage, at(1000)), "5h: 2%, 7d: 47%");
        assert!(from_codex(&serde_json::json!({"rateLimits": {}}), at(1000)).is_none());
    }

    #[test]
    fn a_window_that_has_reset_reads_zero_and_old_numbers_say_how_old() {
        let usage = Usage {
            five_hour: Some(Window {
                used: 0.8,
                resets_at: Some(1500),
            }),
            seven_day: Some(Window {
                used: 0.5,
                resets_at: Some(900_000),
            }),
            seen_at: at(1000),
        };
        assert_eq!(label(&usage, at(1200)), "5h: 80%, 7d: 50%");
        assert_eq!(
            label(&usage, at(1000 + 12 * 60)),
            "5h: 0%, 7d: 50%, 12 min ago"
        );
        assert_eq!(
            label(&usage, at(1000 + 3 * 3600)),
            "5h: 0%, 7d: 50%, 3 h ago"
        );
    }

    #[test]
    fn a_report_is_counted_even_when_its_save_is_skipped() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("usage.json");
        let usage = |seen_at| Usage {
            five_hour: None,
            seven_day: None,
            seen_at,
        };
        let before = reported("refresh-probe");
        for seen in [at(1000), at(1010)] {
            count_report("refresh-probe");
            save_to(&path, "refresh-probe", usage(seen));
        }
        assert_eq!(load_from(&path)["refresh-probe"].seen_at, at(1000));
        assert_eq!(reported("refresh-probe"), before + 2);
    }

    #[cfg(unix)]
    #[test]
    fn an_unchanged_window_is_not_rewritten() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("usage.json");
        let inode = |path: &Path| std::fs::metadata(path).unwrap().ino();
        let usage = |seen_at| Usage {
            five_hour: Some(Window {
                used: 0.4,
                resets_at: None,
            }),
            seven_day: None,
            seen_at,
        };
        save_to(&path, "claude_code", usage(at(1000)));
        let first = inode(&path);
        save_to(&path, "claude_code", usage(at(1030)));
        assert_eq!(
            inode(&path),
            first,
            "the same windows were written to a new file"
        );
        save_to(&path, "claude_code", usage(at(1120)));
        assert_ne!(inode(&path), first);
        assert_eq!(load_from(&path)["claude_code"].seen_at, at(1120));

        let path = dir.path().join("answers.json");
        let mut last = None;
        let mut written = 0;
        for answer in 0..100 {
            save_to(
                &path,
                "claude_code",
                usage(at(2000) + chrono::Duration::milliseconds(500 * answer)),
            );
            let now = Some(inode(&path));
            if now != last {
                written += 1;
                last = now;
            }
        }
        println!("files written for 100 unchanged answers: {written}");
        assert_eq!(written, 1);
    }

    #[test]
    fn two_providers_saving_at_once_both_survive() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("usage.json");
        let saver = |provider: &'static str| {
            let path = path.clone();
            std::thread::spawn(move || {
                for answer in 0..100 {
                    save_to(
                        &path,
                        provider,
                        Usage {
                            five_hour: Some(Window {
                                used: answer as f64 / 100.0,
                                resets_at: None,
                            }),
                            seven_day: None,
                            seen_at: at(1000 + answer),
                        },
                    );
                }
            })
        };
        let claude = saver("claude_code");
        let codex = saver("codex");
        claude.join().unwrap();
        codex.join().unwrap();
        let all = load_from(&path);
        assert_eq!(all["claude_code"].five_hour.unwrap().used, 0.99);
        assert_eq!(all["codex"].five_hour.unwrap().used, 0.99);
    }

    #[test]
    fn the_latest_numbers_are_kept_per_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("usage.json");
        assert!(load_from(&path).is_empty());
        let usage = |used| Usage {
            five_hour: Some(Window {
                used,
                resets_at: None,
            }),
            seven_day: None,
            seen_at: at(1000),
        };
        save_to(&path, "claude_code", usage(0.1));
        save_to(&path, "codex", usage(0.2));
        save_to(&path, "claude_code", usage(0.3));
        let all = load_from(&path);
        assert_eq!(all.len(), 2);
        assert_eq!(all["claude_code"].five_hour.unwrap().used, 0.3);
        assert_eq!(all["codex"].five_hour.unwrap().used, 0.2);
        std::fs::write(&path, "not json").unwrap();
        assert!(load_from(&path).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_claude_read_spends_nothing_unless_the_account_is_on_a_plan() {
        use std::os::unix::fs::PermissionsExt;
        for answer in [
            r#"{"loggedIn":false}"#,
            r#"{"loggedIn":true,"authMethod":"api-key"}"#,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let calls = dir.path().join("calls");
            let program = dir.path().join("claude");
            std::fs::write(
                &program,
                format!(
                    "#!/bin/sh\necho \"$1\" >> '{}'\nprintf '%s' '{answer}'\n",
                    calls.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut cfg = crate::config::Config::default();
            cfg.providers.insert(
                "claude_code".into(),
                ProviderConfig {
                    bin: Some(program.display().to_string()),
                    ..crate::config::Config::built_in_provider("claude_code").unwrap()
                },
            );
            assert!(refresh_claude(&cfg).unwrap().contains("Claude plan"));
            assert_eq!(std::fs::read_to_string(&calls).unwrap().trim(), "auth");
        }
    }

    #[test]
    fn the_claude_read_uses_the_cheapest_writing_model_and_no_tools() {
        let reader = claude_reader(ProviderConfig::default());
        assert_eq!(reader.model.as_deref(), Some("claude-haiku-5-5"));
        assert!(reader.no_tools);
    }

    #[test]
    #[ignore = "asks the real Claude Code on this computer"]
    fn real_claude_reports_its_limits_with_one_tiny_request() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("LEO_HOME", home.path());
        let note = refresh_claude(&crate::config::Config::default());
        let seen = load();
        println!("note: {note:?}\nseen: {seen:?}");
        assert!(note.is_none());
        assert!(seen["claude_code"].five_hour.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn usage_polling_launches_a_sanitized_native_process() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("codex");
        std::fs::write(
            &program,
            r#"#!/bin/sh
test -z "${CI+x}${ANTHROPIC_API_KEY+x}${OPENAI_API_KEY+x}${CODEX_API_KEY+x}${NODE_OPTIONS+x}${BASH_ENV+x}" || exit 17
if [ "$1" = features ]; then echo 'shell_tool stable true'; exit 0; fi
if [ "$2" = --help ]; then echo 'Usage: codex app-server'; exit 0; fi
case "$*" in *ignore*) exit 9;; esac
while IFS= read -r line; do
  case "$line" in
    *'"method":"account/rateLimits/read"'*)
      printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":300}}}}'
      exit 0 ;;
  esac
done
"#,
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let usage = ask_codex(&program).unwrap();
        assert_eq!(usage.five_hour.unwrap().used, 0.12);
    }
}
