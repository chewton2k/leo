use anyhow::Result;

use crate::ai::chain::{run_chat_chain, ChainOutcome};
use crate::ai::provider::{build_chat_chain, ChatRequest};
use crate::config::secret::SecretStore;
use crate::config::Config;

/// Chat calls run at a low temperature: these are structuring tasks, not
/// creative ones.
const TEMPERATURE: f32 = 0.3;

/// A point the listener typed while recording, and how far in they typed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jotted {
    pub at_secs: u64,
    pub text: String,
}

/// `m:ss` as `mm:ss`, or `h:mm:ss` past an hour.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Typed points with no speech to go with them, as a note body.
pub fn points_as_markdown(points: &[Jotted]) -> String {
    let mut body = "## Key points\n".to_string();
    for p in points {
        body.push_str(&format!("- {}\n", p.text));
    }
    body
}

/// A request in two parts: standing instructions, sent as the system message,
/// and the material to work on, sent as the user's message. Models follow rules
/// given this way more reliably than rules mixed into the material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// How the speech-recognition text is to be read. Shared by the new-note and
/// append prompts so they cannot disagree about it.
const READING_A_TRANSCRIPT: &str = "\
The transcript comes from speech recognition, so expect mistakes:
- Fix words that were clearly misheard, using the subject for context (\"breath first search\" is \"breadth-first search\"). Do not guess beyond that.
- Leave out filler, repetition, and anything unrelated to the topic.

Where the recording is patchy or the speaker was vague, fill the gap with accurate explanation from your own knowledge, so the notes make sense on their own. Add what helps someone understand the topic, not tangents.";

/// Formatting rules both note prompts share.
const FORMATTING: &str = "\
- Use interpretable language: plain words someone new to the subject can follow, with each technical term explained the first time it appears.
- Bold a term where it is defined. Write math in LaTeX: $...$ inside a sentence and $$...$$ on lines of their own. Put code in fenced code blocks.
- Use a table only to compare two or more things across the same attributes.
- Put real commitments in a final \"## Action items\" section as checkboxes (- [ ] ): assignments and submissions, exam and quiz dates, meetings, and follow-ups the speaker or the material actually stated, and only if there are some; otherwise leave the section out. Keep only the fewest that matter: one checkbox per deliverable or date (a homework, an exam, a meeting), never its sub-steps, suggested readings or general advice like \"review the slides\". Practice questions belong in Check yourself, never here.
- In Action items, write each date in full with its weekday and year, like \"— due Friday, October 16, 2026 at 11:59 pm\", working out words like \"Friday\" or \"next week\" from the date in <recorded_on>. If no date is given there, or a date cannot be worked out, keep the words as they were said. Never invent a task, date, time or owner. Only the listener's own commitments are checkboxes; a job someone else was given is a plain bullet under them that names who (\"- Maya sends the slides\").";

pub const LEARNING: &str = "\
How to shape the notes so they are understood and remembered:
- Tell it as a story a learner can follow: open with why the topic matters (the question or problem it answers), give the core idea in plain words before any detail, then build up from the simple case to the full picture, each section leaning on the one before. Make each heading say what its section teaches.
- Make every abstract idea concrete right where it appears: a short example, a worked case or an analogy first, then the precise version.
- Say how ideas connect to each other, and point out the common mistakes and misunderstandings.
- Where a process, structure, comparison or timeline is easier to see than read, add one drawing in a ```svg block: viewBox=\"0 0 320 H\" (H at most 400), xmlns and a <title>, thin round strokes (width 1 to 1.5, fill none), short <text> labels (font-size 13 to 16, fill var(--text), no stroke), colours only var(--text), var(--muted), var(--accent) or var(--accent-soft), each step a top-level <g data-step=\"1\" data-caption=\"…\"> group (2 to 5 steps, each adding to the last); where the material has a picture that explains something, put it there.
- For code or a programming problem: first break the problem down under its own heading (what is asked, the inputs and outputs, the constraints, the key insight, the approach step by step, and its time and space cost), then give the complete code in one fenced block with its language, then explain it in foldable sections, one per part of the code, written as Obsidian callouts like this:
  > [!example]- How the loop picks the next node
  > The explanation, in plain words.
- End with a \"## Check yourself\" section: 3 to 6 questions that make the reader recall the key ideas, each written as a foldable callout whose body is the answer, like this:
  > [!question]- What does breadth-first search use to pick the next node?
  > A queue: the node found earliest is explored first.";

/// What to do with points the listener typed while recording.
const POINTS_RULE: &str = "\
The listener typed points while recording; they show what mattered most to them. Put each one in the section where its topic belongs, in the listener's exact words: copy it as written, as a bullet or a sentence of that section, and build your explanation from the recording around it. Never reword, shorten, merge or drop a typed point. Do not give them a section of their own, do not mark them out, and do not add times. If the transcript never mentions one, put it with the closest topic and explain it from your own knowledge.";

fn points_rule(points: &[Jotted]) -> String {
    if points.is_empty() {
        String::new()
    } else {
        format!("\n\n{POINTS_RULE}")
    }
}

/// The typed points as the user message lists them.
fn points_block(points: &[Jotted]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let list: String = points.iter().map(|p| format!("- {}\n", p.text)).collect();
    format!("<typed_points>\n{list}</typed_points>\n\n")
}

/// The prompt that turns a recording into a new note. Typed points, when there
/// are any, are woven into the sections they belong to.
pub fn build_structure_prompt_with(transcript: &str, points: &[Jotted]) -> Prompt {
    let key_points = points_rule(points);
    let system = format!(
        "You turn lecture and meeting transcripts into study notes in Markdown.

{READING_A_TRANSCRIPT}

Shape of the reply:
1. The first line is the title, as plain text: no \"Title:\", no #, no quotes, no bold.
2. A blank line, then a 2-3 sentence summary of the whole recording.
3. ## sections for the topics, in the order they came up unless another order tells the story more clearly, with bullet points (- ).
{FORMATTING}

{LEARNING}{key_points}

Reply with the note only: no preamble before the title, no remarks after the note, and do not wrap it in a code block."
    );
    let user = format!(
        "{}<transcript>\n{transcript}\n</transcript>\n\n\
         Write the notes for this transcript in interpretable language: the title alone on the \
         first line, then the summary, then the sections.",
        points_block(points)
    );
    Prompt { system, user }
}

/// The prompt that turns a recording into an addition to an existing note.
pub fn build_append_prompt_with(
    transcript: &str,
    existing_body: &str,
    points: &[Jotted],
) -> Prompt {
    let key_points = points_rule(points);
    let system = format!(
        "You add to an existing set of notes in Markdown, from a new transcript.

{READING_A_TRANSCRIPT}

Rules for the addition:
- Write only the new material. No title, no summary of the existing notes.
- Do not repeat anything the existing notes already cover.
- Match the existing notes' style; start each new topic with a ## heading and use bullet points (- ).
{FORMATTING}

{LEARNING}{key_points}

Reply with the addition only: no preamble, no remarks after it, and do not wrap it in a code block."
    );
    let user = format!(
        "<existing_notes>\n{existing_body}\n</existing_notes>\n\n{}<transcript>\n{transcript}\n</transcript>\n\n\
         Write only the new notes to add for this transcript, in interpretable language, with no \
         title.",
        points_block(points)
    );
    Prompt { system, user }
}

pub fn build_part_prompt(
    transcript: &str,
    points: &[Jotted],
    part: usize,
    parts: usize,
    span: &str,
) -> Prompt {
    let key_points = points_rule(points);
    let system = format!(
        "You turn one part of a long lecture or meeting transcript into study notes in Markdown.

{READING_A_TRANSCRIPT}

This is part {part} of {parts} ({span}). Other parts are handled separately, so:
- No title and no summary of the whole recording.
- ## sections for the topics in this part, in the order they came up, with bullet points (- ).
{FORMATTING}

{LEARNING} Leave \"## Check yourself\" to the last part only (part {parts}).{key_points}

Reply with the notes for this part only: no preamble, no remarks after them, and do not wrap them in a code block."
    );
    let user = format!(
        "{}<transcript>\n{transcript}\n</transcript>\n\n\
         Write the notes for this part ({span}) in interpretable language: ## sections only, no \
         title.",
        points_block(points)
    );
    Prompt { system, user }
}

pub fn build_draft_prompt(
    notes_so_far: &str,
    transcript: &str,
    points: &[Jotted],
    span: &str,
) -> Prompt {
    let key_points = points_rule(points);
    let system = format!(
        "You write study notes in Markdown from a lecture or meeting while it is still being recorded, one stretch at a time.

{READING_A_TRANSCRIPT}

The notes already written for the earlier part of the recording are in <notes_so_far> (empty at the start); they stay as they are. The transcript of the next stretch ({span}) is in <transcript>. The recording goes on after it, so:
- No title and no summary: those are written when the recording ends.
- ## sections for the topics in <transcript> only, in the order they came up, with bullet points (- ). Continue from where <notes_so_far> stops; never repeat what it already covers.
{FORMATTING}

{LEARNING}{key_points}

Leave \"## Check yourself\" to the end of the recording: do not write it now. Put only the commitments stated in this stretch in the final \"## Action items\", as above; they are gathered into the finished note.

Reply with the new sections only: no preamble, no remarks after them, and do not wrap them in a code block."
    );
    let user = format!(
        "{}<notes_so_far>\n{notes_so_far}\n</notes_so_far>\n\n<transcript>\n{transcript}\n</transcript>\n\n\
         Write the notes for this stretch ({span}) in interpretable language: ## sections only, \
         continuing the notes so far.",
        points_block(points)
    );
    Prompt { system, user }
}

pub fn build_close_prompt(
    notes_so_far: &str,
    commitments: &str,
    transcript: &str,
    points: &[Jotted],
) -> Prompt {
    let key_points = points_rule(points);
    let system = format!(
        "You finish study notes in Markdown for a lecture or meeting whose start is already written up.

{READING_A_TRANSCRIPT}

The notes for the start of the recording are in <notes_so_far>; they stay as they are. The transcript of the rest of the recording is in <transcript> (it may be empty).

Shape of the reply:
1. The first line is the title of the whole recording, as plain text: no \"Title:\", no #, no quotes, no bold.
2. A blank line, then a 2-3 sentence summary of the whole recording.
3. ## sections for the topics in <transcript> only, in the order they came up, with bullet points (- ). Continue from where <notes_so_far> stops; never repeat what it already covers.
{FORMATTING}

{LEARNING}{key_points}

\"## Action items\" gathers the commitments already found, in <commitments_so_far>, with any in <transcript>, and \"## Check yourself\" covers the whole recording, the notes so far included.

Reply with the title, the summary and the new sections only: no preamble, no remarks after them, and do not wrap them in a code block."
    );
    let user = format!(
        "{}<notes_so_far>\n{notes_so_far}\n</notes_so_far>\n\n<commitments_so_far>\n{commitments}\n</commitments_so_far>\n\n<transcript>\n{transcript}\n</transcript>\n\n\
         Finish the notes in interpretable language: the title alone on the first line, then the \
         summary, then the sections for this transcript only.",
        points_block(points)
    );
    Prompt { system, user }
}

pub fn build_summary_prompt(part_notes: &str) -> Prompt {
    let system = "You name and summarize a long recording from the notes already taken on it.

Shape of the reply:
1. The first line is the title, as plain text: no \"Title:\", no #, no quotes, no bold.
2. A blank line, then a 2-4 sentence summary of the whole recording.

Use interpretable language: plain words someone new to the subject can follow, with each technical term explained the first time it appears.

Reply with the title and summary only: no preamble and no remarks after them."
        .to_string();
    let user = format!(
        "<notes>\n{part_notes}\n</notes>\n\nWrite the title alone on the first line, then the summary."
    );
    Prompt { system, user }
}

/// Strip what models wrap around an answer despite being asked not to: a code
/// block around the whole reply, a preamble line ("Here are your notes:"), and
/// a sign-off ("Let me know if…").
pub fn clean_reply(reply: &str) -> String {
    let mut text = reply.trim();

    // A fence around the whole reply, not one inside it.
    if let Some(rest) = text.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            if !inner.contains("```") {
                text = inner.split_once('\n').map_or("", |(_, body)| body).trim();
            }
        }
    }

    let mut lines: Vec<&str> = text.lines().collect();
    while lines
        .first()
        .is_some_and(|l| l.trim().is_empty() || is_preamble(l))
    {
        lines.remove(0);
    }
    while lines
        .last()
        .is_some_and(|l| l.trim().is_empty() || is_sign_off(l))
    {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

/// A chatty opener, not a title: it starts like one and ends like a sentence,
/// so "Sure-fire study tricks" stays a title.
fn is_preamble(line: &str) -> bool {
    let l = line.trim().to_lowercase();
    if !l.ends_with([':', '.', '!']) {
        return false;
    }
    const OPENERS: &[&str] = &[
        "here are",
        "here is",
        "here's",
        "sure",
        "certainly",
        "of course",
        "below are",
        "below is",
    ];
    OPENERS.iter().any(|o| l.starts_with(o))
}

fn is_sign_off(line: &str) -> bool {
    let l = line.trim().to_lowercase();
    const CLOSERS: &[&str] = &[
        "let me know",
        "i hope this",
        "hope this helps",
        "feel free to",
    ];
    CLOSERS.iter().any(|c| l.starts_with(c))
}

/// Split a reply into (title, body): the first line is the title, cleaned of
/// the "Title:", `#`, bold and quotes models add, and the rest is the body.
pub fn split_title_body(content: &str) -> (String, String) {
    let cleaned = clean_reply(content);
    let mut lines = cleaned.lines();
    let mut title = lines.next().unwrap_or("").trim();
    loop {
        let before = title;
        title = title.trim_start_matches('#').trim();
        title = title.trim_matches('*').trim();
        for label in ["Title:", "title:", "TITLE:"] {
            if let Some(rest) = title.strip_prefix(label) {
                title = rest.trim();
            }
        }
        title = title
            .trim_matches(|c| c == '"' || c == '\'' || c == '“' || c == '”')
            .trim();
        if title == before {
            break;
        }
    }
    let title = if title.is_empty() {
        "Untitled Notes".to_string()
    } else {
        title.to_string()
    };
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (title, body)
}

/// Run one prompt through the configured chat chain.
pub fn complete(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: Prompt,
    max_tokens: u32,
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        system: Some(prompt.system),
        prompt: prompt.user,
        temperature: TEMPERATURE,
        max_tokens,
    };
    run_chat_chain(providers, &req)
}

