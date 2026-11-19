use anyhow::Result;

use serde_json::{json, Value};

use crate::ai::provider::agent_cli::{Agent, AgentCli, ClaudeSession};
use crate::ai::provider::codex_app::{self, tool_specs, CodexThread, CodexTurn};
use crate::ai::provider::mcp;
use crate::ai::provider::openai::{self, OpenAiChat};
use crate::ai::provider::{tokens_of, Spent, NO_MORE_TOOLS};
use crate::ai::provider::{ChatProvider, Sink};
use crate::ai::spend::{answered, Answered};
use crate::config::provider::ProviderKind;
use crate::config::secret::SecretStore;
use crate::config::Config;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

pub struct Turn<'a> {
    pub tail: &'a str,
    pub max_tokens: u32,
    pub most_calls: usize,
    pub sink: Sink<'a>,
    pub restart: &'a mut dyn FnMut(),
    pub call: &'a mut dyn FnMut(&str, &serde_json::Value) -> String,
    pub stopped: &'a dyn Fn() -> bool,
    pub tools_done: &'a dyn Fn() -> bool,
}

pub trait Session: Send {
    fn native(&self) -> bool;
    fn context_chars(&self) -> Option<usize> {
        None
    }
    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)>;
}

pub struct Systems<'a> {
    pub native: &'a str,
    pub text: &'a str,
}

struct ClaudeTalk {
    session: ClaudeSession,
    provider: String,
    config: Config,
    sent: usize,
}

impl Session for ClaudeTalk {
    fn native(&self) -> bool {
        self.session.native()
    }

    fn context_chars(&self) -> Option<usize> {
        Some(self.sent)
    }

    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)> {
        let message = if turn.tail.is_empty() {
            text.to_string()
        } else {
            format!("{text}\n\n{}", turn.tail)
        };
        let mut results = 0usize;
        let mut call = |name: &str, args: &Value| {
            let result = (turn.call)(name, args);
            results = results
                .saturating_add(name.len())
                .saturating_add(args.to_string().len())
                .saturating_add(result.len());
            result
        };
        let (answer, spent) = self
            .session
            .say_controlled(
                &message,
                turn.sink,
                &mut call,
                turn.restart,
                super::provider::agent_cli::Control {
                    stopped: turn.stopped,
                    tools_done: turn.tools_done,
                    most_calls: turn.most_calls,
                },
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.sent = self
            .sent
            .saturating_add(message.len())
            .saturating_add(results)
            .saturating_add(answer.len());
        Ok((answer, Some(answered(&self.config, &self.provider, spent))))
    }
}

struct CodexTalk {
    thread: CodexThread,
    provider: String,
    config: Config,
    sent: usize,
}

impl Session for CodexTalk {
    fn native(&self) -> bool {
        true
    }

    fn context_chars(&self) -> Option<usize> {
        Some(self.sent)
    }

    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)> {
        let mut results = 0usize;
        let mut call = |name: &str, args: &Value| {
            let result = (turn.call)(name, args);
            results = results
                .saturating_add(name.len())
                .saturating_add(args.to_string().len())
                .saturating_add(result.len());
            result
        };
        let (answer, spent) = self
            .thread
            .say(
                text,
                CodexTurn {
                    sink: turn.sink,
                    restart: turn.restart,
                    call: &mut call,
                    stopped: turn.stopped,
                    tools_done: turn.tools_done,
                    most_calls: turn.most_calls,
                },
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.sent = self
            .sent
            .saturating_add(text.len())
            .saturating_add(results)
            .saturating_add(answer.len());
        Ok((answer, Some(answered(&self.config, &self.provider, spent))))
    }
}

struct ApiTalk {
    chat: OpenAiChat,
    provider: String,
    config: Config,
    system: String,
    tools: Vec<Value>,
    history: Vec<Value>,
    sent: usize,
    max_tokens: u32,
}

fn unreadable(args: &str) -> Value {
    serde_json::from_str::<Value>(args)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "__unreadable": args }))
}

fn result_for(call: &mut dyn FnMut(&str, &Value) -> String, name: &str, args: &Value) -> String {
    match args.get("__unreadable") {
        Some(given) => format!(
            "That did not work: the arguments were not a JSON object ({}). Call {name} again with valid JSON.",
            given.as_str().unwrap_or("")
        ),
        None => call(name, args),
    }
}

