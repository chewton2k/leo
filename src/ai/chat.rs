use anyhow::Result;

use crate::ai::chain::{run_chat_chain, ChainOutcome};
use crate::ai::provider::{build_chat_chain, ChatRequest};
use crate::config::secret::SecretStore;
use crate::config::Config;

/// Chat calls run at a low temperature: these are structuring tasks, not
/// creative ones.
const TEMPERATURE: f32 = 0.3;

pub fn build_structure_prompt(transcript: &str) -> String {
    format!(
        "You are a note-taking assistant. Given the following transcript from a lecture or meeting, \
         create well-structured notes in Markdown format.\n\n\
         Rules:\n\
         - The FIRST line must be ONLY a concise title (no # prefix, no formatting, just plain text)\n\
         - Follow it with a blank line, then the structured body\n\
         - Use bullet points (- ) for key points\n\
         - Use checkboxes (- [ ] ) for action items or to-dos mentioned\n\
         - Make tables when grouping like ideas\n\
         - Group related points under ## headings\n\
         - There will sometimes be noise in the transcription so make sure to filter out any extraneous information not related to the main topic \n\
         - Interweave your own notes with the structured output where you deem helpful \n\
         - Don't lose important details and capture notes that are meaningful\n\n\
         Transcript:\n{transcript}"
    )
}

pub fn build_append_prompt(transcript: &str, existing_body: &str) -> String {
    format!(
        "You are a note-taking assistant. You are adding to an EXISTING note. \
         Given the existing notes and a new transcript, create well-structured notes \
         for ONLY the new content in Markdown format.\n\n\
         Rules:\n\
         - Do NOT include a title — this will be appended to an existing note\n\
         - Use bullet points (- ) for key points\n\
         - Use checkboxes (- [ ] ) for action items or to-dos mentioned\n\
         - Group related points under ## headings\n\
         - Filter out noise from transcription\n\
         - Keep it concise but don't lose important details\n\
         - Avoid duplicating information already in the existing notes\n\
         - Use the same style and structure as the existing notes\n\n\
         Existing notes:\n{existing_body}\n\n\
         New transcript:\n{transcript}"
    )
}

pub fn build_expand_prompt(
    question: &str,
    local_context: &str,
    note_title: &str,
    full_body: &str,
) -> String {
    format!(
        "You are a note-taking assistant helping expand a specific section of lecture notes.\n\n\
         Topic: {note_title}\n\n\
         Full lecture notes (for background):\n{full_body}\n\n\
         Local context around the question:\n{local_context}\n\n\
         Question to expand on:\n{question}\n\n\
         Rules:\n\
         - Answer concisely in markdown (bullet points, short paragraphs)\n\
         - Tie your answer back to the lecture context where relevant\n\
         - Do not repeat what's already in the notes\n\
         - Return only the expanded content, no preamble"
    )
}

/// The model is asked to put a bare title on line one; everything after the
/// blank line is the body.
pub fn split_title_body(content: &str) -> (String, String) {
    let mut lines = content.lines();
    let title = lines
        .next()
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .to_string();
    let title = if title.is_empty() {
        "Untitled Notes".to_string()
    } else {
        title
    };
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (title, body)
}

/// Run one prompt through the configured chat chain.
pub fn complete(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: String,
    max_tokens: u32,
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        prompt,
        temperature: TEMPERATURE,
        max_tokens,
    };
    run_chat_chain(providers, &req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_prompt_embeds_the_transcript() {
        let p = build_structure_prompt("the mitochondria is the powerhouse");
        assert!(p.contains("the mitochondria is the powerhouse"));
        assert!(p.contains("FIRST line"));
    }

    #[test]
    fn append_prompt_embeds_both_transcript_and_existing_body() {
        let p = build_append_prompt("new stuff", "## Existing\n- old point");
        assert!(p.contains("new stuff"));
        assert!(p.contains("- old point"));
        assert!(p.contains("Do NOT include a title"));
    }

    #[test]
    fn expand_prompt_embeds_all_four_inputs() {
        let p = build_expand_prompt("what is BFS?", "local ctx", "Graphs", "full body here");
        assert!(p.contains("what is BFS?"));
        assert!(p.contains("local ctx"));
        assert!(p.contains("Graphs"));
        assert!(p.contains("full body here"));
    }

    #[test]
    fn split_title_takes_the_first_line() {
        let (title, body) = split_title_body("Lecture 4: Graphs\n\n- BFS\n- DFS");
        assert_eq!(title, "Lecture 4: Graphs");
        assert_eq!(body, "- BFS\n- DFS");
    }

    #[test]
    fn split_title_strips_a_markdown_heading_marker() {
        let (title, _) = split_title_body("## Lecture 4\n\nbody");
        assert_eq!(title, "Lecture 4");
    }

    #[test]
    fn split_title_handles_a_single_line_response() {
        let (title, body) = split_title_body("Just A Title");
        assert_eq!(title, "Just A Title");
        assert_eq!(body, "");
    }

    #[test]
    fn split_title_falls_back_when_empty() {
        let (title, _) = split_title_body("");
        assert_eq!(title, "Untitled Notes");
    }
}
