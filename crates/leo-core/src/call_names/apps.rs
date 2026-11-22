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
