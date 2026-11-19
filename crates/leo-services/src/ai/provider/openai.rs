use crate::ai::error::{
    classify_reqwest, classify_status, classify_status_with_key, scrub_secret, ProviderError,
    ProviderResult,
};
use crate::ai::provider::{ChatProvider, ChatRequest, Sink, Spent};
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

type Http = std::sync::LazyLock<reqwest::Result<reqwest::blocking::Client>>;

fn http(secs: u64) -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(secs))
        .build()
}

static CLIENT_120: Http = std::sync::LazyLock::new(|| http(120));
static CLIENT_300: Http = std::sync::LazyLock::new(|| http(300));
static CLIENT_600: Http = std::sync::LazyLock::new(|| http(600));

fn capped_lines(
    mut reader: impl std::io::BufRead,
) -> impl Iterator<Item = std::io::Result<String>> {
    let mut total = 0usize;
    std::iter::from_fn(
        move || match crate::ai::provider::process::line(&mut reader) {
            Ok(Some(line)) => {
                total += line.len();
                if total > crate::ai::provider::process::MOST_OUTPUT {
                    return Some(Err(std::io::Error::other(
                        "the stream sent too much output",
                    )));
                }
                Some(Ok(line.trim_end_matches(['\r', '\n']).to_string()))
            }
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        },
    )
}

/// Any endpoint speaking the OpenAI chat-completions protocol: OpenRouter,
/// Ollama, LM Studio, llama.cpp server, vLLM, Groq chat.
pub struct OpenAiChat {
    name: String,
    base_url: String,
    model: String,
    key: Option<Secret>,
    /// Whether this endpoint needs a key at all. Local servers do not.
    needs_key: bool,
    max_tokens: Option<u32>,
    reasoning: bool,
    effort: Option<String>,
    spent: std::sync::Mutex<Option<Spent>>,
}

impl OpenAiChat {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Self {
        let effort = crate::config::choice::effort_for(
            &name,
            cfg.model.as_deref().unwrap_or(""),
            cfg.effort.as_deref(),
        );
        OpenAiChat {
            name,
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            model: cfg
                .model
                .clone()
                .unwrap_or_else(|| "openrouter/free".to_string()),
            key,
            // A provider that names no key_env is a local server needing none.
            needs_key: cfg.key_env.is_some(),
            max_tokens: cfg.max_tokens,
            reasoning: cfg.reasoning.unwrap_or(false),
            effort,
            spent: std::sync::Mutex::new(None),
        }
    }

    fn reports_streamed_usage(&self) -> bool {
        ["api.openai.com", "openrouter.ai", "api.x.ai"]
            .iter()
            .any(|host| self.base_url.contains(host))
    }

    fn note_spent(&self, req: &ChatRequest, answer: &str, usage: Option<(u64, u64, u64)>) {
        let mut spent = Spent::guessed(Some(self.model.clone()), req, answer);
        spent.effort = self.effort.clone();
        if let Some((input, output, cached)) = usage {
            spent.input = input;
            spent.output = output;
            spent.cached = cached;
            spent.estimated = false;
        }
        if let Ok(mut slot) = self.spent.lock() {
            *slot = Some(spent);
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Round {
    pub text: String,
    pub reasoning: String,
    pub calls: Vec<(String, String, String)>,
    pub usage: Option<(u64, u64, u64)>,
    pub finish: String,
}

pub fn function_of(name: &str, description: &str, schema: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": schema },
    })
}

fn cached_of(json: &serde_json::Value) -> u64 {
    let usage = &json["usage"];
    usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| usage["input_tokens_details"]["cached_tokens"].as_u64())
        .unwrap_or(0)
}

