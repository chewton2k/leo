use std::collections::{HashMap, HashSet};

use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc,
    Weekday,
};
use serde::{Deserialize, Serialize};

const MOST_EVENTS: usize = 20_000;
const MOST_STEPS: usize = 5_000;
const MOST_PERIODS: i64 = 1_000_000;
const MOST_SHOWN: usize = 200;
const CONTEXT_CHARS: usize = 3_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub location: String,
    pub context: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_day: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub zone_unknown: bool,
}

#[derive(Debug, Default)]
pub struct Calendar {
    pub name: Option<String>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Default)]
struct Raw {
    uid: String,
    summary: String,
    description: String,
    location: String,
    attendees: Vec<String>,
    organizer: String,
    start: Option<When>,
    end: Option<When>,
    duration: Option<Duration>,
    rule: Option<String>,
    except: Vec<DateTime<Utc>>,
    except_days: Vec<NaiveDate>,
    instead_of: Option<DateTime<Utc>>,
    instead_of_day: Option<NaiveDate>,
    cancelled: bool,
    zone_unknown: bool,
}

#[derive(Debug, Clone, Copy)]
enum When {
    At(DateTime<Utc>, Option<chrono_tz::Tz>),
    Day(NaiveDate),
}

pub fn read(text: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Calendar {
    let lines = unfolded(text);
    let mut name = None;
    let mut raws = Vec::new();
    let mut current: Option<Raw> = None;
    let mut depth = 0usize;
    for line in &lines {
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let (key, params) = match head.split_once(';') {
            Some((k, p)) => (k.to_ascii_uppercase(), p),
            None => (head.to_ascii_uppercase(), ""),
        };
        match (key.as_str(), value.trim()) {
            ("BEGIN", "VEVENT") => {
                if raws.len() >= MOST_EVENTS {
                    break;
                }
                current = Some(Raw::default());
                depth = 0;
                continue;
            }
            ("END", "VEVENT") => {
                if let Some(raw) = current.take() {
                    raws.push(raw);
                }
                continue;
            }
            ("BEGIN", _) if current.is_some() => depth += 1,
            ("END", _) if current.is_some() => depth = depth.saturating_sub(1),
            ("X-WR-CALNAME", v) if current.is_none() && !v.is_empty() => {
                name = Some(unescaped(v).chars().take(80).collect())
            }
            _ => {}
        }
        let Some(raw) = current.as_mut() else {
            continue;
        };
        if depth > 0 {
            continue;
        }
        match key.as_str() {
            "UID" => raw.uid = value.trim().to_string(),
            "SUMMARY" => raw.summary = unescaped(value),
            "DESCRIPTION" => raw.description = unescaped(value),
            "LOCATION" => raw.location = unescaped(value),
            "STATUS" => raw.cancelled = value.trim().eq_ignore_ascii_case("CANCELLED"),
            "DTSTART" => {
                raw.start = when(params, value);
                raw.zone_unknown =
                    param(params, "TZID").is_some_and(|z| z.parse::<chrono_tz::Tz>().is_err());
            }
            "DTEND" => raw.end = when(params, value),
            "DURATION" => raw.duration = duration(value),
            "RRULE" => raw.rule = Some(value.trim().to_string()),
            "EXDATE" => {
                for v in value.split(',').take(MOST_STEPS) {
                    match when(params, v) {
                        Some(When::At(t, _)) => raw.except.push(t),
                        Some(When::Day(d)) => raw.except_days.push(d),
                        None => {}
                    }
                }
            }
            "RECURRENCE-ID" => match when(params, value) {
                Some(When::At(t, _)) => raw.instead_of = Some(t),
                Some(When::Day(d)) => raw.instead_of_day = Some(d),
                None => {}
            },
            "ATTENDEE" => {
                if raw.attendees.len() < 100 {
                    if let Some(who) = person(params, value) {
                        if declined(params) {
                            continue;
                        }
                        raw.attendees.push(who);
                    }
                }
            }
            "ORGANIZER" => raw.organizer = person(params, value).unwrap_or_default(),
            _ => {}
        }
    }
    Calendar {
        name,
        events: expand(&raws, from, to),
    }
}

fn unfolded(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

fn unescaped(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.trim().chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn param<'a>(params: &'a str, name: &str) -> Option<&'a str> {
    params.split(';').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim_matches('"'))
    })
}

