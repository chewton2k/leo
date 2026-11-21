use std::io::Write;
use std::process::Stdio;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::process::{self, Process, Watchdog, MOST_OUTPUT, QUEUED_MESSAGES};
use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::agent_cli::{codex_disables, codex_scope, locate, Agent, AgentCli};
use crate::ai::provider::{Sink, Spent};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

pub struct CodexThread {
    name: String,
    child: Process,
    stdin: Option<std::process::ChildStdin>,
    messages: Receiver<std::io::Result<Value>>,
    thread: String,
    next: u64,
    quiet: Duration,
    total_limit: Duration,
    model: Option<String>,
    effort: Option<String>,
    total: Tokens,
    _room: tempfile::TempDir,
}

pub struct CodexTurn<'a> {
    pub sink: Sink<'a>,
    pub restart: &'a mut dyn FnMut(),
    pub call: &'a mut dyn FnMut(&str, &Value) -> String,
    pub stopped: &'a dyn Fn() -> bool,
    pub tools_done: &'a dyn Fn() -> bool,
    pub most_calls: usize,
}

pub fn tool_specs(tools: &[(String, String, Value)]) -> Vec<Value> {
    tools
        .iter()
        .map(|(name, description, schema)| {
            json!({ "type": "function", "name": name, "description": description, "inputSchema": schema })
        })
        .collect()
}

pub fn tokens_of(usage: &Value) -> Tokens {
    let total = &usage["total"];
    Tokens {
        input: total["inputTokens"].as_u64().unwrap_or(0),
        cached: total["cachedInputTokens"].as_u64().unwrap_or(0),
        output: total["outputTokens"].as_u64().unwrap_or(0)
            + total["reasoningOutputTokens"].as_u64().unwrap_or(0),
    }
}

pub fn declined(method: &str) -> Value {
    match method {
        "execCommandApproval" | "applyPatchApproval" => json!({ "decision": "denied" }),
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({ "decision": "decline" })
        }
        "item/tool/requestUserInput" => json!({ "answers": {} }),
        _ => json!({}),
    }
}

static NATIVE_PROBLEM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn note_native(problem: Option<String>) {
    if let Ok(mut held) = NATIVE_PROBLEM.lock() {
        *held = problem;
    }
}

pub fn native_problem() -> Option<String> {
    NATIVE_PROBLEM.lock().ok().and_then(|held| held.clone())
}

impl CodexThread {
    pub fn start(agent: &AgentCli, system: &str, tools: &[Value]) -> ProviderResult<CodexThread> {
        let name = agent.provider_name().to_string();
        let fail = |what: String| ProviderError::Retryable(format!("{name}: {what}"));
        let program = locate(agent.bin())
            .ok_or_else(|| ProviderError::Retryable(Agent::Codex.missing(&name, agent.bin())))?;
        let room = agent.room()?;
        let mut command = leo_core::children::quiet_command(&program);
        super::scoped_cli::plan_environment(&mut command);
        command.arg("app-server");
        command.args(codex_scope(&program, "app-server", room.path())?);
        for feature in codex_disables(&program)? {
            command.args(["-c", &format!("features.{feature}=false")]);
        }
        command
            .current_dir(room.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|e| fail(format!("could not run {}: {e}", agent.bin())))?;
        leo_core::children::mark(child.id(), "Codex");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let child = Process::new(child);
        let stdout = stdout.ok_or_else(|| fail("could not read codex".into()))?;
        let (tx, messages) = std::sync::mpsc::sync_channel(QUEUED_MESSAGES);
        process::lines(stdout, tx, |line| match line {
            Ok(Some(line)) => serde_json::from_str::<Value>(&line).ok().map(Ok),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        });
        let mut thread = CodexThread {
            name: name.clone(),
            child,
            stdin,
            messages,
            thread: String::new(),
            next: 1,
            quiet: agent.quiet_limit(),
            total_limit: agent.total_limit(),
            model: agent.model().map(str::to_string),
            effort: agent.effort().map(str::to_string),
            total: Tokens::default(),
            _room: room,
        };
        thread.request(
            "initialize",
            json!({
                "clientInfo": { "name": "leo", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true },
            }),
        )?;
        thread.notify("initialized")?;
        let mut params = json!({
            "cwd": thread._room.path(),
            "sandbox": "read-only",
            "approvalPolicy": "never",
            "ephemeral": true,
            "developerInstructions": system,
            "dynamicTools": tools,
        });
        if let Some(model) = &thread.model {
            params["model"] = json!(model);
        }
        let started = thread.request("thread/start", params)?;
        thread.thread = started["thread"]["id"]
            .as_str()
            .ok_or_else(|| fail("codex did not start a conversation".into()))?
            .to_string();
        if let Some(model) = started["model"]
            .as_str()
            .or_else(|| started["thread"]["model"].as_str())
        {
            thread.model = Some(model.to_string());
        }
        if thread.effort.is_none() {
            thread.effort = started["reasoningEffort"].as_str().map(str::to_string);
        }
        Ok(thread)
    }

