use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::chat::Spent;

const KEPT_DAYS: u64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Answer,
    ToolContinuation,
    Retry,
    StudyGrading,
    Summary,
    OtherWriting,
    QuickReview,
    MarginRead,
    Connections,
    Combine,
    Margin,
    Recording,
    Import,
    Pictures,
    Dictation,
}

impl Purpose {
    pub const ALL: [Purpose; 15] = [
        Purpose::Answer,
        Purpose::ToolContinuation,
        Purpose::Retry,
        Purpose::StudyGrading,
        Purpose::Summary,
        Purpose::OtherWriting,
        Purpose::QuickReview,
        Purpose::MarginRead,
        Purpose::Connections,
        Purpose::Combine,
        Purpose::Margin,
        Purpose::Recording,
        Purpose::Import,
        Purpose::Pictures,
        Purpose::Dictation,
    ];
}

#[derive(Default)]
struct Counts {
    calls: u64,
    failures: u64,
    input: u64,
    cached: u64,
    output: u64,
    unreported: u64,
    estimated: u64,
    latency: Vec<u64>,
}

#[derive(Default)]
pub struct Usage {
    #[cfg(test)]
    rows: Mutex<BTreeMap<Purpose, Counts>>,
    #[cfg(test)]
    tools: Mutex<Tools>,
    saved_in: OnceLock<PathBuf>,
}

