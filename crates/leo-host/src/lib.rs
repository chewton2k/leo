mod felix;
pub mod health;
mod housekeeping;
mod powers;
mod record;
mod settings;
mod speech;

pub use housekeeping::Housekeeping;
pub use powers::powers;
pub use settings::WebSettings;

pub fn stop_writing() {
    leo_services::ai::stop_writing();
}

pub fn polish(
    usage: std::sync::Arc<leo_web::usage::Usage>,
    text: &str,
    writing: leo_services::dictation::settings::Writing,
    language: Option<&str>,
    app: Option<&str>,
    words: &[String],
    cancelled: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<String, &'static str> {
    leo_services::dictation::polish::polish(
        text,
        writing,
        language,
        app,
        words,
        cancelled,
        move |prompt, tokens, quick, stop| {
            felix::counted(&usage, leo_web::usage::Purpose::Dictation, || {
                leo_services::ai::dictation_outcome(prompt, tokens, quick, stop)
            })
            .map(|outcome| outcome.value)
        },
    )
}
