use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use toml_edit::DocumentMut;

use crate::ai::provider::agent_cli::{self, Agent, AgentCli};
use crate::ai::provider::ChatProvider;
use crate::config::choice::{self, Local};
use crate::config::edit::{self, Task};
use crate::config::secret::{self, SecretStore};
use crate::config::Config;

fn task_named(name: &str) -> Option<Task> {
    match name {
        "writing" => Some(Task::Chat),
        "speech" => Some(Task::Transcribe),
        _ => None,
    }
}

fn task_name(task: Task) -> &'static str {
    match task {
        Task::Chat => "writing",
        Task::Transcribe => "speech",
    }
}

fn accounts(cfg: &Config) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for task in [Task::Chat, Task::Transcribe] {
        for c in choice::choices(task) {
            let Some(pc) = cfg.provider(c.provider) else {
                continue;
            };
            if pc.key_env.is_none() || Agent::of(pc).is_some() {
                continue;
            }
            let account = pc.account(c.provider).to_string();
            if !out.iter().any(|(a, _)| *a == account) {
                out.push((account, c.name.to_string()));
            }
        }
    }
    out
}

fn models(task: Task, provider: &str, local: &Local) -> Vec<Value> {
    match (task, provider) {
        (Task::Chat, "ollama") => local
            .ollama
            .iter()
            .map(|m| json!({ "id": m, "price": "free" }))
            .collect(),
        _ => choice::find(task, provider)
            .map(|c| {
                c.models
                    .iter()
                    .map(|m| json!({ "id": m.id, "price": m.price }))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn task_view(
    cfg: &Config,
    store: &dyn SecretStore,
    local: &Local,
    usage: &crate::usage::Seen,
    task: Task,
) -> Value {
    let selection = choice::selection(cfg, store, task);
    let provider = selection
        .as_ref()
        .map(|s| s.provider.clone())
        .unwrap_or_default();
    let chosen = choice::find(task, &provider);
    let pc = cfg.provider(&provider);
    let agent = pc.and_then(Agent::of);
    let key = match (pc, agent) {
        (Some(pc), None) if pc.key_env.is_some() => {
            let account = pc.account(&provider).to_string();
            let hint = store
                .get(&account)
                .ok()
                .flatten()
                .map(|key| secret::masked(key.as_str()));
            let stored = hint.is_some();
            let ignored = pc
                .key_env
                .as_deref()
                .filter(|var| !stored && std::env::var(var).is_ok_and(|v| !v.trim().is_empty()));
            json!({
                "account": account,
                "name": chosen.map(|c| c.name).unwrap_or(provider.as_str()),
                "stored": stored,
                "hint": hint,
                "ignored": ignored,
            })
        }
        _ => Value::Null,
    };
    let signin = agent.map(|agent| {
        let located = pc
            .map(|p| AgentCli::new(provider.clone(), agent, p))
            .map_or_else(|| agent_cli::locate(agent.program()), |cli| agent_cli::locate(cli.bin()))
            .is_some();
        let installed = pc.is_some_and(|p| AgentCli::new(provider.clone(), agent, p).available());
        json!({
            "installed": installed,
            "text": if installed {
                format!("Uses {}. Sign in with the Sign in button if leo asks.", agent.plan())
            } else if located {
                format!("{} needs an update before leo can use it.", chosen.map(|c| c.name).unwrap_or(provider.as_str()))
            } else {
                format!("{} is not installed on this computer.", chosen.map(|c| c.name).unwrap_or(provider.as_str()))
            },
        })
    });
    let note = match (task, provider.as_str()) {
        (Task::Chat, "ollama") if !local.ollama_running => {
            Some(if crate::health::on_path("ollama") {
                "Ollama is not running: open the Ollama app on this computer."
            } else {
                "Ollama is not installed on this computer."
            })
        }
        (Task::Chat, "ollama") if local.ollama.is_empty() => {
            Some("Ollama has no models yet. Choose one below.")
        }
        (Task::Transcribe, "parakeet") if !local.speech_ready => {
            Some("Download a local model in Settings → Speech models.")
        }
        _ => None,
    };
    let usage = usage
        .get(&provider)
        .filter(|_| agent.is_some())
        .map(|seen| crate::usage::label(seen, chrono::Utc::now()));
    let model = selection.as_ref().and_then(|s| s.model.clone());
    let efforts = if task == Task::Chat {
        choice::efforts(&provider, model.as_deref().unwrap_or(""))
    } else {
        &[]
    };
    json!({
        "task": task_name(task),
        "provider": provider,
        "effort": choice::effort_for(&provider, model.as_deref().unwrap_or(""), pc.and_then(|p| p.effort.as_deref())),
        "efforts": efforts,
        "ready": selection.as_ref().is_some_and(|s| s.ready),
        "custom": chosen.is_none() && !provider.is_empty(),
        "choices": choice::choices(task).iter().map(|c| json!({ "id": c.provider, "label": c.label() })).collect::<Vec<_>>(),
        "model": selection.as_ref().and_then(|s| s.model.clone()),
        "models": models(task, &provider, local),
        "fixed_model": chosen.is_some_and(|c| c.local()) && task == Task::Transcribe,
        "key": key,
        "signin": signin,
        "usage": usage,
        "note": note,
    })
}

pub fn describe(
    cfg: &Config,
    store: &dyn SecretStore,
    local: &Local,
    usage: &crate::usage::Seen,
    notes_dir: Option<&Path>,
) -> Value {
    json!({
        "tasks": [
            task_view(cfg, store, local, usage, Task::Chat),
            task_view(cfg, store, local, usage, Task::Transcribe),
        ],
        "writing": crate::health::writing(cfg, store, Some(local)),
        "paths": {
            "notes": notes_dir.map(|p| p.display().to_string()),
            "config": Config::config_path().ok().map(|p| p.display().to_string()),
        },
        "prices": {
            "as_of": choice::API_PRICES_AS_OF,
            "models": choice::WRITING
                .iter()
                .flat_map(|c| c.models)
                .map(|m| (m.id, m.api_cents))
                .collect::<std::collections::BTreeMap<_, _>>(),
        },
    })
}

fn agents(cfg: &Config, probe: bool) -> Vec<(choice::Choice, Agent, bool, Option<bool>)> {
    choice::WRITING
        .iter()
        .filter_map(|choice| {
            let pc = cfg.provider(choice.provider)?;
            let agent = Agent::of(pc)?;
            let cli = AgentCli::new(choice.provider.to_string(), agent, pc);
            let installed = cli.available();
            let signed_in = (probe && installed)
                .then(|| agent_cli::signed_in(agent, cli.bin()))
                .flatten();
            Some((*choice, agent, installed, signed_in))
        })
        .collect()
}

pub fn coding_plans(cfg: &Config, usage: &crate::usage::Seen) -> Vec<Value> {
    agents(cfg, true)
        .into_iter()
        .map(|(choice, agent, installed, signed_in)| {
            json!({
                "provider": choice.provider,
                "name": choice.name,
                "installed": installed,
                "signed_in": signed_in,
                "signin": agent.sign_in(),
                "limits": usage.get(choice.provider),
            })
        })
        .collect()
}

pub fn candidates(cfg: &Config, local: &Local, probe: bool) -> Vec<Value> {
    let mut found: Vec<Value> = agents(cfg, probe)
        .into_iter()
        .map(|(choice, _, installed, signed_in)| {
            json!({"id": choice.provider, "installed": installed, "signed_in": signed_in})
        })
        .collect();
    found.push(json!({
        "id": "ollama",
        "installed": local.ollama_running || crate::health::on_path("ollama"),
        "running": local.ollama_running,
        "models": local.ollama,
    }));
    found
}

fn open(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        leo_core::paths::write_atomic(path, Config::default_toml())?;
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    text.parse::<DocumentMut>()
        .with_context(|| format!("{} is not valid TOML", path.display()))
}

fn text<'a>(change: &'a Value, field: &str) -> Result<&'a str> {
    change
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("the change has no {field}"))
}

pub fn apply(
    change: &Value,
    config_path: &Path,
    store: &dyn SecretStore,
    local: &Local,
) -> Result<String> {
    let _saving = edit::saving();
    match text(change, "set")? {
        "provider" => {
            let task = task_named(text(change, "task")?).context("no such task")?;
            let value = text(change, "value")?;
            let Some(chosen) = choice::find(task, value) else {
                bail!("leo does not offer {value} there");
            };
            let mut doc = open(config_path)?;
            choice::write_choice(&mut doc, task, chosen.provider);
            if task == Task::Chat && chosen.provider == "ollama" {
                let installed = |m: &str| local.ollama.iter().any(|o| o == m);
                let asked = change.get("model").and_then(Value::as_str);
                let set = Config::load_from(config_path)
                    .provider("ollama")
                    .and_then(|p| p.model.clone());
                let model = match asked.filter(|m| installed(m)) {
                    Some(model) => Some(model),
                    None if set.as_deref().is_some_and(installed) => None,
                    None => local.ollama.first().map(String::as_str),
                };
                if let Some(model) = model {
                    choice::write_model(&mut doc, "ollama", model);
                }
            }
            edit::save_document(config_path, &doc)?;
            Ok(format!(
                "{} now uses {}.",
                if task == Task::Chat {
                    "Writing"
                } else {
                    "Speech"
                },
                chosen.label()
            ))
        }
        "model" => {
            let task = task_named(text(change, "task")?).context("no such task")?;
            let value = text(change, "value")?;
            let cfg = Config::load_from(config_path);
            let Some(selection) = choice::selection(&cfg, store, task) else {
                bail!("choose an AI first");
            };
            let options = choice::model_options(task, &selection.provider, local);
            if !options.iter().any(|o| o == value) {
                bail!(
                    "{value} is not one of the models offered for {}",
                    selection.provider
                );
            }
            let mut doc = open(config_path)?;
            choice::write_model(&mut doc, &selection.provider, value);
            edit::save_document(config_path, &doc)?;
            Ok(format!(
                "Model set to {}.",
                choice::priced(task, &selection.provider, value)
            ))
        }
        "key" => {
            let account = text(change, "account")?;
            let cfg = Config::load_from(config_path);
            let Some((_, name)) = accounts(&cfg).into_iter().find(|(a, _)| a == account) else {
                bail!("there is no key called {account}");
            };
            match change.get("value").and_then(Value::as_str).map(str::trim) {
                None | Some("") => {
                    store.delete(account)?;
                    Ok(format!("The {name} key is removed."))
                }
                Some(key) => {
                    if key.len() > 512 || key.chars().any(char::is_whitespace) {
                        bail!("that does not look like an API key");
                    }
                    store.set(account, key)?;
                    Ok(format!("The {name} key is stored on this computer."))
                }
            }
        }
        "effort" => {
            let cfg = Config::load_from(config_path);
            let Some(selection) = choice::selection(&cfg, store, Task::Chat) else {
                bail!("choose an AI first");
            };
            let levels = choice::efforts(
                &selection.provider,
                selection.model.as_deref().unwrap_or(""),
            );
            if levels.is_empty() {
                bail!("{} has no effort setting", selection.provider);
            }
            let value = change
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !levels.contains(&value) {
                bail!("{value} is not an effort {} offers", selection.provider);
            }
            let effort = Some(value);
            let mut doc = open(config_path)?;
            choice::write_effort(&mut doc, &selection.provider, effort);
            edit::save_document(config_path, &doc)?;
            Ok(format!("Effort set to {value}."))
        }
        other => bail!("{other} is not a setting"),
    }
}

pub fn test(cfg: &Config, store: &dyn SecretStore, task: &str) -> Result<String> {
    let task = task_named(task).context("no such task")?;
    let Some(selection) = choice::selection(cfg, store, task) else {
        bail!("choose an AI first");
    };
    crate::providers::test_provider(cfg, store, &selection.provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::MemoryStore;

    fn setup(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn view(path: &Path, store: &dyn SecretStore) -> Value {
        describe(
            &Config::load_from(path),
            store,
            &Local::default(),
            &Default::default(),
            None,
        )
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_config_is_created_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leo").join("config.toml");
        open(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.toml created with {mode:o}");
    }

    #[test]
    fn the_page_offers_local_or_signed_in_writers_and_only_local_speech() {
        let (_dir, path) = setup("");
        let store = MemoryStore::default();
        store.set("openai", "old-private-key").unwrap();
        let page = view(&path, &store);
        assert_eq!(page["tasks"][0]["choices"].as_array().unwrap().len(), 3);
        assert_eq!(page["tasks"][1]["choices"].as_array().unwrap().len(), 1);
        assert!(page["tasks"][1]["key"].is_null());
        assert!(!page.to_string().contains("old-private-key"));
    }

    #[test]
    fn a_coding_plan_shows_its_sign_in_instead_of_a_key() {
        let (_d, path) = setup("[chat]\nchain = [\"codex\"]\n[providers.codex]\nbin = \"leo-no-such-codex\"\n[transcribe]\nchain = []\n");
        let page = view(&path, &MemoryStore::default());
        let writing = &page["tasks"][0];
        assert!(writing["key"].is_null());
        assert_eq!(page["writing"]["state"], "not_installed");
        assert_eq!(page["writing"]["provider"], "codex");
        assert_eq!(writing["signin"]["installed"], false);
        let text = writing["signin"]["text"].as_str().unwrap();
        assert!(
            text.ends_with(" is not installed on this computer."),
            "{text}"
        );
        assert!(!text.contains("npm"));
    }

    #[cfg(unix)]
    #[test]
    fn coding_plans_detect_both_accounts_without_exposing_auth_output() {
        use std::os::unix::fs::PermissionsExt;
        for (codex, claude, expected) in [
            (
                "Logged in using ChatGPT",
                r#"{"loggedIn":true,"token":"PRIVATE_SENTINEL"}"#,
                json!(true),
            ),
            ("Not logged in", r#"{"loggedIn":false}"#, json!(false)),
            (
                "Unknown command PRIVATE_SENTINEL",
                "PRIVATE_SENTINEL",
                Value::Null,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut cfg = Config::default();
            for (name, output) in [("codex", codex), ("claude_code", claude)] {
                let bin = dir.path().join(name);
                std::fs::write(&bin, format!("#!/bin/sh\nprintf '%s\\n' '{output}'\n")).unwrap();
                std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
                cfg.providers.get_mut(name).unwrap().bin = Some(bin.to_string_lossy().into());
            }
            let seen = crate::usage::Seen::from([(
                "codex".into(),
                crate::usage::Usage {
                    five_hour: Some(crate::usage::Window {
                        used: 0.25,
                        resets_at: Some(1234),
                    }),
                    seven_day: None,
                    seen_at: chrono::Utc::now(),
                },
            )]);
            let plans = coding_plans(&cfg, &seen);
            assert_eq!(plans.len(), 2);
            for plan in &plans {
                assert_eq!(plan["installed"], true);
                assert_eq!(plan["signed_in"], expected);
                assert!(!plan.to_string().contains("PRIVATE_SENTINEL"));
            }
            assert_eq!(plans[0]["limits"]["five_hour"]["used"], 0.25);
            assert!(plans[1]["limits"].is_null());
            for name in ["codex", "claude_code"] {
                cfg.providers.get_mut(name).unwrap().bin = Some("leo-missing-coding-app".into());
            }
            for plan in coding_plans(&cfg, &seen) {
                assert_eq!(plan["installed"], false);
                assert!(plan["signed_in"].is_null());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn candidates_ask_for_sign_in_only_when_probed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ran = dir.path().join("ran");
        let mut cfg = Config::default();
        for name in ["codex", "claude_code"] {
            let bin = dir.path().join(name);
            std::fs::write(&bin, format!("#!/bin/sh\ntouch '{}'\nprintf '%s\\n' 'Logged in using ChatGPT' '{{\"loggedIn\":true}}'\n", ran.display())).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
            cfg.providers.get_mut(name).unwrap().bin = Some(bin.to_string_lossy().into());
        }
        let local = Local {
            ollama_running: true,
            ollama: vec!["qwen3:4b".into()],
            ..Local::default()
        };
        let quiet = candidates(&cfg, &local, false);
        assert!(!ran.exists());
        assert_eq!(quiet.len(), 3);
        for found in &quiet[..2] {
            assert_eq!(found["installed"], true);
            assert!(found["signed_in"].is_null());
        }
        assert_eq!(
            quiet[2],
            json!({"id":"ollama","installed":true,"running":true,"models":["qwen3:4b"]})
        );
        let probed = candidates(&cfg, &local, true);
        assert!(ran.exists());
        let ids: Vec<_> = probed.iter().map(|found| found["id"].clone()).collect();
        assert_eq!(ids, [json!("codex"), json!("claude_code"), json!("ollama")]);
        assert_eq!(probed[0]["signed_in"], true);
    }

    #[cfg(unix)]
    #[test]
    fn coding_plans_use_normalized_bins_for_availability_and_signin() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD_HOME: &str = "LEO_TEST_CODING_BIN_DIR";
        if std::env::var_os(CHILD_HOME).is_some() {
            for bin in [None, Some(""), Some(" \t ")] {
                let mut cfg = Config::default();
                for name in ["codex", "claude_code"] {
                    cfg.providers.get_mut(name).unwrap().bin = bin.map(String::from);
                }
                let plans = coding_plans(&cfg, &Default::default());
                assert_eq!(plans.len(), 2);
                for plan in plans {
                    assert_eq!(plan["installed"], true, "{bin:?}: {plan}");
                    assert_eq!(plan["signed_in"], true, "{bin:?}: {plan}");
                }
            }
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        for (name, output) in [
            ("codex", "Logged in using ChatGPT"),
            ("claude", r#"{"loggedIn":true}"#),
        ] {
            let bin = dir.path().join(name);
            std::fs::write(&bin, format!("#!/bin/sh\nprintf '%s\\n' '{output}'\n")).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "web_settings::tests::coding_plans_use_normalized_bins_for_availability_and_signin",
                "--nocapture",
            ])
            .env(CHILD_HOME, dir.path())
            .env("PATH", dir.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        const CHILD_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
        let deadline = std::time::Instant::now() + CHILD_WAIT;
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("coding-account checks exceeded {CHILD_WAIT:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn effort_is_chosen_from_what_the_program_offers_and_unset_reads_as_medium() {
        let (_d, path) = setup("[chat]\nchain = [\"codex\"]\n[transcribe]\nchain = []\n");
        let store = MemoryStore::default();
        let local = Local::default();
        let set = |value: &str| {
            apply(
                &json!({"set": "effort", "value": value}),
                &path,
                &store,
                &local,
            )
        };
        let writing = &view(&path, &store)["tasks"][0];
        assert_eq!(
            writing["efforts"],
            json!(["low", "medium", "high", "xhigh"])
        );
        assert_eq!(writing["effort"], "medium");
        assert_eq!(set("high").unwrap(), "Effort set to high.");
        assert_eq!(view(&path, &store)["tasks"][0]["effort"], "high");
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("effort = \"high\""));
        assert!(set("max")
            .unwrap_err()
            .to_string()
            .contains("not an effort"));
        assert!(set("").is_err(), "there is no default to go back to");
        assert!(set("default").is_err());
        assert_eq!(view(&path, &store)["tasks"][0]["effort"], "high");
        let (_d2, other) = setup("[chat]\nchain = [\"ollama\"]\n[transcribe]\nchain = []\n");
        assert!(apply(
            &json!({"set": "effort", "value": "high"}),
            &other,
            &store,
            &local
        )
        .is_err());
        assert_eq!(view(&other, &store)["tasks"][0]["efforts"], json!([]));
    }

    #[test]
    fn choosing_a_provider_and_model_writes_the_config() {
        let (_d, path) = setup("[chat]\nchain = [\"ollama\"]\n[providers.claude_code]\nbin = \"leo-no-such-claude\"\n[transcribe]\nchain = []\n");
        let store = MemoryStore::default();
        let local = Local::default();
        let said = apply(
            &json!({"set": "provider", "task": "writing", "value": "claude_code"}),
            &path,
            &store,
            &local,
        )
        .unwrap();
        assert_eq!(said, "Writing now uses Claude Code.");
        let said = apply(
            &json!({"set": "model", "task": "writing", "value": "claude-sonnet-5-5"}),
            &path,
            &store,
            &local,
        )
        .unwrap();
        assert!(
            said.starts_with("Model set to claude-sonnet-5-5 (included in your"),
            "{said}"
        );
        let cfg = Config::load_from(&path);
        assert_eq!(cfg.chat.chain, ["claude_code"]);
        assert_eq!(
            cfg.provider("claude_code").unwrap().model.as_deref(),
            Some("claude-sonnet-5-5")
        );
        assert!(apply(
            &json!({"set": "model", "task": "writing", "value": "gpt-made-up"}),
            &path,
            &store,
            &local
        )
        .is_err());
        assert!(
            apply(
                &json!({"set": "provider", "task": "writing", "value": "parakeet"}),
                &path,
                &store,
                &local
            )
            .is_err(),
            "speech engines cannot write"
        );
        assert!(apply(
            &json!({"set": "provider", "task": "cooking", "value": "openai"}),
            &path,
            &store,
            &local
        )
        .is_err());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[transcribe]"),
            "the rest of the file is kept"
        );
    }

    #[test]
    fn choosing_ollama_uses_an_installed_model() {
        let (_d, path) = setup("[chat]\nchain = [\"claude_code\"]\n[providers.ollama]\nmodel = \"qwen3:8b\"\n[transcribe]\nchain = []\n");
        let store = MemoryStore::default();
        let model = || {
            Config::load_from(&path)
                .provider("ollama")
                .unwrap()
                .model
                .clone()
        };
        let choose = |extra: Value, models: &[&str]| {
            let mut change = json!({"set": "provider", "task": "writing", "value": "ollama"});
            if let Some(m) = extra.as_str() {
                change["model"] = json!(m);
            }
            let local = Local {
                ollama_running: true,
                ollama: models.iter().map(|m| m.to_string()).collect(),
                speech_ready: false,
            };
            apply(&change, &path, &store, &local).unwrap();
        };
        choose(Value::Null, &[]);
        assert_eq!(model().as_deref(), Some("qwen3:8b"));
        choose(Value::Null, &["qwen3:4b", "gemma3:4b"]);
        assert_eq!(model().as_deref(), Some("qwen3:4b"));
        assert_eq!(Config::load_from(&path).chat.chain, ["ollama"]);
        choose(Value::Null, &["gemma3:4b", "qwen3:4b"]);
        assert_eq!(model().as_deref(), Some("qwen3:4b"));
        choose(json!("gemma3:4b"), &["qwen3:4b", "gemma3:4b"]);
        assert_eq!(model().as_deref(), Some("gemma3:4b"));
        choose(json!("made-up"), &["qwen3:4b", "gemma3:4b"]);
        assert_eq!(model().as_deref(), Some("gemma3:4b"));
    }

    #[test]
    fn removed_writing_providers_cannot_be_selected_or_change_stored_keys() {
        let (_d, path) = setup("[chat]\nchain = [\"ollama\"]\n");
        let store = MemoryStore::default();
        store.set("openrouter", "existing-secret").unwrap();
        for provider in [
            "openai",
            "anthropic",
            "openrouter",
            "gemini",
            "xai",
            "agy",
            "copilot",
        ] {
            assert!(apply(
                &json!({"set": "provider", "task": "writing", "value": provider}),
                &path,
                &store,
                &Local::default()
            )
            .is_err());
        }
        assert!(apply(
            &json!({"set": "key", "account": "openrouter", "value": "replacement"}),
            &path,
            &store,
            &Local::default()
        )
        .is_err());
        assert_eq!(
            store.get("openrouter").unwrap().unwrap().as_str(),
            "existing-secret"
        );
        assert_eq!(Config::load_from(&path).chat.chain, ["ollama"]);
    }

    #[test]
    fn cloud_speech_keys_cannot_be_added_or_changed_from_the_page() {
        let (_dir, path) = setup("");
        let store = MemoryStore::default();
        for account in [
            "openai",
            "gemini",
            "xai",
            "openai_whisper",
            "gemini_speech",
            "xai_speech",
        ] {
            for value in [json!("fake-key"), serde_json::Value::Null] {
                assert!(apply(
                    &json!({"set":"key", "account":account, "value":value}),
                    &path,
                    &store,
                    &Local::default()
                )
                .is_err());
            }
            assert!(!store.has(account));
        }
    }

    #[test]
    fn a_settings_change_keeps_sections_it_does_not_know() {
        let legacy = "[chat]\nchain = [\"ollama\"]\n[sync]\nauto_push = \"when_idle\"\nidle_secs = 1\n[theme]\npreset = \"coral\"\n[calendar]\nkept = true\n";
        let (_d, path) = setup(legacy);
        let store = MemoryStore::default();
        let cfg = Config::parse_with_built_ins(legacy).unwrap();
        assert_eq!(cfg.chat.chain, ["ollama"]);
        apply(
            &json!({"set": "provider", "task": "writing", "value": "claude_code"}),
            &path,
            &store,
            &Local::default(),
        )
        .unwrap();
        let retained = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(retained["sync"]["auto_push"].as_str(), Some("when_idle"));
        assert_eq!(retained["theme"]["preset"].as_str(), Some("coral"));
        assert_eq!(retained["calendar"]["kept"].as_bool(), Some(true));
        assert!(!Config::default_toml().contains("[sync]"));
    }

    #[test]
    fn a_missing_config_file_is_created_with_its_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("leo/config.toml");
        apply(
            &json!({"set": "provider", "task": "writing", "value": "claude_code"}),
            &path,
            &MemoryStore::default(),
            &Local::default(),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# leo configuration."));
        assert_eq!(Config::load_from(&path).chat.chain, ["claude_code"]);
    }
}

/// The writing AI as the menu bar shows it: "Claude Code · claude-opus-5-5",
/// and its plan's 5 hour and 7 day use when the agent has reported them.
/// Claude Code reports its use with every answer; Codex has to be asked
/// through its app-server, so `may_ask` lets the caller do that only now and
/// then, and only when Codex is the chosen writing AI and what is saved is stale.
pub fn writing_status(may_ask: bool) -> (String, Option<String>) {
    let cfg = Config::load();
    let store = secret::default_store();
    let selection = choice::selection(&cfg, store.as_ref(), Task::Chat);
    let now = chrono::Utc::now();
    let mut seen = crate::usage::load();
    if let Some(selection) = selection.as_ref() {
        let codex = cfg
            .provider(&selection.provider)
            .and_then(Agent::of)
            .is_some_and(|agent| agent == Agent::Codex);
        let stale = seen.get(&selection.provider).is_none_or(|usage| {
            (now - usage.seen_at).num_seconds() >= crate::usage::STALE_AFTER.as_secs() as i64
        });
        if may_ask && codex && stale {
            crate::usage::refresh_codex(&cfg);
            seen = crate::usage::load();
        }
    }
    status_line(selection.as_ref(), &seen, now)
}

fn status_line(
    selection: Option<&choice::Selection>,
    seen: &crate::usage::Seen,
    now: chrono::DateTime<chrono::Utc>,
) -> (String, Option<String>) {
    let Some(selection) = selection else {
        return ("No writing AI chosen".into(), None);
    };
    let name = choice::find(Task::Chat, &selection.provider)
        .map(|c| c.name)
        .unwrap_or(selection.provider.as_str());
    let label = match selection.model.as_deref().filter(|m| !m.is_empty()) {
        Some(model) => format!("{name} · {model}"),
        None => name.to_string(),
    };
    let limits = seen
        .get(&selection.provider)
        .map(|usage| crate::usage::label(usage, now))
        .filter(|l| !l.is_empty());
    (label, limits)
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn the_menu_names_the_writing_ai_its_model_and_known_limits() {
        let now = chrono::Utc::now();
        let selection = choice::Selection {
            provider: "claude_code".into(),
            choice: None,
            model: Some("claude-opus-5-5".into()),
            ready: true,
        };
        let mut seen = crate::usage::Seen::new();
        assert_eq!(
            status_line(Some(&selection), &seen, now),
            ("Claude Code · claude-opus-5-5".into(), None)
        );
        seen.insert(
            "claude_code".into(),
            crate::usage::Usage {
                five_hour: Some(crate::usage::Window {
                    used: 0.12,
                    resets_at: None,
                }),
                seven_day: Some(crate::usage::Window {
                    used: 0.4,
                    resets_at: None,
                }),
                seen_at: now,
            },
        );
        assert_eq!(
            status_line(Some(&selection), &seen, now).1.as_deref(),
            Some("5h: 12%, 7d: 40%")
        );
        assert_eq!(status_line(None, &seen, now).0, "No writing AI chosen");
    }
}
