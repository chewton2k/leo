use anyhow::Result;

use super::chat::{self, clock, points_as_markdown, Jotted, Prompt};
use crate::session::Part;

const SUMMARY_INPUT_CHARS: usize = 24_000;

type Written = (String, Option<String>);

pub type Chat<'a> = &'a (dyn Fn(Prompt, u32) -> Result<String> + Sync);
pub type Progress<'a> = &'a (dyn Fn(usize, usize) + Sync);

pub fn recorded_on(mut prompt: Prompt, day: chrono::NaiveDate) -> Prompt {
    prompt.user = format!(
        "<recorded_on>{}</recorded_on>\n\n{}",
        day.format("%A, %B %-d, %Y"),
        prompt.user
    );
    prompt
}

fn read_of(text: &str, most: usize) -> &str {
    let text = text.trim();
    text.char_indices()
        .nth(most)
        .map_or(text, |(at, _)| text[..at].trim_end())
}

pub fn with_profile(prompt: Prompt, profile: &leo_core::recording::Profile) -> Prompt {
    let mut prompt = recorded_on(prompt, chrono::Local::now().date_naive());
    let context = read_of(&profile.context, leo_core::recording::CONTEXT_READ_CHARS);
    if !context.is_empty() {
        prompt.user = format!(
            "<recording_context>\n{context}\n</recording_context>\n\n{}",
            prompt.user
        );
    }
    let wants = read_of(&profile.wants, leo_core::recording::WANTS_READ_CHARS);
    if !wants.is_empty() {
        prompt.system.push_str(&format!("\n\n{WANTS}"));
        prompt.user = format!(
            "<what_the_user_wants>\n{wants}\n</what_the_user_wants>\n\n{}\n\nFollow what the user wants from these notes.",
            prompt.user
        );
    }
    prompt
}

const WANTS: &str = "The user said what they want from these notes in <what_the_user_wants>. Shape the notes around it wherever it fits (what to focus on, how to lay them out, how deep to go, what to emphasise), while still keeping everything important from the recording, filling gaps where the speaker was unclear and weaving in the user's typed points. Use interpretable language.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structured {
    pub title: String,
    pub body: String,
    pub problems: Vec<String>,
    pub writer_failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub start_secs: u64,
    pub end_secs: u64,
    pub text: String,
}

pub fn groups(parts: &[Part], words_per_group: usize) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    let mut words = 0;
    for part in parts {
        let count = part.text.split_whitespace().count();
        let start_new = match out.last() {
            None => true,
            Some(_) => words > 0 && words + count > words_per_group,
        };
        if start_new {
            out.push(Group {
                start_secs: part.start_secs,
                end_secs: part.end_secs,
                text: String::new(),
            });
            words = 0;
        }
        let group = out.last_mut().expect("pushed above");
        group.text = super::live::stitch(&group.text, &part.text);
        group.end_secs = part.end_secs;
        words += count;
    }
    out.retain(|g| !g.text.trim().is_empty());
    out
}

fn span(g: &Group) -> String {
    format!("{}–{}", clock(g.start_secs), clock(g.end_secs))
}

fn raw(points: &[Jotted], heading: &str, text: &str) -> String {
    let mut body = String::new();
    if !points.is_empty() {
        body.push_str(&points_as_markdown(points));
        body.push('\n');
    }
    body.push_str(&format!("## {heading}\n\n{}\n", text.trim()));
    body
}

fn points_for(points: &[Jotted], groups: &[Group], index: usize) -> Vec<Jotted> {
    let starts_after = |at: u64| {
        groups
            .get(index + 1)
            .is_none_or(|next| at < next.start_secs)
    };
    let starts_from = |at: u64| index == 0 || at >= groups[index].start_secs;
    points
        .iter()
        .filter(|p| starts_from(p.at_secs) && starts_after(p.at_secs))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Drafted {
    pub upto: u32,
    pub key: String,
    pub notes: String,
    #[serde(default)]
    pub commitments: String,
}

const DRAFT_TAIL_CHARS: usize = 4_000;
const DRAFT_CONTEXT_CHARS: usize = 8_000;

fn boundary(parts: &[Part], upto: u32) -> u64 {
    parts
        .iter()
        .filter(|p| p.index < upto)
        .map(|p| p.end_secs)
        .max()
        .unwrap_or(0)
}

pub fn draft_key(
    parts: &[Part],
    points: &[Jotted],
    profile: &leo_core::recording::Profile,
    upto: u32,
) -> String {
    use sha2::{Digest, Sha256};
    let end = boundary(parts, upto);
    let texts: Vec<(u32, &str)> = parts
        .iter()
        .filter(|p| p.index < upto)
        .map(|p| (p.index, p.text.as_str()))
        .collect();
    let typed: Vec<(u64, &str)> = points
        .iter()
        .filter(|p| p.at_secs < end)
        .map(|p| (p.at_secs, p.text.as_str()))
        .collect();
    let bytes = serde_json::to_vec(&(texts, typed, profile)).unwrap_or_default();
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        Sha256::digest(bytes),
    )
}

pub fn notes_context(notes: &str, cap: usize) -> String {
    if notes.chars().count() <= cap {
        return notes.trim().to_string();
    }
    let chars: Vec<char> = notes.chars().collect();
    let tail_len = DRAFT_TAIL_CHARS.min(cap / 2);
    let tail: String = chars[chars.len() - tail_len..].iter().collect();
    let mut room = cap - tail_len;
    let mut headings: Vec<&str> = notes
        .lines()
        .filter(|l| l.starts_with("## "))
        .rev()
        .take_while(|h| {
            let fits = h.chars().count() < room;
            room = room.saturating_sub(h.chars().count() + 1);
            fits
        })
        .collect();
    headings.reverse();
    format!(
        "Headings so far:\n{}\n\nThe last part of the notes:\n…{}",
        headings.join("\n"),
        tail.trim_end()
    )
}

pub fn split_drafted(reply: &str) -> (String, String) {
    let (mut notes, mut commitments) = (Vec::new(), Vec::new());
    let mut into = 0;
    for line in reply.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            into = match heading.trim().to_lowercase().as_str() {
                "action items" => 1,
                "check yourself" => 2,
                _ => 0,
            };
            if into > 0 {
                continue;
            }
        }
        match into {
            0 => notes.push(line),
            1 if !line.trim().is_empty() => commitments.push(line),
            _ => {}
        }
    }
    (notes.join("\n").trim().to_string(), commitments.join("\n"))
}