#[derive(Clone, Default, Serialize)]
pub struct Tools {
    calls: u64,
    failures: u64,
    reused: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    pub day: String,
    pub name: String,
    pub calls: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Days {
    pub models: Vec<Day>,
    pub kinds: Vec<Day>,
}

#[derive(Default, Serialize)]
pub struct Span {
    pub kinds: Vec<Row>,
    pub tools: Tools,
}

#[derive(Default, Serialize)]
pub struct Recent {
    pub hours: Days,
    pub spans: BTreeMap<u32, Span>,
}

struct Call {
    at: i64,
    purpose: Purpose,
    model: String,
    failed: bool,
    unreported: bool,
    estimated: bool,
    ms: u64,
    input: u64,
    cached: u64,
    output: u64,
}

#[derive(Serialize)]
pub struct Row {
    purpose: Purpose,
    calls: u64,
    failures: u64,
    input: u64,
    cached: u64,
    output: u64,
    unreported: u64,
    estimated: u64,
    median_ms: u64,
    p95_ms: u64,
}

fn day_of(ago: u64) -> Option<String> {
    let today = chrono::Local::now().date_naive();
    let day = today.checked_sub_days(chrono::Days::new(ago))?;
    Some(day.format("%Y-%m-%d").to_string())
}

fn midnight_of(ago: u64) -> Option<i64> {
    let today = chrono::Local::now().date_naive();
    let day = today.checked_sub_days(chrono::Days::new(ago))?;
    Some(
        day.and_hms_opt(0, 0, 0)?
            .and_local_timezone(chrono::Local)
            .earliest()?
            .timestamp(),
    )
}

fn first_hour() -> i64 {
    let now = chrono::Local::now().timestamp();
    now - now.rem_euclid(3600) - 23 * 3600
}

fn hour_of(at: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(at, 0)
        .earliest()
        .map(|t| t.format("%Y-%m-%d %H").to_string())
        .unwrap_or_default()
}

fn timing(mut times: Vec<u64>) -> (u64, u64) {
    times.sort_unstable();
    let median = times.get(times.len().saturating_sub(1) / 2);
    let p95 = times.get((times.len() * 95).div_ceil(100).saturating_sub(1));
    (median.copied().unwrap_or(0), p95.copied().unwrap_or(0))
}

fn purpose_name(purpose: Purpose) -> String {
    serde_json::to_value(purpose)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

impl Call {
    fn of(purpose: Purpose, spent: Option<&Spent>, ms: u64, failed: bool) -> Call {
        Call {
            at: chrono::Local::now().timestamp(),
            purpose,
            model: spent.and_then(|s| s.model.clone()).unwrap_or_default(),
            failed,
            unreported: spent.is_none(),
            estimated: spent.is_some_and(|s| s.estimated),
            ms,
            input: spent.map_or(0, |s| s.input),
            cached: spent.map_or(0, |s| s.cached.min(s.input)),
            output: spent.map_or(0, |s| s.output),
        }
    }
}

impl Counts {
    fn add(&mut self, call: &Call) {
        self.calls = self.calls.saturating_add(1);
        self.failures = self.failures.saturating_add(u64::from(call.failed));
        self.unreported = self.unreported.saturating_add(u64::from(call.unreported));
        self.estimated = self.estimated.saturating_add(u64::from(call.estimated));
        self.input = self.input.saturating_add(call.input);
        self.cached = self.cached.saturating_add(call.cached);
        self.output = self.output.saturating_add(call.output);
        self.latency.push(call.ms);
    }
}

fn rows_of(held: &BTreeMap<Purpose, Counts>) -> Vec<Row> {
    held.iter()
        .map(|(purpose, row)| {
            let (median_ms, p95_ms) = timing(row.latency.clone());
            Row {
                purpose: *purpose,
                calls: row.calls,
                failures: row.failures,
                input: row.input,
                cached: row.cached,
                output: row.output,
                unreported: row.unreported,
                estimated: row.estimated,
                median_ms,
                p95_ms,
            }
        })
        .collect()
}

fn by_hour(calls: &[Call], name: impl Fn(&Call) -> (String, Option<String>)) -> Vec<Day> {
    let mut sums: BTreeMap<(String, String, Option<String>), Day> = BTreeMap::new();
    for call in calls {
        let (day, (name, model)) = (hour_of(call.at), name(call));
        let held = sums
            .entry((day.clone(), name.clone(), model.clone()))
            .or_insert(Day {
                day,
                name,
                calls: 0,
                input: 0,
                cached: 0,
                output: 0,
                model,
            });
        held.calls += 1;
        held.input = held.input.saturating_add(call.input);
        held.cached = held.cached.saturating_add(call.cached);
        held.output = held.output.saturating_add(call.output);
    }
    sums.into_values().collect()
}

impl Usage {
    pub fn save_in(&self, chats: PathBuf) {
        let _ = self.saved_in.set(chats);
    }

    fn save(&self, call: &Call) {
        let Some(chats) = self.saved_in.get() else {
            return;
        };
        let (Some(today), Some(oldest)) = (day_of(0), day_of(KEPT_DAYS)) else {
            return;
        };
        let purpose = purpose_name(call.purpose);
        let [input, cached, output, ms] =
            [call.input, call.cached, call.output, call.ms].map(|n| n.min(i64::MAX as u64) as i64);
        let saved = crate::db::for_chats(chats).with(|c| {
            c.execute(
                "INSERT INTO usage_days (day, purpose, model, calls, input, cached, output)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)
                 ON CONFLICT (day, purpose, model) DO UPDATE SET
                   calls = calls + 1,
                   input = input + excluded.input,
                   cached = cached + excluded.cached,
                   output = output + excluded.output",
                rusqlite::params![today, purpose, call.model, input, cached, output],
            )?;
            c.execute("DELETE FROM usage_days WHERE day < ?1", [oldest])?;
            c.execute(
                "INSERT INTO usage_calls (at, purpose, model, failed, unreported, estimated, ms, input, cached, output)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    call.at,
                    purpose,
                    call.model,
                    call.failed,
                    call.unreported,
                    call.estimated,
                    ms,
                    input,
                    cached,
                    output
                ],
            )?;
            c.execute(
                "DELETE FROM usage_calls WHERE at < ?1",
                [call.at - KEPT_DAYS as i64 * 86_400],
            )
        });
        if let Err(e) = saved {
            leo_core::diag::warn(format!("could not save usage: {e}"));
        }
    }

    pub fn days(&self) -> Days {
        let (Some(chats), Some(first)) = (self.saved_in.get(), day_of(KEPT_DAYS - 1)) else {
            return Days::default();
        };
        let db = crate::db::for_chats(chats);
        let by = |column: &'static str, model: bool| -> Vec<Day> {
            let with = if model { ", model" } else { "" };
            db.with(|c| {
                c.prepare(&format!(
                    "SELECT day, {column}, SUM(calls), SUM(input), SUM(cached), SUM(output){with}
                     FROM usage_days WHERE day >= ?1 GROUP BY day, {column}{with} ORDER BY day, {column}{with}"
                ))?
                .query_map([&first], |r| {
                    Ok(Day {
                        day: r.get(0)?,
                        name: r.get(1)?,
                        calls: r.get::<_, i64>(2)?.max(0) as u64,
                        input: r.get::<_, i64>(3)?.max(0) as u64,
                        cached: r.get::<_, i64>(4)?.max(0) as u64,
                        output: r.get::<_, i64>(5)?.max(0) as u64,
                        model: if model { Some(r.get(6)?) } else { None },
                    })
                })?
                .collect()
            })
            .unwrap_or_default()
        };
        Days {
            models: by("model", false),
            kinds: by("purpose", true),
        }
    }

