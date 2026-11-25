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
        Purpose::Answer
        | Purpose::ToolContinuation
        | Purpose::Retry
        | Purpose::StudyGrading
        | Purpose::OtherWriting
        | Purpose::Connections
        | Purpose::Combine
        | Purpose::Margin
        | Purpose::Recording
        | Purpose::Import
        | Purpose::Pictures
        | Purpose::Dictation => Route::Full,
    }
}

pub fn counted<T>(
    usage: &leo_web::usage::Usage,
    purpose: leo_web::usage::Purpose,
    run: impl FnOnce() -> anyhow::Result<leo_services::ai::chain::ChainOutcome<T>>,
) -> anyhow::Result<leo_services::ai::chain::ChainOutcome<T>> {
    counted_until(usage, purpose, &|| false, run)
}

pub fn counted_until<T>(
    usage: &leo_web::usage::Usage,
    purpose: leo_web::usage::Purpose,
    stopped: &dyn Fn() -> bool,
    run: impl FnOnce() -> anyhow::Result<leo_services::ai::chain::ChainOutcome<T>>,
) -> anyhow::Result<leo_services::ai::chain::ChainOutcome<T>> {
    let started = std::time::Instant::now();
    let result = run();
    if let Some((spent, failed)) = tally(&result, stopped) {
        usage.record(purpose, spent.as_ref(), started.elapsed(), failed);
    }
    result
}

pub fn tally<T>(
    result: &anyhow::Result<leo_services::ai::chain::ChainOutcome<T>>,
    stopped: &dyn Fn() -> bool,
) -> Option<(Option<leo_web::Spent>, bool)> {
    if result.is_err() && stopped() {
        return None;
    }
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
    Some((spent, result.is_err()))
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
        counted(&usage, purpose, || match route_of(purpose) {
            Route::Toolless => leo_core::children::with_stop(
                leo_core::children::Stop {
                    deadline: Some(started + leo_web::margin::READ_LIMIT),
                    ..Default::default()
                },
                || leo_services::ai::toolless_outcome(prompt, most),
            ),
            Route::Quick => leo_services::ai::quick_outcome(prompt, most),
            Route::Full => leo_services::ai::chat_outcome(prompt, most),
        })
        .map(|outcome| outcome.value)
        .map_err(said)
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
        kind: answered
            .kind
            .and_then(|kind| serde_json::to_value(kind).ok())
            .and_then(|kind| kind.as_str().map(str::to_string))
            .unwrap_or_default(),
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

pub fn seer(usage: Arc<leo_web::usage::Usage>) -> leo_web::captions::Seer {
    Arc::new(
        move |system: &str, user: &str, pictures: Vec<leo_web::captions::Picture>| {
            let images: Vec<leo_services::ai::provider::Image> = pictures
                .into_iter()
                .map(|p| leo_services::ai::provider::Image {
                    mime: p.mime,
                    bytes: p.bytes,
                })
                .collect();
            let prompt = leo_services::ai::chat::Prompt {
                system: system.to_string(),
                user: user.to_string(),
            };
            counted(&usage, leo_web::usage::Purpose::Pictures, || {
                leo_services::ai::see(prompt, &images, 1_500)
            })
            .map(|outcome| outcome.value)
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

fn check_and_fetch(
    wanted: impl FnOnce() -> bool + Send + 'static,
    fetch: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if wanted() {
            if let Err(error) = fetch() {
                leo_core::diag::warn(format!(
                    "The model that finds notes by meaning could not be downloaded: {error:#}. leo will try again when it opens."
                ));
            }
        }
    })
}

pub fn meaning() -> leo_web::Meaning {
    check_and_fetch(leo_services::meaning::wanted, || {
        leo_services::meaning::fetch().map(drop)
    });
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
        for full in Purpose::ALL.into_iter().filter(|purpose| {
            !matches!(
                purpose,
                Purpose::MarginRead | Purpose::QuickReview | Purpose::Summary
            )
        }) {
            assert_eq!(route_of(full), Route::Full, "{full:?}");
        }
    }

    #[test]
    fn a_counted_call_is_recorded_under_its_purpose() {
        let home = tempfile::tempdir().unwrap();
        let usage = leo_web::usage::Usage::default();
        usage.save_in(home.path().join("chats"));
        let answered = counted(&usage, Purpose::Import, || {
            Ok(leo_services::ai::chain::ChainOutcome {
                value: "notes",
                provider: "codex".into(),
                fallbacks: Vec::new(),
                spent: None,
            })
        });
        assert_eq!(answered.unwrap().value, "notes");
        let failed = counted::<()>(&usage, Purpose::Dictation, || anyhow::bail!("offline"));
        assert!(failed.is_err());
        let stopped = counted_until::<()>(&usage, Purpose::Recording, &|| true, || {
            anyhow::bail!("stopped")
        });
        assert!(stopped.is_err());
        let days = serde_json::to_value(usage.days()).unwrap();
        let mut kinds: Vec<_> = days["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| (kind["name"].as_str().unwrap(), kind["calls"].as_u64()))
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, [("dictation", Some(1)), ("import", Some(1))]);
        assert_eq!(
            tally(
                &anyhow::Result::<leo_services::ai::chain::ChainOutcome<()>>::Err(anyhow::anyhow!(
                    "offline"
                )),
                &|| false
            ),
            Some((None, true))
        );
    }

    #[test]
    fn a_codex_answer_carries_the_kind_the_warm_cache_looks_for() {
        let codex = spent_of(Answered {
            kind: Some(leo_services::config::provider::ProviderKind::Codex),
            ..Answered::default()
        });
        assert_eq!(codex.kind, "codex");
        assert_eq!(spent_of(Answered::default()).kind, "");
    }

    #[test]
    fn the_log_keeps_one_short_line_never_the_whole_body() {
        let body = format!("openai returned 429: {}\nprompt text", "x".repeat(500));
        let line = logged(&body);
        assert_eq!(line.chars().count(), 200);
        assert!(!line.contains("prompt text"));
    }

    #[test]
    fn the_model_check_does_not_run_on_the_caller() {
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (asked, checked) = std::sync::mpsc::channel::<()>();
        let fetched = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = Arc::clone(&fetched);
        let thread = check_and_fetch(
            move || {
                asked.send(()).unwrap();
                held.recv().unwrap();
                true
            },
            move || {
                seen.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        );
        checked
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!thread.is_finished());
        assert!(!fetched.load(std::sync::atomic::Ordering::SeqCst));
        release.send(()).unwrap();
        thread.join().unwrap();
        assert!(fetched.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn a_failed_fetch_does_not_panic_the_thread() {
        let thread = check_and_fetch(|| true, || Err(anyhow::anyhow!("offline")));
        thread.join().unwrap();
    }

    #[test]
    fn a_model_that_is_there_is_not_fetched() {
        let thread = check_and_fetch(|| false, || panic!("fetched a model that is there"));
        thread.join().unwrap();
    }
}
