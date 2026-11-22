mod apps;
mod parse;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub use apps::{
    app_kind, call_window, is_call_tree, kind_of, pick, Kind, Seat, Seen, BROWSERS, TEAMS_APPS,
    ZOOM_APPS,
};
pub use parse::read_windows;

pub const EVERY: Duration = Duration::from_millis(400);
pub const MOST_READ: Duration = Duration::from_secs(2);
pub const QUIET: Duration = Duration::from_secs(5);
pub const MOST_RESTARTS: u32 = 20;
pub const LAG_SECS: f64 = 0.75;
pub const HOLD_SECS: f64 = 2.0;
pub const MOST_TEXT: usize = 300;
pub const MOST_WINDOWS: usize = 4;
pub const FILE: &str = "call-names.jsonl";

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Node {
    pub role: String,
    pub subrole: Option<String>,
    pub description: Option<String>,
    pub title: Option<String>,
    pub value: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Window {
    pub title: String,
    pub id: String,
    pub tree: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Look {
    Call {
        app: String,
        windows: Vec<Window>,
        seat: Seat,
    },
    Unreadable(String),
}

pub struct Ask<'a> {
    pub until: Instant,
    pub stop: &'a AtomicBool,
    pub seat: Option<&'a Seat>,
}

impl Ask<'_> {
    pub fn over(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || Instant::now() >= self.until
    }
}

pub const SLOW: &str = "the call app answered too slowly";

pub trait Reader {
    fn read(&mut self, ask: &Ask) -> Look;
}

pub type Readers = std::sync::Arc<dyn Fn() -> Box<dyn Reader> + Send + Sync>;

#[derive(Clone)]
pub struct Names {
    pub readers: Readers,
    pub own: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    pub speaking: Vec<String>,
    pub participants: Vec<String>,
    pub me: Option<String>,
    pub shown: Option<usize>,
    pub unmarked_self: bool,
}

pub fn shows_call(app: &str, window: &Window) -> bool {
    let windows = std::slice::from_ref(window);
    let Some(kind) = kind_of(app, windows) else {
        return false;
    };
    if is_call_tree(&window.tree) {
        return true;
    }
    let reading = read_windows(kind, windows);
    match kind {
        Kind::Teams => false,
        Kind::Meet => !reading.speaking.is_empty(),
        Kind::Zoom => !reading.speaking.is_empty() || !reading.participants.is_empty(),
    }
}

pub fn read(look: &Look) -> Option<Reading> {
    let Look::Call { app, windows, .. } = look else {
        return None;
    };
    let kind = kind_of(app, windows)?;
    Some(read_windows(kind, windows))
}

pub fn keeps_value(role: &str) -> bool {
    parse::TEXTS.contains(&role)
}