pub fn complete_until(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: Prompt,
    max_tokens: u32,
    stopped: &dyn Fn() -> bool,
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        system: Some(prompt.system),
        prompt: prompt.user,
        temperature: TEMPERATURE,
        max_tokens,
    };
    crate::ai::chain::run_chat_chain_until(providers, &req, stopped)
}

/// The same completion, delivered as it arrives.
pub fn complete_streaming(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: Prompt,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
    stopped: &dyn Fn() -> bool,
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        system: Some(prompt.system),
        prompt: prompt.user,
        temperature: TEMPERATURE,
        max_tokens,
    };
    crate::ai::chain::run_chat_chain_streaming(providers, &req, on_fragment, on_restart, stopped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_writing_instruction_asks_for_interpretable_language() {
        let prompts = [
            build_structure_prompt_with("t", &[]),
            build_append_prompt_with("t", "body", &[]),
            build_part_prompt("t", &[], 1, 2, "0:00-5:00"),
            build_draft_prompt("n", "t", &[], "0:00-5:00"),
            build_close_prompt("n", "", "t", &[]),
            build_summary_prompt("notes"),
        ];
        for prompt in prompts {
            assert!(
                prompt.system.contains("Use interpretable language"),
                "{}",
                prompt.system
            );
        }
    }

    #[test]
    fn recording_notes_follow_the_learning_guide() {
        for prompt in [
            build_structure_prompt_with("t", &[]),
            build_append_prompt_with("t", "body", &[]),
            build_part_prompt("t", &[], 1, 2, "0:00-5:00"),
            build_draft_prompt("n", "t", &[], "0:00-5:00"),
            build_close_prompt("n", "", "t", &[]),
        ] {
            assert!(prompt.system.contains(LEARNING), "{}", prompt.system);
        }
        assert!(build_part_prompt("t", &[], 1, 3, "x")
            .system
            .contains("to the last part only (part 3)"));
    }

    #[test]
    fn the_last_words_after_a_transcript_ask_for_interpretable_language_again() {
        for prompt in [
            build_structure_prompt_with("t", &[]),
            build_append_prompt_with("t", "body", &[]),
            build_part_prompt("t", &[], 1, 2, "0:00-5:00"),
            build_draft_prompt("n", "t", &[], "0:00-5:00"),
            build_close_prompt("n", "", "t", &[]),
        ] {
            let after = prompt.user.rsplit("</transcript>").next().unwrap();
            assert!(after.contains("interpretable language"), "{after}");
        }
    }

    fn points() -> Vec<Jotted> {
        vec![
            Jotted {
                at_secs: 134,
                text: "BFS uses a queue".to_string(),
            },
            Jotted {
                at_secs: 610,
                text: "exam: know Dijkstra".to_string(),
            },
        ]
    }

    /// With no speech, the typed points are still a note.
    #[test]
    fn points_alone_make_a_note_body() {
        let body = points_as_markdown(&points());
        assert!(body.starts_with("## Key points\n"), "{body}");
        assert!(body.contains("- BFS uses a queue\n"), "{body}");
        assert!(!body.contains("02:14"), "{body}");
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

    // ── reading the reply ───────────────────────────────────────────────────

    /// The ways free models dress up a title, each of which used to become the
    /// note's title verbatim.
    #[test]
    fn a_dressed_up_title_is_cleaned() {
        for reply in [
            "Lecture 4: Graphs\n\n- BFS",
            "# Lecture 4: Graphs\n\n- BFS",
            "Title: Lecture 4: Graphs\n\n- BFS",
            "**Lecture 4: Graphs**\n\n- BFS",
            "\"Lecture 4: Graphs\"\n\n- BFS",
            "**Title:** Lecture 4: Graphs\n\n- BFS",
        ] {
            let (title, body) = split_title_body(reply);
            assert_eq!(title, "Lecture 4: Graphs", "from {reply:?}");
            assert_eq!(body, "- BFS", "from {reply:?}");
        }
    }

    #[test]
    fn a_preamble_before_the_title_is_skipped() {
        for reply in [
            "Here are your notes:\n\nLecture 4: Graphs\n\n- BFS",
            "Sure! Here are the structured notes.\nLecture 4: Graphs\n\n- BFS",
            "Certainly, here is the note:\n\n# Lecture 4: Graphs\n\n- BFS",
        ] {
            let (title, body) = split_title_body(reply);
            assert_eq!(title, "Lecture 4: Graphs", "from {reply:?}");
            assert_eq!(body, "- BFS", "from {reply:?}");
        }
    }

    #[test]
    fn a_title_that_merely_starts_like_a_preamble_is_kept() {
        assert_eq!(
            split_title_body("Sure-fire study tricks\n\n- sleep").0,
            "Sure-fire study tricks"
        );
    }

    /// A reply wrapped in a code block would otherwise save the fences into
    /// the note.
    #[test]
    fn a_reply_wrapped_in_a_code_block_is_unwrapped() {
        let (title, body) = split_title_body("```markdown\nLecture 4\n\n- BFS\n```");
        assert_eq!(title, "Lecture 4");
        assert_eq!(body, "- BFS");
        assert_eq!(clean_reply("```\n- added point\n```"), "- added point");
    }

    /// A code block inside the note is content, not wrapping.
    #[test]
    fn a_code_block_inside_the_note_is_kept() {
        let reply = "Lecture 4\n\n```python\nprint(1)\n```\n\n- BFS";
        let (_, body) = split_title_body(reply);
        assert!(body.contains("```python\nprint(1)\n```"), "{body}");
    }

    #[test]
    fn a_sign_off_after_the_note_is_dropped() {
        let reply = "Lecture 4\n\n- BFS\n\nLet me know if you want more detail!";
        assert_eq!(split_title_body(reply).1, "- BFS");
        assert_eq!(clean_reply("- BFS\n\nI hope this helps."), "- BFS");
    }

    // ── the structure prompt ────────────────────────────────────────────────

    #[test]
    fn rules_go_in_the_system_message_and_the_transcript_in_the_user_message() {
        let p = build_structure_prompt_with("the mitochondria is the powerhouse", &[]);
        assert!(p.system.contains("first line is the title"), "{}", p.system);
        assert!(!p.system.contains("mitochondria"));
        assert!(
            p.user
                .contains("<transcript>\nthe mitochondria is the powerhouse\n</transcript>"),
            "{}",
            p.user
        );
    }

    /// Long transcripts push early instructions out of a model's attention, so
    /// the essentials are repeated after the material.
    #[test]
    fn the_rules_are_restated_after_the_transcript() {
        let p = build_structure_prompt_with("some speech", &[]);
        let after = &p.user[p.user.find("</transcript>").unwrap()..];
        assert!(after.contains("title"), "{}", p.user);
    }

    #[test]
    fn the_structure_prompt_asks_for_a_clean_useful_shape() {
        let s = build_structure_prompt_with("x", &[]).system;
        for wanted in [
            "misheard",      // fix speech-recognition errors
            "own knowledge", // fill gaps the recording or speaker left
            "summary",       // a short summary under the title
            "order",         // sections follow the lecture
            "Bold",          // defined terms stand out
            "code block",    // formulas and code
            "LaTeX",         // math the page can draw
            "compare",       // tables only for real comparisons
            "Action items",  // tasks only when there were some
            "no preamble",   // nothing before the title
            "one checkbox per deliverable",
        ] {
            assert!(
                s.contains(wanted),
                "structure prompt lacks {wanted:?}:\n{s}"
            );
        }
    }

    #[test]
    fn without_typed_points_the_prompt_says_nothing_about_them() {
        let p = build_structure_prompt_with("some speech", &[]);
        assert!(!p.system.contains("typed points"));
        assert!(!p.user.contains("<typed_points>"));
    }

    /// What the listener typed is what they found important, so it goes into
    /// the notes where it belongs, with no section of its own and no times.
    #[test]
    fn typed_points_are_woven_in_without_times() {
        let transcript = vec!["word"; 900].join(" ");
        let p = build_structure_prompt_with(&transcript, &points());
        assert!(
            p.system.contains("in the listener's exact words"),
            "{}",
            p.system
        );
        assert!(p.system.contains("do not add times"), "{}", p.system);
        assert!(!p.system.contains("## Key points"), "{}", p.system);
        assert!(
            p.user.contains("<typed_points>\n- BFS uses a queue\n"),
            "{}",
            p.user
        );
        assert!(!p.user.contains("02:14"), "{}", p.user);
        assert!(!p.user.contains("[~"), "{}", p.user);
    }

    #[test]
    fn typed_points_reach_an_append_too() {
        let p = build_append_prompt_with("more", "## Existing", &points());
        assert!(p.user.contains("BFS uses a queue"), "{}", p.user);
        assert!(
            p.user
                .contains("<existing_notes>\n## Existing\n</existing_notes>"),
            "{}",
            p.user
        );
        assert!(p
            .system
            .contains("Never reword, shorten, merge or drop a typed point"));
    }

    #[test]
    fn an_append_writes_only_the_new_material() {
        let s = build_append_prompt_with("new stuff", "## Existing", &[]).system;
        assert!(s.contains("No title"), "{s}");
        assert!(s.contains("Do not repeat"), "{s}");
        assert!(s.contains("misheard"), "{s}");
    }
}
