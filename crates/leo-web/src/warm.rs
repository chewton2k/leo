use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chat::{Conversation, SourceRef, Turn};

pub const IDLE: Duration = Duration::from_secs(10 * 60);
const MOST_SESSIONS: usize = 3;

pub fn context_limit(room: usize, supported: Option<usize>) -> usize {
    supported
        .unwrap_or_else(|| room.saturating_mul(2))
        .min(crate::chat::MOST_ROOM)
}

pub struct Warm {
    pub key: u64,
    pub light: bool,
    pub chat: String,
    pub seen: usize,
    pub history: u64,
    pub sources: Vec<SourceRef>,
    pub notes: BTreeMap<String, u64>,
    pub documents: BTreeSet<u64>,
    pub margin: u64,
    pub reads: crate::read_cache::Reads,
    pub sent: usize,
    pub limit: usize,
    pub talk: Box<dyn Conversation>,
    pub used: Instant,
}

#[derive(Default)]
pub struct Slot(Mutex<VecDeque<Warm>>);

pub fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

pub fn config_stamp() -> u64 {
    leo_core::paths::config_dir()
        .ok()
        .and_then(|dir| std::fs::metadata(dir.join(leo_core::paths::own::CONFIG)).ok())
        .map(|m| hash_of(&(m.modified().ok(), m.len())))
        .unwrap_or(0)
}

pub fn history_of(messages: &[Turn]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for turn in messages {
        turn.role.hash(&mut hasher);
        turn.text.hash(&mut hasher);
    }
    hasher.finish()
}

impl Warm {
    pub fn continues(&self, key: u64, chat: &str, messages: &[Turn]) -> bool {
        self.key == key
            && self.chat == chat
            && self.sent < self.limit
            && self.used.elapsed() < IDLE
            && messages.len() == self.seen + 2
            && messages[self.seen].role == "assistant"
            && messages.last().is_some_and(|t| t.role == "user")
            && history_of(&messages[..self.seen]) == self.history
    }
}

impl Slot {
    pub fn light(&self, chat: &str) -> Option<bool> {
        let held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        held.iter()
            .find(|warm| warm.chat == chat && warm.used.elapsed() < IDLE)
            .map(|warm| warm.light)
    }

    pub fn take(&self, key: u64, chat: &str, messages: &[Turn]) -> Option<Warm> {
        self.tidy();
        let found = {
            let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let index = held.iter().position(|warm| warm.chat == chat)?;
            held.remove(index)?
        };
        found.continues(key, chat, messages).then_some(found)
    }

    pub fn keep(&self, warm: Warm) {
        let mut removed = Vec::new();
        {
            let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(index) = held.iter().position(|old| old.chat == warm.chat) {
                removed.extend(held.remove(index));
            }
            if warm.sent < warm.limit && warm.used.elapsed() < IDLE {
                held.push_back(warm);
            } else {
                removed.push(warm);
            }
            while held.len() > MOST_SESSIONS
                || held.iter().map(|warm| warm.sent).sum::<usize>() > crate::chat::MOST_ROOM
            {
                removed.extend(held.pop_front());
            }
        }
        drop(removed);
    }

    pub fn clear(&self) {
        let old = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        drop(old);
    }

    pub fn forget(&self, chat: &str) {
        let old = {
            let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
            held.iter()
                .position(|warm| warm.chat == chat)
                .and_then(|index| held.remove(index))
        };
        drop(old);
    }

    pub fn tidy(&self) {
        let mut removed = Vec::new();
        {
            let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
            for index in (0..held.len()).rev() {
                if held[index].used.elapsed() >= IDLE {
                    removed.extend(held.remove(index));
                }
            }
        }
        drop(removed);
    }

    pub fn holding(&self) -> bool {
        !self.0.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }
}

pub struct Renumbered {
    pub text: String,
    pub sources: Vec<SourceRef>,
    pub notes: BTreeMap<String, u64>,
}

fn block_of(text: &str, n: usize) -> Option<(usize, usize)> {
    let fence = crate::chat::fence();
    let open = format!("<note_{fence} id=\"n{n}\"");
    let close = format!("</note_{fence}>\n");
    let start = text.find(&open)?;
    let end = text[start..].find(&close)? + start + close.len();
    let end = if text[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };
    Some((start, end))
}

