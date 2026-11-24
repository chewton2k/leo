use std::ops::Range;

use serde::Serialize;

use crate::record::MOST_TRANSCRIPT_CHARS;

const PASSED_WORDS: usize = 12;
const MOST_ALIGNED: usize = 1 << 22;
const WINDOW_WORDS: usize = 1 << 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Turn {
    pub n: u64,
    pub speaker: &'static str,
    pub text: String,
}

#[derive(Default)]
pub(crate) struct Turns {
    sides: Vec<Side>,
    starts: Vec<Start>,
    next: u64,
}

struct Side {
    speaker: &'static str,
    text: String,
    said: usize,
    cut: usize,
}

struct Start {
    n: u64,
    side: usize,
    from: usize,
}

impl Turns {
    pub(crate) fn hear(&mut self, speaker: &'static str, whole: &str) {
        let side = match self.sides.iter().position(|s| s.speaker == speaker) {
            Some(side) => side,
            None => {
                self.sides.push(Side {
                    speaker,
                    text: String::new(),
                    said: 0,
                    cut: 0,
                });
                self.sides.len() - 1
            }
        };
        let passed = self.said_since(side) >= PASSED_WORDS;
        let kept = &self.sides[side];
        let (old_text, cut) = (kept.text.as_str(), kept.cut);
        let heard = whole
            .char_indices()
            .nth(cut)
            .and_then(|(at, _)| whole.get(at..))
            .unwrap_or("");
        let old = words(old_text);
        let new = words(heard);
        let mut found = align(&texts(old_text, &old), &texts(heard, &new));
        let last = found.iter().rposition(Option::is_some);
        let (old_end, new_end) = last.map_or((0, 0), |j| (j + 1, found[j].unwrap_or(0) + 1));
        let mut text = heard.to_string();
        let mut fresh = new_end + (old.len() - old_end).min(new.len() - new_end);
        if new_end == new.len() && old_end < old.len() && !passed {
            text.push(' ');
            text.push_str(&old_text[old[old_end].start..]);
            for (k, at) in found[old_end..].iter_mut().enumerate() {
                *at = Some(new.len() + k);
            }
            fresh += old.len() - old_end;
        }
        let now = words(&text);
        let place = |from: usize| {
            let b = old.partition_point(|w| w.start < from);
            if let Some(Some(at)) = found.get(b) {
                return *at;
            }
            let before = found[..b].iter().rposition(Option::is_some);
            let after = found[b..].iter().flatten().next().copied();
            let at = before.map_or(b, |j| found[j].unwrap_or(0) + b - j);
            at.min(after.unwrap_or(now.len()))
        };
        let byte = |word: usize| now.get(word).map_or(text.len(), |w| w.start);
        let mut first = true;
        for start in self.starts.iter_mut().filter(|s| s.side == side) {
            start.from = if first { 0 } else { byte(place(start.from)) };
            first = false;
        }
        if self.starts.last().is_none_or(|s| s.side != side) {
            let at = if first || passed {
                Some(fresh)
            } else {
                (fresh..now.len()).find(|k| *k == 0 || ends(&text[now[k - 1].clone()]))
            };
            if let Some(word) = at.and_then(|at| now.get(at)) {
                self.starts.push(Start {
                    n: self.next,
                    side,
                    from: if first { 0 } else { word.start },
                });
                self.next += 1;
            }
        }
        let side_of = &mut self.sides[side];
        side_of.said = heard.len();
        side_of.text = text;
        self.drop_empty(side);
        self.trim();
    }