pub fn read_round(
    reader: impl std::io::BufRead,
    name: &str,
    sink: Sink<'_>,
    stopped: &dyn Fn() -> bool,
) -> ProviderResult<Round> {
    let mut round = Round::default();
    let mut calls: Vec<(String, String, String)> = Vec::new();
    for line in capped_lines(reader) {
        if stopped() {
            return Err(ProviderError::Fatal("The answer was stopped.".into()));
        }
        let line =
            line.map_err(|e| ProviderError::Retryable(format!("{name}: stream ended early: {e}")))?;
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) else {
            continue;
        };
        if let Some(error) = json.get("error").filter(|e| !e.is_null()) {
            let message = error["message"].as_str().unwrap_or("the stream failed");
            return Err(ProviderError::Retryable(format!("{name}: {message}")));
        }
        if let Some((input, output)) = usage_of(&json) {
            round.usage = Some((input, output, cached_of(&json)));
        }
        let choice = &json["choices"][0];
        if let Some(reason) = choice["finish_reason"].as_str().filter(|r| !r.is_empty()) {
            round.finish = reason.to_string();
        }
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            round.text.push_str(text);
            sink(text);
        } else if let Some(text) = delta["reasoning"].as_str().filter(|t| !t.is_empty()) {
            round.reasoning.push_str(text);
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let id = call["id"].as_str().unwrap_or("");
            let at = match call["index"].as_u64() {
                Some(index) => index as usize,
                None if !id.is_empty() && calls.iter().all(|(known, _, _)| known != id) => {
                    calls.len()
                }
                None => calls.len().saturating_sub(1),
            };
            while calls.len() <= at {
                calls.push((String::new(), String::new(), String::new()));
            }
            let slot = &mut calls[at];
            if !id.is_empty() {
                slot.0 = id.to_string();
            }
            if let Some(name) = call["function"]["name"].as_str() {
                slot.1.push_str(name);
            }
            if let Some(args) = call["function"]["arguments"].as_str() {
                slot.2.push_str(args);
            }
        }
    }
    round.calls = calls
        .into_iter()
        .enumerate()
        .filter(|(_, (_, name, _))| !name.is_empty())
        .map(|(n, (id, name, args))| {
            let id = if id.is_empty() {
                format!("call_{n}")
            } else {
                id
            };
            (id, name, args)
        })
        .collect();
    Ok(round)
}

