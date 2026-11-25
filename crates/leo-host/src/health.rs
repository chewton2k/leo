use anyhow::Result;
use leo_services::{
    config::{secret::default_store, Config},
    doctor,
    health::State,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

const SIGN_IN_REUSE: Duration = Duration::from_secs(20);

type LastSignIn = Mutex<Option<(String, Instant)>>;

static LAST_SIGN_IN: LastSignIn = Mutex::new(None);

fn reusable(last: Option<&(String, Instant)>, bin: &str, now: Instant, fresh: bool) -> bool {
    !fresh
        && last.is_some_and(|(seen, at)| {
            seen == bin && now.saturating_duration_since(*at) < SIGN_IN_REUSE
        })
}

fn signed_in_now_with(
    last: &LastSignIn,
    bin: &str,
    fresh: bool,
    now: Instant,
    probe: impl FnOnce() -> Option<bool>,
) -> Option<bool> {
    if reusable(
        last.lock().unwrap_or_else(PoisonError::into_inner).as_ref(),
        bin,
        now,
        fresh,
    ) {
        return Some(true);
    }
    let answer = probe();
    *last.lock().unwrap_or_else(PoisonError::into_inner) =
        (answer == Some(true)).then(|| (bin.to_string(), now));
    answer
}

fn signed_in_now(
    agent: leo_services::ai::provider::agent_cli::Agent,
    bin: &str,
    fresh: bool,
) -> Option<bool> {
    signed_in_now_with(&LAST_SIGN_IN, bin, fresh, Instant::now(), || {
        leo_services::ai::provider::agent_cli::signed_in(agent, bin)
    })
}

pub fn scan(notes: &Path, facts: leo_core::store::Facts, fresh: bool) -> Result<Value> {
    let config = Config::load();
    let secrets = default_store();
    let path = Config::config_path()?;
    let sections = doctor::scan(&config, secrets.as_ref(), notes, &path, facts);
    let sign_in=config.chat.chain.first().and_then(|name|config.provider(name)).and_then(|provider| {
        let agent=leo_services::ai::provider::agent_cli::Agent::of(provider)?;
        let cli=leo_services::ai::provider::agent_cli::AgentCli::new(agent.program().into(),agent,provider);
        let signed_in=signed_in_now(agent,cli.bin(),fresh);
        Some(json!({"title":"Writing AI sign-in","checks":[{"name":format!("{} sign-in",agent.program()),"state":match signed_in { Some(true)=>"ready",Some(false)=>"missing",None=>"warn" },"fix":format!("If sign-in is needed, {}.",leo_services::ai::provider::agent_cli::SIGN_IN),"section":"writing"}]}))
    });
    let mut sections:Vec<Value>=sections.into_iter().map(|section| {
        let checks:Vec<Value>=section.checks.into_iter().map(|check| {
            let (state,fix)=match check.state { State::Ready=>("ready",String::new()),State::Missing{fix}=>("missing",fix),State::Warn{note}=>("warn",note) };
            let section=section_of(&check.what);
            json!({"name":check.what,"state":state,"fix":fix,"detail":check.detail,"needed_for":check.needed_for,"section":section})
        }).collect();
        json!({"title":section.title,"checks":checks})
    }).collect();
    if let Some(sign_in) = sign_in {
        sections.insert(0, sign_in);
    }
    if let Some(problem) = config
        .chat
        .chain
        .iter()
        .any(|name| name == "codex")
        .then(leo_services::ai::provider::codex_app::native_problem)
        .flatten()
    {
        sections.insert(0, json!({"title":"Felix and Codex","checks":[{"name":"Codex native session","state":"warn","fix":problem,"section":"writing"}]}));
    }
    if let Some(path) = leo_web::db::set_aside() {
        sections.insert(0, json!({"title":"leo","checks":[{"name":"leo's database","state":"missing","fix":format!("leo's database was damaged and set aside at {}. Chats and margin notes from before are in that file.", path.display())}]}));
    }
    Ok(json!({"sections":sections}))
}

fn section_of(what: &str) -> Option<&'static str> {
    match what {
        "a chat model" => Some("writing"),
        "speech model" => Some("models"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recent_sign_in_answer_is_reused_and_a_missing_or_old_one_is_not() {
        let now = Instant::now();
        let seen = ("claude".to_string(), now);
        assert!(reusable(
            Some(&seen),
            "claude",
            now + Duration::from_secs(19),
            false
        ));
        assert!(!reusable(
            Some(&seen),
            "codex",
            now + Duration::from_secs(1),
            false
        ));
        assert!(!reusable(
            Some(&seen),
            "claude",
            now + Duration::from_secs(21),
            false
        ));
        assert!(!reusable(None, "claude", now, false));
    }

    #[test]
    fn a_fresh_scan_ignores_a_recent_answer() {
        let now = Instant::now();
        let seen = ("claude".to_string(), now);
        assert!(!reusable(
            Some(&seen),
            "claude",
            now + Duration::from_secs(1),
            true
        ));
        let last = LastSignIn::new(Some(seen));
        let mut probes = 0;
        let answer = signed_in_now_with(&last, "claude", true, now, || {
            probes += 1;
            Some(true)
        });
        assert_eq!((answer, probes), (Some(true), 1));
    }

    #[test]
    fn a_signed_out_answer_is_checked_again() {
        let last = LastSignIn::new(None);
        let now = Instant::now();
        let mut probes = 0;
        for answer in [Some(false), None, Some(false)] {
            assert_eq!(
                signed_in_now_with(&last, "claude", false, now, || {
                    probes += 1;
                    answer
                }),
                answer
            );
        }
        assert_eq!(probes, 3);
        assert!(last.lock().unwrap().is_none());
    }

    #[test]
    fn two_health_loads_and_two_allows_probe_once() {
        let last = LastSignIn::new(None);
        let start = Instant::now();
        let mut probes = 0;
        for after in [0, 2, 5] {
            signed_in_now_with(
                &last,
                "claude",
                false,
                start + Duration::from_secs(after),
                || {
                    probes += 1;
                    Some(true)
                },
            );
        }
        assert_eq!(probes, 1, "before: one sign-in program per load, 3");
        signed_in_now_with(
            &last,
            "claude",
            true,
            start + Duration::from_secs(6),
            || {
                probes += 1;
                Some(true)
            },
        );
        assert_eq!(probes, 2, "Check again always asks");
    }

    #[test]
    fn the_writing_and_speech_checks_open_their_settings() {
        assert_eq!(section_of("a chat model"), Some("writing"));
        assert_eq!(section_of("speech model"), Some("models"));
        assert_eq!(section_of("microphone"), None);
    }
}