fn person(params: &str, value: &str) -> Option<String> {
    let name = param(params, "CN").map(str::trim).filter(|n| !n.is_empty());
    let mail = value
        .trim()
        .strip_prefix("mailto:")
        .or_else(|| value.trim().strip_prefix("MAILTO:"))
        .unwrap_or("");
    name.map(str::to_string)
        .or_else(|| (!mail.is_empty()).then(|| mail.to_string()))
}

fn declined(params: &str) -> bool {
    param(params, "PARTSTAT").is_some_and(|p| p.eq_ignore_ascii_case("DECLINED"))
}

fn when(params: &str, value: &str) -> Option<When> {
    let value = value.trim();
    if param(params, "VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .ok()
            .map(When::Day);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let t = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(When::At(Utc.from_utc_datetime(&t), None));
    }
    let t = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let zone = param(params, "TZID").and_then(|z| z.parse::<chrono_tz::Tz>().ok());
    Some(When::At(local_to_utc(t, zone)?, zone))
}

fn local_to_utc(t: NaiveDateTime, zone: Option<chrono_tz::Tz>) -> Option<DateTime<Utc>> {
    let pick = |r: LocalResult<DateTime<Utc>>| match r {
        LocalResult::Single(x) | LocalResult::Ambiguous(x, _) => Some(x),
        LocalResult::None => None,
    };
    match zone {
        Some(z) => pick(z.from_local_datetime(&t).map(|x| x.with_timezone(&Utc))).or_else(|| {
            pick(
                z.from_local_datetime(&(t + Duration::hours(1)))
                    .map(|x| x.with_timezone(&Utc)),
            )
        }),
        None => pick(Local.from_local_datetime(&t).map(|x| x.with_timezone(&Utc))).or_else(|| {
            pick(
                Local
                    .from_local_datetime(&(t + Duration::hours(1)))
                    .map(|x| x.with_timezone(&Utc)),
            )
        }),
    }
}

fn duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    let (negative, value) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let value = value.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut number = String::new();
    let mut in_time = false;
    for c in value.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                let part = match (unit, in_time) {
                    ('W', _) => Duration::try_weeks(n)?,
                    ('D', _) => Duration::try_days(n)?,
                    ('H', true) => Duration::try_hours(n)?,
                    ('M', true) => Duration::try_minutes(n)?,
                    ('S', true) => Duration::try_seconds(n)?,
                    _ => return None,
                };
                total = total.checked_add(&part)?;
                if total > Duration::days(366) {
                    return None;
                }
            }
        }
    }
    Some(if negative { -total } else { total })
}

struct Rule {
    freq: String,
    interval: i64,
    count: Option<usize>,
    until: Option<DateTime<Utc>>,
    days: Vec<Weekday>,
}