pub fn joined(first: &str, second: &str) -> String {
    [first.trim(), second.trim()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn text_from(parts: &[Part], from: usize, to: usize) -> String {
    let joined = |slice: &[Part]| {
        slice
            .iter()
            .fold(String::new(), |out, p| super::live::stitch(&out, &p.text))
    };
    let prior = joined(&parts[..from]);
    let full = joined(&parts[..to]);
    full.strip_prefix(prior.as_str())
        .map_or_else(|| joined(&parts[from..to]), |rest| rest.trim().to_string())
}

fn points_between(points: &[Jotted], from: u64, to: Option<u64>) -> Vec<Jotted> {
    points
        .iter()
        .filter(|p| p.at_secs >= from && to.is_none_or(|to| p.at_secs < to))
        .cloned()
        .collect()
}

fn fits(prompt: &Prompt, budget: super::budget::Budget) -> bool {
    prompt.system.chars().count() + prompt.user.chars().count() <= budget.chars()
}

pub fn draft_prompt(
    new_parts: &[Part],
    notes: &str,
    points: &[Jotted],
    budget: super::budget::Budget,
) -> Option<(Prompt, usize)> {
    let start = new_parts.first()?.start_secs;
    let cap = (budget.chars() / 3).min(DRAFT_CONTEXT_CHARS);
    [cap, cap / 8].into_iter().find_map(|cap| {
        let context = notes_context(notes, cap);
        (1..=new_parts.len()).rev().find_map(|n| {
            let end = new_parts[n - 1].end_secs;
            let text = new_parts[..n]
                .iter()
                .fold(String::new(), |out, p| super::live::stitch(&out, &p.text));
            let typed = points_between(points, start, Some(end));
            if text.trim().is_empty() && typed.is_empty() {
                return None;
            }
            let span = format!("{}–{}", clock(start), clock(end));
            let prompt = chat::build_draft_prompt(&context, &text, &typed, &span);
            fits(&prompt, budget).then_some((prompt, n))
        })
    })
}

fn untitled(reply: &str) -> bool {
    reply.starts_with("## ")
        && reply
            .lines()
            .skip(1)
            .find(|l| !l.trim().is_empty())
            .is_none_or(|l| {
                ["- ", "* ", "#", ">", "|", "```", "$$", "1. "]
                    .iter()
                    .any(|m| l.starts_with(m))
            })
}

fn commitments_kept(body: String, commitments: &str) -> String {
    let flat = body.to_lowercase();
    let missing: Vec<&str> = commitments
        .lines()
        .filter(|l| {
            let text = l.trim().trim_start_matches(['-', '*', ' ']);
            let text = text
                .trim_start_matches("[ ]")
                .trim_start_matches("[x]")
                .trim();
            !text.is_empty() && !flat.contains(&text.to_lowercase())
        })
        .collect();
    if missing.is_empty() {
        return body;
    }
    let mut lines: Vec<&str> = body.lines().collect();
    let find = |lines: &[&str], heading: &str| lines.iter().position(|l| l.trim() == heading);
    match (
        find(&lines, "## Action items"),
        find(&lines, "## Check yourself"),
    ) {
        (Some(i), _) => drop(lines.splice(i + 1..i + 1, missing)),
        (None, Some(k)) => {
            let section = std::iter::once("## Action items")
                .chain(missing)
                .chain([""]);
            drop(lines.splice(k..k, section));
        }
        (None, None) => lines.extend(["", "## Action items"].into_iter().chain(missing)),
    }
    lines.join("\n").trim().to_string()
}

fn close_with_drafts(
    parts: &[Part],
    points: &[Jotted],
    drafted: &Drafted,
    fallback_title: &str,
    chat_fn: Chat<'_>,
    progress: Progress<'_>,
    budget: super::budget::Budget,
) -> Option<Structured> {
    let mut covered = parts.iter().take_while(|p| p.index < drafted.upto).count();
    let mut notes = drafted.notes.trim().to_string();
    let mut commitments = drafted.commitments.trim().to_string();
    let mut problems = Vec::new();
    progress(0, 1);
    let (prompt, rest) = loop {
        let rest = text_from(parts, covered, parts.len());
        let from = covered.checked_sub(1).map_or(0, |i| parts[i].end_secs);
        let prompt = chat::build_close_prompt(
            &notes_context(&notes, budget.chars() / 3),
            &commitments,
            &rest,
            &points_between(points, from, None),
        );
        if fits(&prompt, budget) {
            break (prompt, rest);
        }
        let (draft, n) = draft_prompt(&parts[covered..], &notes, points, budget)?;
        let drafted = chat_fn(draft, super::STRUCTURE_MAX_TOKENS)
            .map(|reply| split_drafted(&chat::clean_reply(&reply)))
            .map_err(|e| super::writing_error(&e))
            .and_then(|(more, found)| {
                if more.is_empty() && found.is_empty() {
                    Err("The AI returned an empty part.".to_string())
                } else {
                    Ok((more, found))
                }
            });
        match drafted {
            Ok((more, found)) => {
                notes = joined(&notes, &more);
                commitments = joined(&commitments, &found).replace("\n\n", "\n");
            }
            Err(why) => {
                let (start, end) = (parts[covered].start_secs, parts[covered + n - 1].end_secs);
                problems.push(format!(
                    "{why} The transcript from {} to {} is saved as it is.",
                    clock(start),
                    clock(end)
                ));
                notes = joined(
                    &notes,
                    &raw(&[], "Transcript", &text_from(parts, covered, covered + n)),
                );
            }
        }
        covered += n;
    };
    let reply = chat_fn(prompt, super::STRUCTURE_MAX_TOKENS);
    progress(1, 1);
    let (title, body, writer_failed) = match reply.map(|r| chat::clean_reply(&r)) {
        Ok(reply) if !reply.is_empty() => {
            let (title, body) = if untitled(&reply) {
                (fallback_title.to_string(), reply)
            } else {
                chat::split_title_body(&reply)
            };
            let at = body
                .lines()
                .position(|l| l.starts_with("## "))
                .unwrap_or(usize::MAX);
            let summary: Vec<&str> = body.lines().take(at).collect();
            let sections: Vec<&str> = body.lines().skip(at).collect();
            let body = joined(&joined(&summary.join("\n"), &notes), &sections.join("\n"));
            (title, commitments_kept(body, &commitments), false)
        }
        outcome => {
            let why = match outcome {
                Err(e) => super::writing_error(&e),
                Ok(_) => "The AI returned an empty ending.".to_string(),
            };
            let mut body = notes.clone();
            if !commitments.is_empty() {
                body = joined(&body, &format!("## Action items\n{commitments}"));
            }
            if !rest.trim().is_empty() {
                body = joined(&body, &raw(&[], "Transcript", &rest));
            }
            problems.push(format!(
                "{why} The notes written during the recording are kept, and the rest of the transcript is saved as it is."
            ));
            (fallback_title.to_string(), body, true)
        }
    };
    Some(Structured {
        title,
        body: points_kept(body, points),
        problems,
        writer_failed,
    })
}

pub fn room_after_profile(
    budget: super::budget::Budget,
    profile: &leo_core::recording::Profile,
) -> super::budget::Budget {
    let added = read_of(&profile.context, leo_core::recording::CONTEXT_READ_CHARS)
        .chars()
        .count()
        + read_of(&profile.wants, leo_core::recording::WANTS_READ_CHARS)
            .chars()
            .count()
            * 2
        + 600;
    super::budget::Budget {
        words: budget.words.saturating_sub(added.div_ceil(6)),
        ..budget
    }
}

#[allow(clippy::too_many_arguments)]
pub fn structure_recording(
    parts: &[Part],
    points: &[Jotted],
    existing: Option<&str>,
    drafted: Option<&Drafted>,
    fallback_title: &str,
    chat_fn: Chat<'_>,
    progress: Progress<'_>,
    budget: super::budget::Budget,
) -> Structured {
    if let Some(drafted) = drafted.filter(|_| existing.is_none()) {
        if let Some(done) = close_with_drafts(
            parts,
            points,
            drafted,
            fallback_title,
            chat_fn,
            progress,
            budget,
        ) {
            return done;
        }
    }
    let groups = groups(parts, budget.words);
    let mut problems = Vec::new();

    if groups.len() <= 1 {
        let text = groups.first().map(|g| g.text.clone()).unwrap_or_default();
        progress(0, 1);
        let result = match existing {
            Some(body) => chat_fn(
                chat::build_append_prompt_with(&text, body, points),
                super::STRUCTURE_MAX_TOKENS,
            )
            .map(|reply| (fallback_title.to_string(), chat::clean_reply(&reply))),
            None => chat_fn(
                chat::build_structure_prompt_with(&text, points),
                super::STRUCTURE_MAX_TOKENS,
            )
            .map(|reply| chat::split_title_body(&reply)),
        };
        progress(1, 1);
        return match result {
            Ok((title, body)) if !body.trim().is_empty() => Structured {
                title: if title.trim().is_empty() {
                    fallback_title.to_string()
                } else {
                    title
                },
                body: points_kept(body, points),
                problems,
                writer_failed: false,
            },
            Ok(_) => Structured {
                title: fallback_title.to_string(),
                body: raw(points, "Transcript", &text),
                problems: vec![
                    "the AI returned an empty note, so the transcript was saved as it is".into(),
                ],
                writer_failed: true,
            },
            Err(e) => Structured {
                title: fallback_title.to_string(),
                body: raw(points, "Transcript", &text),
                problems: vec![format!(
                    "{} The transcript was saved as it is.",
                    super::writing_error(&e)
                )],
                writer_failed: true,
            },
        };
    }

    let total = groups.len() + usize::from(existing.is_none());
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Written>>> =
        groups.iter().map(|_| std::sync::Mutex::new(None)).collect();
    progress(0, total);
    let stop = leo_core::children::current_stop();
    std::thread::scope(|scope| {
        for _ in 0..budget.at_once.max(1).min(groups.len()) {
            scope.spawn(|| {
                leo_core::children::with_stop(stop.clone().unwrap_or_default(), || loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(g) = groups.get(i) else {
                        return;
                    };
                    let own = points_for(points, &groups, i);
                    let prompt =
                        chat::build_part_prompt(&g.text, &own, i + 1, groups.len(), &span(g));
                    let outcome = match chat_fn(prompt, super::STRUCTURE_MAX_TOKENS) {
                        Ok(reply) => {
                            let cleaned = chat::clean_reply(&reply);
                            if cleaned.trim().is_empty() {
                                (
                                    raw(&own, "Transcript", &g.text),
                                    Some(format!(
                                        "part {} came back empty; its transcript is included instead",
                                        i + 1
                                    )),
                                )
                            } else {
                                (cleaned, None)
                            }
                        }
                        Err(e) => (
                            raw(&own, "Transcript", &g.text),
                            Some(format!(
                                "{} Part {}'s transcript is included instead.",
                                super::writing_error(&e),
                                i + 1
                            )),
                        ),
                    };
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = Some(outcome);
                    }
                    let done = finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    progress(done, total);
                })
            });
        }
    });
    let mut sections = Vec::with_capacity(groups.len());
    let mut failed = 0;
    for (i, (g, slot)) in groups.iter().zip(results).enumerate() {
        let (notes, problem) = slot.into_inner().ok().flatten().unwrap_or_else(|| {
            failed += 1;
            (
                raw(&points_for(points, &groups, i), "Transcript", &g.text),
                None,
            )
        });
        if let Some(p) = problem {
            failed += 1;
            problems.push(p);
        }
        sections.push(format!("*{}*\n\n{}", span(g), notes.trim()));
    }

    let writer_failed = failed == groups.len();
    let joined = sections.join("\n\n---\n\n");
    if let Some(_existing) = existing {
        progress(total, total);
        let mut body = format!("## Recording ({})\n\n", fallback_title);
        body.push_str(&joined);
        return Structured {
            title: fallback_title.to_string(),
            body: points_kept(body, points),
            problems,
            writer_failed,
        };
    }

    progress(groups.len(), total);
    let per_section = SUMMARY_INPUT_CHARS / sections.len().max(1);
    let digest = sections
        .iter()
        .map(|s| {
            let chars: Vec<char> = s.chars().collect();
            if chars.len() <= per_section {
                return s.clone();
            }
            let half = per_section / 2;
            format!(
                "{}\n…\n{}",
                chars[..half].iter().collect::<String>(),
                chars[chars.len() - half..].iter().collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let (title, summary) = match chat_fn(chat::build_summary_prompt(&digest), 600) {
        Ok(reply) => {
            let (t, s) = chat::split_title_body(&reply);
            (
                if t.trim().is_empty() {
                    fallback_title.to_string()
                } else {
                    t
                },
                s,
            )
        }
        Err(e) => {
            problems.push(format!(
                "{} The title and summary were not written.",
                super::writing_error(&e)
            ));
            (fallback_title.to_string(), String::new())
        }
    };
    progress(total, total);

    let mut body = String::new();
    if !summary.trim().is_empty() {
        body.push_str(summary.trim());
        body.push_str("\n\n");
    }
    body.push_str(&joined);
    Structured {
        title,
        body: points_kept(body, points),
        problems,
        writer_failed,
    }
}

const KEPT_WORDS: f32 = 0.85;
pub const OWN_WORDS: &str = "## Your points, as you wrote them";

fn plain_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn point_kept(
    point: &str,
    body_words: &std::collections::HashSet<String>,
    body_flat: &str,
) -> bool {
    let words = plain_words(point);
    if words.is_empty() || body_flat.contains(&words.join(" ")) {
        return true;
    }
    let found = words.iter().filter(|w| body_words.contains(*w)).count();
    found as f32 / words.len() as f32 >= KEPT_WORDS
}

pub fn points_kept(body: String, points: &[Jotted]) -> String {
    let body_list = plain_words(&body);
    let body_flat = body_list.join(" ");
    let body_words: std::collections::HashSet<String> = body_list.into_iter().collect();
    let lost: Vec<&str> = points
        .iter()
        .map(|p| p.text.trim())
        .filter(|t| !t.is_empty() && !point_kept(t, &body_words, &body_flat))
        .collect();
    if lost.is_empty() {
        return body;
    }
    let list: String = lost.iter().map(|t| format!("- {t}\n")).collect();
    format!("{}\n\n{OWN_WORDS}\n\n{list}", body.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recordings_carry_their_date_so_relative_deadlines_become_real_dates() {
        let prompt = recorded_on(
            Prompt {
                system: "rules".into(),
                user: "transcript".into(),
            },
            chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
        );
        assert!(prompt
            .user
            .starts_with("<recorded_on>Tuesday, October 13, 2026</recorded_on>"));
        let rules = crate::ai::chat::build_structure_prompt_with("x", &[]).system;
        assert!(rules.contains("<recorded_on>") && rules.contains("Never invent a task"));
        assert!(rules.contains("Practice questions belong in Check yourself"));
    }
    use crate::ai::budget::Budget;

    const TEST: Budget = Budget {
        words: 4000,
        at_once: 3,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn part(index: u32, words: usize, tag: &str) -> Part {
        Part {
            index,
            start_secs: index as u64 * 300,
            end_secs: (index as u64 + 1) * 300,
            text: (0..words)
                .map(|i| format!("{tag}{i}"))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    #[test]
    fn what_the_user_wants_shapes_the_notes_and_keeps_gap_filling_and_points() {
        let points = vec![Jotted {
            at_secs: 5,
            text: "Explain the queue".into(),
        }];
        let plain = with_profile(
            chat::build_structure_prompt_with("partial words", &points),
            &Default::default(),
        );
        assert!(!plain.user.contains("<what_the_user_wants>"));
        assert!(!plain.user.contains("<recording_context>"));
        let p = with_profile(
            chat::build_structure_prompt_with("partial words", &points),
            &leo_core::recording::Profile {
                context: "Design review with Sam".into(),
                wants: "Focus on what will be on the exam".into(),
                ..Default::default()
            },
        );
        assert!(p.system.contains("<what_the_user_wants>"));
        assert!(p.system.contains(chat::LEARNING));
        assert!(p
            .system
            .contains("fill the gap with accurate explanation from your own knowledge"));
        assert!(p.user.starts_with(
            "<what_the_user_wants>\nFocus on what will be on the exam\n</what_the_user_wants>"
        ));
        assert!(p
            .user
            .contains("<recording_context>\nDesign review with Sam\n</recording_context>"));
        assert!(p.user.contains("Explain the queue"));
        assert!(p
            .user
            .ends_with("Follow what the user wants from these notes."));
    }

    #[test]
    fn typed_points_survive_word_for_word_and_a_lost_one_is_put_back() {
        let points = vec![
            Jotted {
                at_secs: 5,
                text: "exam: Dijkstra fails with negative edges!".into(),
            },
            Jotted {
                at_secs: 9,
                text: "ask about office hours".into(),
            },
            Jotted {
                at_secs: 12,
                text: "heap gives O(log n) extract min".into(),
            },
        ];
        let body = "## Shortest paths\n- Exam: Dijkstra fails with negative edges!\n\nWhy: it never revisits a settled node.\n\n## Heaps\n- heap gives O(log n) extract-min, which makes Dijkstra fast.".to_string();
        let kept = points_kept(body.clone(), &points);
        assert!(kept.starts_with(&body));
        assert!(kept.ends_with(&format!("{OWN_WORDS}\n\n- ask about office hours\n")));
        assert!(
            !kept.contains("- exam: Dijkstra"),
            "a point kept with different capitals is not repeated"
        );
        assert_eq!(
            points_kept(kept.clone(), &points),
            kept,
            "nothing is added twice"
        );
        assert_eq!(points_kept(body.clone(), &[]), body);
        assert!(chat::build_structure_prompt_with("words", &points)
            .system
            .contains("in the listener's exact words"));
    }

    #[test]
    #[ignore = "asks the real writing AI set up in LEO_HOME"]
    fn the_real_writing_ai_keeps_typed_points_word_for_word() {
        let transcript = "Okay so today we're doing shortest paths. Dijkstra's algorithm keeps a priority queue of vertices keyed by their tentative distance, and each time it pulls out the closest one and relaxes its edges. Now this only works when edges are non negative, because once a vertex is settled we never look at it again. With a negative edge you could find a cheaper path later and Dijkstra would miss it, so you'd use Bellman-Ford instead, which relaxes every edge V minus one times. For the midterm, it's closed book. Running time with a binary heap is O of E log V. There's a fancier heap, the Fibonacci heap, that gets it down to E plus V log V.";
        let points = vec![
            Jotted {
                at_secs: 40,
                text: "dijkstra != neg edges (settled = final)".into(),
            },
            Jotted {
                at_secs: 80,
                text: "prof: NO calculators on midterm 2!!".into(),
            },
            Jotted {
                at_secs: 95,
                text: "look up fib heap later".into(),
            },
        ];
        let reply = crate::ai::chat_outcome(
            chat::build_structure_prompt_with(transcript, &points),
            crate::ai::STRUCTURE_MAX_TOKENS,
        )
        .expect("the writing AI answered")
        .value;
        println!("{reply}");
        let flat = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
        let body = flat(&reply);
        let missing: Vec<&str> = points
            .iter()
            .map(|p| p.text.as_str())
            .filter(|t| !body.contains(&flat(t)))
            .collect();
        println!("MISSING {missing:?}");
        assert!(missing.is_empty(), "reworded or dropped: {missing:?}");
    }

    #[test]
    fn short_recordings_are_one_group_and_long_ones_split_by_words() {
        let parts: Vec<Part> = (0..10).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let g = groups(&parts, 3500);
        assert_eq!(g.len(), 4);
        assert_eq!((g[0].start_secs, g[0].end_secs), (0, 900));
        assert_eq!((g[3].start_secs, g[3].end_secs), (2700, 3000));
        let all: usize = g.iter().map(|g| g.text.split_whitespace().count()).sum();
        assert_eq!(all, 10_000, "a word was lost or doubled");
        assert_eq!(groups(&parts[..2], 3500).len(), 1);
    }

    #[test]
    fn silent_parts_add_no_empty_groups() {
        let parts = vec![part(0, 0, "x"), part(1, 5, "y"), part(2, 0, "z")];
        let g = groups(&parts, 3);
        assert_eq!(g.len(), 1);
        assert!(g[0].text.starts_with("y0"));
    }

    #[test]
    fn a_short_recording_is_one_request() {
        let calls = AtomicUsize::new(0);
        let chat = |_: Prompt, _: u32| -> Result<String> {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok("Graphs\n\nAbout graphs.\n\n## BFS\n- queue".to_string())
        };
        let s = structure_recording(
            &[part(0, 50, "w")],
            &[],
            None,
            None,
            "Recording",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(s.title, "Graphs");
        assert!(s.body.contains("## BFS"));
        assert!(s.problems.is_empty());
        assert!(!s.writer_failed);
    }

    #[test]
    fn when_the_ai_fails_the_transcript_is_still_the_note() {
        let chat =
            |_: Prompt, _: u32| -> Result<String> { anyhow::bail!("context length exceeded") };
        let points = vec![Jotted {
            at_secs: 5,
            text: "exam".into(),
        }];
        let s = structure_recording(
            &[part(0, 50, "w")],
            &points,
            None,
            None,
            "Recording, Sep 29",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert_eq!(s.title, "Recording, Sep 29");
        assert!(s.body.contains("w0 w1"), "{}", s.body);
        assert!(s.body.contains("- exam\n"), "{}", s.body);
        assert_eq!(s.problems.len(), 1);
        assert!(s.writer_failed);
    }

    #[test]
    fn a_stop_reaches_the_long_recording_workers() {
        let parts: Vec<Part> = (0..12).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let caller = std::thread::current().id();
        let on_workers = AtomicUsize::new(0);
        let unstopped = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            if std::thread::current().id() != caller {
                on_workers.fetch_add(1, Ordering::SeqCst);
            }
            if leo_core::children::current_stop().is_none() {
                unstopped.fetch_add(1, Ordering::SeqCst);
            }
            if p.system.contains("name and summarize") {
                return Ok("A long day\n\nEverything covered.".to_string());
            }
            Ok("## Part\n- covered".to_string())
        };
        leo_core::children::with_stop(leo_core::children::Stop::default(), || {
            structure_recording(
                &parts,
                &[],
                None,
                None,
                "Recording",
                &chat,
                &|_, _| {},
                TEST,
            )
        });
        assert!(on_workers.load(Ordering::SeqCst) > 1);
        assert_eq!(unstopped.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_long_recording_is_written_part_by_part_then_summarized() {
        let parts: Vec<Part> = (0..40).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let prompts = std::sync::Mutex::new(Vec::new());
        let asked = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            prompts.lock().unwrap().push(p.user.clone());
            if p.system.contains("name and summarize") {
                return Ok("A long day\n\nEverything covered.".to_string());
            }
            let n = asked.fetch_add(1, Ordering::SeqCst);
            if n == 2 {
                anyhow::bail!("503")
            } else {
                Ok(format!("## Section {n}\n- point"))
            }
        };
        let points = vec![Jotted {
            at_secs: 7000,
            text: "midterm".into(),
        }];
        let steps = std::sync::Mutex::new(Vec::new());
        let s = structure_recording(
            &parts,
            &points,
            None,
            None,
            "Recording",
            &chat,
            &|d, t| steps.lock().unwrap().push((d, t)),
            TEST,
        );
        let asked = prompts.lock().unwrap();
        let group_count = groups(&parts, TEST.words).len();
        assert_eq!(asked.len(), group_count + 1);
        assert!(
            asked.iter().all(|u| u.chars().count() < 60_000),
            "a request grew with the recording"
        );
        assert_eq!(s.title, "A long day");
        assert!(
            s.body.starts_with("Everything covered."),
            "{}",
            &s.body[..80]
        );
        assert!(
            asked
                .iter()
                .any(|u| u.contains("<typed_points>\n- midterm\n")),
            "the point never reached the part it was typed during"
        );
        assert!(!s.body.starts_with("## Key points"));
        assert!(s.body.contains("## Section 0"));
        assert!(
            s.body.contains("## Transcript"),
            "the failed part fell back to its transcript"
        );
        assert_eq!(s.problems.len(), 1);
        assert!(
            !s.writer_failed,
            "one failed part is a warning, not a failed write-up"
        );
        let last = *steps.lock().unwrap().last().unwrap();
        assert_eq!(last.0, last.1);
        for w in ["p0w0", "p39w999"] {
            assert!(
                s.body.contains(w) || asked.iter().any(|u| u.contains(w)),
                "{w} never reached the AI or the note"
            );
        }
    }

    #[test]
    fn every_typed_point_goes_to_exactly_one_part() {
        let group = |start_secs, end_secs| Group {
            start_secs,
            end_secs,
            text: "x".into(),
        };
        let groups = [group(10, 100), group(100, 200), group(200, 300)];
        let points: Vec<Jotted> = [0, 50, 100, 250, 900]
            .into_iter()
            .map(|at_secs| Jotted {
                at_secs,
                text: format!("at {at_secs}"),
            })
            .collect();
        let texts = |i| -> Vec<String> {
            points_for(&points, &groups, i)
                .into_iter()
                .map(|p| p.text)
                .collect()
        };
        assert_eq!(texts(0), ["at 0", "at 50"]);
        assert_eq!(texts(1), ["at 100"]);
        assert_eq!(texts(2), ["at 250", "at 900"]);
    }

    #[test]
    fn parts_are_written_at_the_same_time_and_kept_in_order() {
        let parts: Vec<Part> = (0..30).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            if p.system.contains("name and summarize") {
                return Ok("T\n\nS".to_string());
            }
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(40));
            running.fetch_sub(1, Ordering::SeqCst);
            let first = p
                .user
                .split_whitespace()
                .find(|w| w.starts_with('p'))
                .unwrap_or("")
                .to_string();
            Ok(format!("## From {first}\n- x"))
        };
        let s = structure_recording(&parts, &[], None, None, "R", &chat, &|_, _| {}, TEST);
        assert!(
            most.load(Ordering::SeqCst) >= 2,
            "parts were not written in parallel"
        );
        assert!(most.load(Ordering::SeqCst) <= TEST.at_once);
        let firsts: Vec<usize> = groups(&parts, TEST.words)
            .iter()
            .map(|g| {
                s.body
                    .find(&format!(
                        "## From {}",
                        g.text.split_whitespace().next().unwrap()
                    ))
                    .unwrap()
            })
            .collect();
        assert!(
            firsts.windows(2).all(|w| w[0] < w[1]),
            "parts came back out of order"
        );
    }

    #[test]
    fn a_long_addition_to_a_note_has_no_title_request() {
        let parts: Vec<Part> = (0..20).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let calls = AtomicUsize::new(0);
        let chat = |p: Prompt, _: u32| -> Result<String> {
            calls.fetch_add(1, Ordering::Relaxed);
            assert!(!p.system.contains("name and summarize"));
            Ok("## More\n- x".to_string())
        };
        let s = structure_recording(
            &parts,
            &[],
            Some("old"),
            None,
            "Sep 29",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            groups(&parts, TEST.words).len()
        );
        assert!(s.body.starts_with("## Recording (Sep 29)"));
    }

    #[test]
    fn a_long_recording_with_every_part_failed_is_a_failed_write_up() {
        let parts: Vec<Part> = (0..20).map(|i| part(i, 1000, &format!("p{i}w"))).collect();
        let chat = |_: Prompt, _: u32| -> Result<String> { anyhow::bail!("no writer") };
        let s = structure_recording(&parts, &[], None, None, "R", &chat, &|_, _| {}, TEST);
        assert!(s.writer_failed);
        assert!(s.body.contains("p0w0"), "the transcript was kept");
    }

    fn drafted(upto: u32, notes: &str) -> Drafted {
        Drafted {
            upto,
            key: String::new(),
            notes: notes.to_string(),
            commitments: String::new(),
        }
    }

    #[test]
    fn with_drafts_one_close_call_writes_the_rest_around_them() {
        let parts = [part(0, 20, "a"), part(1, 20, "b"), part(2, 20, "c")];
        let seen = std::sync::Mutex::new(Vec::new());
        let chat = |p: Prompt, _: u32| -> Result<String> {
            seen.lock().unwrap().push(p);
            Ok("Disks\n\nHow disks work.\n\n## Caching\n- c\n\n## Check yourself\n> [!question]- Why?\n> Because.".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(2, "## Layout\n- a\n\n## Seeking\n- b")),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].user.contains("## Seeking") && seen[0].user.contains("c0"));
        assert!(!seen[0].user.contains("a0") && !seen[0].user.contains("b0"));
        assert_eq!(s.title, "Disks");
        let at = |t: &str| {
            s.body
                .find(t)
                .unwrap_or_else(|| panic!("{t} in {}", s.body))
        };
        assert!(at("How disks work.") < at("## Layout"));
        assert!(at("## Seeking") < at("## Caching"));
        assert!(at("## Caching") < at("## Check yourself"));
        assert!(!s.writer_failed && s.problems.is_empty());
    }

    #[test]
    fn a_close_with_nothing_left_or_no_sections_still_keeps_the_drafts() {
        let parts = [part(0, 20, "a")];
        let chat = |p: Prompt, _: u32| -> Result<String> {
            assert!(p.user.contains("<transcript>\n\n</transcript>"));
            Ok("Disks\n\nOnly a summary.".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert_eq!(s.body, "Only a summary.\n\n## Layout\n- a");
    }

    #[test]
    fn a_failed_close_keeps_the_drafts_and_the_rest_of_the_transcript() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let chat = |_: Prompt, _: u32| -> Result<String> { anyhow::bail!("busy") };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "Recording",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert!(s.writer_failed);
        assert_eq!(s.title, "Recording");
        assert!(s.body.starts_with("## Layout") && s.body.contains("b0"));
        assert!(!s.body.contains("a0"));
        assert_eq!(s.problems.len(), 1);
    }

    #[test]
    fn typed_points_go_to_their_own_stretch_and_are_never_lost() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let points = vec![
            Jotted {
                at_secs: 10,
                text: "early point kept".into(),
            },
            Jotted {
                at_secs: 400,
                text: "late point".into(),
            },
        ];
        let chat = |p: Prompt, _: u32| -> Result<String> {
            assert!(p.user.contains("late point") && !p.user.contains("early point kept"));
            Ok("T\n\nS.\n\n## More\n- late point".into())
        };
        let s = structure_recording(
            &parts,
            &points,
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert!(s.body.contains("early point kept"));
        assert_eq!(s.body.matches("late point").count(), 1);
    }

    #[test]
    fn drafts_are_ignored_when_adding_to_an_existing_note() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let chat = |p: Prompt, _: u32| -> Result<String> {
            assert!(!p.user.contains("notes_so_far"));
            Ok("- more".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            Some("old"),
            Some(&drafted(1, "## Layout")),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert!(!s.body.contains("## Layout"));
    }

    #[test]
    fn the_draft_key_changes_with_its_own_stretch_only() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let profile = leo_core::recording::Profile::default();
        let point = |at_secs| Jotted {
            at_secs,
            text: "p".into(),
        };
        let key = draft_key(&parts, &[], &profile, 1);
        assert_eq!(key, draft_key(&parts, &[point(500)], &profile, 1));
        assert_ne!(key, draft_key(&parts, &[point(20)], &profile, 1));
        assert_ne!(key, draft_key(&parts, &[], &profile, 2));
        let wants = leo_core::recording::Profile {
            wants: "focus on proofs".into(),
            ..Default::default()
        };
        assert_ne!(key, draft_key(&parts, &[], &wants, 1));
        let mut changed = parts.clone();
        changed[0].text.push_str(" again");
        assert_ne!(key, draft_key(&changed, &[], &profile, 1));
    }

    #[test]
    fn a_draft_takes_the_longest_stretch_that_fits() {
        let parts = [part(0, 200, "a"), part(1, 200, "b"), part(2, 200, "c")];
        let roomy = draft_prompt(&parts, "", &[], TEST).unwrap();
        assert_eq!(roomy.1, 3);
        let tight = Budget {
            words: 1_200,
            at_once: 1,
        };
        let (prompt, n) = draft_prompt(&parts, "", &[], tight).unwrap();
        assert!((1..3).contains(&n));
        assert!(prompt.user.contains("a0") && !prompt.user.contains("c0"));
        let tiny = Budget {
            words: 10,
            at_once: 1,
        };
        assert!(draft_prompt(&parts, "", &[], tiny).is_none());
        assert!(prompt
            .system
            .contains("Leave \"## Check yourself\" to the end of the recording"));
    }

    #[test]
    fn long_drafts_are_cut_to_their_most_recent_headings_and_tail() {
        let many: String = (0..2_000)
            .map(|i| format!("## Heading {i}\n- x\n"))
            .collect();
        let notes = format!("{many}## Last\n- end");
        let context = notes_context(&notes, 6_000);
        assert!(context.contains("## Heading 1999") && context.contains("## Last\n- end"));
        assert!(!context.contains("## Heading 0\n"));
        assert!(
            context.chars().count() < 6_100,
            "{}",
            context.chars().count()
        );
        assert_eq!(notes_context("## A\n- a", 6_000), "## A\n- a");
    }

    #[test]
    fn a_draft_reply_keeps_its_commitments_apart_and_drops_its_questions() {
        let (notes, found) = split_drafted(
            "## Seeking\n- arm moves\n\n## Action items\n- [ ] HW 4 due Friday\n\n## Check yourself\n> q\n> a",
        );
        assert_eq!(notes, "## Seeking\n- arm moves");
        assert_eq!(found, "- [ ] HW 4 due Friday");
    }

    #[test]
    fn early_commitments_reach_the_close_even_when_the_drafts_are_cut() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let mut early = drafted(1, &"## Long\n- words\n".repeat(2_000));
        early.commitments = "- [ ] HW 4 due Friday".into();
        let chat = |p: Prompt, _: u32| -> Result<String> {
            assert!(p.user.contains("HW 4 due Friday"));
            assert!(p.user.contains("Headings so far"));
            Ok("T\n\nS.\n\n## Action items\n- [ ] HW 4 due Friday".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&early),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert!(!s.writer_failed);
        let failing = |_: Prompt, _: u32| -> Result<String> { anyhow::bail!("busy") };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&early),
            "R",
            &failing,
            &|_, _| {},
            TEST,
        );
        assert!(s.writer_failed && s.body.contains("## Action items\n- [ ] HW 4 due Friday"));
    }

    #[test]
    fn a_rest_too_long_for_one_close_is_drafted_first_then_closed() {
        let parts = [part(0, 900, "a"), part(1, 900, "b"), part(2, 900, "c")];
        let window = Budget {
            words: 1_700,
            at_once: 1,
        };
        let seen = std::sync::Mutex::new(Vec::new());
        let chat = |p: Prompt, _: u32| -> Result<String> {
            let closing = p.system.contains("finish study notes");
            seen.lock()
                .unwrap()
                .push((closing, p.user.contains("b0 "), p.user.contains("c0 ")));
            Ok(if closing {
                "T\n\nS.\n\n## Caching\n- c".into()
            } else {
                "## Seeking\n- b".into()
            })
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "R",
            &chat,
            &|_, _| {},
            window,
        );
        assert_eq!(
            *seen.lock().unwrap(),
            [(false, true, false), (true, false, true)]
        );
        assert_eq!(
            s.body,
            "S.\n\n## Layout\n- a\n\n## Seeking\n- b\n\n## Caching\n- c"
        );
    }

    #[test]
    fn a_failed_catch_up_draft_keeps_the_drafts_and_the_rest() {
        let parts = [part(0, 900, "a"), part(1, 900, "b"), part(2, 900, "c")];
        let window = Budget {
            words: 1_700,
            at_once: 1,
        };
        let chat = |_: Prompt, _: u32| -> Result<String> { anyhow::bail!("busy") };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "R",
            &chat,
            &|_, _| {},
            window,
        );
        assert!(s.writer_failed);
        assert!(
            s.body.starts_with("## Layout\n- a") && s.body.contains("b0") && s.body.contains("c0")
        );
    }

    #[test]
    fn a_stretch_that_cannot_fit_at_all_falls_back_to_the_one_pass_write_up() {
        let parts = [part(0, 20, "a"), part(1, 900, "b")];
        let tiny = Budget {
            words: 900,
            at_once: 1,
        };
        let chat = |p: Prompt, _: u32| -> Result<String> {
            assert!(!p.user.contains("notes_so_far"));
            Ok("T\n\nS.\n\n## All\n- x".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "R",
            &chat,
            &|_, _| {},
            tiny,
        );
        assert!(!s.body.contains("## Layout"));
    }

    #[test]
    fn the_close_reads_the_overlap_between_stretches_once() {
        let mut second = part(1, 20, "b");
        second.text = format!("a12 a13 a14 a15 a16 a17 a18 a19 {}", second.text);
        let parts = [part(0, 20, "a"), second];
        let chat = |p: Prompt, _: u32| -> Result<String> {
            let rest = p.user.split("<transcript>").nth(1).unwrap();
            assert!(rest.trim_start().starts_with("b0"), "{rest}");
            Ok("T\n\nS.".into())
        };
        structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout")),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
    }

    #[test]
    fn a_close_reply_without_a_title_keeps_its_first_section() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let chat = |_: Prompt, _: u32| -> Result<String> { Ok("## Caching\n- c".into()) };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "Recording",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert_eq!(s.title, "Recording");
        assert_eq!(s.body, "## Layout\n- a\n\n## Caching\n- c");
    }

    #[test]
    fn a_stretch_with_long_typed_points_still_drafts_with_less_context() {
        let ollama = Budget {
            words: 2_764,
            at_once: 1,
        };
        let parts = [part(0, 540, "a")];
        let points = vec![Jotted {
            at_secs: 10,
            text: "slide ".repeat(900),
        }];
        let notes = "## Earlier\n- words\n".repeat(600);
        let (prompt, n) = draft_prompt(&parts, &notes, &points, ollama).unwrap();
        assert_eq!(n, 1);
        assert!(prompt.user.contains("slide slide") && prompt.user.contains("a539"));
        assert!(!prompt
            .user
            .contains(&notes_context(&notes, ollama.chars() / 3)));
    }

    #[test]
    fn drafted_commitments_are_kept_when_the_close_leaves_them_out() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let mut early = drafted(1, "## Layout\n- a");
        early.commitments = "- [ ] HW 4 due Friday".into();
        let chat = |_: Prompt, _: u32| -> Result<String> {
            Ok("T\n\nS.\n\n## Caching\n- c\n\n## Check yourself\n> q".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&early),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert!(
            s.body
                .ends_with("## Action items\n- [ ] HW 4 due Friday\n\n## Check yourself\n> q"),
            "{}",
            s.body
        );
        let chat = |_: Prompt, _: u32| -> Result<String> {
            Ok("T\n\nS.\n\n## Action items\n- [ ] Quiz Monday".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&early),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert!(
            s.body
                .contains("## Action items\n- [ ] HW 4 due Friday\n- [ ] Quiz Monday"),
            "{}",
            s.body
        );
        assert_eq!(s.body.matches("## Action items").count(), 1);
    }

    #[test]
    fn an_empty_catch_up_draft_keeps_that_stretch_as_transcript() {
        let parts = [part(0, 900, "a"), part(1, 900, "b"), part(2, 900, "c")];
        let window = Budget {
            words: 1_700,
            at_once: 1,
        };
        let chat = |p: Prompt, _: u32| -> Result<String> {
            Ok(if p.system.contains("finish study notes") {
                "T\n\nS.\n\n## Caching\n- c".into()
            } else {
                "## Check yourself\n> q".into()
            })
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout\n- a")),
            "R",
            &chat,
            &|_, _| {},
            window,
        );
        assert!(!s.writer_failed);
        assert!(
            s.body.contains("b0 b1") && s.body.contains("c0 c1") && s.body.contains("## Caching")
        );
        assert!(!s.problems.is_empty());
    }

    #[test]
    fn a_title_written_as_a_heading_is_still_the_title() {
        let parts = [part(0, 20, "a"), part(1, 20, "b")];
        let chat = |_: Prompt, _: u32| -> Result<String> {
            Ok("## Disk Scheduling\n\nHow the arm is scheduled.\n\n## Seek time\n- s".into())
        };
        let s = structure_recording(
            &parts,
            &[],
            None,
            Some(&drafted(1, "## Layout")),
            "R",
            &chat,
            &|_, _| {},
            TEST,
        );
        assert_eq!(s.title, "Disk Scheduling");
        assert!(s.body.starts_with("How the arm is scheduled."));
    }

    #[test]
    fn a_long_brief_is_read_at_its_budget_and_never_zeroes_the_room() {
        use leo_core::recording::{CONTEXT_READ_CHARS, WANTS_READ_CHARS};
        let long = leo_core::recording::Profile {
            context: "ß".repeat(CONTEXT_READ_CHARS * 50),
            wants: "界".repeat(WANTS_READ_CHARS * 50),
            ..Default::default()
        };
        let p = with_profile(chat::build_structure_prompt_with("words", &[]), &long);
        assert_eq!(p.user.matches('ß').count(), CONTEXT_READ_CHARS);
        assert_eq!(p.user.matches('界').count(), WANTS_READ_CHARS);
        let at_budget = leo_core::recording::Profile {
            context: "ß".repeat(CONTEXT_READ_CHARS),
            wants: "界".repeat(WANTS_READ_CHARS),
            ..Default::default()
        };
        assert_eq!(
            room_after_profile(TEST, &long).words,
            room_after_profile(TEST, &at_budget).words
        );
        assert!(room_after_profile(TEST, &long).words > 0);
    }

    #[test]
    fn what_the_profile_adds_is_taken_out_of_the_room() {
        let profile = leo_core::recording::Profile {
            context: "x".repeat(6_000),
            wants: "w".repeat(600),
            ..Default::default()
        };
        let room = room_after_profile(TEST, &profile);
        assert!(room.chars() <= TEST.chars() - 7_200 - 600);
        assert_eq!(
            room_after_profile(TEST, &Default::default()).words,
            TEST.words - 100
        );
    }

    #[test]
    fn a_silent_start_with_no_points_is_never_drafted_alone() {
        let parts = [part(0, 0, "a"), part(1, 20, "b")];
        let (prompt, n) = draft_prompt(&parts, "", &[], TEST).unwrap();
        assert_eq!(n, 2);
        assert!(prompt.user.contains("b0"));
        assert!(draft_prompt(&parts[..1], "", &[], TEST).is_none());
    }
}