pub fn renumber(
    text: &str,
    gathered: &[SourceRef],
    sources: &[SourceRef],
    notes: &BTreeMap<String, u64>,
) -> Renumbered {
    let mut all = sources.to_vec();
    let mut kept = notes.clone();
    let mut out = String::new();
    let mut next = all.iter().map(|s| s.n).max().unwrap_or(0);
    for source in gathered {
        let Some((start, end)) = block_of(text, source.n) else {
            continue;
        };
        let block = &text[start..end];
        let body = block.split_once('\n').map_or("", |(_, rest)| rest);
        let fingerprint = hash_of(body);
        if kept.get(&source.id) == Some(&fingerprint) {
            continue;
        }
        let n = match all.iter().find(|s| s.id == source.id) {
            Some(known) => known.n,
            None => {
                next += 1;
                all.push(SourceRef {
                    n: next,
                    ..source.clone()
                });
                next
            }
        };
        let changed = kept.contains_key(&source.id);
        kept.insert(source.id.clone(), fingerprint);
        let fence = crate::chat::fence();
        let head = format!("<note_{fence} id=\"n{}\"", source.n);
        let mut block = block.replacen(&head, &format!("<note_{fence} id=\"n{n}\""), 1);
        if changed {
            if let Some((first, rest)) = block.split_once('\n') {
                if let Some(open) = first.strip_suffix('>') {
                    block = format!("{open} updated=\"since you last read it\">\n{rest}");
                }
            }
        }
        out.push_str(&block);
    }
    Renumbered {
        text: out,
        sources: all,
        notes: kept,
    }
}