    fn send(&mut self, message: Value) -> ProviderResult<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| ProviderError::Retryable(format!("{}: codex has stopped", self.name)))?;
        writeln!(stdin, "{message}")
            .and_then(|_| stdin.flush())
            .map_err(|e| ProviderError::Retryable(format!("{}: codex has stopped: {e}", self.name)))
    }

    fn notify(&mut self, method: &str) -> ProviderResult<()> {
        self.send(json!({ "jsonrpc": "2.0", "method": method }))
    }

    fn next_message(
        &mut self,
        until: Instant,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<Value> {
        let idle = Instant::now();
        loop {
            if stopped() {
                self.child.stop();
                return Err(ProviderError::Retryable("The answer was stopped.".into()));
            }
            if Instant::now() >= until {
                self.child.stop();
                return Err(ProviderError::Retryable(format!(
                    "{}: codex stopped answering and was stopped",
                    self.name
                )));
            }
            let wait = self
                .quiet
                .saturating_sub(idle.elapsed())
                .min(until.saturating_duration_since(Instant::now()))
                .min(Duration::from_millis(50));
            match self.messages.recv_timeout(wait) {
                Ok(Ok(message)) => return Ok(message),
                Ok(Err(_)) => {
                    self.child.stop();
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex sent unreadable or excessive output. Try a smaller request.",
                        self.name
                    )));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if idle.elapsed() < self.quiet && Instant::now() < until {
                        continue;
                    }
                    self.child.stop();
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex stopped answering and was stopped",
                        self.name
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex ended. If it is not signed in, run `codex login`.",
                        self.name
                    )))
                }
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> ProviderResult<Value> {
        let watch = Watchdog::start(&self.child, Some(self.quiet), self.total_limit);
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let until = Instant::now() + self.total_limit;
        let mut bytes = 0usize;
        loop {
            let message = self.next_message(until, &|| false)?;
            watch.touch();
            bytes = bytes.saturating_add(message.to_string().len());
            if bytes > MOST_OUTPUT {
                self.child.stop();
                return Err(ProviderError::Retryable(format!(
                    "{}: codex sent too much output. Try a smaller request.",
                    self.name
                )));
            }
            if message["id"] == id && message.get("method").is_none() {
                if let Some(error) = message.get("error") {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex refused {method}: {}",
                        self.name,
                        error["message"].as_str().unwrap_or("unknown error")
                    )));
                }
                return Ok(message["result"].clone());
            }
            self.answer_server(&message, &mut |_, _| String::new())?;
        }
    }

    fn answer_server(
        &mut self,
        message: &Value,
        call: &mut dyn FnMut(&str, &Value) -> String,
    ) -> ProviderResult<bool> {
        let (Some(id), Some(method)) = (message.get("id"), message["method"].as_str()) else {
            return Ok(false);
        };
        let result = if method == "item/tool/call" {
            let params = &message["params"];
            let text = call(params["tool"].as_str().unwrap_or(""), &params["arguments"]);
            json!({ "contentItems": [{ "type": "inputText", "text": text }], "success": true })
        } else {
            declined(method)
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))?;
        Ok(method == "item/tool/call")
    }

    pub fn say(&mut self, text: &str, turn: CodexTurn<'_>) -> ProviderResult<(String, Spent)> {
        for _ in 0..QUEUED_MESSAGES {
            let Ok(queued) = self.messages.try_recv() else {
                break;
            };
            if queued.is_err() {
                self.child.stop();
                return Err(ProviderError::Retryable(format!(
                    "{}: codex sent unreadable or excessive output. Try a smaller request.",
                    self.name
                )));
            }
        }
        let watch = Watchdog::start(&self.child, Some(self.quiet), self.total_limit);
        let CodexTurn {
            sink,
            restart,
            call,
            stopped,
            tools_done,
            most_calls,
        } = turn;
        let id = self.next;
        self.next += 1;
        let mut params = json!({
            "threadId": self.thread,
            "input": [{ "type": "text", "text": text }],
        });
        if let Some(effort) = &self.effort {
            params["effort"] = json!(effort);
        }
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": "turn/start", "params": params }))?;
        let before = self.total.clone();
        let until = Instant::now() + self.total_limit;
        let mut answer = String::new();
        let mut bytes = 0usize;
        let mut calls = 0usize;
        let mut limited = false;
        loop {
            let message = self.next_message(until, stopped)?;
            watch.touch();
            bytes = bytes.saturating_add(message.to_string().len());
            if bytes > MOST_OUTPUT {
                self.child.stop();
                return Err(ProviderError::Retryable(format!(
                    "{}: codex sent too much output. Try a smaller request.",
                    self.name
                )));
            }
            if message["id"] == id && message.get("method").is_none() {
                if let Some(error) = message.get("error") {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex refused the message: {}",
                        self.name,
                        error["message"].as_str().unwrap_or("unknown error")
                    )));
                }
                continue;
            }
            if message.get("id").is_some() {
                let tool = message["method"] == "item/tool/call";
                if tool && limited {
                    self.child.stop();
                    return Err(ProviderError::Retryable("The writing program kept requesting tools after its limit. Try a smaller request.".into()));
                }
                if tool && !answer.trim().is_empty() {
                    restart();
                    answer.clear();
                }
                if tool && (calls >= most_calls || tools_done()) {
                    limited = true;
                    self.answer_server(&message, &mut |_, _| super::NO_MORE_TOOLS.into())?;
                } else {
                    if tool {
                        calls += 1;
                    }
                    self.answer_server(&message, call)?;
                }
                continue;
            }
            let params = &message["params"];
            match message["method"].as_str().unwrap_or("") {
                "item/agentMessage/delta" => {
                    if let Some(delta) = params["delta"].as_str() {
                        answer.push_str(delta);
                        sink(delta);
                    }
                }
                "thread/tokenUsage/updated" => self.total = tokens_of(&params["tokenUsage"]),
                "model/rerouted" => {
                    if let Some(model) = params["toModel"]
                        .as_str()
                        .or_else(|| params["model"].as_str())
                    {
                        self.model = Some(model.to_string());
                    }
                }
                "error" if params["willRetry"] == true => {}
                "error" => {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex failed: {}",
                        self.name,
                        params["error"]["message"]
                            .as_str()
                            .or_else(|| params["message"].as_str())
                            .unwrap_or("unknown error")
                    )));
                }
                "turn/completed" if params["threadId"] == self.thread.as_str() => {
                    if let Some(error) = params["turn"]["error"]["message"].as_str() {
                        return Err(ProviderError::Retryable(format!(
                            "{}: codex failed: {error}",
                            self.name
                        )));
                    }
                    break;
                }
                _ => {}
            }
        }
        let used = Tokens {
            input: self.total.input.saturating_sub(before.input),
            cached: self.total.cached.saturating_sub(before.cached),
            output: self.total.output.saturating_sub(before.output),
        };
        let estimated = used == Tokens::default();
        Ok((
            answer.trim().to_string(),
            Spent {
                model: self.model.clone(),
                effort: self.effort.clone(),
                input: if estimated {
                    super::tokens_of(text.chars().count())
                } else {
                    used.input
                },
                output: if estimated {
                    super::tokens_of(answer.chars().count())
                } else {
                    used.output
                },
                estimated,
                cached: used.cached,
            },
        ))
    }
}

