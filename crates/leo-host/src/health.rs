use anyhow::Result;
use leo_services::{
    config::{secret::default_store, Config},
    doctor,
    health::State,
};
use serde_json::{json, Value};
use std::path::Path;
pub fn scan(notes: &Path) -> Result<Value> {
    let config = Config::load();
    let secrets = default_store();
    let path = Config::config_path()?;
    let sections = doctor::scan(&config, secrets.as_ref(), notes, &path);
    let sign_in=config.chat.chain.first().and_then(|name|config.provider(name)).and_then(|provider| {
        let agent=leo_services::ai::provider::agent_cli::Agent::of(provider)?;
        let cli=leo_services::ai::provider::agent_cli::AgentCli::new(agent.program().into(),agent,provider);
        let signed_in=leo_services::ai::provider::agent_cli::signed_in(agent,cli.bin());
        Some(json!({"title":"Writing AI sign-in","checks":[{"name":format!("{} sign-in",agent.program()),"state":match signed_in { Some(true)=>"ready",Some(false)=>"missing",None=>"warn" },"fix":format!("If sign-in is needed, {}. You can also use Test in Writing AI.",agent.sign_in()),"section":"writing"}]}))
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
    use super::section_of;

    #[test]
    fn the_writing_and_speech_checks_open_their_settings() {
        assert_eq!(section_of("a chat model"), Some("writing"));
        assert_eq!(section_of("speech model"), Some("models"));
        assert_eq!(section_of("microphone"), None);
    }
}