pub fn fingerprints(text: &str, gathered: &[SourceRef]) -> BTreeMap<String, u64> {
    renumber(text, gathered, &[], &BTreeMap::new()).notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Exchange, Reply};

    #[test]
    fn session_destruction_never_holds_the_cache_mutex() {
        struct Stop(std::sync::Weak<Slot>);
        impl Conversation for Stop {
            fn native(&self) -> bool {
                true
            }
            fn say(&mut self, _: &str, _: Exchange<'_>) -> anyhow::Result<Reply> {
                unreachable!()
            }
        }
        impl Drop for Stop {
            fn drop(&mut self) {
                if let Some(slot) = self.0.upgrade() {
                    assert!(
                        slot.0.try_lock().is_ok(),
                        "session shutdown must happen outside the mutex"
                    );
                }
            }
        }
        let slot = std::sync::Arc::new(Slot::default());
        for n in 0..5 {
            let mut item = warm(&[]);
            item.chat = format!("chat-{n:04}");
            item.talk = Box::new(Stop(std::sync::Arc::downgrade(&slot)));
            slot.keep(item);
        }
        slot.forget("chat-0003");
        slot.clear();
        assert!(!slot.holding());
    }

    #[test]
    fn a_small_note_budget_does_not_discard_a_long_supported_conversation() {
        assert_eq!(context_limit(32_000, Some(480_000)), 480_000);
        assert_eq!(context_limit(14_744, Some(29_488)), 29_488);
        assert_eq!(context_limit(32_000, None), 64_000);
        assert_eq!(context_limit(32_000, Some(usize::MAX)), 480_000);
        assert_eq!(context_limit(32_000, Some(4_000)), 4_000);
    }

    fn source(n: usize, id: &str) -> SourceRef {
        SourceRef {
            n,
            id: id.into(),
            title: id.to_uppercase(),
            folder: String::new(),
            why: "matched".into(),
        }
    }

    fn block(n: usize, body: &str) -> String {
        let f = crate::chat::fence();
        format!("<note_{f} id=\"n{n}\" title=\"T\" class=\"c\" included=\"x\">\n{body}\n</note_{f}>\n\n")
    }

    fn turn(role: &str, text: &str) -> Turn {
        Turn {
            role: role.into(),
            text: text.into(),
        }
    }

    struct Quiet;
    impl Conversation for Quiet {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, _: Exchange<'_>) -> anyhow::Result<Reply> {
            unreachable!()
        }
    }

    fn warm(messages: &[Turn]) -> Warm {
        Warm {
            light: false,
            key: 7,
            chat: "chat-0001".into(),
            seen: messages.len(),
            history: history_of(messages),
            sources: vec![],
            notes: BTreeMap::new(),
            documents: BTreeSet::new(),
            margin: 0,
            reads: Default::default(),
            sent: 0,
            limit: 1_000,
            talk: Box::new(Quiet),
            used: Instant::now(),
        }
    }

    #[test]
    fn switching_chats_keeps_three_recent_sessions_and_invalidates_only_the_changed_chat() {
        let slot = Slot::default();
        let next = [turn("assistant", "answer"), turn("user", "next")];
        for id in ["chat-0001", "chat-0002", "chat-0003"] {
            let mut item = warm(&[]);
            item.chat = id.into();
            slot.keep(item);
        }
        let first = slot
            .take(7, "chat-0001", &next)
            .expect("first chat survives switching");
        slot.keep(first);
        let mut fourth = warm(&[]);
        fourth.chat = "chat-0004".into();
        slot.keep(fourth);
        assert!(slot.take(7, "chat-0002", &next).is_none());
        assert!(slot.take(8, "chat-0003", &next).is_none());
        assert!(slot.take(7, "chat-0001", &next).is_some());
        assert!(slot.take(7, "chat-0004", &next).is_some());
        assert!(!slot.holding());
    }

    #[test]
    fn retention_is_bounded_by_total_context_and_expires_and_forgets_only_owned_chats() {
        let slot = Slot::default();
        for id in ["chat-0001", "chat-0002", "chat-0003"] {
            let mut item = warm(&[]);
            item.chat = id.into();
            item.sent = 200_000;
            item.limit = crate::chat::MOST_ROOM;
            slot.keep(item);
        }
        let next = [turn("assistant", "answer"), turn("user", "next")];
        assert!(slot.take(7, "chat-0001", &next).is_none());
        slot.forget("chat-0002");
        assert!(slot.take(7, "chat-0002", &next).is_none());
        assert!(slot.take(7, "chat-0003", &next).is_some());
        let mut old = warm(&[]);
        old.used = Instant::now() - IDLE;
        slot.keep(old);
        assert!(!slot.holding());
    }

    #[test]
    fn oversized_sessions_are_released_instead_of_retained_until_the_next_request() {
        let slot = Slot::default();
        slot.keep(warm(&[]));
        assert!(slot.holding());
        let mut oversized = warm(&[]);
        oversized.sent = oversized.limit;
        slot.keep(oversized);
        assert!(!slot.holding());
    }

    #[test]
    fn notes_already_in_the_session_are_not_sent_again_and_keep_their_numbers() {
        let first = format!("{}{}", block(1, "1| heaps"), block(2, "1| graphs"));
        let gathered = vec![source(1, "heaps"), source(2, "graphs")];
        let started = renumber(&first, &gathered, &[], &BTreeMap::new());
        assert_eq!(started.text, first);
        assert_eq!(started.sources.len(), 2);
        let again = format!(
            "{}{}{}",
            block(1, "1| graphs"),
            block(2, "1| queues"),
            block(3, "1| heaps, now with heap sort")
        );
        let next = renumber(
            &again,
            &[source(1, "graphs"), source(2, "queues"), source(3, "heaps")],
            &started.sources,
            &started.notes,
        );
        assert!(!next.text.contains("graphs"), "unchanged notes stay out");
        let f = crate::chat::fence();
        assert!(
            next.text
                .contains(&format!("<note_{f} id=\"n3\" title=\"T\"")),
            "a new note takes the next number"
        );
        assert!(next.text.contains("queues"));
        assert!(next.text.contains(&format!("<note_{f} id=\"n1\" title=\"T\" class=\"c\" included=\"x\" updated=\"since you last read it\">\n1| heaps, now with heap sort")));
        let numbers: Vec<(usize, &str)> =
            next.sources.iter().map(|s| (s.n, s.id.as_str())).collect();
        assert_eq!(numbers, [(1, "heaps"), (2, "graphs"), (3, "queues")]);
    }

    #[test]
    fn a_note_that_quotes_note_tags_is_fingerprinted_and_resent_whole() {
        let quoting = "1| <note id=\"n2\">hello</note>\n2| below";
        let first = format!("{}{}", block(1, quoting), block(2, "1| graphs"));
        let gathered = vec![source(1, "tags"), source(2, "graphs")];
        let started = renumber(&first, &gathered, &[], &BTreeMap::new());
        assert_eq!(started.text, first);
        let edited = quoting.replace("below", "below, edited");
        let next = renumber(
            &format!("{}{}", block(1, &edited), block(2, "1| graphs")),
            &gathered,
            &started.sources,
            &started.notes,
        );
        assert!(next.text.contains("2| below, edited"), "{}", next.text);
        assert!(!next.text.contains("graphs"));
    }

    #[test]
    fn only_the_next_message_of_the_same_chat_continues_a_warm_session() {
        let earlier = vec![turn("user", "what is a heap?")];
        let slot = Slot::default();
        let next = vec![
            turn("user", "what is a heap?"),
            turn("assistant", "A tree…"),
            turn("user", "and heap sort?"),
        ];
        slot.keep(warm(&earlier));
        assert!(slot.take(7, "chat-0001", &next).is_some());
        assert!(!slot.holding(), "taking empties the slot");
        for (key, chat, messages) in [
            (8, "chat-0001", next.clone()),
            (7, "chat-0002", next.clone()),
            (
                7,
                "chat-0001",
                vec![
                    turn("user", "edited"),
                    turn("assistant", "A tree…"),
                    turn("user", "and heap sort?"),
                ],
            ),
            (7, "chat-0001", next[..2].to_vec()),
        ] {
            slot.keep(warm(&earlier));
            assert!(slot.take(key, chat, &messages).is_none());
            assert_eq!(slot.holding(), chat != "chat-0001");
            slot.clear();
        }
        let mut full = warm(&earlier);
        full.sent = 1_000;
        slot.keep(full);
        assert!(
            slot.take(7, "chat-0001", &next).is_none(),
            "a long conversation starts fresh"
        );
        let mut old = warm(&earlier);
        old.used = Instant::now() - IDLE;
        slot.keep(old);
        slot.tidy();
        assert!(!slot.holding(), "an idle session is stopped");
        slot.keep(warm(&earlier));
        slot.keep(warm(&next));
        assert!(slot.holding());
        slot.clear();
        assert!(!slot.holding());
    }
}