impl Drop for CodexThread {
    fn drop(&mut self) {
        self.stdin.take();
        self.child.stop();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::provider::{ProviderConfig, ProviderKind};

    fn fake(dir: &std::path::Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("sent.log");
        let path = dir.join("codex");
        let body = format!(
            r##"#!/bin/sh
test -z "${{CI+x}}${{ANTHROPIC_API_KEY+x}}${{OPENAI_API_KEY+x}}${{CODEX_API_KEY+x}}${{NODE_OPTIONS+x}}${{BASH_ENV+x}}" || exit 17
if [ "$1" = "features" ]; then echo 'shell_tool stable true'; exit 0; fi
if [ "$2" = "--help" ]; then echo '--ignore-user-config --ignore-rules'; exit 0; fi
printf '%s\n' "$@" > "{log}.args"
while IFS= read -r line; do
  echo "$line" >> "{log}"
  case "$line" in
    *'"method":"initialize"'*) printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{}}}}' ;;
    *'"method":"thread/start"'*) printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"t1"}},"model":"gpt-6.1-sol","reasoningEffort":"medium"}}}}' ;;
    *'"method":"turn/start"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"u1"}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"delta":"Let me look. "}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":90,"method":"item/tool/call","params":{{"tool":"search_notes","arguments":{{"query":"heap"}},"callId":"c1","threadId":"t1","turnId":"u1"}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":91,"method":"item/commandExecution/requestApproval","params":{{}}}}' ;;
    *'"id":91'*)
      printf '%s\n' '{{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{{"threadId":"t1","turnId":"u1","tokenUsage":{{"total":{{"inputTokens":1200,"cachedInputTokens":800,"outputTokens":40,"reasoningOutputTokens":10,"totalTokens":1250}},"last":{{}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"delta":"Heaps keep the minimum on top."}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"t1","turn":{{"id":"u1"}}}}}}' ;;
  esac
