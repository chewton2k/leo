use std::sync::Arc;

use leo_services::ai::session::{Session, Systems, ToolDef, Turn};
use leo_services::ai::spend::Answered;

#[derive(Debug, PartialEq)]
enum Route {
    Toolless,
    Quick,
    Full,
}

fn route_of(purpose: leo_web::usage::Purpose) -> Route {
    use leo_web::usage::Purpose;
    match purpose {
        Purpose::MarginRead => Route::Toolless,
        Purpose::Summary | Purpose::QuickReview => Route::Quick,
        _ => Route::Full,
    }
}

pub fn writer(
    usage: Arc<leo_web::usage::Usage>,
    purpose: leo_web::usage::Purpose,
) -> leo_web::Writer {
    Arc::new(move |system, user, most| {
        let started = std::time::Instant::now();
        let prompt = leo_services::ai::chat::Prompt {
            system: system.into(),
            user: user.into(),
        };
        let result = match route_of(purpose) {
            Route::Toolless => leo_core::children::with_stop(
                leo_core::children::Stop {
                    deadline: Some(started + leo_web::margin::READ_LIMIT),
                    ..Default::default()
                },
                || leo_services::ai::toolless_outcome(prompt, most),
            ),
            Route::Quick => leo_services::ai::quick_outcome(prompt, most),
            Route::Full => leo_services::ai::chat_outcome(prompt, most),
        };
        let spent = result
            .as_ref()
            .ok()
            .and_then(|outcome| outcome.spent.as_ref())
            .map(|spent| leo_web::Spent {
                model: spent.model.clone(),
                input: spent.input,
                output: spent.output,
                cached: spent.cached,
                estimated: spent.estimated,
                ..Default::default()
            });
        usage.record(purpose, spent.as_ref(), started.elapsed(), result.is_err());
        result.map(|outcome| outcome.value).map_err(said)
    })
}

pub fn said(e: anyhow::Error) -> anyhow::Error {
    let said = leo_services::ai::writing_error(&e);
    leo_core::diag::warn(format!(
        "the writing AI failed: {} ({})",
        logged(&said),
        logged(&e.to_string())
    ));
    anyhow::anyhow!(said)
}

fn logged(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

pub fn spent_of(answered: Answered) -> leo_web::Spent {
    leo_web::Spent {
        by: answered.by,
        model: answered.model,
        effort: answered.effort,
        input: answered.input,
        output: answered.output,
        estimated: answered.estimated,
        cost: answered.cost,
        plan: answered.plan,
        local: answered.local,
        cached: answered.cached,
        steps: 1,
        auto: false,
    }
}

struct Talk(Box<dyn Session>);

impl leo_web::Conversation for Talk {
    fn native(&self) -> bool {
        self.0.native()
    }

    fn context_chars(&self) -> Option<usize> {
        self.0.context_chars()
    }

    fn say(
        &mut self,
        text: &str,
        exchange: leo_web::Exchange<'_>,
    ) -> anyhow::Result<leo_web::Reply> {
        let (text, answered) = self
            .0
            .say(
                text,
                Turn {
                    tail: exchange.tail,
                    max_tokens: exchange.max_tokens,
                    most_calls: exchange.most_calls,
                    sink: exchange.piece,
                    restart: exchange.restart,
                    call: exchange.call,
                    stopped: exchange.stopped,
                    tools_done: exchange.tools_done,
                },
            )
            .map_err(said)?;
        Ok(leo_web::Reply {
            text,
            spent: answered.map(spent_of),
        })
    }
}

pub fn seer() -> leo_web::captions::Seer {
    Arc::new(
        |system: &str, user: &str, pictures: Vec<leo_web::captions::Picture>| {
            let images: Vec<leo_services::ai::provider::Image> = pictures
                .into_iter()
                .map(|p| leo_services::ai::provider::Image {
                    mime: p.mime,
                    bytes: p.bytes,
                })
                .collect();
            leo_services::ai::see(
                leo_services::ai::chat::Prompt {
                    system: system.to_string(),
                    user: user.to_string(),
                },
                &images,
                1_500,
            )
        },
    )
}

pub fn converser() -> leo_web::Converser {
    Arc::new(
        |given: &leo_web::Instructions, specs: &[leo_web::ToolSpec]| {
            let tools: Vec<ToolDef> = specs
                .iter()
                .map(|spec| ToolDef {
                    name: spec.name.clone(),
                    description: spec.description.clone(),
                    schema: spec.schema.clone(),
                })
                .collect();
            let systems = Systems {
                native: given.native,
                text: given.text,
            };
            leo_services::ai::open_session(&systems, &tools, given.light)
                .map(|session| Box::new(Talk(session)) as Box<dyn leo_web::Conversation>)
        },
    )
}

pub fn meaning() -> leo_web::Meaning {
    if leo_services::meaning::wanted() {
        std::thread::spawn(|| {
            let _ = leo_services::meaning::fetch(true);
        });
    }
    std::sync::Arc::new(|texts: &[String], query: bool| leo_services::meaning::embed(texts, query))
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use leo_web::usage::Purpose;

    #[test]
    fn only_a_margin_read_takes_the_toolless_path() {
        assert_eq!(route_of(Purpose::MarginRead), Route::Toolless);
        assert_eq!(route_of(Purpose::QuickReview), Route::Quick);
        assert_eq!(route_of(Purpose::Summary), Route::Quick);
        assert_eq!(route_of(Purpose::OtherWriting), Route::Full);
        assert_eq!(route_of(Purpose::Answer), Route::Full);
    }

    #[test]
    fn the_log_keeps_one_short_line_never_the_whole_body() {
        let body = format!("openai returned 429: {}\nprompt text", "x".repeat(500));
        let line = logged(&body);
        assert_eq!(line.chars().count(), 200);
        assert!(!line.contains("prompt text"));
    }
}
