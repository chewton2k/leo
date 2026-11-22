//! The domain: notes, the store that keeps them on disk, and the retention
//! choices in `keep`. Nothing here draws to a terminal or makes a network
//! request, which is what keeps it testable on its own.

pub const NO_WRITING_AI: &str = "leo can't write yet: ";

const SETUP_MARKERS: [&str; 10] = [
    "not signed in",
    "not logged in",
    "/login",
    "codex login",
    "rejected the key",
    "refused the key",
    "unauthorized",
    "not installed",
    "isn't installed",
    "no writing ai",
];

const DIDNT_ANSWER: &str = "Your writing AI didn't answer just now";

pub fn writing_said(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.starts_with(DIDNT_ANSWER) {
        return line.to_string();
    }
    let lower = line.to_lowercase();
    let prefixed = line.starts_with(NO_WRITING_AI)
        || SETUP_MARKERS.iter().any(|marker| lower.contains(marker))
        || (lower.contains("model") && lower.contains("not found"));
    if !prefixed {
        let reason = if ["429", "rate limit", "busy", "overloaded", "too many"]
            .iter()
            .any(|marker| lower.contains(marker))
        {
            "busy"
        } else if ["timed out", "timeout", "deadline"]
            .iter()
            .any(|marker| lower.contains(marker))
        {
            "timed out"
        } else {
            "unavailable"
        };
        return format!("{DIDNT_ANSWER} ({reason}). Try again in a moment.");
    }
    let line = line.strip_prefix(NO_WRITING_AI).unwrap_or(line);
    let line: String = line.chars().take(240).collect();
    match line.trim().trim_end_matches('.') {
        "" => format!("{NO_WRITING_AI}the writing AI didn't answer."),
        line => format!("{NO_WRITING_AI}{line}."),
    }
}

pub mod attachments;
pub mod call_names;
pub mod children;
pub mod diag;
pub mod filename;
pub mod keep;
pub mod notes;
pub mod open;
pub mod paths;
pub mod recording;
pub mod store;
pub mod tasks;
pub mod watch;

pub mod activity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setup_failure_is_one_short_sentence_after_the_prefix() {
        let said = writing_said("leo can't write yet: ollama: down\nmore");
        assert_eq!(said, "leo can't write yet: ollama: down.");
        assert_eq!(writing_said(&said), said);
        for auth in [
            "openai rejected the key stored in leo (401).",
            "claude ended. If it is not signed in, run it",
            "ollama: model 'x' not found",
            "claude: claude is not installed",
        ] {
            assert!(writing_said(auth).starts_with(NO_WRITING_AI), "{auth}");
        }
    }

    #[test]
    fn a_transient_failure_has_no_prefix_and_never_echoes_the_body() {
        for (text, reason) in [
            ("openai returned 429: prompt text here", "busy"),
            ("claude: request timed out", "timed out"),
            ("ollama returned 503: down", "unavailable"),
            ("", "unavailable"),
        ] {
            let said = writing_said(text);
            assert_eq!(
                said,
                format!(
                    "Your writing AI didn't answer just now ({reason}). Try again in a moment."
                )
            );
            assert!(!said.starts_with(NO_WRITING_AI));
            assert_eq!(writing_said(&said), said);
        }
    }
}