done
"##,
            log = log.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn codex_keeps_one_conversation_and_calls_leos_tools_natively() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = fake(dir.path());
        let quiet = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(bin.clone()),
            ..ProviderConfig::default()
        };
        let default_effort = CodexThread::start(
            &AgentCli::new("codex".into(), Agent::Codex, &quiet),
            "x",
            &[],
        )
        .unwrap();
        assert_eq!(
            default_effort.effort.as_deref(),
            Some("medium"),
            "Codex says its own effort"
        );
        drop(default_effort);
        std::fs::remove_file(dir.path().join("sent.log")).unwrap();
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(bin),
            effort: Some("high".into()),
            ..ProviderConfig::default()
        };
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let tools = tool_specs(&[(
            "search_notes".into(),
            "Find notes.".into(),
            json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
        )]);
        let mut thread = CodexThread::start(&agent, "You are Felix.", &tools).unwrap();
        let mut shown = String::new();
        let mut restarts = 0;
        let mut asked = Vec::new();
        let (answer, spent) = thread
            .say(
                "what is a heap?",
                CodexTurn {
                    stopped: &|| false,
                    tools_done: &|| false,
                    most_calls: 12,
                    sink: &mut |t| shown.push_str(t),
                    restart: &mut || restarts += 1,
                    call: &mut |name, args| {
                        asked.push(format!("{name} {}", args["query"]));
                        "[n1] \"Heaps\"".into()
                    },
                },
            )
            .unwrap();
        assert_eq!(answer, "Heaps keep the minimum on top.");
        assert_eq!(asked, ["search_notes \"heap\""]);
        assert_eq!(restarts, 1, "text before a tool call is taken back");
        assert_eq!(
            (
                spent.model.as_deref(),
                spent.effort.as_deref(),
                spent.input,
                spent.output,
                spent.estimated
            ),
            (Some("gpt-6.1-sol"), Some("high"), 1200, 50, false)
        );
        let sent = std::fs::read_to_string(dir.path().join("sent.log")).unwrap();
        let args = std::fs::read_to_string(dir.path().join("sent.log.args")).unwrap();
        assert!(args.lines().any(|a| a == "--ignore-user-config"));
        assert!(args.lines().any(|a| a == "--ignore-rules"));
        let lines: Vec<Value> = sent
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["params"]["capabilities"]["experimentalApi"], true);
        let start = &lines[2]["params"];
        assert_eq!(
            (
                start["sandbox"].as_str(),
                start["approvalPolicy"].as_str(),
                start["ephemeral"].as_bool()
            ),
            (Some("read-only"), Some("never"), Some(true))
        );
        assert_eq!(start["developerInstructions"], "You are Felix.");
        assert_eq!(start["dynamicTools"][0]["name"], "search_notes");
        assert_eq!(lines[3]["params"]["effort"], "high");
        let tool_reply = lines.iter().find(|l| l["id"] == 90).unwrap();
        assert_eq!(
            tool_reply["result"]["contentItems"][0]["text"],
            "[n1] \"Heaps\""
        );
        let approval = lines.iter().find(|l| l["id"] == 91).unwrap();
        assert_eq!(approval["result"]["decision"], "decline");
    }

    #[test]
    fn a_codex_error_it_will_retry_does_not_end_the_turn() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = fake(dir.path());
        let completed = r#"      printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed""#;
        let retrying = r#"      printf '%s\n' '{"jsonrpc":"2.0","method":"error","params":{"error":{"message":"Reconnecting... 1/5"},"willRetry":true}}'"#;
        let body = std::fs::read_to_string(&bin).unwrap();
        assert!(body.contains(completed));
        std::fs::write(
            &bin,
            body.replacen(completed, &format!("{retrying}\n{completed}"), 1),
        )
        .unwrap();
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(bin),
            ..ProviderConfig::default()
        };
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let mut thread = CodexThread::start(&agent, "x", &[]).unwrap();
        let (answer, _) = thread
            .say(
                "what is a heap?",
                CodexTurn {
                    stopped: &|| false,
                    tools_done: &|| false,
                    most_calls: 12,
                    sink: &mut |_| {},
                    restart: &mut || {},
                    call: &mut |_, _| String::new(),
                },
            )
            .unwrap();
        assert_eq!(answer, "Heaps keep the minimum on top.");
    }

    fn fallback_agent(dir: &std::path::Path, listing: &str) -> AgentCli {
        let bin = fake(dir);
        let body = std::fs::read_to_string(&bin)
            .unwrap()
            .replace(
                "echo '--ignore-user-config --ignore-rules'; exit 0",
                "echo 'Usage: codex app-server'; exit 0",
            )
            .replace(
                "printf '%s\\n' \"$@\" >",
                &format!("case \"$*\" in *'mcp list'*) {listing}; exit $?;; esac\nprintf '%s\\n' \"$@\" >"),
            );
        std::fs::write(&bin, body).unwrap();
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(bin),
            ..ProviderConfig::default()
        };
        AgentCli::new("codex".into(), Agent::Codex, &cfg)
    }

    #[test]
    fn a_codex_without_isolation_flags_is_scoped_by_switching_its_tool_servers_off() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent = fallback_agent(
            dir.path(),
            r#"echo '[{"name":"srv","enabled":true},{"name":"off","enabled":false}]'"#,
        );
        CodexThread::start(&agent, "x", &[]).unwrap();
        let args = std::fs::read_to_string(dir.path().join("sent.log.args")).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(lines[0], "app-server");
        assert!(!lines.contains(&"--ignore-user-config"));
        assert!(lines.contains(&"mcp_servers.srv.enabled=false"));
        assert!(lines.contains(&"mcp_servers.srv.command=\"false\""));
        assert!(!lines.contains(&"mcp_servers.off.enabled=false"));
        assert!(lines.contains(&"notify=[]"));
        assert!(lines.contains(&"web_search=\"cached\""));
    }

    #[test]
    fn a_codex_whose_tool_servers_cannot_be_listed_never_starts_a_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent = fallback_agent(dir.path(), "exit 3");
        assert!(CodexThread::start(&agent, "x", &[]).is_err());
        assert!(!dir.path().join("sent.log.args").exists());
    }

    #[test]
    #[ignore = "talks to the real Codex on this computer"]
    fn real_codex_uses_a_dynamic_tool() {
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            ..ProviderConfig::default()
        };
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let tools = tool_specs(&[(
            "lookup".into(),
            "Look up the secret word. Returns it.".into(),
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        )]);
        let mut thread =
            CodexThread::start(&agent, "Answer in one short sentence.", &tools).unwrap();
        let mut calls = 0;
        let (answer, spent) = thread
            .say(
                "Use the lookup tool, then tell me the secret word.",
                CodexTurn {
                    stopped: &|| false,
                    tools_done: &|| false,
                    most_calls: 12,
                    sink: &mut |_| {},
                    restart: &mut || {},
                    call: &mut |_, _| {
                        calls += 1;
                        "The secret word is maple.".into()
                    },
                },
            )
            .unwrap();
        println!("answer: {answer}\nspent: {spent:?}\ncalls: {calls}");
        assert!(calls >= 1);
        assert!(answer.to_lowercase().contains("maple"));
        let (again, _) = thread
            .say(
                "What was the secret word again? One word.",
                CodexTurn {
                    stopped: &|| false,
                    tools_done: &|| false,
                    most_calls: 12,
                    sink: &mut |_| {},
                    restart: &mut || {},
                    call: &mut |_, _| "unused".into(),
                },
            )
            .unwrap();
        println!("again: {again}");
        assert!(
            again.to_lowercase().contains("maple"),
            "the thread remembers"
        );
    }
}
