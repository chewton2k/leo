use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::chats::{self, Chat, Ref};

pub const DUE_AFTER_HOURS: i64 = 20;
pub const MOST_MISSED: usize = 20;

pub fn count(dir: &Path) -> usize {
    chats::db_of(dir)
        .with(|c| c.query_row("SELECT COUNT(*) FROM reviewed", [], |r| r.get::<_, i64>(0)))
        .map_or(0, |n| n.max(0) as usize)
}

pub fn reset(dir: &Path) -> bool {
    chats::db_of(dir)
        .with(|c| c.execute("DELETE FROM reviewed", []))
        .is_ok_and(|n| n > 0)
}

pub(crate) fn insert_all(c: &rusqlite::Connection, keys: &[String]) -> rusqlite::Result<()> {
    let base: i64 = c.query_row("SELECT coalesce(MAX(n), 0) FROM reviewed", [], |r| r.get(0))?;
    let mut insert = c.prepare("INSERT OR IGNORE INTO reviewed (key, n) VALUES (?1, ?2)")?;
    for (i, key) in keys.iter().filter(|k| valid_key(k)).enumerate() {
        insert.execute(rusqlite::params![key, base + 1 + i as i64])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Missed {
    pub key: String,
    pub chat: String,
    pub question: String,
    pub answer: String,
    pub notes: Vec<Ref>,
    pub at: DateTime<Utc>,
    pub due: bool,
}

fn fnv(text: &str) -> u64 {
    crate::vectors::fnv64(text.bytes())
}

pub fn key_of(chat: &str, reply: &str) -> String {
    format!("{chat}:{:016x}", fnv(reply))
}

pub fn valid_key(key: &str) -> bool {
    key.rsplit_once(':').is_some_and(|(chat, hash)| {
        chats::valid_id(chat) && hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit())
    })
}

fn text_of(message: &serde_json::Value) -> &str {
    message.get("text").and_then(|t| t.as_str()).unwrap_or("")
}

fn role_of(message: &serde_json::Value) -> &str {
    message.get("role").and_then(|r| r.as_str()).unwrap_or("")
}

pub const VERDICT_WITHIN: usize = 240;

fn verdict_off(text: &str) -> Option<(&str, &str)> {
    let lower = text.to_ascii_lowercase();
    ["[[correct]]", "[[incorrect]]"]
        .iter()
        .filter_map(|verdict| lower.find(verdict).map(|at| (at, *verdict)))
        .filter(|(at, _)| *at <= VERDICT_WITHIN)
        .min_by_key(|(at, _)| *at)
        .map(|(at, verdict)| {
            (
                &verdict[2..verdict.len() - 2],
                text[at + verdict.len()..].trim_start(),
            )
        })
}

fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[n") {
        let (before, tail) = rest.split_at(start);
        out.push_str(before);
        match tail.find(']') {
            Some(end)
                if tail[2..end]
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c == ' ' || c == 'n') =>
            {
                rest = &tail[end + 1..];
            }
            _ => {
                out.push_str("[n");
                rest = &tail[2..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn question_in(text: &str) -> String {
    let text = verdict_off(text).map_or(text, |(_, rest)| rest);
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let asked = paragraphs
        .iter()
        .rev()
        .find(|p| p.contains('?'))
        .or(paragraphs.last())
        .copied()
        .unwrap_or("");
    chats::clipped(&plain(asked), 300)
}

fn cited_in(message: &serde_json::Value) -> Vec<Ref> {
    let text = text_of(message);
    let sources = message
        .get("sources")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<Ref> = Vec::new();
    for source in &sources {
        let (Some(n), Some(id), Some(title)) = (
            source.get("n").and_then(|n| n.as_u64()),
            source.get("id").and_then(|i| i.as_str()),
            source.get("title").and_then(|t| t.as_str()),
        ) else {
            continue;
        };
        let tag = format!("n{n}");
        let used = text.match_indices(&tag).any(|(at, _)| {
            let after = text[at + tag.len()..].chars().next();
            !after.is_some_and(|c| c.is_ascii_digit())
        });
        if used && !out.iter().any(|r| r.id == id) {
            out.push(Ref {
                id: id.to_string(),
                title: title.to_string(),
            });
        }
    }
    out.truncate(3);
    out
}

fn missed_cards(chat: &Chat, message: &serde_json::Value, now: DateTime<Utc>) -> Vec<Missed> {
    let text = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    };
    let at = message
        .get("at")
        .and_then(|a| a.as_str())
        .and_then(|a| DateTime::parse_from_rfc3339(a).ok())
        .map_or(chat.updated_at, |a| a.with_timezone(&Utc));
    message
        .get("quizzes")
        .and_then(|q| q.as_array())
        .into_iter()
        .flatten()
        .filter(|q| q.get("given").is_some_and(|g| g.is_string()))
        .filter(|q| q.get("predict").is_none())
        .filter(|q| match q.get("correct") {
            Some(serde_json::Value::Bool(right)) => !right,
            _ => q
                .get("reply")
                .map(|r| text(r, "text"))
                .is_some_and(|t| matches!(verdict_off(&t), Some(("incorrect", _)))),
        })
        .map(|q| {
            let question = chats::clipped(&plain(&text(q, "question")), 200);
            let given = text(q, "given");
            Missed {
                key: key_of(&chat.id, &format!("{question}\n{given}")),
                chat: chat.id.clone(),
                question,
                answer: chats::clipped(&plain(&given), 200),
                notes: cited_in(message),
                due: at <= now - Duration::hours(DUE_AFTER_HOURS),
                at,
            }
        })
        .collect()
}

fn missed_in(chat: &Chat, now: DateTime<Utc>) -> Vec<Missed> {
    let mut out = Vec::new();
    for (i, reply) in chat.messages.iter().enumerate() {
        if role_of(reply) != "assistant" {
            continue;
        }
        out.extend(missed_cards(chat, reply, now));
        let text = text_of(reply);
        if !matches!(verdict_off(text), Some(("incorrect", _))) {
            continue;
        }
        let said = i
            .checked_sub(1)
            .map(|j| &chat.messages[j])
            .filter(|m| role_of(m) == "user");
        let quiz = said.and_then(|m| m.get("quiz")).filter(|q| q.is_object());
        let field = |key: &str| {
            quiz.and_then(|q| q.get(key))
                .and_then(|v| v.as_str())
                .map(|v| chats::clipped(&plain(v), 200))
        };
        let answer = field("given")
            .or_else(|| said.map(|m| chats::clipped(&plain(text_of(m)), 200)))
            .unwrap_or_default();
        let asked = i
            .checked_sub(2)
            .map(|j| &chat.messages[j])
            .filter(|m| role_of(m) == "assistant");
        let question = field("question")
            .or_else(|| asked.map(|m| question_in(text_of(m))))
            .unwrap_or_default();
        let mut notes = cited_in(reply);
        if notes.is_empty() {
            notes = asked.map(cited_in).unwrap_or_default();
        }
        let at = reply
            .get("at")
            .and_then(|a| a.as_str())
            .and_then(|a| DateTime::parse_from_rfc3339(a).ok())
            .map_or(chat.updated_at, |a| a.with_timezone(&Utc));
        out.push(Missed {
            key: key_of(&chat.id, text),
            chat: chat.id.clone(),
            question,
            answer,
            notes,
            due: at <= now - Duration::hours(DUE_AFTER_HOURS),
            at,
        });
    }
    out
}

fn reviewed(dir: &Path) -> BTreeSet<String> {
    chats::db_of(dir)
        .with(|c| {
            let mut found = c.prepare("SELECT key FROM reviewed")?;
            let rows = found.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
        .unwrap_or_default()
}

pub fn missed(dir: &Path, now: DateTime<Utc>) -> Vec<Missed> {
    let done = reviewed(dir);
    let mut out: Vec<Missed> = chats::with_misses(dir)
        .iter()
        .flat_map(|chat| missed_in(chat, now))
        .filter(|m| !done.contains(&m.key) && !m.question.is_empty())
        .collect();
    out.sort_by(|a, b| a.at.cmp(&b.at).then(a.key.cmp(&b.key)));
    let mut seen = BTreeSet::new();
    out.retain(|m| seen.insert(m.question.to_lowercase()));
    out.truncate(MOST_MISSED);
    out
}

pub fn mark_reviewed(dir: &Path, keys: &[String]) -> Result<usize> {
    let live: BTreeSet<String> = chats::list(dir).into_iter().map(|c| c.id).collect();
    chats::db_of(dir).with(|c| {
        let tx = c.transaction()?;
        let before: i64 = tx.query_row("SELECT COUNT(*) FROM reviewed", [], |r| r.get(0))?;
        let alive: Vec<String> = keys
            .iter()
            .filter(|key| {
                key.rsplit_once(':')
                    .is_some_and(|(chat, _)| live.contains(chat))
            })
            .cloned()
            .collect();
        insert_all(&tx, &alive)?;
        let after: i64 = tx.query_row("SELECT COUNT(*) FROM reviewed", [], |r| r.get(0))?;
        let held: Vec<String> = {
            let mut found = tx.prepare("SELECT key FROM reviewed")?;
            let rows = found.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for key in held {
            let alive = key
                .rsplit_once(':')
                .is_some_and(|(chat, _)| live.contains(chat));
            if !alive {
                tx.execute("DELETE FROM reviewed WHERE key = ?1", [&key])?;
            }
        }
        tx.commit()?;
        Ok((after - before).max(0) as usize)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn said(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "text": text })
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn study(dir: &Path, id: &str, messages: Vec<serde_json::Value>, when: DateTime<Utc>) {
        chats::save(
            dir,
            id,
            chats::Saving {
                existed: false,
                title: String::new(),
                refs: vec![],
                messages,
            },
            when,
        )
        .unwrap();
    }

    fn quiz() -> Vec<serde_json::Value> {
        vec![
            said("user", "quiz me on graphs"),
            serde_json::json!({
                "role": "assistant",
                "text": "Let's start.\n\nWhich data structure does BFS use to pick the next vertex? [n1]",
                "sources": [{"n": 1, "id": "note-graphs", "title": "Graph traversals"}],
            }),
            said("user", "a stack"),
            serde_json::json!({
                "role": "assistant",
                "text": "[[incorrect]] Not quite: BFS uses a queue [n2]. Next: what does DFS use?",
                "sources": [{"n": 2, "id": "note-queues", "title": "Queues"}],
                "at": "2026-10-05T09:00:00Z",
            }),
            said("user", "a stack"),
            said("assistant", "[[correct]] Yes. (Score: 1/2)"),
        ]
    }

    #[test]
    fn a_wrong_answer_is_remembered_with_its_question_answer_and_notes() {
        let dir = tempfile::tempdir().unwrap();
        study(dir.path(), "chat-00000001", quiz(), at(5, 9));
        let missed = missed(dir.path(), at(6, 12));
        assert_eq!(missed.len(), 1);
        let one = &missed[0];
        assert_eq!(
            one.question,
            "Which data structure does BFS use to pick the next vertex?"
        );
        assert_eq!(one.answer, "a stack");
        assert_eq!(
            one.notes,
            vec![Ref {
                id: "note-queues".into(),
                title: "Queues".into()
            }]
        );
        assert_eq!(one.at, at(5, 9));
        assert!(one.due);
        assert!(valid_key(&one.key));
        assert!(
            !missed_in(
                &chats::load(dir.path(), "chat-00000001").unwrap(),
                at(5, 10)
            )[0]
            .due
        );
    }

    #[test]
    fn a_missed_quiz_card_is_remembered_with_its_own_question_and_the_choice_made() {
        let dir = tempfile::tempdir().unwrap();
        study(
            dir.path(),
            "chat-quiz-1",
            vec![
                said("user", "quiz me on BFS"),
                said("assistant", "Here is one to try."),
                serde_json::json!({
                    "role": "user",
                    "text": "Quiz answer. Question: What does BFS use?",
                    "quiz": { "question": "What does BFS use?", "given": "a stack", "correct": false }
                }),
                said("assistant", "[[incorrect]] It uses a queue."),
            ],
            at(1, 9),
        );
        let found = missed(dir.path(), at(3, 9));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].question, "What does BFS use?");
        assert_eq!(found[0].answer, "a stack");
    }

    #[test]
    fn a_wrong_prediction_is_never_brought_back_for_review() {
        let dir = tempfile::tempdir().unwrap();
        study(
            dir.path(),
            "chat-predict-1",
            vec![
                said("user", "teach me BFS"),
                serde_json::json!({
                    "role": "assistant",
                    "text": "Before we start, a guess.",
                    "quizzes": [
                        { "question": "What will BFS visit first?", "given": "the deepest node", "correct": false, "predict": true }
                    ]
                }),
            ],
            at(1, 9),
        );
        assert!(missed(dir.path(), at(3, 9)).is_empty());
    }

    #[test]
    fn a_wrong_answer_given_in_a_quiz_card_is_offered_for_review() {
        let dir = tempfile::tempdir().unwrap();
        study(
            dir.path(),
            "chat-card-0001",
            vec![
                said("user", "quiz me"),
                serde_json::json!({
                    "role": "assistant",
                    "text": "Try this.",
                    "quizzes": [
                        { "kind": "multiple_choice", "question": "What does BFS use?", "answer": "a queue", "given": "a stack", "correct": false },
                        { "kind": "free_response", "question": "Why a queue?", "answer": "oldest first", "given": "dunno", "correct": null, "reply": { "text": "[[incorrect]] It takes the oldest first." } },
                        { "kind": "multiple_choice", "question": "Right one?", "answer": "a", "given": "a", "correct": true }
                    ]
                }),
            ],
            at(1, 9),
        );
        let found = missed(dir.path(), at(3, 9));
        let questions: Vec<&str> = found.iter().map(|m| m.question.as_str()).collect();
        assert_eq!(questions, ["What does BFS use?", "Why a queue?"]);
        assert_eq!(found[0].answer, "a stack");
    }

    #[test]
    fn a_verdict_after_a_short_lead_in_counts() {
        assert_eq!(
            verdict_off("I'll mark this. [[incorrect]] It uses a queue."),
            Some(("incorrect", "It uses a queue."))
        );
        assert_eq!(
            verdict_off(&format!("{}[[correct]]", "x".repeat(500))),
            None
        );
    }

    #[test]
    fn a_reviewed_question_is_not_suggested_again_and_deleted_chats_are_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        study(dir.path(), "chat-00000001", quiz(), at(5, 9));
        let key = missed(dir.path(), at(6, 12))[0].key.clone();
        assert_eq!(
            mark_reviewed(dir.path(), &[key.clone(), "../evil:0".into()]).unwrap(),
            1
        );
        assert!(missed(dir.path(), at(6, 12)).is_empty());
        assert!(chats::list(dir.path()).iter().all(|c| c.id != "review"));
        chats::remove(dir.path(), "chat-00000001");
        mark_reviewed(dir.path(), &[]).unwrap();
        assert!(reviewed(dir.path()).is_empty());
    }

    #[test]
    fn citations_are_dropped_and_old_replies_fall_back_to_the_chat_time() {
        assert_eq!(
            plain("a queue [n2, n3] and [note] here"),
            "a queue and [note] here"
        );
        assert_eq!(
            question_in("[[correct]] Yes.\n\nWhat is a heap? [n1]"),
            "What is a heap?"
        );
        assert_eq!(question_in("Explain heaps."), "Explain heaps.");
        assert_eq!(verdict_off("ü"), None);
        let dir = tempfile::tempdir().unwrap();
        let mut messages = quiz();
        messages[3].as_object_mut().unwrap().remove("at");
        study(dir.path(), "chat-00000002", messages, at(7, 8));
        assert_eq!(missed(dir.path(), at(7, 9))[0].at, at(7, 8));
        assert!(!missed(dir.path(), at(7, 9))[0].due);
        assert!(!valid_key("chat-00000002:xyz"));
        assert_eq!(key_of("chat-00000002", "a"), key_of("chat-00000002", "a"));
    }

    #[test]
    fn review_finds_every_kind_of_miss_and_skips_plain_chats() {
        let dir = tempfile::tempdir().unwrap();
        let mut shouted = quiz();
        shouted[3]["text"] = serde_json::json!("[[Incorrect]] Not quite: BFS uses a queue.");
        study(dir.path(), "chat-reply-0001", shouted, at(1, 9));
        study(
            dir.path(),
            "chat-card-00001",
            vec![
                said("user", "quiz me"),
                serde_json::json!({
                    "role": "assistant",
                    "text": "Try this.",
                    "quizzes": [
                        { "question": "What does DFS use?", "answer": "a stack", "given": "a queue", "correct": false }
                    ]
                }),
            ],
            at(1, 10),
        );
        study(
            dir.path(),
            "chat-graded-001",
            vec![
                said("user", "quiz me"),
                serde_json::json!({
                    "role": "assistant",
                    "text": "Try this.",
                    "quizzes": [
                        { "question": "Why a queue?", "answer": "oldest first", "given": "dunno", "correct": null, "reply": { "text": "[[INCORRECT]] It takes the oldest first." } }
                    ]
                }),
            ],
            at(1, 11),
        );
        study(
            dir.path(),
            "chat-plain-0001",
            vec![
                said("user", "hello"),
                said("assistant", "Hi, what are we reading?"),
            ],
            at(1, 12),
        );
        study(
            dir.path(),
            "chat-right-0001",
            vec![
                said("user", "quiz me"),
                serde_json::json!({
                    "role": "assistant",
                    "text": "[[correct]] Yes.",
                    "quizzes": [
                        { "question": "Right one?", "answer": "a", "given": "a", "correct": true }
                    ]
                }),
            ],
            at(1, 13),
        );
        let found = missed(dir.path(), at(3, 9));
        let seen: Vec<(&str, &str, &str)> = found
            .iter()
            .map(|m| (m.chat.as_str(), m.question.as_str(), m.answer.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                ("chat-card-00001", "What does DFS use?", "a queue"),
                ("chat-graded-001", "Why a queue?", "dunno"),
                (
                    "chat-reply-0001",
                    "Which data structure does BFS use to pick the next vertex?",
                    "a stack"
                ),
            ]
        );
    }

    fn marks_of(chat: &str, count: usize) -> Vec<String> {
        (0..count).map(|i| format!("{chat}:{i:016x}")).collect()
    }

    #[test]
    fn every_mark_is_kept_and_marks_of_a_deleted_chat_are_not() {
        let dir = tempfile::tempdir().unwrap();
        study(dir.path(), "chat-00000001", quiz(), at(5, 9));
        let keys = marks_of("chat-00000001", 2500);
        assert_eq!(mark_reviewed(dir.path(), &keys).unwrap(), 2500);
        assert_eq!(count(dir.path()), 2500);
        let unknown = marks_of("chat-00000002", 10);
        assert_eq!(mark_reviewed(dir.path(), &unknown).unwrap(), 0);
        assert_eq!(count(dir.path()), 2500);
    }

    #[test]
    #[ignore = "benchmark"]
    fn marking_costs_in_step_with_the_count() {
        let dir = tempfile::tempdir().unwrap();
        study(dir.path(), "chat-00000001", quiz(), at(5, 9));
        let mut took = Vec::new();
        for size in [10_000, 40_000] {
            reset(dir.path());
            let keys = marks_of("chat-00000001", size);
            let started = std::time::Instant::now();
            assert_eq!(mark_reviewed(dir.path(), &keys).unwrap(), size);
            took.push(started.elapsed());
        }
        assert!(took[1] < took[0] * 6, "{took:?}");
    }
}