impl OpenAiChat {
    fn client(&self, client: &'static Http) -> ProviderResult<&'static reqwest::blocking::Client> {
        client
            .as_ref()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))
    }

    fn refused(&self, resp: reqwest::blocking::Response) -> ProviderError {
        let status = resp.status().as_u16();
        let text = resp.text().unwrap_or_default();
        let text = scrub_secret(&text, self.key.as_ref().map(Secret::as_str));
        match self.key {
            Some(_) => classify_status_with_key(status, &self.name, &text),
            None => classify_status(status, &self.name, &text),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn effort(&self) -> Option<&str> {
        self.effort.as_deref()
    }

    pub fn tool_body(
        &self,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
        max_tokens: u32,
        no_tools: bool,
    ) -> serde_json::Value {
        let mut body = self.body_of(messages.to_vec(), max_tokens, 0.3, true);
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
            if no_tools {
                body["tool_choice"] = serde_json::json!("none");
            }
        }
        body
    }

    pub fn stream_round(
        &self,
        body: &serde_json::Value,
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<Round> {
        let client = self.client(&CLIENT_600)?;
        let resp = self
            .authorized(client.post(self.endpoint()))
            .json(body)
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;
        if !resp.status().is_success() {
            return Err(self.refused(resp));
        }
        read_round(std::io::BufReader::new(resp), &self.name, sink, stopped)
    }
}

fn usage_of(json: &serde_json::Value) -> Option<(u64, u64)> {
    let usage = &json["usage"];
    let input = usage["prompt_tokens"]
        .as_u64()
        .or_else(|| usage["input_tokens"].as_u64())?;
    let output = usage["completion_tokens"]
        .as_u64()
        .or_else(|| usage["output_tokens"].as_u64())?;
    Some((input, output))
}

impl OpenAiChat {
    /// The request body, shared by the streaming and non-streaming paths so they
    /// cannot drift apart on model or temperature.
    fn body(&self, req: &ChatRequest, stream: bool) -> serde_json::Value {
        let mut messages = Vec::new();
        if let Some(system) = &req.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": req.prompt}));
        self.body_of(messages, req.max_tokens, req.temperature, stream)
    }

    fn body_of(
        &self,
        messages: Vec<serde_json::Value>,
        max_tokens: u32,
        temperature: f32,
        stream: bool,
    ) -> serde_json::Value {
        let mut body = if self.reasoning {
            serde_json::json!({
                "model": self.model,
                "messages": messages,
                "max_completion_tokens": max_tokens,
                "stream": stream,
            })
        } else {
            serde_json::json!({
                "model": self.model,
                "messages": messages,
                "temperature": temperature,
                "max_tokens": max_tokens,
                "stream": stream,
            })
        };
        if let Some(effort) = &self.effort {
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        if stream && self.reports_streamed_usage() {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        body
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn authorized(
        &self,
        builder: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        let builder = builder
            .header("Content-Type", "application/json")
            // OpenRouter attribution headers; harmless elsewhere.
            .header("HTTP-Referer", "https://github.com/leo-cli")
            .header("X-Title", "leo");
        match &self.key {
            Some(key) => builder.header("Authorization", format!("Bearer {}", key.as_str())),
            None => builder,
        }
    }
}

/// What to tell the user when an answer stopped at the token limit. The text
/// is still used — most of a note beats none — but it is cut off, and the fix
/// is a setting they can change.
fn cut_off_warning(name: &str, finish: &str, max_tokens: u32) -> Option<String> {
    (finish == "length").then(|| {
        format!(
            "{name}: the answer hit its {max_tokens}-token limit and is cut off; \
             raise max_tokens for {name} in config.toml"
        )
    })
}

impl OpenAiChat {
    fn send(
        &self,
        req: &ChatRequest,
        body: serde_json::Value,
        client: &'static Http,
    ) -> ProviderResult<String> {
        let client = self.client(client)?;

        let resp = self
            .authorized(client.post(self.endpoint()))
            .json(&body)
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        if !resp.status().is_success() {
            return Err(self.refused(resp));
        }

        let text = resp.text().map_err(|e| {
            ProviderError::Retryable(format!("{}: the answer stopped arriving: {e}", self.name))
        })?;
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            ProviderError::Fatal(format!("{}: unreadable response: {e}", self.name))
        })?;

        let message = &json["choices"][0]["message"];
        let finish = json["choices"][0]["finish_reason"].as_str().unwrap_or("");

        // Prefer `content`. Reasoning models on OpenRouter's free tier often
        // return an empty or null `content` while putting text in `reasoning`,
        // especially when the token budget ran out mid-thought, so fall back to
        // that rather than discarding a usable answer.
        let text = message["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                message["reasoning"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
            });

        match text {
            Some(t) => {
                if let Some(note) = cut_off_warning(&self.name, finish, req.max_tokens) {
                    leo_core::diag::warn(note);
                }
                self.note_spent(
                    req,
                    t,
                    usage_of(&json).map(|(input, output)| (input, output, cached_of(&json))),
                );
                Ok(t.to_string())
            }
            // Retryable, not fatal: an empty completion is this provider
            // failing to answer, and the next one in the chain may well do
            // better. Treating it as fatal would abort the whole chain over a
            // truncated reasoning trace.
            None if finish == "length" => Err(ProviderError::Retryable(format!(
                "{}: hit the token limit before producing any content",
                self.name
            ))),
            None => Err(ProviderError::Retryable(format!(
                "{}: returned no content",
                self.name
            ))),
        }
    }
}

impl OpenAiChat {
    fn stream_until(
        &self,
        req: &ChatRequest,
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        let client = self.client(&CLIENT_300)?;

        let resp = self
            .authorized(client.post(self.endpoint()))
            .json(&self.body(req, true))
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;

        if !resp.status().is_success() {
            return Err(self.refused(resp));
        }

        let round = read_round(std::io::BufReader::new(resp), &self.name, sink, stopped)?;
        if !round.text.trim().is_empty() {
            if let Some(note) = cut_off_warning(&self.name, &round.finish, req.max_tokens) {
                leo_core::diag::warn(note);
            }
            self.note_spent(req, &round.text, round.usage);
            return Ok(round.text);
        }
        if !round.reasoning.trim().is_empty() {
            sink(&round.reasoning);
            self.note_spent(req, &round.reasoning, round.usage);
            return Ok(round.reasoning);
        }
        Err(ProviderError::Retryable(format!(
            "{}: streamed an empty response",
            self.name
        )))
    }
}

impl ChatProvider for OpenAiChat {
    fn complete_streaming(&self, req: &ChatRequest, sink: Sink<'_>) -> ProviderResult<String> {
        let stop = leo_core::children::current_stop();
        self.stream_until(req, sink, &|| {
            stop.as_ref().is_some_and(leo_core::children::Stop::asked)
        })
    }

    fn complete_until(
        &self,
        req: &ChatRequest,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        if stopped() {
            return Err(ProviderError::Fatal("The answer was stopped.".into()));
        }
        if crate::ai::loopback(&self.base_url) {
            return self.stream_until(req, &mut |_| {}, stopped);
        }
        self.complete(req)
    }

    fn complete(&self, req: &ChatRequest) -> ProviderResult<String> {
        if crate::ai::loopback(&self.base_url) {
            self.complete_streaming(req, &mut |_| {})
        } else {
            self.send(req, self.body(req, false), &CLIENT_120)
        }
    }

    fn complete_with_images(
        &self,
        req: &ChatRequest,
        images: &[crate::ai::provider::Image],
    ) -> ProviderResult<String> {
        let mut body = self.body(req, false);
        let mut parts = vec![serde_json::json!({"type": "text", "text": req.prompt})];
        for image in images {
            parts.push(
                serde_json::json!({"type": "image_url", "image_url": {"url": image.data_url()}}),
            );
        }
        if let Some(last) = body["messages"].as_array_mut().and_then(|m| m.last_mut()) {
            last["content"] = serde_json::Value::Array(parts);
        }
        self.send(req, body, &CLIENT_300)
    }

    fn available(&self) -> bool {
        !self.needs_key || self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn spent(&self) -> Option<Spent> {
        self.spent.lock().ok().and_then(|slot| slot.clone())
    }

    fn unavailable_reason(&self) -> String {
        format!("{}: no API key (add it in Settings in leo)", self.name)
    }

    fn max_tokens(&self) -> Option<u32> {
        self.max_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streamed_round_gives_text_and_tool_calls_put_together_from_pieces() {
        let stream = [
            r#"data: {"choices":[{"delta":{"content":"Let me look."}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"search_notes","arguments":"{\"que"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ry\": \"heap\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"open_note","arguments":"{\"note\":\"n1\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":5000,"completion_tokens":30,"prompt_tokens_details":{"cached_tokens":4096}}}"#,
            "data: [DONE]",
        ]
        .join("\n\n");
        let mut shown = String::new();
        let round = read_round(
            stream.as_bytes(),
            "OpenAI",
            &mut |t| shown.push_str(t),
            &|| false,
        )
        .unwrap();
        assert_eq!(shown, "Let me look.");
        assert_eq!(round.finish, "tool_calls");
        assert_eq!(
            round.calls,
            [
                (
                    "call_a".into(),
                    "search_notes".into(),
                    "{\"query\": \"heap\"}".into()
                ),
                (
                    "call_b".into(),
                    "open_note".into(),
                    "{\"note\":\"n1\"}".into()
                )
            ]
        );
        assert_eq!(round.usage, Some((5000, 30, 4096)));
    }

    #[test]
    fn whole_tool_calls_without_an_index_are_each_kept() {
        let stream = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"g1","function":{"name":"search_notes","arguments":"{}"}},{"id":"g2","function":{"name":"open_note","arguments":"{}"}}]}}]}"#;
        let round = read_round(stream.as_bytes(), "Gemini", &mut |_| {}, &|| false).unwrap();
        let names: Vec<&str> = round.calls.iter().map(|c| c.1.as_str()).collect();
        assert_eq!(names, ["search_notes", "open_note"]);
    }

    #[test]
    fn a_tool_request_offers_functions_and_can_forbid_them() {
        let chat = OpenAiChat::new("openai".into(), &ProviderConfig::default(), None);
        let tools = [function_of(
            "search_notes",
            "Search.",
            &serde_json::json!({"type": "object"}),
        )];
        let body = chat.tool_body(
            &[serde_json::json!({"role": "user", "content": "hi"})],
            &tools,
            900,
            false,
        );
        assert_eq!(body["tools"][0]["function"]["name"], "search_notes");
        assert_eq!(body["stream"], true);
        assert!(body.get("tool_choice").is_none());
        assert_eq!(
            chat.tool_body(&[], &tools, 900, true)["tool_choice"],
            "none"
        );
    }
    use crate::ai::provider::ChatProvider;
    use crate::config::secret::{resolve, MemoryStore, SecretStore};

    #[test]
    fn usage_is_read_in_either_spelling_and_asked_for_only_where_it_is_known_to_work() {
        let json = serde_json::json!({"usage": {"prompt_tokens": 120, "completion_tokens": 30}});
        assert_eq!(usage_of(&json), Some((120, 30)));
        let json = serde_json::json!({"usage": {"input_tokens": 5, "output_tokens": 6}});
        assert_eq!(usage_of(&json), Some((5, 6)));
        assert_eq!(usage_of(&serde_json::json!({"usage": null})), None);
        let at = |url: &str| {
            let cfg = ProviderConfig {
                base_url: Some(url.into()),
                ..ProviderConfig::default()
            };
            OpenAiChat::new("p".into(), &cfg, None)
        };
        let req = ChatRequest {
            system: None,
            prompt: "p".into(),
            temperature: 0.2,
            max_tokens: 10,
        };
        assert_eq!(
            at("https://api.openai.com/v1").body(&req, true)["stream_options"]["include_usage"],
            true
        );
        assert!(at("http://localhost:11434/v1")
            .body(&req, true)
            .get("stream_options")
            .is_none());
        assert!(at("https://api.openai.com/v1")
            .body(&req, false)
            .get("stream_options")
            .is_none());
    }

    #[test]
    fn max_bytes_is_not_applicable_but_max_tokens_reflects_config() {
        let cfg = ProviderConfig {
            max_tokens: Some(777),
            ..Default::default()
        };
        let provider = OpenAiChat::new("ollama".to_string(), &cfg, None);
        assert_eq!(ChatProvider::max_tokens(&provider), Some(777));
        let unset = OpenAiChat::new("mine".to_string(), &ProviderConfig::default(), None);
        assert_eq!(ChatProvider::max_tokens(&unset), None);
    }

    fn local(base_url: String) -> OpenAiChat {
        let cfg = ProviderConfig {
            base_url: Some(base_url),
            model: Some("qwen3:8b".to_string()),
            ..Default::default()
        };
        OpenAiChat::new("ollama".to_string(), &cfg, None)
    }

    fn ask() -> ChatRequest {
        ChatRequest {
            system: None,
            prompt: "hi".to_string(),
            temperature: 0.0,
            max_tokens: 16,
        }
    }

    fn serve_stream(body: String) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let body = body.clone();
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
                    let mut buffer = [0u8; 65536];
                    let _ = stream.read(&mut buffer);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[test]
    fn both_stream_paths_read_a_body_the_same_way() {
        let bodies = [
            ("reasoning only", "data: {\"choices\":[{\"delta\":{\"reasoning\":\"The minimum is at the root.\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"),
            ("error object after text", "data: {\"choices\":[{\"delta\":{\"content\":\"Heaps\"}}]}\n\ndata: {\"error\":{\"message\":\"model crashed\"}}\n\ndata: [DONE]\n\n"),
            ("cut off at the limit", "data: {\"choices\":[{\"delta\":{\"content\":\"Heaps keep\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":16,\"prompt_tokens_details\":{\"cached_tokens\":32}}}\n\ndata: [DONE]\n\n"),
        ];
        let mut differ = 0;
        for (label, body) in bodies {
            let round = read_round(body.as_bytes(), "ollama", &mut |_| {}, &|| false)
                .map(|r| {
                    if r.text.trim().is_empty() {
                        r.reasoning
                    } else {
                        r.text
                    }
                })
                .map_err(|e| e.to_string());
            let plain = local(serve_stream(body.to_string()));
            let streamed = plain
                .stream_until(&ask(), &mut |_| {}, &|| false)
                .map_err(|e| e.to_string());
            if round.is_err() != streamed.is_err()
                || round.clone().unwrap_or_default() != streamed.clone().unwrap_or_default()
            {
                differ += 1;
                eprintln!("{label}: read_round={round:?} stream_until={streamed:?}");
            }
        }
        assert_eq!(differ, 0, "{differ} of 3 bodies read differently");
    }

    #[test]
    fn a_good_stream_answers_byte_for_byte_and_counts_cached_tokens() {
        let body = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning\":\"thinking\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Own\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"ership \\u00e9 \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"moves.\\n\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":9,\"prompt_tokens_details\":{\"cached_tokens\":32}}}\n\ndata: [DONE]\n\n";
        let provider = local(serve_stream(body.to_string()));
        let mut shown = String::new();
        let answer = provider
            .stream_until(&ask(), &mut |t| shown.push_str(t), &|| false)
            .unwrap();
        assert_eq!(answer, "Ownership \u{e9} moves.\n");
        assert_eq!(shown, answer);
        let spent = provider.spent().unwrap();
        assert_eq!((spent.input, spent.output, spent.cached), (50, 9, 32));
        assert!(!spent.estimated);
    }

    #[test]
    fn a_stream_that_reports_an_error_after_text_fails_so_the_next_writer_is_tried() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Heaps\"}}]}\n\ndata: {\"error\":{\"message\":\"model crashed\"}}\n\n";
        let provider = local(serve_stream(body.to_string()));
        match provider.complete(&ask()) {
            Err(ProviderError::Retryable(message)) => assert!(message.contains("model crashed")),
            other => panic!("expected a retryable failure, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_answer_streams_so_a_slow_local_model_is_bounded_by_silence_not_length() {
        let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"Heaps\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" stay ordered.\"}}]}\n\ndata: [DONE]\n\n";
        let provider = local(refuse_once("200 OK", stream));
        assert_eq!(provider.complete(&ask()).unwrap(), "Heaps stay ordered.");
    }

    #[test]
    fn a_local_answer_stops_between_streamed_lines_when_asked() {
        let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"Heaps\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" stay\"}}]}\n\ndata: [DONE]\n\n";
        let provider = local(refuse_once("200 OK", stream));
        let asked = std::cell::Cell::new(0);
        let later = || {
            asked.set(asked.get() + 1);
            asked.get() > 2
        };
        match provider.complete_until(&ask(), &later) {
            Err(ProviderError::Fatal(message)) => assert!(message.contains("stopped")),
            other => panic!("expected a stop, got {other:?}"),
        }
        let provider = local(refuse_once("200 OK", stream));
        assert_eq!(
            provider.complete_until(&ask(), &|| false).unwrap(),
            "Heaps stay"
        );
    }

    #[test]
    fn an_answer_cut_off_mid_body_lets_the_chain_try_the_next_provider() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 500\r\nConnection: close\r\n\r\n{{\"choices\":"
                );
            }
        });
        let provider = local(format!("http://127.0.0.1:{port}/v1"));
        let req = ask();
        match provider.send(&req, provider.body(&req, false), &CLIENT_300) {
            Err(ProviderError::Retryable(message)) => {
                assert!(message.contains("stopped arriving"), "{message}")
            }
            other => panic!("expected a retryable failure, got {other:?}"),
        }
    }

    /// Instructions travel as a system message and the material as the user's,
    /// which models follow more reliably than one mixed message.
    #[test]
    fn a_system_prompt_is_sent_as_its_own_message() {
        let provider = OpenAiChat::new("ollama".to_string(), &ProviderConfig::default(), None);
        let req = ChatRequest {
            system: Some("the rules".to_string()),
            prompt: "the material".to_string(),
            temperature: 0.3,
            max_tokens: 100,
        };
        for stream in [false, true] {
            let body = provider.body(&req, stream);
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["messages"][0]["content"], "the rules");
            assert_eq!(body["messages"][1]["role"], "user");
            assert_eq!(body["messages"][1]["content"], "the material");
        }

        let plain = ChatRequest {
            system: None,
            ..req
        };
        let body = provider.body(&plain, false);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn a_reasoning_model_gets_max_completion_tokens_and_no_temperature() {
        let cfg = ProviderConfig {
            reasoning: Some(true),
            ..ProviderConfig::default()
        };
        let provider = OpenAiChat::new("openai".to_string(), &cfg, None);
        let req = ChatRequest {
            system: None,
            prompt: "hi".to_string(),
            temperature: 0.3,
            max_tokens: 100,
        };
        for stream in [false, true] {
            let body = provider.body(&req, stream);
            assert_eq!(body["max_completion_tokens"], 100);
            assert!(body.get("max_tokens").is_none());
            assert!(body.get("temperature").is_none());
            assert_eq!(body["stream"], stream);
        }
        let plain = OpenAiChat::new("ollama".to_string(), &ProviderConfig::default(), None);
        let body = plain.body(&req, false);
        assert_eq!(body["max_tokens"], 100);
        assert!(body.get("max_completion_tokens").is_none());
    }

    /// A reply that ran into the token limit is kept — most of a note beats
    /// none — but the user is told it is cut off and what to raise.
    #[test]
    fn a_cut_off_answer_says_so_and_names_the_setting() {
        let note = cut_off_warning("openrouter", "length", 4096).expect("a warning");
        assert!(note.contains("openrouter"), "{note}");
        assert!(note.contains("4096"), "{note}");
        assert!(note.contains("max_tokens"), "{note}");
        assert_eq!(cut_off_warning("openrouter", "stop", 4096), None);
        assert_eq!(cut_off_warning("openrouter", "", 4096), None);
    }

    #[test]
    fn the_finish_reason_is_read_from_a_stream_chunk() {
        let read = |body: &str| read_round(body.as_bytes(), "p", &mut |_| {}, &|| false).unwrap();
        assert_eq!(
            read(r#"data: {"choices":[{"delta":{},"finish_reason":"length"}]}"#).finish,
            "length"
        );
        assert_eq!(
            read(r#"data: {"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#).finish,
            ""
        );
        assert_eq!(read("data: [DONE]").finish, "");
    }

    /// leo speaks HTTPS through rustls with bundled root certificates, so a
    /// release binary does not depend on the system's OpenSSL. Run by hand:
    /// `cargo test -p leo-services tls -- --ignored`.
    #[test]
    #[ignore = "reaches the network"]
    fn tls_reaches_a_public_https_endpoint() {
        let status = reqwest::blocking::get("https://openrouter.ai/api/v1/models")
            .expect("an HTTPS connection")
            .status();
        assert!(status.is_success(), "{status}");
    }

    fn refuse_once(status: &'static str, body: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[test]
    fn a_rejected_stored_key_tells_the_user_where_to_replace_it() {
        let store = MemoryStore::default();
        store.set("openrouter", "sk-old-key").unwrap();
        let cfg = ProviderConfig {
            base_url: Some(refuse_once(
                "401 Unauthorized",
                r#"{"error":{"message":"User not found.","code":401}}"#,
            )),
            model: Some("openrouter/free".to_string()),
            key_env: Some("OPENROUTER_API_KEY".to_string()),
            ..Default::default()
        };
        let provider = OpenAiChat::new(
            "openrouter".to_string(),
            &cfg,
            resolve("openrouter", &store),
        );
        let error = provider
            .complete(&ChatRequest {
                system: None,
                prompt: "hi".to_string(),
                temperature: 0.0,
                max_tokens: 16,
            })
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with("OpenRouter rejected the key stored in leo (401). Choose another writing AI in Settings."),
            "{message}"
        );
        assert!(message.contains("User not found."), "{message}");
        assert!(!message.contains("sk-old-key"), "{message}");
    }

    #[test]
    fn unavailable_reason_never_contains_the_key_value() {
        let store = MemoryStore::default();
        store.set("openrouter", "sk-super-secret-value").unwrap();
        let key = resolve("openrouter", &store);
        let cfg = ProviderConfig {
            key_env: Some("LEO_TEST_UNUSED_KEY_ENV".to_string()),
            ..Default::default()
        };
        let provider = OpenAiChat::new("openrouter".to_string(), &cfg, key);
        assert!(!provider
            .unavailable_reason()
            .contains("sk-super-secret-value"));
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::{capped_lines, read_round, Round};

    fn read(lines: &[&str]) -> (Round, String) {
        let body: String = lines.iter().map(|l| format!("data: {l}\n\n")).collect();
        let mut shown = String::new();
        let round =
            read_round(body.as_bytes(), "p", &mut |t| shown.push_str(t), &|| false).unwrap();
        (round, shown)
    }

    #[test]
    fn a_content_delta_yields_its_text() {
        let (round, shown) = read(&[r#"{"choices":[{"delta":{"content":"Hello"}}]}"#]);
        assert_eq!((round.text.as_str(), shown.as_str()), ("Hello", "Hello"));
    }

    #[test]
    fn reasoning_is_kept_separate_from_the_answer() {
        let (round, shown) = read(&[r#"{"choices":[{"delta":{"reasoning":"let me think"}}]}"#]);
        assert_eq!(round.text, "");
        assert_eq!(round.reasoning, "let me think");
        assert_eq!(shown, "", "reasoning was reported as answer text");
    }

    #[test]
    fn protocol_noise_yields_nothing() {
        for payload in [
            "[DONE]",
            "",
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"{"choices":[{"delta":{}}]}"#,
            r#"{"choices":[{"delta":{"content":""}}]}"#,
            r#"{"choices":[]}"#,
            "not json at all",
        ] {
            let (round, shown) = read(&[payload]);
            assert_eq!(
                (round.text, round.reasoning, shown),
                (String::new(), String::new(), String::new()),
                "misread {payload:?}"
            );
        }
    }

    #[test]
    fn content_wins_over_reasoning_in_the_same_delta() {
        let (round, _) =
            read(&[r#"{"choices":[{"delta":{"content":"answer","reasoning":"thinking"}}]}"#]);
        assert_eq!(
            (round.text.as_str(), round.reasoning.as_str()),
            ("answer", "")
        );
    }

    #[test]
    fn whitespace_only_content_is_kept_since_it_is_part_of_the_text() {
        let (round, _) = read(&[r#"{"choices":[{"delta":{"content":" "}}]}"#]);
        assert_eq!(round.text, " ");
    }

    #[test]
    fn only_content_deltas_form_the_answer() {
        let (round, shown) = read(&[
            r#"{"choices":[{"delta":{"reasoning":"The user asks about moves."}}]}"#,
            r#"{"choices":[{"delta":{"reasoning":" I should be brief."}}]}"#,
            r#"{"choices":[{"delta":{"content":"Ownership"}}]}"#,
            r#"{"choices":[{"delta":{"content":" transfers."}}]}"#,
            "[DONE]",
        ]);
        assert_eq!(round.text, "Ownership transfers.");
        assert_eq!(shown, "Ownership transfers.");
        assert_eq!(
            round.reasoning,
            "The user asks about moves. I should be brief."
        );
    }

    #[test]
    fn fragments_reassemble_into_the_answer() {
        let (round, _) = read(&[
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"{"choices":[{"delta":{"content":"Own"}}]}"#,
            r#"{"choices":[{"delta":{"content":"ership"}}]}"#,
            r#"{"choices":[{"delta":{"content":" moves."}}]}"#,
            "[DONE]",
        ]);
        assert_eq!(round.text, "Ownership moves.");
    }

    #[test]
    fn a_stream_line_over_the_cap_is_an_error_not_memory() {
        let mut lines = capped_lines(std::io::Cursor::new("a\r\nb\n"));
        assert_eq!(lines.next().unwrap().unwrap(), "a");
        assert_eq!(lines.next().unwrap().unwrap(), "b");
        assert!(lines.next().is_none());
        let huge = vec![b'x'; crate::ai::provider::process::MOST_LINE + 10];
        assert!(capped_lines(std::io::Cursor::new(huge))
            .next()
            .unwrap()
            .is_err());
    }
}
