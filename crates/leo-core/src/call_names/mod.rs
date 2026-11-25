mod apps;
mod parse;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub use apps::{
    app_kind, call_window, is_call_tree, kind_of, pick, same_call, Kind, Seat, Seen, BROWSERS,
    TEAMS_APPS, ZOOM_APPS,
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
pub const HIDDEN: &str =
    "the call window is covered, minimized or on another desktop, so what it shows is frozen";
pub const STALE_SECS: f64 = 3.0;
const SHORT_LINE_SECS: f64 = 1.2;
const GRID: usize = 24;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub pid: i32,
    pub solid: bool,
    pub area: Rect,
}

impl Rect {
    fn holds(&self, x: f64, y: f64) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }

    fn near(&self, other: &Rect) -> bool {
        [
            self.x - other.x,
            self.y - other.y,
            self.w - other.w,
            self.h - other.h,
        ]
        .iter()
        .all(|off| off.abs() <= 2.0)
    }
}

pub fn covered(pid: i32, area: Rect, front_to_back: &[Shown], screens: &[Rect]) -> bool {
    let Some(at) = front_to_back
        .iter()
        .rposition(|s| s.pid == pid && s.area.near(&area))
    else {
        return true;
    };
    let above: Vec<Rect> = front_to_back[..at]
        .iter()
        .filter(|s| s.solid)
        .map(|s| s.area)
        .collect();
    let step = |start: f64, length: f64, i: usize| start + length * (i as f64 + 0.5) / GRID as f64;
    (0..GRID * GRID).all(|i| {
        let x = step(area.x, area.w, i % GRID);
        let y = step(area.y, area.h, i / GRID);
        !screens.iter().any(|r| r.holds(x, y)) || above.iter().any(|r| r.holds(x, y))
    })
}

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
    pub marks_outlast_speech: bool,
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

pub fn page(window: &Window) -> Option<String> {
    fn find(node: &Node) -> Option<&str> {
        if ["AXWebArea", "Document"].contains(&node.role.as_str()) {
            return node.title.as_deref().or(node.description.as_deref());
        }
        node.children.iter().find_map(find)
    }
    find(&window.tree).map(str::to_owned)
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
    Run {
        from: f64,
        to: f64,
        who: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        mark: bool,
    },
    Me {
        me: String,
    },
}

