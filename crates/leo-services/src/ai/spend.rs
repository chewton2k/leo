use crate::ai::provider::Spent;
use crate::config::choice::WRITING;
use crate::config::provider::ProviderKind;
use crate::config::Config;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answered {
    pub by: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub input: u64,
    pub output: u64,
    pub estimated: bool,
    pub cost: Option<f64>,
    pub plan: bool,
    pub local: bool,
    pub cached: u64,
    pub kind: Option<ProviderKind>,
}

pub fn answered(cfg: &Config, provider: &str, spent: Spent) -> Answered {
    let pc = cfg.provider(provider);
    let kind = pc.and_then(|p| p.kind).or(match provider {
        "claude_code" => Some(ProviderKind::ClaudeCode),
        "codex" => Some(ProviderKind::Codex),
        _ => None,
    });
    let choice = WRITING.iter().find(|c| c.provider == provider);
    let plan = matches!(kind, Some(ProviderKind::ClaudeCode | ProviderKind::Codex));
    let model = spent
        .model
        .as_deref()
        .or(pc.and_then(|p| p.model.as_deref()))
        .unwrap_or("");
    let local = pc
        .and_then(|p| p.base_url.as_deref())
        .is_some_and(crate::ai::loopback)
        && !model.to_ascii_lowercase().contains("cloud");
    let cost = (local && !plan).then_some(0.0);
    Answered {
        by: choice.map_or_else(|| provider.to_string(), |c| c.name.to_string()),
        model: spent.model,
        effort: spent.effort,
        input: spent.input,
        output: spent.output,
        estimated: spent.estimated,
        cost,
        plan,
        local,
        cached: spent.cached,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spent(model: &str, input: u64, output: u64) -> Spent {
        Spent {
            model: Some(model.into()),
            input,
            output,
            ..Spent::default()
        }
    }

    #[test]
    fn plans_and_local_answers_keep_their_labels_without_inventing_retired_prices() {
        let cfg = Config::default();
        for (provider, model, label) in [
            ("claude_code", "claude-sonnet-5-5", "Claude Code"),
            ("codex", "gpt-6-luna", "Codex"),
        ] {
            let plan = answered(&cfg, provider, spent(model, 10, 10));
            assert_eq!(
                (plan.by.as_str(), plan.cost, plan.plan),
                (label, None, true)
            );
            assert!(!plan.local);
        }
        assert_eq!(
            answered(&cfg, "codex", spent("gpt-6-luna", 10, 10)).kind,
            Some(ProviderKind::Codex)
        );
        let local = answered(&cfg, "ollama", spent("qwen3:8b", 10, 10));
        assert_eq!((local.cost, local.local), (Some(0.0), true));
        let cloud = answered(&cfg, "ollama", spent("gpt-oss:120b-cloud", 10, 10));
        assert!(!cloud.local && cloud.cost != Some(0.0));
        let mut away = Config::default();
        away.providers.get_mut("ollama").unwrap().base_url =
            Some("http://192.168.1.20:11434/v1".into());
        assert!(!answered(&away, "ollama", spent("qwen3:8b", 10, 10)).local);
        for provider in [
            "openai",
            "anthropic",
            "openrouter",
            "gemini",
            "xai",
            "custom",
        ] {
            let retired = answered(
                &cfg,
                provider,
                spent("claude-sonnet-5-5", 1_000_000, 100_000),
            );
            assert_eq!(retired.by, provider);
            assert_eq!(retired.cost, None);
            assert!(!retired.plan && !retired.local);
            assert_eq!((retired.input, retired.output), (1_000_000, 100_000));
        }
    }

    #[test]
    fn legacy_cache_usage_stays_visible_without_assuming_an_unlisted_price() {
        let cfg = Config::default();
        let cached = answered(
            &cfg,
            "anthropic",
            Spent {
                cached: 800_000,
                cache_written: 100_000,
                ..spent("claude-sonnet-5-5", 1_000_000, 0)
            },
        );
        assert_eq!((cached.cached, cached.input), (800_000, 1_000_000));
        assert_eq!(cached.cost, None);
    }
}
