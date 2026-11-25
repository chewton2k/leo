use super::{Node, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zoom,
    Teams,
    Meet,
}

pub const ZOOM_APPS: &[&str] = &["us.zoom.xos", "zoom.exe"];
pub const TEAMS_APPS: &[&str] = &[
    "com.microsoft.teams2",
    "com.microsoft.teams",
    "ms-teams.exe",
    "teams.exe",
];
pub const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "company.thebrowser.Browser",
    "com.apple.Safari",
    "org.mozilla.firefox",
    "chrome.exe",
    "msedge.exe",
    "brave.exe",
    "arc.exe",
    "firefox.exe",
];

pub fn app_kind(app: &str) -> Option<Kind> {
    let found = |list: &[&str]| list.iter().any(|id| id.eq_ignore_ascii_case(app));
    if found(ZOOM_APPS) {
        Some(Kind::Zoom)
    } else if found(TEAMS_APPS) {
        Some(Kind::Teams)
    } else if found(BROWSERS) {
        Some(Kind::Meet)
    } else {
        None
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seat {
    pub pid: u32,
    pub app: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    pub pid: u32,
    pub app: String,
    pub title: String,
    pub page: Option<String>,
    pub call: bool,
}

fn browser(app: &str) -> bool {
    BROWSERS.iter().any(|id| id.eq_ignore_ascii_case(app))
}

fn of_page(title: &str, page: &str) -> bool {
    title.strip_prefix(page).is_some_and(|rest| {
        rest.is_empty()
            || [" - ", " – ", " — "]
                .iter()
                .any(|dash| rest.starts_with(dash))
    })
}

fn call_page(one: &Seen) -> String {
    one.page
        .as_deref()
        .map(str::trim)
        .filter(|page| page.contains(" - ") || page.contains(" – "))
        .filter(|page| of_page(&one.title, page))
        .unwrap_or(&one.title)
        .to_owned()
}

pub fn same_call(seat: &Seat, page: Option<&str>) -> bool {
    match (&seat.title, page) {
        (Some(title), Some(page)) if page.contains(" - ") || page.contains(" – ") => {
            page.trim() == title || !of_page(page.trim(), title)
        }
        _ => true,
    }
}

pub fn pick(seat: Option<&Seat>, seen: &[Seen]) -> Result<(Seat, Vec<usize>), &'static str> {
    let same = |s: &Seen, pid: u32, app: &str| s.pid == pid && s.app == app;
    if let Some(seat) = seat {
        let at: Vec<usize> = (0..seen.len())
            .filter(|&i| {
                same(&seen[i], seat.pid, &seat.app)
                    && seat
                        .title
                        .as_ref()
                        .is_none_or(|t| of_page(&seen[i].title, t))
            })
            .collect();
        if at.is_empty() {
            return Err("the call window closed");
        }
        return Ok((seat.clone(), at));
    }
    let calls: Vec<usize> = (0..seen.len()).filter(|&i| seen[i].call).collect();
    let Some(&first) = calls.first() else {
        return Err("no call window is open");
    };
    let one = calls
        .iter()
        .map(|&i| &seen[i])
        .find(|one| one.page.is_some())
        .unwrap_or(&seen[first]);
    let title = browser(&one.app).then(|| call_page(one));
    let of_call = |s: &Seen| {
        title.as_ref().is_none_or(|t| match (&s.page, &one.page) {
            (Some(page), Some(chosen)) => page.trim() == chosen.trim(),
            _ => of_page(&s.title, t),
        })
    };
    let alone = calls
        .iter()
        .all(|&i| same(&seen[i], one.pid, &one.app) && of_call(&seen[i]));
    if !alone {
        return Err("more than one call is open");
    }
    let at = (0..seen.len())
        .filter(|&i| same(&seen[i], one.pid, &one.app) && of_call(&seen[i]))
        .collect();
    Ok((
        Seat {
            pid: one.pid,
            app: one.app.clone(),
            title,
        },
        at,
    ))
}

fn teams_title(title: &str) -> bool {
    title.contains("Microsoft Teams")
        || title
            .rsplit_once('|')
            .is_some_and(|(_, last)| last.trim().eq_ignore_ascii_case("teams"))
}

pub fn kind_of(app: &str, windows: &[Window]) -> Option<Kind> {
    match app_kind(app)? {
        Kind::Meet if windows.iter().any(|w| teams_title(&w.title)) => Some(Kind::Teams),
        kind => Some(kind),
    }
}

pub(crate) fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case(word))
}

pub fn call_window(kind: Kind, title: &str, id: &str) -> bool {
    match kind {
        Kind::Zoom => {
            id.starts_with("zm.meeting")
                || title.starts_with("Zoom Meeting")
                || title.starts_with("Zoom Webinar")
                || title == "Meeting"
                || title == "zoom floating video window"
        }
        Kind::Teams => true,
        Kind::Meet => has_word(title, "meet") || teams_title(title),
    }
}

const CALL_CONTROLS: &[&str] = &[
    "leave",
    "leave call",
    "hang up",
    "end call",
    "mute mic",
    "unmute mic",
    "close participants pane",
];
const BUTTONS: &[&str] = &[
    "AXButton",
    "AXCheckBox",
    "AXMenuItem",
    "Button",
    "CheckBox",
    "MenuItem",
    "SplitButton",
];

fn control_label(text: &str) -> String {
    let text = text.trim();
    let text = match (text.ends_with(')'), text.rfind('(')) {
        (true, Some(at)) => &text[..at],
        _ => text,
    };
    text.trim().to_lowercase()
}

pub fn is_call_tree(node: &Node) -> bool {
    if node.id.as_deref() == Some("hangup-button") {
        return true;
    }
    if BUTTONS.contains(&node.role.as_str())
        && [&node.description, &node.title]
            .into_iter()
            .flatten()
            .any(|label| CALL_CONTROLS.contains(&control_label(label).as_str()))
    {
        return true;
    }
    node.children.iter().any(is_call_tree)
}