fn rule_of(text: &str) -> Option<Rule> {
    let mut rule = Rule {
        freq: String::new(),
        interval: 1,
        count: None,
        until: None,
        days: Vec::new(),
    };
    for part in text.split(';') {
        let (k, v) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => rule.freq = v.to_ascii_uppercase(),
            "INTERVAL" => rule.interval = v.parse::<i64>().ok()?.max(1),
            "COUNT" => rule.count = Some(v.parse().ok()?),
            "UNTIL" => {
                rule.until = match when("", v) {
                    Some(When::At(t, _)) => Some(t),
                    Some(When::Day(d)) => {
                        d.and_hms_opt(23, 59, 59).map(|d| Utc.from_utc_datetime(&d))
                    }
                    None => None,
                }
            }
            "BYDAY" => {
                for day in v.split(',') {
                    let code: String = day.chars().filter(|c| c.is_ascii_alphabetic()).collect();
                    if day.len() != code.len() {
                        return None;
                    }
                    rule.days.push(match code.to_ascii_uppercase().as_str() {
                        "MO" => Weekday::Mon,
                        "TU" => Weekday::Tue,
                        "WE" => Weekday::Wed,
                        "TH" => Weekday::Thu,
                        "FR" => Weekday::Fri,
                        "SA" => Weekday::Sat,
                        "SU" => Weekday::Sun,
                        _ => return None,
                    });
                }
            }
            "BYMONTHDAY" | "BYMONTH" | "BYSETPOS" | "BYYEARDAY" | "BYWEEKNO" | "BYHOUR"
            | "BYMINUTE" | "BYSECOND" => return None,
            _ => {}
        }
    }
    let by_day_fits = rule.days.is_empty() || matches!(rule.freq.as_str(), "DAILY" | "WEEKLY");
    (by_day_fits
        && matches!(
            rule.freq.as_str(),
            "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY"
        ))
    .then_some(rule)
}

fn naive_in(t: DateTime<Utc>, zone: Option<chrono_tz::Tz>) -> NaiveDateTime {
    match zone {
        Some(z) => t.with_timezone(&z).naive_local(),
        None => t.with_timezone(&Local).naive_local(),
    }
}