#[derive(Debug, Clone)]
struct Run {
    from: f64,
    to: f64,
    who: String,
    mark: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    runs: Vec<Run>,
    me: Vec<String>,
    own: Option<String>,
    open: Option<(f64, String, bool)>,
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
                Mark::Run {
                    from,
                    to,
                    who,
                    mark,
                } if from.is_finite() && to > from => match timeline.runs.last_mut() {
                    Some(last) if last.from == from && last.who == who => {
                        last.to = last.to.max(to);
                    }
                    _ => timeline.runs.push(Run {
                        from,
                        to,
                        who,
                        mark,
                    }),
                },
                Mark::Me { me } => {
                    timeline.learn(&me);
                }
                Mark::Run { .. } => {}
            }
        }
        timeline.runs.sort_by(|a, b| a.from.total_cmp(&b.from));
        timeline.measure_longest();
        timeline
    }

    fn measure_longest(&mut self) {
        self.longest = self
            .runs
            .iter()
            .map(|run| run.to - run.from)
            .fold(0.0, f64::max);
    }

    pub fn doubt_marks_through_silence(&mut self, sound: &[(f64, f64)], lines: &[(f64, f64)]) {
        let sound = union(sound.to_vec());
        for run in self.runs.iter_mut().filter(|run| run.mark) {
            let first = sound.partition_point(|(_, end)| *end <= run.from);
            let mut quiet_from = run.from;
            let mut stale = false;
            for (start, end) in sound[first..]
                .iter()
                .take_while(|(start, _)| *start < run.to)
            {
                stale |= start - quiet_from > STALE_SECS;
                quiet_from = *end;
            }
            stale |= run.to - quiet_from > STALE_SECS;
            if stale {
                let late = |(start, _): &&(f64, f64)| *start - run.from <= STALE_SECS;
                let came_with = lines
                    .iter()
                    .filter(|(_, end)| *end > run.from)
                    .filter(late)
                    .map(|(_, end)| *end)
                    .min_by(f64::total_cmp);
                run.to = came_with.map_or(run.from, |end| run.to.min(end));
            }
        }
        self.measure_longest();
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

    fn who(&self, reading: &Reading) -> Option<(String, bool)> {
        let marked = |name: &str| {
            self.me.contains(&norm(name))
                || reading.me.as_deref().is_some_and(|m| norm(m) == norm(name))
        };
        let mine = |name: &str| marked(name) || self.is_me(name);
        let people = unique(&reading.participants);
        let roster: Vec<String> = people.iter().map(|p| norm(p)).collect();
        let outside = reading
            .speaking
            .iter()
            .any(|p| !mine(p) && !roster.contains(&norm(p)));
        if people.len() == 2 && reading.shown == Some(2) && !outside {
            let others: Vec<&String> = people.iter().filter(|p| !marked(p)).collect();
            if let [other] = others.as_slice() {
                if !mine(other) {
                    return usable(other).map(|who| (who, false));
                }
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
            [one] => usable(one).map(|who| (who, !reading.marks_outlast_speech)),
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
            (Some((_, open, was)), Some((who, mark)), Some(last)) => {
                open == who && was == mark && at - last <= HOLD_SECS
            }
            _ => false,
        };
        if goes_on {
            if let Some((from, who, mark)) = &self.open {
                marks.push(Mark::Run {
                    from: *from,
                    to: at,
                    who: who.clone(),
                    mark: *mark,
                });
            }
        } else {
            marks.extend(self.close(at));
            self.open = who.map(|(who, mark)| (at, who, mark));
        }
        self.last = Some(at);
        marks
    }

    fn close(&mut self, next: f64) -> Option<Mark> {
        let (from, who, mark) = self.open.take()?;
        let to = next.min(self.last.unwrap_or(from) + HOLD_SECS);
        if to <= from {
            return None;
        }
        self.runs.push(Run {
            from,
            to,
            who: who.clone(),
            mark,
        });
        self.longest = self.longest.max(to - from);
        Some(Mark::Run {
            from,
            to,
            who,
            mark,
        })
    }

    pub fn cut(&mut self, at: f64) -> Option<Mark> {
        self.close(at)
    }

    pub fn end(&mut self) -> Option<Mark> {
        self.close(f64::INFINITY)
    }

    fn reaching(&self, start: f64) -> &[Run] {
        let reach = start - self.longest;
        &self.runs[self.runs.partition_point(|run| run.from < reach)..]
    }

    pub fn name(&self, start: f64, end: f64) -> Option<String> {
        if end <= start {
            return self
                .reaching(start)
                .iter()
                .take_while(|run| run.from <= start)
                .find(|run| start < run.to && !self.is_me(&run.who))
                .filter(|run| self.sure_of(run, start, end))
                .map(|run| run.who.clone());
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
        let mut time: Vec<(String, String, Spans)> = Vec::new();
        for &(start, end) in &spans {
            for Run { from, to, who, .. } in
                self.reaching(start).iter().take_while(|run| run.from < end)
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
            .filter(|who| {
                let ours = |run: &&Run| norm(&run.who) == norm(who);
                whole >= SHORT_LINE_SECS
                    || spans.iter().all(|&(start, end)| {
                        let mut near = self.reaching(start).iter().filter(ours);
                        near.any(|run| self.sure_of(run, start, end))
                    })
            })
    }

    fn sure_of(&self, run: &Run, start: f64, end: f64) -> bool {
        !run.mark
            || end - start >= SHORT_LINE_SECS
            || (run.from <= start - LAG_SECS && run.to >= end + LAG_SECS)
    }
}

type Spans = Vec<(f64, f64)>;

fn union(mut spans: Spans) -> Spans {
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