pub fn clip(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(match text.char_indices().nth(MOST_TEXT) {
        Some((at, _)) => text[..at].to_owned(),
        None => text.to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Mark {
    Run { from: f64, to: f64, who: String },
    Me { me: String },
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    runs: Vec<(f64, f64, String)>,
    me: Vec<String>,
    own: Option<String>,
    open: Option<(f64, String)>,
    last: Option<f64>,
    longest: f64,
}

pub(crate) fn norm(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn usable(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && !matches!(norm(name).as_str(), "you" | "others" | "me"))
        .then(|| name.to_owned())
}

impl Timeline {
    pub fn owned_by(own: Option<&str>) -> Self {
        Self {
            own: own.map(norm).filter(|own| !own.is_empty()),
            ..Self::default()
        }
    }

    pub fn from_marks(marks: impl IntoIterator<Item = Mark>) -> Self {
        let mut timeline = Self::default();
        for mark in marks {
            match mark {
                Mark::Run { from, to, who } if from.is_finite() && to > from => {
                    match timeline.runs.last_mut() {
                        Some(last) if last.0 == from && last.2 == who => last.1 = last.1.max(to),
                        _ => timeline.runs.push((from, to, who)),
                    }
                }
                Mark::Me { me } => {
                    timeline.learn(&me);
                }
                Mark::Run { .. } => {}
            }
        }
        timeline.runs.sort_by(|a, b| a.0.total_cmp(&b.0));
        timeline.longest = timeline
            .runs
            .iter()
            .map(|(from, to, _)| to - from)
            .fold(0.0, f64::max);
        timeline
    }

    pub fn parse(text: &str) -> Self {
        Self::from_marks(text.lines().filter_map(|line| {
            serde_json::from_str(line).ok().or_else(|| {
                line.rfind('{')
                    .and_then(|at| serde_json::from_str(&line[at..]).ok())
            })
        }))
    }

    fn is_me(&self, name: &str) -> bool {
        let name = norm(name);
        self.me.contains(&name) || self.own.as_ref() == Some(&name)
    }

    fn learn(&mut self, me: &str) -> bool {
        let me = norm(me);
        if me.is_empty() || self.me.contains(&me) {
            return false;
        }
        self.me.push(me);
        true
    }

    fn who(&self, reading: &Reading) -> Option<String> {
        let mine = |name: &str| {
            self.is_me(name) || reading.me.as_deref().is_some_and(|m| norm(m) == norm(name))
        };
        let people = unique(&reading.participants);
        let roster: Vec<String> = people.iter().map(|p| norm(p)).collect();
        let outside = reading
            .speaking
            .iter()
            .any(|p| !mine(p) && !roster.contains(&norm(p)));
        if people.len() == 2 && reading.shown == Some(2) && !outside {
            let others: Vec<&String> = people.iter().filter(|p| !mine(p)).collect();
            if let [other] = others.as_slice() {
                return usable(other);
            }
        }
        if reading.unmarked_self && self.me.is_empty() && reading.me.is_none() {
            return None;
        }
        let talking: Vec<String> = unique(&reading.speaking)
            .into_iter()
            .filter(|p| !mine(p))
            .collect();
        match talking.as_slice() {
            [one] => usable(one),
            _ => None,
        }
    }

    pub fn heard(&mut self, at: f64, reading: Option<&Reading>) -> Vec<Mark> {
        let mut marks = Vec::new();
        if !at.is_finite() || self.last.is_some_and(|last| at < last) {
            return marks;
        }
        if let Some(me) = reading.and_then(|r| r.me.as_deref()) {
            if self.learn(me) {
                marks.push(Mark::Me {
                    me: me.trim().to_owned(),
                });
            }
        }
        let who = reading.and_then(|r| self.who(r));
        let goes_on = match (&self.open, &who, self.last) {
            (Some((_, open)), Some(who), Some(last)) => open == who && at - last <= HOLD_SECS,
            _ => false,
        };
        if goes_on {
            if let Some((from, who)) = &self.open {
                marks.push(Mark::Run {
                    from: *from,
                    to: at,
                    who: who.clone(),
                });
            }
        } else {
            marks.extend(self.close(at));
            self.open = who.map(|who| (at, who));
        }
        self.last = Some(at);
        marks
    }

    fn close(&mut self, next: f64) -> Option<Mark> {
        let (from, who) = self.open.take()?;
        let to = next.min(self.last.unwrap_or(from) + HOLD_SECS);
        if to <= from {
            return None;
        }
        self.runs.push((from, to, who.clone()));
        self.longest = self.longest.max(to - from);
        Some(Mark::Run { from, to, who })
    }

    pub fn end(&mut self) -> Option<Mark> {
        self.close(f64::INFINITY)
    }

    fn from_reaching(&self, start: f64) -> &[(f64, f64, String)] {
        let reach = start - self.longest;
        &self.runs[self.runs.partition_point(|(from, _, _)| *from < reach)..]
    }

    pub fn name(&self, start: f64, end: f64) -> Option<String> {
        if end <= start {
            return self
                .from_reaching(start)
                .iter()
                .take_while(|(from, _, _)| *from <= start)
                .find(|(_, to, who)| start < *to && !self.is_me(who))
                .map(|(_, _, who)| who.clone());
        }
        self.name_over(&[(start, end)])
    }

    pub fn name_over(&self, spans: &[(f64, f64)]) -> Option<String> {
        let spans = union(
            spans
                .iter()
                .copied()
                .filter(|(start, end)| end > start)
                .collect(),
        );
        let whole = measure(&spans);
        let mut time: Vec<(String, String, Vec<(f64, f64)>)> = Vec::new();
        for &(start, end) in &spans {
            for (from, to, who) in self
                .from_reaching(start)
                .iter()
                .take_while(|(from, _, _)| *from < end)
            {
                if *to <= start || self.is_me(who) {
                    continue;
                }
                let clipped = (from.max(start), to.min(end));
                let key = norm(who);
                match time.iter_mut().find(|(k, _, _)| *k == key) {
                    Some(entry) => entry.2.push(clipped),
                    None => time.push((key, who.clone(), vec![clipped])),
                }
            }
        }
        time.into_iter()
            .map(|(_, who, spoken)| (who, measure(&union(spoken))))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, spent)| *spent > 0.5 * whole)
            .map(|(who, _)| who)
    }
}

fn union(mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(spans.len());
    for (from, to) in spans {
        match out.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => out.push((from, to)),
        }
    }
    out
}

fn measure(spans: &[(f64, f64)]) -> f64 {
    spans.iter().map(|(from, to)| to - from).sum()
}

pub(crate) fn unique(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|seen| norm(seen) == norm(name)) {
            out.push(name.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests;