fn occurrences(
    start: DateTime<Utc>,
    zone: Option<chrono_tz::Tz>,
    rule: &Rule,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<DateTime<Utc>> {
    let first = naive_in(start, zone);
    let monday = first.date() - Duration::days(first.weekday().num_days_from_monday() as i64);
    let mut days = if rule.days.is_empty() {
        vec![first.weekday()]
    } else {
        rule.days.clone()
    };
    days.sort_by_key(|d| d.num_days_from_monday());
    days.dedup();
    let mut out = Vec::new();
    let skipped = if rule.count.is_some() {
        0
    } else {
        let gap = (naive_in(from, zone) - first).num_days().max(0);
        let periods = match rule.freq.as_str() {
            "DAILY" => gap,
            "WEEKLY" => gap / 7,
            "MONTHLY" => gap / 31,
            _ => gap / 366,
        };
        (periods / rule.interval - 1).max(0)
    };
    for n in skipped..skipped + MOST_STEPS as i64 {
        let Some(k) = n.checked_mul(rule.interval).filter(|k| *k <= MOST_PERIODS) else {
            return out;
        };
        let batch: Vec<NaiveDateTime> = match rule.freq.as_str() {
            "DAILY" => Some(first + Duration::days(k))
                .filter(|t| rule.days.is_empty() || rule.days.contains(&t.weekday()))
                .into_iter()
                .collect(),
            "WEEKLY" => days
                .iter()
                .map(|d| {
                    (monday + Duration::weeks(k) + Duration::days(d.num_days_from_monday() as i64))
                        .and_time(first.time())
                })
                .filter(|t| *t >= first)
                .collect(),
            "MONTHLY" => {
                let months = first.month0() as i64 + k;
                let year = first.year() as i64 + months.div_euclid(12);
                let month = months.rem_euclid(12) as u32 + 1;
                NaiveDate::from_ymd_opt(year as i32, month, first.day())
                    .map(|d| d.and_time(first.time()))
                    .into_iter()
                    .collect()
            }
            _ => NaiveDate::from_ymd_opt(first.year() + k as i32, first.month(), first.day())
                .map(|d| d.and_time(first.time()))
                .into_iter()
                .collect(),
        };
        for local in batch {
            let Some(t) = local_to_utc(local, zone) else {
                continue;
            };
            if rule.count.is_some_and(|c| out.len() >= c)
                || rule.until.is_some_and(|u| t > u)
                || t > to
            {
                return out;
            }
            out.push(t);
        }
    }
    out
}

fn most_shown(from: DateTime<Utc>, to: DateTime<Utc>) -> usize {
    (MOST_SHOWN * ((to - from).num_days().max(0) as usize / 7 + 1)).min(MOST_EVENTS)
}

fn expand(raws: &[Raw], from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Event> {
    let mut moved: HashMap<(String, DateTime<Utc>), &Raw> = HashMap::new();
    for raw in raws.iter().filter(|r| r.instead_of.is_some()) {
        moved.insert((raw.uid.clone(), raw.instead_of.unwrap_or(from)), raw);
    }
    let moved_days: HashSet<(String, NaiveDate)> = raws
        .iter()
        .filter_map(|r| r.instead_of_day.map(|d| (r.uid.clone(), d)))
        .collect();
    let mut seen_days: HashSet<(String, NaiveDate)> = HashSet::new();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut emit = |raw: &Raw, start: DateTime<Utc>, length: Duration, out: &mut Vec<Event>| {
        let Some(end) = start.checked_add_signed(length) else {
            return;
        };
        if raw.cancelled || end < from || start > to || !seen.insert((raw.uid.clone(), start)) {
            return;
        }
        out.push(event_of(raw, start, end));
    };
    for raw in raws {
        if let Some(When::Day(first)) = raw.start {
            if raw.cancelled {
                continue;
            }
            let days = match (raw.end, raw.duration) {
                (Some(When::Day(last)), _) if last > first => (last - first).num_days(),
                (_, Some(d)) if d.num_days() > 0 => d.num_days(),
                _ => 1,
            }
            .min(366);
            let dates: Vec<NaiveDate> = if raw.instead_of_day.is_some() {
                vec![first]
            } else {
                match raw.rule.as_deref().and_then(rule_of) {
                    Some(rule) => match first.and_hms_opt(12, 0, 0) {
                        Some(noon) => occurrences(
                            Utc.from_utc_datetime(&noon),
                            Some(chrono_tz::UTC),
                            &rule,
                            from - Duration::days(days),
                            to + Duration::days(1),
                        )
                        .into_iter()
                        .map(|t| t.date_naive())
                        .collect(),
                        None => vec![first],
                    },
                    None => vec![first],
                }
            };
            let except_days: HashSet<&NaiveDate> = raw.except_days.iter().collect();
            for day in dates {
                if raw.instead_of_day.is_none()
                    && (except_days.contains(&day) || moved_days.contains(&(raw.uid.clone(), day)))
                {
                    continue;
                }
                let Some(start) = day.and_hms_opt(0, 0, 0).and_then(|t| local_to_utc(t, None))
                else {
                    continue;
                };
                let end = start + Duration::days(days);
                if end < from || start > to || !seen_days.insert((raw.uid.clone(), day)) {
                    continue;
                }
                let mut event = event_of(raw, start, end);
                event.id = format!("{}@{}", raw.uid, day.format("%Y%m%d"));
                event.all_day = true;
                event.date = Some(day);
                event.end_date = Some(day + Duration::days(days));
                out.push(event);
            }
            continue;
        }
        let Some(When::At(start, zone)) = raw.start else {
            continue;
        };
        let length = match (raw.end, raw.duration) {
            (Some(When::At(end, _)), _) if end > start => end - start,
            (_, Some(d)) if d > Duration::zero() => d,
            _ => Duration::hours(1),
        };
        if raw.instead_of.is_some() {
            emit(raw, start, length, &mut out);
            continue;
        }
        let starts = match raw.rule.as_deref() {
            None => vec![start],
            Some(text) => match rule_of(text) {
                Some(rule) => occurrences(
                    start,
                    zone,
                    &rule,
                    from.checked_sub_signed(length).unwrap_or(from),
                    to,
                ),
                None => vec![start],
            },
        };
        let except: HashSet<&DateTime<Utc>> = raw.except.iter().collect();
        for at in starts {
            if except.contains(&at) || moved.contains_key(&(raw.uid.clone(), at)) {
                continue;
            }
            emit(raw, at, length, &mut out);
        }
    }
    out.sort_by_key(|e| e.start);
    out.truncate(most_shown(from, to));
    out
}

fn event_of(raw: &Raw, start: DateTime<Utc>, end: DateTime<Utc>) -> Event {
    let title = if raw.summary.trim().is_empty() {
        "Untitled event".to_string()
    } else {
        raw.summary.trim().chars().take(200).collect()
    };
    let mut context = format!("Calendar event: {title}");
    if !raw.location.trim().is_empty() {
        context.push_str(&format!("\nPlace: {}", raw.location.trim()));
    }
    if !raw.organizer.is_empty() {
        context.push_str(&format!("\nOrganised by: {}", raw.organizer));
    }
    if !raw.attendees.is_empty() {
        context.push_str(&format!("\nInvited: {}", raw.attendees.join(", ")));
    }
    if !raw.description.trim().is_empty() {
        context.push_str(&format!("\nDescription:\n{}", raw.description.trim()));
    }
    Event {
        id: format!("{}@{}", raw.uid, start.timestamp()),
        title,
        start,
        end,
        location: raw.location.trim().chars().take(200).collect(),
        context: context.chars().take(CONTEXT_CHARS).collect(),
        all_day: false,
        date: None,
        end_date: None,
        zone_unknown: raw.zone_unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    const LECTURES: &str = "BEGIN:VCALENDAR\r
X-WR-CALNAME:CS 130\r
BEGIN:VTIMEZONE\r
TZID:America/Los_Angeles\r
BEGIN:STANDARD\r
DTSTART:19701101T020000\r
END:STANDARD\r
END:VTIMEZONE\r
BEGIN:VEVENT\r
UID:lecture-1\r
SUMMARY:Algorithms lecture\r
DTSTART;TZID=America/Los_Angeles:20261020T090000\r
DTEND;TZID=America/Los_Angeles:20261020T105000\r
RRULE:FREQ=WEEKLY;BYDAY=TU,TH;UNTIL=20261231T000000Z\r
EXDATE;TZID=America/Los_Angeles:20261029T090000\r
LOCATION:Boelter 3400\r
DESCRIPTION:Week 4: shortest paths\\, Dijkstra and\\nBellman-Ford.\r
ATTENDEE;CN=Ada Lovelace;PARTSTAT=ACCEPTED:mailto:ada@example.com\r
ATTENDEE;CN=Bob;PARTSTAT=DECLINED:mailto:bob@example.com\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:lecture-1\r
RECURRENCE-ID;TZID=America/Los_Angeles:20261027T090000\r
SUMMARY:Algorithms lecture (moved)\r
DTSTART;TZID=America/Los_Angeles:20261027T130000\r
DTEND;TZID=America/Los_Angeles:20261027T145000\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:holiday\r
SUMMARY:No class\r
DTSTART;VALUE=DATE:20261111\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:gone\r
SUMMARY:Cancelled review\r
STATUS:CANCELLED\r
DTSTART:20261022T180000Z\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:office\r
SUMMARY:Office hours with a very long title that goes on\r
  and on\r
DTSTART:20261021T200000Z\r
DURATION:PT30M\r
END:VEVENT\r
END:VCALENDAR\r
";

    #[test]
    fn a_calendar_link_becomes_upcoming_events_in_real_time() {
        let cal = read(
            LECTURES,
            at("2026-10-20T00:00:00Z"),
            at("2026-11-06T00:00:00Z"),
        );
        assert_eq!(cal.name.as_deref(), Some("CS 130"));
        let shown: Vec<(String, String)> = cal
            .events
            .iter()
            .map(|e| (e.start.to_rfc3339(), e.title.clone()))
            .collect();
        assert_eq!(
            shown,
            [
                ("2026-10-20T16:00:00+00:00", "Algorithms lecture"),
                ("2026-10-21T20:00:00+00:00", "Office hours with a very long title that goes on and on"),
                ("2026-10-22T16:00:00+00:00", "Algorithms lecture"),
                ("2026-10-27T20:00:00+00:00", "Algorithms lecture (moved)"),
                ("2026-11-03T17:00:00+00:00", "Algorithms lecture"),
                ("2026-11-05T17:00:00+00:00", "Algorithms lecture"),
            ]
            .map(|(a, b)| (a.to_string(), b.to_string())),
            "the 29th is excepted, the 27th moved, all-day and cancelled events skipped, and 9:00 stays 9:00 after daylight saving ends"
        );
        let first = &cal.events[0];
        assert_eq!(first.end - first.start, Duration::minutes(110));
        assert_eq!(first.location, "Boelter 3400");
        assert!(first
            .context
            .contains("Week 4: shortest paths, Dijkstra and\nBellman-Ford."));
        assert!(first.context.contains("Invited: Ada Lovelace"));
        assert!(
            !first.context.contains("Bob"),
            "declined guests are left out"
        );
        assert_eq!(
            cal.events[1].end - cal.events[1].start,
            Duration::minutes(30)
        );
    }

    #[test]
    fn all_day_items_keep_their_dates_with_repeats_exclusions_and_moves() {
        let text = "BEGIN:VEVENT\nUID:final\nSUMMARY:Biology final\nDTSTART;VALUE=DATE:20261216\nDTEND;VALUE=DATE:20261217\nEND:VEVENT\nBEGIN:VEVENT\nUID:trip\nSUMMARY:Field trip\nDTSTART;VALUE=DATE:20261203\nDTEND;VALUE=DATE:20261205\nEND:VEVENT\nBEGIN:VEVENT\nUID:hw\nSUMMARY:Homework due\nDTSTART;VALUE=DATE:20261202\nRRULE:FREQ=WEEKLY;COUNT=3\nEXDATE;VALUE=DATE:20261209\nEND:VEVENT\nBEGIN:VEVENT\nUID:hw\nRECURRENCE-ID;VALUE=DATE:20261216\nSUMMARY:Homework due (moved)\nDTSTART;VALUE=DATE:20261217\nEND:VEVENT\nBEGIN:VEVENT\nUID:zone\nSUMMARY:Somewhere\nDTSTART;TZID=Not/AZone:20261210T090000\nEND:VEVENT\n";
        let cal = read(text, at("2026-12-01T00:00:00Z"), at("2026-12-31T00:00:00Z"));
        let days: Vec<(String, String, String)> = cal
            .events
            .iter()
            .filter(|e| e.all_day)
            .map(|e| {
                (
                    e.title.clone(),
                    e.date.unwrap().to_string(),
                    e.end_date.unwrap().to_string(),
                )
            })
            .collect();
        let want = [
            ("Homework due", "2026-12-02", "2026-12-03"),
            ("Field trip", "2026-12-03", "2026-12-05"),
            ("Biology final", "2026-12-16", "2026-12-17"),
            ("Homework due (moved)", "2026-12-17", "2026-12-18"),
        ];
        let mut got = days.clone();
        got.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            got,
            want.map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string())),
            "the 9th is excepted and the 16th moved to the 17th"
        );
        let zone = cal.events.iter().find(|e| e.title == "Somewhere").unwrap();
        assert!(zone.zone_unknown && !zone.all_day);
        assert!(cal
            .events
            .iter()
            .filter(|e| !e.all_day)
            .all(|e| e.date.is_none()));
    }

    #[test]
    fn repeats_stop_at_their_count_and_unknown_rules_show_the_first_time_only() {
        let text = "BEGIN:VEVENT\nUID:a\nSUMMARY:Standup\nDTSTART:20261019T160000Z\nRRULE:FREQ=DAILY;COUNT=3\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nSUMMARY:Board\nDTSTART:20261019T170000Z\nRRULE:FREQ=MONTHLY;BYDAY=2TU\nEND:VEVENT\nBEGIN:VEVENT\nUID:c\nSUMMARY:Review\nDTSTART:20261001T170000Z\nRRULE:FREQ=MONTHLY;INTERVAL=1\nEND:VEVENT\n";
        let cal = read(text, at("2026-10-01T00:00:00Z"), at("2026-12-31T00:00:00Z"));
        let count = |title: &str| cal.events.iter().filter(|e| e.title == title).count();
        assert_eq!(count("Standup"), 3);
        assert_eq!(count("Board"), 1);
        assert_eq!(count("Review"), 3);
    }

    #[test]
    fn broken_or_hostile_input_is_bounded_and_never_panics() {
        for text in [
            "",
            "BEGIN:VEVENT\nDTSTART:nonsense\nEND:VEVENT",
            "BEGIN:VEVENT\nDTSTART:20261019T160000Z\nRRULE:FREQ=DAILY;INTERVAL=0\nEND:VEVENT",
            "BEGIN:VEVENT\nDTSTART:20261019T160000Z\nDURATION:P-1D\nEND:VEVENT",
            "BEGIN:VEVENT\nDTSTART;TZID=Not/AZone:20261019T090000\nSUMMARY:\\\nEND:VEVENT",
        ] {
            let _ = read(text, at("2026-10-01T00:00:00Z"), at("2026-12-31T00:00:00Z"));
        }
        let many: String = (0..30_000)
            .map(|i| format!("BEGIN:VEVENT\nUID:{i}\nDTSTART:20261019T160000Z\nEND:VEVENT\n"))
            .collect();
        let cal = read(
            &many,
            at("2026-10-01T00:00:00Z"),
            at("2026-12-31T00:00:00Z"),
        );
        assert_eq!(
            cal.events.len(),
            most_shown(at("2026-10-01T00:00:00Z"), at("2026-12-31T00:00:00Z"))
        );
        let daily = "BEGIN:VEVENT\nUID:d\nDTSTART:19000101T160000Z\nRRULE:FREQ=DAILY\nEND:VEVENT";
        assert_eq!(
            read(
                daily,
                at("2026-10-01T00:00:00Z"),
                at("2026-10-08T00:00:00Z")
            )
            .events
            .len(),
            7
        );
    }

    #[test]
    fn a_busy_month_keeps_every_event_in_view() {
        let text: String = (0..245)
            .map(|i| {
                format!(
                    "BEGIN:VEVENT\nUID:e{i}\nSUMMARY:Meeting {i}\nDTSTART:2026110{}T{:02}0000Z\nEND:VEVENT\n",
                    1 + i / 49 % 9,
                    8 + i % 49 / 5
                )
            })
            .collect();
        let cal = read(
            &text,
            at("2026-11-01T00:00:00Z"),
            at("2026-12-06T00:00:00Z"),
        );
        assert_eq!(cal.events.len(), 245);
    }

    #[test]
    fn every_weekday_repeats_skip_the_weekend_and_monthly_weekday_rules_show_once() {
        let text = "BEGIN:VEVENT\nUID:a\nSUMMARY:Standup\nDTSTART:20261019T160000Z\nRRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nSUMMARY:Tuesdays\nDTSTART:20261020T170000Z\nRRULE:FREQ=MONTHLY;BYDAY=TU\nEND:VEVENT\n";
        let cal = read(text, at("2026-10-19T00:00:00Z"), at("2026-11-30T00:00:00Z"));
        let standups: Vec<_> = cal.events.iter().filter(|e| e.title == "Standup").collect();
        assert_eq!(standups.len(), 30);
        assert!(standups
            .iter()
            .all(|e| !matches!(e.start.weekday(), Weekday::Sat | Weekday::Sun)));
        assert_eq!(
            cal.events.iter().filter(|e| e.title == "Tuesdays").count(),
            1
        );
    }

    #[test]
    fn a_series_that_began_long_ago_still_shows_but_a_counted_one_ends() {
        let long = "BEGIN:VEVENT\nUID:a\nSUMMARY:Stand-up\nDTSTART:20120102T160000Z\nRRULE:FREQ=DAILY\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nSUMMARY:Weekly\nDTSTART:20120102T170000Z\nRRULE:FREQ=WEEKLY;INTERVAL=2\nEND:VEVENT\nBEGIN:VEVENT\nUID:c\nSUMMARY:Counted\nDTSTART:20120102T180000Z\nRRULE:FREQ=DAILY;COUNT=5\nEND:VEVENT\n";
        let cal = read(long, at("2026-11-02T00:00:00Z"), at("2026-11-09T00:00:00Z"));
        let count = |title: &str| cal.events.iter().filter(|e| e.title == title).count();
        assert_eq!(count("Stand-up"), 7);
        assert_eq!(count("Weekly"), 1);
        assert_eq!(count("Counted"), 0);
    }

    #[test]
    fn a_huge_interval_ends_the_series_instead_of_overflowing() {
        for freq in ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"] {
            let text = format!("BEGIN:VEVENT\nUID:a\nSUMMARY:Odd\nDTSTART:20261019T160000Z\nRRULE:FREQ={freq};INTERVAL=4294967295\nEND:VEVENT\n");
            let cal = read(
                &text,
                at("2026-10-19T00:00:00Z"),
                at("2030-01-01T00:00:00Z"),
            );
            assert!(cal.events.len() <= 1, "{freq}");
        }
    }

    #[test]
    fn a_feed_full_of_exclusion_dates_is_expanded_quickly() {
        let first = NaiveDate::from_ymd_opt(1100, 1, 1).unwrap();
        let mut text =
            String::from("BEGIN:VEVENT\nUID:d\nDTSTART:20260101T160000Z\nRRULE:FREQ=DAILY\n");
        for line in 0..60 {
            let days: Vec<String> = (0..5_000)
                .map(|n| {
                    (first + Duration::days(line * 5_000 + n))
                        .format("%Y%m%dT160000Z")
                        .to_string()
                })
                .collect();
            text.push_str(&format!("EXDATE:{}\n", days.join(",")));
        }
        text.push_str("END:VEVENT\n");
        let began = std::time::Instant::now();
        let cal = read(
            &text,
            at("2026-01-01T00:00:00Z"),
            at("2040-01-01T00:00:00Z"),
        );
        assert!(began.elapsed() < std::time::Duration::from_secs(5));
        assert!(!cal.events.is_empty());
    }

    fn reads_without_panic(duration: &str) -> Result<usize, String> {
        let feed = format!("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20260101T100000Z\nDURATION:{duration}\nEND:VEVENT\nEND:VCALENDAR");
        let from = at("2025-12-01T00:00:00Z");
        let to = at("2026-02-01T00:00:00Z");
        std::panic::catch_unwind(|| read(&feed, from, to).events.len()).map_err(|e| {
            e.downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        })
    }

    #[test]
    fn a_huge_week_count_does_not_panic() {
        let read = reads_without_panic("P99999999999999999W");
        assert!(read.is_ok(), "the reader panicked: {read:?}");
    }

    #[test]
    fn a_huge_day_count_on_a_timed_event_does_not_panic() {
        let read = reads_without_panic("P300000000D");
        assert!(read.is_ok(), "the reader panicked: {read:?}");
    }

    #[test]
    fn a_huge_duration_gives_the_event_its_default_length() {
        assert_eq!(reads_without_panic("P300000000D"), Ok(1));
        assert_eq!(duration("P2D"), Some(Duration::days(2)));
    }
}