impl ApiTalk {
    fn new(
        chat: OpenAiChat,
        provider: &str,
        config: &Config,
        system: &str,
        tools: &[ToolDef],
    ) -> ApiTalk {
        let tools = tools
            .iter()
            .map(|t| openai::function_of(&t.name, &t.description, &t.schema))
            .collect();
        let history = vec![json!({ "role": "system", "content": system })];
        let sent = history[0].to_string().chars().count();
        let max_tokens = ChatProvider::max_tokens(&chat).unwrap_or(0);
        ApiTalk {
            chat,
            provider: provider.to_string(),
            config: config.clone(),
            system: system.to_string(),
            tools,
            history,
            sent,
            max_tokens,
        }
    }

    fn push(&mut self, message: Value) {
        self.sent += message.to_string().chars().count();
        self.history.push(message);
    }

    fn model_and_effort(&self) -> (String, Option<String>) {
        (
            self.chat.model().to_string(),
            self.chat.effort().map(str::to_string),
        )
    }
}

impl Session for ApiTalk {
    fn native(&self) -> bool {
        true
    }

    fn context_chars(&self) -> Option<usize> {
        Some(self.system.chars().count().saturating_add(self.sent))
    }

    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)> {
        let Turn {
            tail,
            max_tokens,
            most_calls,
            sink,
            restart,
            call,
            stopped,
            tools_done,
        } = turn;
        let message = if tail.is_empty() {
            text.to_string()
        } else {
            format!("{text}\n\n{tail}")
        };
        self.push(json!({ "role": "user", "content": message }));
        let max_tokens = max_tokens.max(self.max_tokens);
        let (model, effort) = self.model_and_effort();
        let mut spent = Spent {
            model: Some(model),
            effort,
            ..Spent::default()
        };
        let mut sent_chars = self.system.chars().count() + self.sent;
        let mut calls = 0usize;
        for _ in 0..=most_calls {
            anyhow::ensure!(!stopped(), "The answer was stopped.");
            let no_tools = calls >= most_calls || tools_done();
            let (answer, asked) = {
                let body = self
                    .chat
                    .tool_body(&self.history, &self.tools, max_tokens, no_tools);
                let round = self
                    .chat
                    .stream_round(&body, sink, stopped)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                match round.usage {
                    Some((input, output, cached)) => {
                        spent.input += input;
                        spent.output += output;
                        spent.cached += cached;
                    }
                    None => {
                        spent.estimated = true;
                        spent.input += tokens_of(sent_chars);
                        spent.output += tokens_of(
                            round.text.chars().count()
                                + round
                                    .calls
                                    .iter()
                                    .map(|c| c.2.chars().count())
                                    .sum::<usize>(),
                        );
                    }
                }
                if round.calls.is_empty() {
                    let text = if round.text.trim().is_empty() {
                        sink(&round.reasoning);
                        round.reasoning
                    } else {
                        round.text
                    };
                    self.push(json!({ "role": "assistant", "content": text }));
                    (text, Vec::new())
                } else {
                    let listed: Vec<Value> = round
                    .calls
                    .iter()
                    .map(|(id, name, args)| {
                        json!({ "id": id, "type": "function", "function": { "name": name, "arguments": args } })
                    })
                    .collect();
                    self.push(json!({
                    "role": "assistant",
                    "content": if round.text.is_empty() { Value::Null } else { Value::String(round.text.clone()) },
                    "tool_calls": listed,
                }));
                    let asked: Vec<(String, String, Value)> = round
                        .calls
                        .into_iter()
                        .map(|(id, name, args)| (id, name, unreadable(&args)))
                        .collect();
                    (round.text, asked)
                }
            };
            if asked.is_empty() {
                if answer.trim().is_empty() {
                    anyhow::bail!("{} returned no answer", self.provider);
                }
                return Ok((answer, Some(answered(&self.config, &self.provider, spent))));
            }
            if !answer.is_empty() {
                restart();
            }
            let mut results = Vec::new();
            for (id, name, args) in &asked {
                anyhow::ensure!(!stopped(), "The answer was stopped.");
                if calls >= most_calls || tools_done() {
                    results.push((id.clone(), NO_MORE_TOOLS.to_string()));
                    continue;
                }
                calls += 1;
                let result = result_for(call, name, args);
                sent_chars += result.chars().count();
                results.push((id.clone(), result));
            }
            for (id, result) in results {
                self.push(json!({ "role": "tool", "tool_call_id": id, "content": result }));
            }
        }
        anyhow::bail!(
            "{} kept requesting tools after its limit. Try a smaller request.",
            self.provider
        )
    }
}

