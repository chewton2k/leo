use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};

use crate::notes::checkbox_line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub start: usize,
    pub end: usize,
}

struct Word<'a> {
    start: usize,
    text: &'a str,
}

fn words(text: &str) -> Vec<Word<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(s)) => {
                out.push(Word {
                    start: s,
                    text: &text[s..i],
                });
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(Word {
            start: s,
            text: &text[s..],
        });
    }
    out
}

fn bare(word: &str) -> String {
    word.trim_matches(|c: char| {
        matches!(c, ',' | '.' | ';' | ':' | '(' | ')' | '!' | '?' | '*' | '_')
    })
    .to_lowercase()
}

fn month_of(word: &str) -> Option<u32> {
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let w = bare(word);
    if w.len() < 3 {
        return None;
    }
    NAMES
        .iter()
        .position(|n| {
            *n == w || (w.len() >= 3 && n.starts_with(&w) && (w.len() == 3 || w == "sept"))
        })
        .map(|i| i as u32 + 1)
}

fn day_of(word: &str) -> Option<u32> {
    let w = bare(word);
    let digits = w
        .strip_suffix("st")
        .or_else(|| w.strip_suffix("nd"))
        .or_else(|| w.strip_suffix("rd"))
        .or_else(|| w.strip_suffix("th"))
        .unwrap_or(&w);
    if digits.is_empty() || digits.len() > 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

fn year_of(word: &str) -> Option<i32> {
    let w = bare(word);
    (w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
        .then(|| w.parse().ok())
        .flatten()
        .filter(|y| (1900..=2200).contains(y))
}

fn weekday_of(word: &str) -> Option<Weekday> {
    let w = bare(word);
    if w.len() < 3 {
        return None;
    }
    [
        ("monday", Weekday::Mon),
        ("tuesday", Weekday::Tue),
        ("wednesday", Weekday::Wed),
        ("thursday", Weekday::Thu),
        ("friday", Weekday::Fri),
        ("saturday", Weekday::Sat),
        ("sunday", Weekday::Sun),
    ]
    .iter()
    .find(|(name, _)| *name == w || (w.len() == 3 && name.starts_with(&w)))
    .map(|(_, d)| *d)
}

fn iso_of(word: &str) -> Option<NaiveDate> {
    let w = bare(word);
    if w.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(&w, "%Y-%m-%d").ok()
}

fn clock(text: &str) -> Option<(u32, u32)> {
    let (h, m) = match text.split_once(':') {
        Some((h, m)) => (h, m),
        None => (text, "00"),
    };
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h <= 23 && m <= 59).then_some((h, m))
}

fn meridiem(word: &str) -> Option<bool> {
    match bare(word).replace('.', "").as_str() {
        "am" => Some(false),
        "pm" => Some(true),
        _ => None,
    }
}

fn with_meridiem(h: u32, m: u32, pm: bool) -> Option<NaiveTime> {
    if !(1..=12).contains(&h) {
        return None;
    }
    let h = match (h, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (h, true) => h + 12,
        (h, false) => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

fn time_at(list: &[Word], i: usize) -> Option<(NaiveTime, usize)> {
    let word = list.get(i)?;
    let w = bare(word.text);
    if w == "noon" {
        return Some((NaiveTime::from_hms_opt(12, 0, 0)?, i));
    }
    for (end, pm) in [("am", false), ("pm", true), ("a.m", false), ("p.m", true)] {
        if let Some(head) = w.strip_suffix(end) {
            if !head.is_empty() {
                let (h, m) = clock(head)?;
                return Some((with_meridiem(h, m, pm)?, i));
            }
        }
    }
    let (h, m) = clock(&w)?;
    if let Some(pm) = list.get(i + 1).and_then(|n| meridiem(n.text)) {
        return Some((with_meridiem(h, m, pm)?, i + 1));
    }
    (w.contains(':'))
        .then(|| NaiveTime::from_hms_opt(h, m, 0))
        .flatten()
        .map(|t| (t, i))
}

fn due_plain(text: &str) -> Option<Due> {
    let list = words(text);
    let mut found: Vec<Due> = Vec::new();
    let mut vague = false;
    let mut i = 0;
    while i < list.len() {
        let mut matched: Option<(NaiveDate, usize, usize)> = None;
        if let Some(date) = iso_of(list[i].text) {
            matched = Some((date, i, i));
        } else if let (Some(m), Some(d)) = (
            month_of(list[i].text),
            list.get(i + 1).and_then(|w| day_of(w.text)),
        ) {
            match list.get(i + 2).and_then(|w| year_of(w.text)) {
                Some(y) => matched = NaiveDate::from_ymd_opt(y, m, d).map(|date| (date, i, i + 2)),
                None => vague = true,
            }
        } else if let (Some(d), Some(m)) = (
            day_of(list[i].text),
            list.get(i + 1).and_then(|w| month_of(w.text)),
        ) {
            match list.get(i + 2).and_then(|w| year_of(w.text)) {
                Some(y) => matched = NaiveDate::from_ymd_opt(y, m, d).map(|date| (date, i, i + 2)),
                None => vague = true,
            }
        }
        let Some((date, first, last)) = matched else {
            i += 1;
            continue;
        };
        let first = if first > 0 && weekday_of(list[first - 1].text).is_some() {
            first - 1
        } else {
            first
        };
        let mut end_word = last;
        let mut time = None;
        let mut next = last + 1;
        if list
            .get(next)
            .is_some_and(|w| matches!(bare(w.text).as_str(), "at" | "by" | "@"))
        {
            next += 1;
        }
        if let Some((t, upto)) = time_at(&list, next) {
            time = Some(t);
            end_word = upto;
        }
        let end = list[end_word].start
            + list[end_word]
                .text
                .trim_end_matches([',', '.', ';', ')'])
                .len();
        found.push(Due {
            date,
            time,
            start: list[first].start,
            end,
        });
        i = end_word + 1;
    }
    (found.len() == 1 && !vague).then(|| found[0])
}

pub fn date_phrase(date: NaiveDate, time: Option<NaiveTime>) -> String {
    let day = date.format("%A, %B ").to_string()
        + &date.day().to_string()
        + &date.format(", %Y").to_string();
    match time {
        Some(t) => {
            let (pm, h) = t
                .format("%p %I")
                .to_string()
                .split_once(' ')
                .map(|(p, h)| (p.to_lowercase(), h.trim_start_matches('0').to_string()))
                .unwrap_or_default();
            format!("{day} at {h}:{} {pm}", t.format("%M"))
        }
        None => day,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Redated {
    pub text: String,
    pub exact: bool,
}

pub fn due_in(text: &str) -> Option<Due> {
    let Some(at) = own_due_tail(text) else {
        return due_plain(text);
    };
    let last = text[at..]
        .rfind(" — due ")
        .map(|i| at + i + " — due ".len())?;
    let part = text[last..].trim_end().trim_end_matches('.');
    due_plain(part).map(|due| Due {
        start: last + due.start,
        end: last + due.end,
        ..due
    })
}

fn own_due_tail(text: &str) -> Option<usize> {
    const MARK: &str = " — due ";
    text.match_indices(MARK).map(|(at, _)| at).find(|&at| {
        text[at..].split(MARK).skip(1).all(|part| {
            let part = part.trim_end().trim_end_matches('.');
            due_plain(part).is_some_and(|due| due.start == 0 && due.end == part.len())
        })
    })
}

pub fn with_due(text: &str, date: Option<NaiveDate>, time: Option<NaiveTime>) -> Redated {
    if let Some(at) = own_due_tail(text) {
        let base = &text[..at];
        return Redated {
            text: match date {
                Some(date) => format!("{base} — due {}.", date_phrase(date, time)),
                None => format!("{}.", base.trim_end_matches('.')),
            },
            exact: true,
        };
    }
    let has_vague = due_in(text).is_none()
        && words(text).windows(2).any(|w| {
            (month_of(w[0].text).is_some() && day_of(w[1].text).is_some())
                || (day_of(w[0].text).is_some() && month_of(w[1].text).is_some())
        });
    match (due_in(text), date) {
        (Some(due), Some(date)) => Redated {
            text: format!(
                "{}{}{}",
                &text[..due.start],
                date_phrase(date, time),
                &text[due.end..]
            ),
            exact: true,
        },
        (Some(due), None) => {
            let head = text[..due.start].trim_end();
            let head = ["— due", "- due", "due", "by", "—", "-", ","]
                .iter()
                .find_map(|c| head.strip_suffix(c))
                .unwrap_or(head)
                .trim_end();
            let tail = text[due.end..].trim_start_matches(['.', ',']).trim();
            Redated {
                text: if tail.is_empty() {
                    format!("{head}.")
                        .replace("..", ".")
                        .trim_end_matches(" .")
                        .to_string()
                } else {
                    format!("{head} {tail}")
                },
                exact: true,
            }
        }
        (None, Some(date)) => {
            let base = text.trim_end().trim_end_matches('.');
            Redated {
                text: format!("{base} — due {}.", date_phrase(date, time)),
                exact: !has_vague,
            }
        }
        (None, None) => Redated {
            text: text.to_string(),
            exact: true,
        },
    }
}

pub fn set_box(line: &str, done: bool) -> Option<String> {
    let found = checkbox_line(line)?;
    let at = found.state;
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..at]);
    out.push(if done { 'x' } else { ' ' });
    out.push_str(&line[at + 1..]);
    Some(out)
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    (rest.is_empty() || rest.starts_with(' '))
        .then(|| (level, rest.trim().trim_end_matches('#').trim()))
}

pub fn is_action_heading(title: &str) -> bool {
    let t = title.trim().trim_end_matches(':').trim().to_lowercase();
    matches!(
        t.as_str(),
        "action items" | "action item" | "to do" | "todo" | "to-do" | "deadlines" | "next steps"
    )
}

pub fn action_items(body: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut inside: Option<usize> = None;
    let mut fence = false;
    for (n, line) in body.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        if let Some((level, title)) = heading(line) {
            inside = match inside {
                Some(l) if level > l => Some(l),
                _ => is_action_heading(title).then_some(level),
            };
            continue;
        }
        if inside.is_some() && !trimmed.starts_with('>') && checkbox_line(line).is_some() {
            out.push(n);
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
pub enum Located {
    At(usize),
    Missing,
    Unclear,
}

fn neighbour(lines: &[&str], n: usize, up: bool) -> String {
    let mut k = n as i64;
    loop {
        k += if up { -1 } else { 1 };
        if k < 0 || k as usize >= lines.len() {
            return String::new();
        }
        let line = lines[k as usize].trim();
        if !line.is_empty() {
            return line.to_string();
        }
    }
}

pub fn context_of(body: &str, n: usize) -> (String, String) {
    let lines: Vec<&str> = body.lines().collect();
    (neighbour(&lines, n, true), neighbour(&lines, n, false))
}

pub fn locate(body: &str, text: &str, before: &str, after: &str) -> Located {
    let lines: Vec<&str> = body.lines().collect();
    let same: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| checkbox_line(l).is_some_and(|c| c.text.trim() == text.trim()))
        .map(|(n, _)| n)
        .collect();
    match same.len() {
        0 => Located::Missing,
        1 => Located::At(same[0]),
        _ => {
            let fits: Vec<usize> = same
                .iter()
                .copied()
                .filter(|&n| {
                    neighbour(&lines, n, true) == before.trim()
                        && neighbour(&lines, n, false) == after.trim()
                })
                .collect();
            if fits.len() == 1 {
                Located::At(fits[0])
            } else {
                Located::Unclear
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn one_absolute_date_and_an_optional_time_are_read() {
        let due = due_in("Submit Homework 4 — due October 16, 2026 at 11:59 pm.").unwrap();
        assert_eq!(
            (due.date, due.time),
            (day(2026, 10, 16), NaiveTime::from_hms_opt(23, 59, 0))
        );
        assert_eq!(
            &"Submit Homework 4 — due October 16, 2026 at 11:59 pm."[due.start..due.end],
            "October 16, 2026 at 11:59 pm"
        );
        let cases = [
            ("Exam on Friday, Oct 16 2026", day(2026, 10, 16), None),
            ("Lab 2026-03-05", day(2026, 3, 5), None),
            (
                "Talk 5 March 2026 9am",
                day(2026, 3, 5),
                NaiveTime::from_hms_opt(9, 0, 0),
            ),
            (
                "Meet Sept 8th, 2026, 15:30",
                day(2026, 9, 8),
                NaiveTime::from_hms_opt(15, 30, 0),
            ),
            (
                "Lunch December 1, 2026 at noon",
                day(2026, 12, 1),
                NaiveTime::from_hms_opt(12, 0, 0),
            ),
            ("Late 12 am Jan 2 2027", day(2027, 1, 2), None),
        ];
        for (text, date, time) in cases {
            let due = due_in(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!((due.date, due.time), (date, time), "{text}");
        }
        assert_eq!(
            due_in("Friday, October 16, 2026").unwrap().start,
            0,
            "the weekday is part of the phrase"
        );
    }

    #[test]
    fn unclear_dates_are_not_guessed() {
        for text in [
            "Submit the essay next Friday",
            "Due October 16",
            "Read chapters 3 and 4",
            "Between Oct 1 2026 and Oct 9 2026",
            "Bring 2 pens to may 2026 meeting",
            "Room 2026",
        ] {
            assert_eq!(due_in(text), None, "{text}");
        }
    }

    #[test]
    fn a_task_with_leos_own_due_phrase_is_dated_even_beside_a_date_it_cannot_read() {
        let text = "Book the study room for Dec 2 — due Thursday, November 26, 2026 at 9:00 am.";
        let due = due_in(text).expect("leo's phrase dates the task");
        assert_eq!(
            (due.date, due.time),
            (day(2026, 11, 26), NaiveTime::from_hms_opt(9, 0, 0))
        );
        assert_eq!(
            &text[due.start..due.end],
            "Thursday, November 26, 2026 at 9:00 am"
        );
        let piled = "Book the study room for Dec 2 — due Thursday, November 26, 2026 at 9:00 am — due Thursday, November 26, 2026 at 7:45 am.";
        assert_eq!(
            due_in(piled).map(|d| d.time),
            Some(NaiveTime::from_hms_opt(7, 45, 0)),
            "the newest phrase wins"
        );
        assert!(
            due_in("Book the study room for Dec 2").is_none(),
            "a date without a year still dates nothing"
        );
    }

    #[test]
    fn leos_own_due_phrase_is_replaced_even_beside_a_date_it_cannot_read() {
        let nine = NaiveTime::from_hms_opt(9, 0, 0);
        let first = with_due(
            "Book the study room for Dec 2",
            Some(day(2026, 11, 26)),
            nine,
        );
        assert!(
            !first.exact,
            "the first date still asks, since Dec 2 has no year"
        );
        assert_eq!(
            with_due(
                &first.text,
                Some(day(2026, 11, 27)),
                NaiveTime::from_hms_opt(10, 0, 0)
            ),
            Redated {
                text: "Book the study room for Dec 2 — due Friday, November 27, 2026 at 10:00 am."
                    .into(),
                exact: true
            },
            "moving it again replaces leo's phrase without asking"
        );
        let piled = "Book the study room for Dec 2 — due Thursday, November 26, 2026 at 9:00 am — due Thursday, November 26, 2026 at 9:00 am — due Thursday, November 26, 2026 at 8:15 am — due Thursday, November 26, 2026 at 7:45 am.";
        assert_eq!(
            with_due(
                piled,
                Some(day(2026, 11, 26)),
                NaiveTime::from_hms_opt(10, 0, 0)
            ),
            Redated {
                text:
                    "Book the study room for Dec 2 — due Thursday, November 26, 2026 at 10:00 am."
                        .into(),
                exact: true
            },
            "phrases piled up by the old bug collapse into one"
        );
        assert_eq!(
            with_due(piled, None, None),
            Redated {
                text: "Book the study room for Dec 2.".into(),
                exact: true
            }
        );
        assert_eq!(
            with_due(
                "Meet Sam — due later this week",
                Some(day(2026, 11, 26)),
                None
            )
            .text,
            "Meet Sam — due later this week — due Thursday, November 26, 2026.",
            "words after a dash that are not a date are never taken for leo's phrase"
        );
    }

    #[test]
    fn a_date_is_written_in_replacing_the_old_one_or_appended() {
        let time = NaiveTime::from_hms_opt(23, 59, 0);
        assert_eq!(
            with_due(
                "Submit lab — due October 1, 2026.",
                Some(day(2026, 10, 16)),
                time
            ),
            Redated {
                text: "Submit lab — due Friday, October 16, 2026 at 11:59 pm.".into(),
                exact: true
            }
        );
        assert_eq!(
            with_due("Ask tutor about the exam", Some(day(2026, 10, 2)), None),
            Redated {
                text: "Ask tutor about the exam — due Friday, October 2, 2026.".into(),
                exact: true
            }
        );
        assert_eq!(
            with_due("Essay due Oct 16", Some(day(2026, 10, 16)), None),
            Redated {
                text: "Essay due Oct 16 — due Friday, October 16, 2026.".into(),
                exact: false
            },
            "a date leo cannot read stays, and the change is previewed"
        );
        assert_eq!(
            with_due("Submit lab — due October 1, 2026.", None, None).text,
            "Submit lab."
        );
        assert_eq!(
            date_phrase(day(2026, 10, 16), NaiveTime::from_hms_opt(9, 5, 0)),
            "Friday, October 16, 2026 at 9:05 am"
        );
    }

    #[test]
    fn action_items_are_the_checkboxes_under_that_heading_only() {
        let body = "# Lecture\n- [ ] not a task\n## Action items\n- [ ] Submit HW 4\n- [x] Email Maya\n> - [ ] quoted\n```\n- [ ] code\n```\n### Details\n- [ ] still under action items\n## Check yourself\n- [ ] Practice question\n## To do:\n1. [ ] Register for the exam";
        assert_eq!(action_items(body), [3, 4, 10, 14]);
    }

    #[test]
    fn a_checkbox_is_found_again_after_edits_or_marked_unclear() {
        let body = "## Action items\n- [ ] Read\n- [ ] Submit HW\nmid\n- [ ] Read\nend";
        assert_eq!(locate(body, "Submit HW", "", ""), Located::At(2));
        assert_eq!(locate(body, "Gone", "", ""), Located::Missing);
        assert_eq!(locate(body, "Read", "", ""), Located::Unclear);
        assert_eq!(locate(body, "Read", "mid", "end"), Located::At(4));
        assert_eq!(context_of(body, 4), ("mid".into(), "end".into()));
        assert_eq!(set_box("  - [ ] Read", true).unwrap(), "  - [x] Read");
        assert_eq!(set_box("1. [x] Read", false).unwrap(), "1. [ ] Read");
        assert_eq!(set_box("plain", true), None);
        for (open, done) in [
            ("* [ ] star", "* [x] star"),
            ("+ [ ] plus", "+ [x] plus"),
            ("2) [ ] two", "2) [x] two"),
            ("  - [ ]", "  - [x]"),
            ("-   [ ] wide", "-   [x] wide"),
            ("a) [ ] one", "a) [x] one"),
            ("bb. [ ] two", "bb. [x] two"),
            ("B.  [ ] four", "B.  [x] four"),
        ] {
            assert_eq!(set_box(open, true).as_deref(), Some(done));
            assert_eq!(set_box(done, false).as_deref(), Some(open));
        }
    }
}