    pub(crate) fn turns(&self) -> Vec<Turn> {
        let mut out: Vec<Turn> = Vec::new();
        for (k, start) in self.starts.iter().enumerate() {
            let side = &self.sides[start.side];
            let end = self.end(k).min(side.said);
            let said = side.text.get(start.from..end).unwrap_or("").trim();
            if said.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.speaker == side.speaker => {
                    last.text.push(' ');
                    last.text.push_str(said);
                }
                _ => out.push(Turn {
                    n: start.n,
                    speaker: side.speaker,
                    text: said.to_string(),
                }),
            }
        }
        out
    }

    fn end(&self, k: usize) -> usize {
        let side = self.starts[k].side;
        self.starts[k + 1..]
            .iter()
            .find(|s| s.side == side)
            .map_or(self.sides[side].text.len(), |s| s.from)
    }

    fn said_since(&self, side: usize) -> usize {
        let after = self
            .starts
            .iter()
            .rposition(|s| s.side == side)
            .map_or(0, |k| k + 1);
        (after..self.starts.len())
            .map(|k| {
                let start = &self.starts[k];
                let other = &self.sides[start.side];
                let end = self.end(k).min(other.said);
                words(other.text.get(start.from..end).unwrap_or("")).len()
            })
            .sum()
    }

    fn drop_empty(&mut self, side: usize) {
        let mut until = self.sides[side].text.len();
        for k in (0..self.starts.len()).rev() {
            if self.starts[k].side != side {
                continue;
            }
            if self.starts[k].from >= until {
                self.starts.remove(k);
            } else {
                until = self.starts[k].from;
            }
        }
    }

    fn trim(&mut self) {
        let line = |side: &Side| side.speaker.len() + 3;
        let held: usize = self
            .sides
            .iter()
            .map(|s| s.text.chars().count())
            .chain(self.starts.iter().map(|s| line(&self.sides[s.side])))
            .sum();
        let mut over = held.saturating_sub(MOST_TRANSCRIPT_CHARS);
        while over > 0 && !self.starts.is_empty() {
            let side = self.starts[0].side;
            let end = self.end(0);
            let chars = self.sides[side].text[..end].chars().count();
            let take = over.min(chars);
            let bytes = self.sides[side]
                .text
                .char_indices()
                .nth(take)
                .map_or(end, |(at, _)| at);
            let from = &mut self.sides[side];
            from.text.drain(..bytes);
            from.said = from.said.saturating_sub(bytes);
            from.cut += take;
            for start in self.starts.iter_mut().filter(|s| s.side == side) {
                start.from = start.from.saturating_sub(bytes);
            }
            over -= take;
            if take == chars {
                self.starts.remove(0);
            }
        }
    }
}

fn words(text: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut open = false;
    for (at, c) in text.char_indices() {
        if c.is_whitespace() {
            open = false;
            continue;
        }
        let alone = matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}');
        match out.last_mut().filter(|_| open && !alone) {
            Some(word) => word.end = at + c.len_utf8(),
            None => out.push(at..at + c.len_utf8()),
        }
        open = true;
    }
    out
}

fn texts<'a>(text: &'a str, words: &[Range<usize>]) -> Vec<&'a str> {
    words.iter().map(|w| &text[w.clone()]).collect()
}

fn ends(word: &str) -> bool {
    word.chars()
        .rev()
        .take_while(|c| !c.is_alphanumeric())
        .any(|c| ".!?…。！？؟।".contains(c))
}

fn sound(word: &str) -> impl Iterator<Item = char> + '_ {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
}

fn align(old: &[&str], new: &[&str]) -> Vec<Option<usize>> {
    let same = |(a, b): &(&&str, &&str)| sound(a).eq(sound(b));
    let head = old.iter().zip(new).take_while(same).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(same)
        .count();
    let mut found = vec![None; old.len()];
    for (k, at) in found[..head].iter_mut().enumerate() {
        *at = Some(k);
    }
    for k in 1..=tail {
        found[old.len() - k] = Some(new.len() - k);
    }
    let key =
        |words: &[&str]| -> Vec<String> { words.iter().map(|w| sound(w).collect()).collect() };
    let (a, b) = (
        key(&old[head..old.len() - tail]),
        key(&new[head..new.len() - tail]),
    );
    let window = if a.len().saturating_mul(b.len()) > MOST_ALIGNED {
        WINDOW_WORDS
    } else {
        a.len().max(b.len())
    };
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let x = &a[i..a.len().min(i + window)];
        let y = &b[j..b.len().min(j + window)];
        let last = i + x.len() == a.len() && j + y.len() == b.len();
        let until = if last { x.len() } else { x.len().div_ceil(2) };
        let pairs = common(x, y);
        let (kept, rest) = pairs.split_at(pairs.partition_point(|(p, _)| *p < until));
        for (p, q) in kept {
            found[head + i + p] = Some(head + j + q);
        }
        i += until;
        j += match (kept.last(), rest.first()) {
            (Some((_, q)), _) => q + 1,
            (None, Some((_, q))) => (*q).min(y.len() / 2),
            (None, None) => y.len() / 2,
        };
    }
    found
}