pub fn open(
    config: &Config,
    store: &dyn SecretStore,
    systems: &Systems,
    tools: &[ToolDef],
) -> Option<Box<dyn Session>> {
    let schemas = tools
        .iter()
        .map(|tool| tool.name.len() + tool.description.len() + tool.schema.to_string().len())
        .sum::<usize>();
    for name in &config.chat.chain {
        let Some(provider) = config
            .provider(name)
            .cloned()
            .or_else(|| Config::built_in_provider(name))
        else {
            continue;
        };
        match provider.kind {
            Some(ProviderKind::ClaudeCode) => {
                let agent = AgentCli::new(name.clone(), Agent::ClaudeCode, &provider);
                if !agent.available() {
                    continue;
                }
                let offered: Vec<mcp::Tool> = tools
                    .iter()
                    .map(|t| mcp::Tool {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        schema: t.schema.clone(),
                    })
                    .collect();
                let started = if offered.is_empty() {
                    agent.claude_session(systems.text)
                } else {
                    agent
                        .claude_native(systems.native, offered)
                        .or_else(|_| agent.claude_session(systems.text))
                };
                let session = match started {
                    Ok(session) => session,
                    Err(e) => {
                        leo_core::diag::warn(format!("claude code native session: {e}"));
                        continue;
                    }
                };
                return Some(Box::new(ClaudeTalk {
                    sent: schemas
                        + if session.native() {
                            systems.native.len()
                        } else {
                            systems.text.len()
                        },
                    session,
                    provider: name.clone(),
                    config: config.clone(),
                }));
            }
            Some(ProviderKind::Codex) => {
                let agent = AgentCli::new(name.clone(), Agent::Codex, &provider);
                if !agent.available() {
                    continue;
                }
                let specs: Vec<(String, String, serde_json::Value)> = tools
                    .iter()
                    .map(|t| (t.name.clone(), t.description.clone(), t.schema.clone()))
                    .collect();
                match CodexThread::start(&agent, systems.native, &tool_specs(&specs)) {
                    Ok(thread) => {
                        codex_app::note_native(None);
                        return Some(Box::new(CodexTalk {
                            sent: schemas + systems.native.len(),
                            thread,
                            provider: name.clone(),
                            config: config.clone(),
                        }));
                    }
                    Err(e) => {
                        let reason = e.to_string();
                        leo_core::diag::warn(format!("codex native session: {reason}"));
                        codex_app::note_native(Some(reason));
                        continue;
                    }
                }
            }
            Some(ProviderKind::Openai) => {
                let key = match provider.key_env.as_deref() {
                    Some(_) => crate::config::secret::resolve(provider.account(name), store),
                    None => None,
                };
                let chat = OpenAiChat::new(name.clone(), &provider, key);
                if !chat.available() {
                    continue;
                }
                return Some(Box::new(ApiTalk::new(
                    chat,
                    name,
                    config,
                    systems.native,
                    tools,
                )));
            }
            _ => {
                let usable = crate::ai::provider::build_one_chat(name, &provider, store)
                    .is_some_and(|p| p.available());
                if usable {
                    return None;
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::provider::ProviderConfig;
    use crate::config::secret::{MemoryStore, SecretStore};
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    type Heard = Arc<Mutex<Vec<(String, Value)>>>;

    fn serve(answers: Vec<String>) -> (String, Heard) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&seen);
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut raw = Vec::new();
                let mut buffer = [0u8; 65536];
                let body_at = loop {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    raw.extend_from_slice(&buffer[..n]);
                    if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(at + 4);
                    }
                };
                let Some(body_at) = body_at else { return };
                let head = String::from_utf8_lossy(&raw[..body_at]).to_string();
                let length: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while raw.len() < body_at + length {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buffer[..n]);
                }
                let body =
                    serde_json::from_slice(&raw[body_at..body_at + length]).unwrap_or(Value::Null);
                heard.lock().unwrap().push((head, body));
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (format!("http://127.0.0.1:{port}/v1"), seen)
    }

    fn tools() -> Vec<ToolDef> {
        vec![ToolDef {
            name: "search_notes".into(),
            description: "Search the notes.".into(),
            schema: json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
        }]
    }

    struct Ran {
        answer: String,
        answered: Answered,
        shown: String,
        restarts: usize,
        calls: Vec<(String, Value)>,
    }

    fn run(talk: &mut dyn Session, most_calls: usize) -> Ran {
        let mut shown = String::new();
        let mut restarts = 0;
        let mut calls = Vec::new();
        let (answer, answered) = talk
            .say(
                "What is a heap?",
                Turn {
                    tail: "",
                    max_tokens: 16000,
                    most_calls,
                    stopped: &|| false,
                    tools_done: &|| false,
                    sink: &mut |t| shown.push_str(t),
                    restart: &mut || restarts += 1,
                    call: &mut |name, args| {
                        calls.push((name.to_string(), args.clone()));
                        "[n1] \"Heaps\": the minimum sits at the root.".to_string()
                    },
                },
            )
            .unwrap();
        Ran {
            answer,
            answered: answered.unwrap(),
            shown,
            restarts,
            calls,
        }
    }

    fn sse(events: &[&str]) -> String {
        events.iter().map(|e| format!("data: {e}\n\n")).collect()
    }

    #[test]
    fn an_openai_compatible_model_calls_leos_tools_natively_and_answers() {
        let (url, seen) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"content":"Checking."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{\"query\":\"heap\"}"}},{"index":1,"id":"c2","function":{"name":"search_notes","arguments":"not json"}}]}}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":3000,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":2048}}}"#,
                "[DONE]",
            ]),
            sse(&[
                r#"{"choices":[{"delta":{"content":"The minimum is at the root [n1]."}}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":3200,"completion_tokens":15,"prompt_tokens_details":{"cached_tokens":2944}}}"#,
                "[DONE]",
            ]),
        ]);
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Openai),
            base_url: Some(url.replace("/v1", "/api.openai.com/v1")),
            model: Some("gpt-5-mini".into()),
            ..ProviderConfig::default()
        };
        let chat = OpenAiChat::new("openai".into(), &cfg, None);
        let mut talk = ApiTalk::new(chat, "openai", &Config::default(), "Be Felix.", &tools());
        let ran = run(&mut talk, 12);
        assert_eq!(ran.answer, "The minimum is at the root [n1].");
        assert_eq!(ran.shown, "Checking.The minimum is at the root [n1].");
        assert_eq!(ran.restarts, 1, "words before a tool call are taken back");
        assert_eq!(
            ran.calls,
            [("search_notes".to_string(), json!({ "query": "heap" }))]
        );
        assert_eq!(
            (ran.answered.input, ran.answered.output, ran.answered.cached),
            (6200, 35, 4992)
        );
        let seen = seen.lock().unwrap();
        let first = &seen[0].1;
        assert_eq!(
            first["messages"][0],
            json!({ "role": "system", "content": "Be Felix." })
        );
        assert_eq!(first["tools"][0]["function"]["name"], "search_notes");
        let second = &seen[1].1["messages"];
        assert_eq!(second[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(second[3]["tool_call_id"], "c1");
        assert!(second[3]["content"]
            .as_str()
            .unwrap()
            .contains("minimum sits at the root"));
        assert_eq!(second[4]["tool_call_id"], "c2");
        assert!(second[4]["content"]
            .as_str()
            .unwrap()
            .starts_with("That did not work: the arguments were not a JSON object"));
    }

    #[cfg(unix)]
    #[test]
    fn a_claude_code_that_cannot_start_falls_through_to_the_next_writer() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "not a program").unwrap();
        let (url, _) = serve(Vec::new());
        let mut cfg = Config::default();
        cfg.chat.chain = vec!["claude_code".into(), "ollama".into()];
        cfg.providers.insert(
            "claude_code".into(),
            ProviderConfig {
                kind: Some(ProviderKind::ClaudeCode),
                bin: Some(bin.to_string_lossy().to_string()),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "ollama".into(),
            ProviderConfig {
                kind: Some(ProviderKind::Openai),
                base_url: Some(url),
                model: Some("llama3.2".into()),
                ..Default::default()
            },
        );
        let systems = Systems {
            native: "n",
            text: "t",
        };
        assert!(open(&cfg, &MemoryStore::default(), &systems, &[]).is_some());
        assert!(open(&cfg, &MemoryStore::default(), &systems, &tools()).is_some());
    }

    #[test]
    fn a_non_ascii_reply_is_estimated_by_characters() {
        let reply = "日".repeat(300);
        let (url, _) = serve(vec![sse(&[
            &format!(r#"{{"choices":[{{"delta":{{"content":"{reply}"}}}}]}}"#),
            "[DONE]",
        ])]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let ran = run(&mut talk, 2);
        assert_eq!(ran.answered.output, 75);
        assert!(ran.answered.estimated);
    }

    #[test]
    fn the_running_count_matches_a_full_count() {
        let (url, _) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"content":"Checking."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{\"query\":\"heap\"}"}}]}}]}"#,
                "[DONE]",
            ]),
            sse(&[
                r#"{"choices":[{"delta":{"content":"The minimum is at the root."}}]}"#,
                "[DONE]",
            ]),
            sse(&[
                r#"{"choices":[{"delta":{"content":"日本語の答え。"}}]}"#,
                "[DONE]",
            ]),
        ]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "Be Felix 日本語.",
            &tools(),
        );
        let full = |talk: &ApiTalk| {
            talk.system.chars().count()
                + talk
                    .history
                    .iter()
                    .map(|m| m.to_string().chars().count())
                    .sum::<usize>()
        };
        assert_eq!(talk.context_chars(), Some(full(&talk)));
        run(&mut talk, 12);
        assert_eq!(talk.context_chars(), Some(full(&talk)));
        let ran = run(&mut talk, 12);
        assert_eq!(ran.answer, "日本語の答え。");
        assert_eq!(talk.context_chars(), Some(full(&talk)));
        assert!(talk.history.len() > 5);
    }

    #[test]
    fn context_chars_counts_characters() {
        let cfg = ProviderConfig::default();
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "日本語",
            &tools(),
        );
        talk.push(json!({ "role": "user", "content": "日本語の質問" }));
        let from_scratch = 3 + talk
            .history
            .iter()
            .map(|m| m.to_string().chars().count())
            .sum::<usize>();
        assert_eq!(talk.context_chars(), Some(from_scratch));
    }

    #[test]
    #[ignore = "benchmark; run by hand"]
    fn benchmark_history_count() {
        for chars in [40_000usize, 400_000] {
            let mut talk = ApiTalk::new(
                OpenAiChat::new("ollama".into(), &ProviderConfig::default(), None),
                "ollama",
                &Config::default(),
                "system",
                &tools(),
            );
            let piece = "Heaps keep the minimum at the root. ".repeat(56);
            for i in 0..chars / piece.len() {
                let role = if i % 2 == 0 { "user" } else { "assistant" };
                talk.push(json!({ "role": role, "content": piece.clone() }));
            }
            let runs = 2000u32;
            let started = std::time::Instant::now();
            let mut total = 0usize;
            for _ in 0..runs {
                total = total.wrapping_add(talk.context_chars().unwrap());
            }
            println!(
                "history {chars} chars: {} messages, context_chars() = {:?} per count ({total})",
                talk.history.len(),
                started.elapsed() / runs
            );
        }
    }

    #[test]
    fn a_reasoning_only_reply_answers_in_the_native_session() {
        let (url, _) = serve(vec![sse(&[
            r#"{"choices":[{"delta":{"reasoning":"The minimum sits at the root."}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ])]);
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Openai),
            base_url: Some(url),
            model: Some("qwen3".into()),
            ..Default::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "system",
            &tools(),
        );
        let ran = run(&mut talk, 2);
        assert_eq!(ran.answer, "The minimum sits at the root.");
        assert_eq!(ran.shown, "The minimum sits at the root.");
    }

    #[test]
    fn a_stop_during_a_streamed_round_ends_the_session_answer() {
        let (url, _) = serve(vec![sse(&[
            r#"{"choices":[{"delta":{"content":"Heaps"}}]}"#,
            r#"{"choices":[{"delta":{"content":" stay"}}]}"#,
            "[DONE]",
        ])]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let asked = std::cell::Cell::new(0);
        let result = talk.say(
            "What is a heap?",
            Turn {
                tail: "",
                max_tokens: 100,
                most_calls: 2,
                sink: &mut |_| {},
                restart: &mut || {},
                call: &mut |_, _| String::new(),
                stopped: &|| {
                    asked.set(asked.get() + 1);
                    asked.get() > 2
                },
                tools_done: &|| false,
            },
        );
        assert!(result.unwrap_err().to_string().contains("stopped"));
    }

    #[test]
    fn once_the_calls_run_out_the_model_is_told_to_answer_without_tools() {
        let (url, seen) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{}"}}]}}]}"#,
            ]),
            sse(&[r#"{"choices":[{"delta":{"content":"Done."}}]}"#]),
        ]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let ran = run(&mut talk, 1);
        assert_eq!(ran.answer, "Done.");
        assert!(
            ran.answered.local || ran.answered.cost.is_none() || ran.answered.cost == Some(0.0)
        );
        let seen = seen.lock().unwrap();
        assert!(seen[0].1.get("tool_choice").is_none());
        assert_eq!(seen[1].1["tool_choice"], "none");
    }

    #[test]
    fn stopping_after_a_tool_prevents_the_next_model_request() {
        let (url, seen) = serve(vec![sse(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{}"}}]}}]}"#,
        ])]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let stopped = std::cell::Cell::new(false);
        let result = talk.say(
            "Look up heaps.",
            Turn {
                tail: "",
                max_tokens: 100,
                most_calls: 12,
                sink: &mut |_| {},
                restart: &mut || {},
                call: &mut |_, _| {
                    stopped.set(true);
                    "The minimum is at the root.".into()
                },
                stopped: &|| stopped.get(),
                tools_done: &|| false,
            },
        );
        assert!(result.unwrap_err().to_string().contains("stopped"));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn ignoring_the_no_tools_request_cannot_create_an_unbounded_model_loop() {
        let response = sse(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{}"}}]}}]}"#,
        ]);
        let (url, seen) = serve(vec![response.clone(), response]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let mut calls = 0;
        let result = talk.say(
            "Look up heaps.",
            Turn {
                tail: "",
                max_tokens: 100,
                most_calls: 1,
                sink: &mut |_| {},
                restart: &mut || {},
                call: &mut |_, _| {
                    calls += 1;
                    "The minimum is at the root.".into()
                },
                stopped: &|| false,
                tools_done: &|| false,
            },
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("kept requesting tools"));
        assert_eq!(calls, 1);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].1["tool_choice"], "none");
    }

    #[test]
    fn an_exhausted_tool_exchange_gets_one_final_answer_without_more_tools() {
        let (url, seen) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{}"}}]}}]}"#,
            ]),
            sse(&[r#"{"choices":[{"delta":{"content":"Please give me the note title."}}]}"#]),
        ]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            OpenAiChat::new("ollama".into(), &cfg, None),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let exhausted = std::cell::Cell::new(false);
        let result = talk
            .say(
                "Look up heaps.",
                Turn {
                    tail: "",
                    max_tokens: 100,
                    most_calls: 12,
                    sink: &mut |_| {},
                    restart: &mut || {},
                    call: &mut |_, _| {
                        exhausted.set(true);
                        "That did not work: give a note title.".into()
                    },
                    stopped: &|| false,
                    tools_done: &|| exhausted.get(),
                },
            )
            .unwrap();
        assert_eq!(result.0, "Please give me the note title.");
        assert_eq!(seen.lock().unwrap()[1].1["tool_choice"], "none");
    }

    #[test]
    fn api_providers_get_native_tool_sessions() {
        let store = MemoryStore::default();
        store.set("anthropic", "sk-ant").unwrap();
        let mut config = Config::default();
        config.chat.chain = vec!["openai".into(), "anthropic".into()];
        let systems = Systems {
            native: "n",
            text: "t",
        };
        let talk =
            open(&config, &store, &systems, &tools()).expect("a keyed API provider gets a session");
        assert!(talk.native());
        config.chat.chain = vec!["openai".into()];
        assert!(
            open(&config, &store, &systems, &tools()).is_none(),
            "no key, no session"
        );
    }
}