    pub fn recent(&self) -> Recent {
        let (Some(chats), Some(oldest)) = (self.saved_in.get(), midnight_of(KEPT_DAYS - 1)) else {
            return Recent::default();
        };
        let db = crate::db::for_chats(chats);
        let calls: Vec<Call> = db
            .with(|c| {
                c.prepare(
                    "SELECT at, purpose, model, failed, unreported, estimated, ms, input, cached, output
                     FROM usage_calls WHERE at >= ?1 ORDER BY at",
                )?
                .query_map([oldest], |r| {
                    let purpose: String = r.get(1)?;
                    let count = |i| r.get::<_, i64>(i).map(|n| n.max(0) as u64);
                    Ok(serde_json::from_value(serde_json::Value::String(purpose))
                        .ok()
                        .map(|purpose| Call {
                            at: r.get(0).unwrap_or(0),
                            purpose,
                            model: r.get(2).unwrap_or_default(),
                            failed: r.get(3).unwrap_or(false),
                            unreported: r.get(4).unwrap_or(false),
                            estimated: r.get(5).unwrap_or(false),
                            ms: count(6).unwrap_or(0),
                            input: count(7).unwrap_or(0),
                            cached: count(8).unwrap_or(0),
                            output: count(9).unwrap_or(0),
                        }))
                })?
                .filter_map(|row| row.transpose())
                .collect()
            })
            .unwrap_or_default();
        let tools: Vec<(i64, bool, bool)> = db
            .with(|c| {
                c.prepare("SELECT at, failed, reused FROM usage_tools WHERE at >= ?1")?
                    .query_map([oldest], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect()
            })
            .unwrap_or_default();
        let hour = first_hour();
        let mut spans = BTreeMap::new();
        for (span, from) in [
            (1, Some(hour)),
            (7, midnight_of(6)),
            (30, midnight_of(29)),
            (90, Some(oldest)),
        ] {
            let Some(from) = from else { continue };
            let mut used = Tools::default();
            for &(_, failed, reused) in tools.iter().filter(|t| t.0 >= from) {
                used.calls += 1;
                used.failures += u64::from(failed);
                used.reused += u64::from(reused);
            }
            let mut kinds = BTreeMap::<Purpose, Counts>::new();
            for call in calls.iter().filter(|c| c.at >= from) {
                kinds.entry(call.purpose).or_default().add(call);
            }
            spans.insert(
                span,
                Span {
                    kinds: rows_of(&kinds),
                    tools: used,
                },
            );
        }
        let latest: Vec<Call> = calls.into_iter().filter(|c| c.at >= hour).collect();
        Recent {
            hours: Days {
                models: by_hour(&latest, |c| (c.model.clone(), None)),
                kinds: by_hour(&latest, |c| {
                    (purpose_name(c.purpose), Some(c.model.clone()))
                }),
            },
            spans,
        }
    }

    pub(crate) fn tool(&self, failed: bool, reused: bool) {
        #[cfg(test)]
        {
            let mut held = self.tools.lock().unwrap_or_else(|e| e.into_inner());
            held.calls = held.calls.saturating_add(1);
            held.failures = held.failures.saturating_add(u64::from(failed));
            held.reused = held.reused.saturating_add(u64::from(reused));
        }
        let Some(chats) = self.saved_in.get() else {
            return;
        };
        let now = chrono::Local::now().timestamp();
        let saved = crate::db::for_chats(chats).with(|c| {
            c.execute(
                "INSERT INTO usage_tools (at, failed, reused) VALUES (?1, ?2, ?3)",
                rusqlite::params![now, failed, reused],
            )?;
            c.execute(
                "DELETE FROM usage_tools WHERE at < ?1",
                [now - KEPT_DAYS as i64 * 86_400],
            )
        });
        if let Err(e) = saved {
            leo_core::diag::warn(format!("could not save tool usage: {e}"));
        }
    }

    #[cfg(test)]
    pub fn tools(&self) -> Tools {
        self.tools.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn record(&self, purpose: Purpose, spent: Option<&Spent>, elapsed: Duration, failed: bool) {
        let ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        let call = Call::of(purpose, spent, ms, failed);
        self.save(&call);
        #[cfg(test)]
        self.rows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(purpose)
            .or_default()
            .add(&call);
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> Vec<Row> {
        rows_of(&self.rows.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_separates_work_and_counts_failures() {
        let usage = Usage::default();
        for i in 0..100 {
            usage.record(
                Purpose::Answer,
                Some(&Spent {
                    input: 100,
                    cached: 80,
                    output: 10,
                    ..Default::default()
                }),
                Duration::from_millis(i),
                false,
            );
        }
        usage.record(Purpose::Retry, None, Duration::from_secs(1), true);
        let rows = usage.snapshot();
        assert_eq!(
            (rows[0].calls, rows[0].input, rows[0].cached, rows[0].output),
            (100, 10_000, 8_000, 1_000)
        );
        assert_eq!((rows[0].median_ms, rows[0].p95_ms), (49, 94));
        assert_eq!(
            (rows[1].calls, rows[1].failures, rows[1].unreported),
            (1, 1, 1)
        );
    }

    fn spent(model: &str, input: u64, cached: u64, output: u64) -> Spent {
        Spent {
            model: Some(model.into()),
            input,
            cached,
            output,
            ..Default::default()
        }
    }

    #[test]
    fn saved_usage_survives_a_restart_and_adds_up_by_model() {
        let home = tempfile::tempdir().unwrap();
        let chats = home.path().join("chats");
        let first = Usage::default();
        first.save_in(chats.clone());
        let elapsed = Duration::from_millis(1);
        first.record(
            Purpose::Answer,
            Some(&spent("big", 1_000, 900, 50)),
            elapsed,
            false,
        );
        first.record(
            Purpose::Answer,
            Some(&spent("big", 1_000, 2_000, 50)),
            elapsed,
            true,
        );
        first.record(
            Purpose::Summary,
            Some(&spent("small", 100, 0, 5)),
            elapsed,
            false,
        );
        first.record(Purpose::QuickReview, None, elapsed, false);

        let restarted = Usage::default();
        restarted.save_in(chats.clone());
        let days = restarted.days();
        let mut by_model: std::collections::BTreeMap<&str, (u64, u64, u64, u64)> =
            std::collections::BTreeMap::new();
        for d in &days.models {
            let sum = by_model.entry(d.name.as_str()).or_default();
            *sum = (
                sum.0 + d.calls,
                sum.1 + d.input,
                sum.2 + d.cached,
                sum.3 + d.output,
            );
        }
        let seen: Vec<(&str, u64, u64, u64, u64)> = by_model
            .into_iter()
            .map(|(name, (calls, input, cached, output))| (name, calls, input, cached, output))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("", 1, 0, 0, 0),
                ("big", 2, 2_000, 1_900, 100),
                ("small", 1, 100, 0, 5),
            ]
        );
        let mut by_kind: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
        for d in &days.kinds {
            *by_kind.entry(d.name.as_str()).or_default() += d.calls;
        }
        let kinds: Vec<(&str, u64)> = by_kind.into_iter().collect();
        assert_eq!(
            kinds,
            vec![("answer", 2), ("quick_review", 1), ("summary", 1)]
        );
        let models: Vec<(&str, Option<&str>)> = days
            .kinds
            .iter()
            .map(|d| (d.name.as_str(), d.model.as_deref()))
            .collect();
        assert!(models.contains(&("answer", Some("big"))));
        assert!(models.contains(&("summary", Some("small"))));
        assert!(days.models.iter().all(|d| d.model.is_none()));
    }

    #[test]
    fn every_kind_of_work_has_a_label_on_the_usage_page() {
        use Purpose::*;
        let every = Purpose::ALL;
        assert_eq!(
            every
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            every.len()
        );
        for purpose in every {
            match purpose {
                Answer | ToolContinuation | Retry | StudyGrading | Summary | OtherWriting
                | QuickReview | MarginRead | Connections | Combine | Margin | Recording
                | Import | Pictures | Dictation => {}
            }
        }
        let page = include_str!("web/chat.js");
        let start = page
            .find("const USAGE_KINDS = {")
            .expect("chat.js lists the kinds");
        let kinds = &page[start..start + page[start..].find("};").unwrap()];
        for purpose in every {
            let name = purpose_name(purpose);
            assert!(
                kinds.contains(&format!(" {name}: '")),
                "{name} has no label in USAGE_KINDS"
            );
        }
    }

    #[test]
    fn saved_usage_leaves_old_days_out_and_forgets_past_its_limit() {
        let home = tempfile::tempdir().unwrap();
        let chats = home.path().join("chats");
        let usage = Usage::default();
        usage.save_in(chats.clone());
        let db = crate::db::for_chats(&chats);
        let row = "INSERT INTO usage_days VALUES (?1, 'answer', 'old', 1, 10, 0, 1)";
        db.with(|c| c.execute(row, [day_of(KEPT_DAYS).unwrap()]))
            .unwrap();
        db.with(|c| c.execute(row, [day_of(KEPT_DAYS + 1).unwrap()]))
            .unwrap();
        assert!(usage.days().models.is_empty());
        usage.record(
            Purpose::Answer,
            Some(&spent("new", 1, 0, 1)),
            Duration::ZERO,
            false,
        );
        let left: i64 = db
            .with(|c| c.query_row("SELECT COUNT(*) FROM usage_days", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(left, 2);
        assert_eq!(usage.days().models.len(), 1);
    }

    #[test]
    fn recent_detail_follows_each_period_and_hours_cover_the_last_day() {
        let home = tempfile::tempdir().unwrap();
        let chats = home.path().join("chats");
        let usage = Usage::default();
        usage.save_in(chats.clone());
        for ms in [100, 200, 300] {
            usage.record(
                Purpose::Answer,
                Some(&spent("big", 10, 5, 1)),
                Duration::from_millis(ms),
                false,
            );
        }
        usage.record(Purpose::MarginRead, None, Duration::from_millis(50), true);
        usage.tool(false, true);
        let db = crate::db::for_chats(&chats);
        let ten_days_ago = chrono::Local::now().timestamp() - 10 * 86_400;
        db.with(|c| {
            c.execute(
                "INSERT INTO usage_calls VALUES (?1, 'answer', 'big', 1, 0, 0, 900, 1, 0, 1)",
                [ten_days_ago],
            )?;
            c.execute("INSERT INTO usage_tools VALUES (?1, 1, 0)", [ten_days_ago])
        })
        .unwrap();

        let recent = usage.recent();
        let answer = |span: u32| {
            let row = &recent.spans[&span].kinds[0];
            (
                row.purpose,
                row.calls,
                row.failures,
                row.median_ms,
                row.p95_ms,
            )
        };
        assert_eq!(answer(1), (Purpose::Answer, 3, 0, 200, 300));
        assert_eq!(answer(7), (Purpose::Answer, 3, 0, 200, 300));
        assert_eq!(answer(30), (Purpose::Answer, 4, 1, 200, 900));
        let read = &recent.spans[&1].kinds[1];
        assert_eq!((read.calls, read.failures, read.unreported), (1, 1, 1));
        let tools = |span: u32| {
            let t = &recent.spans[&span].tools;
            (t.calls, t.failures, t.reused)
        };
        assert_eq!(tools(7), (1, 0, 1));
        assert_eq!(tools(90), (2, 1, 1));

        let calls: u64 = recent.hours.models.iter().map(|d| d.calls).sum();
        assert_eq!(calls, 4);
        let hour = &recent.hours.models[0].day;
        assert_eq!(hour.len(), "2026-11-09 14".len());
        let kinds: Vec<&str> = recent.hours.kinds.iter().map(|d| d.name.as_str()).collect();
        assert!(kinds.contains(&"answer") && kinds.contains(&"margin_read"));
    }

    #[test]
    fn usage_without_a_place_to_save_keeps_nothing() {
        let usage = Usage::default();
        usage.record(Purpose::Answer, None, Duration::ZERO, false);
        assert!(usage.days().models.is_empty());
    }
}