fn common(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let wide = b.len() + 1;
    let mut held = vec![0u16; (a.len() + 1) * wide];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            held[i * wide + j] = if a[i] == b[j] {
                held[(i + 1) * wide + j + 1] + 1
            } else {
                held[(i + 1) * wide + j].max(held[i * wide + j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if held[(i + 1) * wide + j] >= held[i * wide + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(turns: &Turns) -> Vec<String> {
        turns
            .turns()
            .iter()
            .map(|t| format!("{}: {}", t.speaker, t.text))
            .collect()
    }

    fn call(events: &[(&'static str, &str)]) -> Turns {
        let mut turns = Turns::default();
        for (speaker, text) in events {
            turns.hear(speaker, text);
        }
        turns
    }

    fn one_side(turns: &Turns, speaker: &str) -> String {
        turns
            .turns()
            .into_iter()
            .filter(|t| t.speaker == speaker)
            .map(|t| t.text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_side_that_goes_on_talking_grows_and_revises_one_turn() {
        let turns = call(&[
            ("You", "Hello."),
            ("You", "Hello, hello um"),
            ("You", "Hello, hello. So what are we"),
            ("You", "Hello, hello. So, what are we talking about today?"),
        ]);
        assert_eq!(
            lines(&turns),
            ["You: Hello, hello. So, what are we talking about today?"]
        );
        assert_eq!(turns.turns()[0].n, 0);
    }

    #[test]
    fn the_sides_take_turns_and_their_numbers_stay() {
        let mut turns = call(&[
            ("You", "How was it?"),
            ("Others", "It went"),
            ("Others", "It went well."),
            ("You", "How was it? Good."),
        ]);
        let numbers = |turns: &Turns| turns.turns().iter().map(|t| t.n).collect::<Vec<_>>();
        assert_eq!(
            lines(&turns),
            ["You: How was it?", "Others: It went well.", "You: Good."]
        );
        assert_eq!(numbers(&turns), [0, 1, 2]);
        turns.hear("You", "How was it? Good. And the date?");
        turns.hear("Others", "It went well. Tuesday.");
        assert_eq!(
            lines(&turns),
            [
                "You: How was it?",
                "Others: It went well.",
                "You: Good. And the date?",
                "Others: Tuesday."
            ]
        );
        assert_eq!(numbers(&turns), [0, 1, 2, 3]);
    }

    #[test]
    fn both_talking_at_once_is_split_by_sentences_not_by_words() {
        let turns = call(&[
            ("You", "I think"),
            ("Others", "Yeah."),
            ("You", "I think we should"),
            ("Others", "Yeah. Right."),
            ("You", "I think we should go. And"),
            ("Others", "Yeah. Right."),
            ("You", "I think we should go. And then eat"),
        ]);
        assert_eq!(
            lines(&turns),
            [
                "You: I think we should go.",
                "Others: Yeah. Right.",
                "You: And then eat"
            ]
        );
        let reply = "so here is what they said back to you in a longer reply";
        let mut unpunctuated = call(&[("You", "we could try it"), ("Others", "so here")]);
        unpunctuated.hear("You", "we could try it now");
        assert_eq!(unpunctuated.turns().len(), 2);
        unpunctuated.hear("Others", reply);
        unpunctuated.hear("You", "we could try it now fine");
        assert_eq!(
            lines(&unpunctuated),
            [
                "You: we could try it now".to_string(),
                format!("Others: {reply}"),
                "You: fine".to_string(),
            ],
            "a model without full stops still changes turns after a longer reply"
        );
    }

    #[test]
    fn a_part_heard_again_keeps_every_word_in_the_turn_it_was_said_in() {
        let mut turns = call(&[
            ("You", "we shipped the parser monday."),
            ("Others", "Was it late?"),
            ("You", "we shipped the parser monday. No it was early."),
            ("Others", "Was it late? Great."),
            (
                "You",
                "we shipped the parser monday. No it was early. Next is docs",
            ),
        ]);
        let before: Vec<u64> = turns.turns().iter().map(|t| t.n).collect();
        let again = "So, um, we shipped the new parser on Monday. No, it was really early. Next is the docs.";
        turns.hear("You", again);
        assert_eq!(
            lines(&turns),
            [
                "You: So, um, we shipped the new parser on Monday.",
                "Others: Was it late?",
                "You: No, it was really early.",
                "Others: Great.",
                "You: Next is the docs."
            ]
        );
        assert_eq!(
            turns.turns().iter().map(|t| t.n).collect::<Vec<_>>(),
            before
        );
        assert_eq!(one_side(&turns, "You"), again);
        turns.hear("You", "Nothing that was there before.");
        assert_eq!(one_side(&turns, "You"), "Nothing that was there before.");
        assert_eq!(one_side(&turns, "Others"), "Was it late? Great.");
    }

    #[test]
    fn an_ending_heard_again_returns_to_its_place() {
        let mut turns = call(&[
            ("You", "Shall we start?"),
            ("Others", "Yes."),
            ("You", "Shall we start? First the budget."),
            ("Others", "Yes. Go on."),
        ]);
        turns.hear("You", "Shall we start?");
        assert_eq!(
            lines(&turns),
            ["You: Shall we start?", "Others: Yes. Go on."]
        );
        turns.hear("You", "Shall we start? First, the budget for May.");
        assert_eq!(
            lines(&turns),
            [
                "You: Shall we start?",
                "Others: Yes.",
                "You: First, the budget for May.",
                "Others: Go on."
            ]
        );
    }

    #[test]
    fn after_a_pause_a_side_goes_on_or_begins_a_new_turn() {
        let mut turns = call(&[("You", "One moment. I will"), ("You", "One moment.")]);
        turns.hear("You", "One moment. I will check. Back");
        assert_eq!(lines(&turns), ["You: One moment. I will check. Back"]);
        turns.hear("Others", "Sure.");
        turns.hear("You", "One moment. I will check. Back now.");
        turns.hear("You", "One moment. I will check. Back now. It works.");
        assert_eq!(
            lines(&turns),
            [
                "You: One moment. I will check. Back now.",
                "Others: Sure.",
                "You: It works."
            ]
        );
    }

    #[test]
    fn a_side_that_fails_and_returns_loses_nothing() {
        let mut turns = call(&[("You", "One. Two."), ("Others", "Three."), ("You", "")]);
        assert_eq!(lines(&turns), ["Others: Three."]);
        turns.hear("You", "One. Two. Four.");
        assert_eq!(one_side(&turns, "You"), "One. Two. Four.");
    }

    #[test]
    fn words_without_spaces_take_turns_too() {
        let turns = call(&[
            ("You", "你好吗？"),
            ("Others", "很好。"),
            ("You", "你好吗？我们开始吧。"),
        ]);
        assert_eq!(
            lines(&turns),
            ["You: 你好吗？", "Others: 很好。", "You: 我们开始吧。"]
        );
    }

    #[test]
    fn a_long_call_drops_its_oldest_words_and_keeps_matching() {
        let mut turns = Turns::default();
        let (mut you, mut others) = (String::new(), String::new());
        let sentence = |who: &str, n: usize| format!("{who} sentence {n} {} é. ", "w".repeat(900));
        for n in 0..150 {
            you.push_str(&sentence("you", n));
            turns.hear("You", &you);
            others.push_str(&sentence("others", n));
            turns.hear("Others", &others);
        }
        let shown = turns.turns();
        let joined = lines(&turns).join("\n");
        assert!(joined.chars().count() <= MOST_TRANSCRIPT_CHARS);
        assert!(joined.chars().count() > MOST_TRANSCRIPT_CHARS - 2_000);
        assert!(shown[0].n > 0);
        assert!(shown.windows(2).all(|w| w[0].n + 1 == w[1].n));
        assert!(you.trim().ends_with(&one_side(&turns, "You")));
        assert!(others.trim().ends_with(&one_side(&turns, "Others")));
        let first = shown[1].n;
        you.push_str("Last words");
        turns.hear("You", &you);
        assert!(one_side(&turns, "You").ends_with("Last words"));
        assert!(turns.turns().iter().any(|t| t.n == first));
    }

    #[test]
    fn a_long_call_rewritten_at_both_ends_keeps_every_turn() {
        let said = |n: usize| format!("Sentence number {n} about the parser.");
        let mut turns = Turns::default();
        let (mut you, mut others) = (String::new(), String::new());
        for n in 0..600 {
            you.push_str(&format!("{} ", said(n)));
            turns.hear("You", &you);
            others.push_str(&format!("Reply {n} says a few more words than that. "));
            turns.hear("Others", &others);
        }
        turns.hear("You", &format!("Um {you} and more."));
        let heard: Vec<String> = turns
            .turns()
            .into_iter()
            .filter(|t| t.speaker == "You")
            .map(|t| t.text)
            .collect();
        assert_eq!(heard.len(), 601);
        assert_eq!(heard[0], format!("Um {}", said(0)));
        assert!((1..600).all(|n| heard[n] == said(n)));
        assert_eq!(heard[600], "and more.");
    }

    #[test]
    fn whatever_is_rewritten_a_side_reads_as_its_latest_text() {
        let sides = ["You", "Others"];
        let mut seed = 7u32;
        let mut roll = move |most: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % most
        };
        let mut turns = Turns::default();
        let mut said = [Vec::<String>::new(), Vec::new()];
        for step in 0..400 {
            let side = roll(2) as usize;
            for _ in 0..roll(5) {
                let stop = if roll(4) == 0 { "." } else { "" };
                said[side].push(format!("w{step}x{}{stop}", roll(9)));
            }
            if roll(6) == 0 {
                let at = said[side].len().saturating_sub(roll(20) as usize + 1);
                said[side].truncate(at);
                said[side].push(format!("again{step}"));
            }
            turns.hear(sides[side], &said[side].join(" "));
            for (k, side) in sides.iter().enumerate() {
                assert_eq!(one_side(&turns, side), said[k].join(" "), "step {step}");
            }
        }
        assert!(turns.turns().len() > 10);
    }
}
