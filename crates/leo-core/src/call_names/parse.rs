use super::apps::has_word;
use super::{norm, unique, Kind, Node, Reading, Window};

const TILES: &[&str] = &["AXTabGroup", "TabItem"];
const TEXTS: &[&str] = &["AXStaticText", "Text"];
const ROWS: &[&str] = &["AXRow", "AXCell", "ListItem", "DataItem", "TreeItem"];
const LISTS: &[&str] = &[
    "AXOutline",
    "AXTable",
    "AXList",
    "List",
    "Tree",
    "Table",
    "DataGrid",
];
const PAGES: &[&str] = &["AXWindow", "AXWebArea", "Window", "Document"];
const LIVE: &[&str] = &[
    "AXApplicationAlert",
    "AXApplicationStatus",
    "AXApplicationLog",
    "AXApplicationTimer",
    "AXApplicationMarquee",
    "StatusBar",
    "ToolTip",
];

fn walk<'a>(node: &'a Node, up: &mut Vec<&'a Node>, f: &mut dyn FnMut(&'a Node, &[&'a Node])) {
    f(node, up);
    up.push(node);
    for child in &node.children {
        walk(child, up, f);
    }
    up.pop();
}

fn each<'a>(
    windows: impl IntoIterator<Item = &'a Window>,
    f: &mut dyn FnMut(&'a Node, &[&'a Node]),
) {
    for window in windows {
        walk(&window.tree, &mut Vec::new(), f);
    }
}

fn is(node: &Node, roles: &[&str]) -> bool {
    roles.contains(&node.role.as_str())
}

fn label(node: &Node) -> Option<&str> {
    node.description.as_deref().or(node.title.as_deref())
}

fn text(node: &Node) -> Option<&str> {
    node.value.as_deref().or(node.title.as_deref())
}

fn texts(node: &Node) -> impl Iterator<Item = &str> {
    [&node.description, &node.title, &node.value]
        .into_iter()
        .flatten()
        .map(String::as_str)
}

fn low(text: &str) -> String {
    text.trim().to_lowercase()
}

const COUNTED: &[&str] = &[
    "participants",
    "people",
    "attendees",
    "in this meeting",
    "in the meeting",
    "in call",
];

fn shown_count(text: &str) -> Option<usize> {
    let body = text.trim().strip_suffix(')')?;
    let open = body.rfind('(')?;
    let head = low(&body[..open]);
    if !COUNTED.contains(&head.strip_prefix("meeting ").unwrap_or(&head)) {
        return None;
    }
    body[open + 1..].trim().parse().ok()
}

fn shown(windows: &[&Window]) -> Option<usize> {
    let mut counts: Vec<usize> = Vec::new();
    each(windows.iter().copied(), &mut |node, _| {
        counts.extend(texts(node).filter_map(shown_count));
    });
    let first = *counts.first()?;
    counts.iter().all(|n| *n == first).then_some(first)
}

pub fn read_windows(kind: Kind, windows: &[Window]) -> Reading {
    let looked: Vec<&Window> = match kind {
        Kind::Teams => windows
            .iter()
            .filter(|w| super::is_call_tree(&w.tree))
            .collect(),
        _ => windows.iter().collect(),
    };
    let reading = match kind {
        Kind::Zoom => zoom(windows),
        Kind::Meet => meet(windows),
        Kind::Teams => teams(looked.iter().copied()),
    };
    Reading {
        speaking: unique(&reading.speaking),
        participants: unique(&reading.participants),
        me: reading.me,
        shown: shown(&looked),
        unmarked_self: kind == Kind::Zoom,
    }
}

const ZOOM_WORDS: &[[&str; 6]] = &[
    [
        "Computer audio unmuted",
        "Computer audio muted",
        "Active speaker",
        "Talking",
        "Participants list",
        "me",
    ],
    [
        "Bei Computeraudio Stummschaltung deaktiviert",
        "Bei Computeraudio Stummschaltung aktiviert",
        "Aktiver Sprecher",
        "Sprechen",
        "Teilnehmerliste",
        "ich",
    ],
    [
        "Audio de la computadora no silenciado",
        "Audio de la computadora silenciado",
        "Orador activo",
        "Hablando",
        "Lista de participantes",
        "yo",
    ],
    [
        "Son de l'ordinateur rétabli",
        "Ordinateur mis en sourdine",
        "Intervenant actif",
        "La parole est à",
        "Liste des participants",
        "moi",
    ],
    [
        "Audio computer attivato",
        "Audio computer disattivato",
        "Oratore attivo",
        "Conversazione in corso",
        "Elenco dei partecipanti",
        "io",
    ],
    [
        "Áudio do computador ativado",
        "Áudio do computador desativado",
        "Orador ativo",
        "Falando",
        "Lista de participantes",
        "eu",
    ],
    [
        "Computeraudio ingeschakeld",
        "Computeraudio gedempt",
        "Actieve spreker",
        "Aan het spreken",
        "Deelnemerslijst",
        "ik",
    ],
    [
        "Datorljudet aktiverades",
        "Datorljudet inaktiverades",
        "Aktiv talare",
        "Talar",
        "Deltagarlista",
        "jag",
    ],
    [
        "ミュート解除されたコンピュータ オーディオ",
        "ミュートされたコンピュータ オーディオ",
        "アクティブ スピーカー",
        "発言中",
        "参加者リスト",
        "自分",
    ],
    [
        "电脑音频开启",
        "电脑音频静音",
        "当前演讲者",
        "正在发言",
        "参会者列表",
        "我",
    ],
];

const ZOOM_TAGS: &[&str] = &["host", "co-host", "guest", "interpreter"];
const ZOOM_STATUS: &[&str] = &["video on", "video off", "no audio", "pinned", "spotlighted"];

fn zoom_status(field: &str) -> bool {
    ZOOM_STATUS.contains(&field)
        || field.starts_with("telephone")
        || field.starts_with("screen sharing")
}

fn zoom_tag(tag: &str) -> bool {
    let tag = low(tag);
    ZOOM_TAGS.contains(&tag.as_str())
        || tag.starts_with("participant id:")
        || ZOOM_WORDS.iter().any(|w| w[5] == tag.as_str())
}

fn zoom_tags(raw: &str) -> (String, Vec<String>) {
    let mut name = raw.trim().to_owned();
    let mut tags = Vec::new();
    while let Some(body) = name.trim_end().strip_suffix([')', '）']) {
        let Some(open) = body.rfind(['(', '（']) else {
            break;
        };
        let inner = &body[open + body[open..].chars().next().map_or(0, char::len_utf8)..];
        let parts: Vec<String> = inner.split([',', '，', '、']).map(low).collect();
        if !parts.iter().all(|t| zoom_tag(t)) {
            break;
        }
        tags.extend(parts);
        name = body[..open].trim_end().to_owned();
    }
    (name, tags)
}

fn zoom_name(raw: &str) -> Option<String> {
    let (name, _) = zoom_tags(raw);
    (!name.is_empty() && !low(&name).starts_with("participants (") && name.chars().count() <= 60)
        .then_some(name)
}

fn zoom_mine(tags: &[String]) -> bool {
    tags.iter()
        .any(|t| ZOOM_WORDS.iter().any(|w| w[5] == t.as_str()))
}

fn zoom_tile(description: &str) -> Option<(String, bool, bool)> {
    let mut fields: Vec<&str> = description.split([',', '、']).map(str::trim).collect();
    let (mut talking, mut known) = (false, false);
    while fields.len() > 1 {
        let field = low(fields[fields.len() - 1]);
        if ZOOM_WORDS.iter().any(|w| low(w[2]) == field) {
            talking = true;
            known = true;
        } else if ZOOM_WORDS
            .iter()
            .any(|w| low(w[0]) == field || low(w[1]) == field)
        {
            known = true;
        } else if !zoom_status(&field) {
            break;
        }
        fields.pop();
    }
    if !known {
        return None;
    }
    let raw = fields.join(", ");
    zoom_name(&raw).map(|name| (name, talking, zoom_mine(&zoom_tags(&raw).1)))
}

fn zoom_talking(text: &str) -> Option<String> {
    ZOOM_WORDS.iter().find_map(|w| {
        let rest = text.strip_prefix(w[3])?.trim_start();
        let rest = rest.strip_prefix([':', '：'])?;
        zoom_name(rest)
    })
}

fn zoom(windows: &[Window]) -> Reading {
    let mut reading = Reading::default();
    each(windows, &mut |node, up| {
        if is(node, TILES) {
            if let Some((name, talking, mine)) = label(node).and_then(zoom_tile) {
                if talking {
                    reading.speaking.push(name.clone());
                }
                if mine {
                    reading.me = Some(name.clone());
                }
                reading.participants.push(name);
            }
        }
        if is(node, TEXTS) {
            if let Some(name) = text(node).and_then(zoom_talking) {
                reading.speaking.push(name);
            }
        }
        let in_list = up.iter().any(|a| {
            is(a, LISTS) && label(a).is_some_and(|l| ZOOM_WORDS.iter().any(|w| low(w[4]) == low(l)))
        });
        if is(node, ROWS) && in_list {
            let mut first = None;
            walk(node, &mut Vec::new(), &mut |x, _| {
                if first.is_none() && is(x, TEXTS) {
                    first = text(x);
                }
            });
            if let Some(raw) = first {
                if let Some(name) = zoom_name(raw) {
                    if zoom_mine(&zoom_tags(raw).1) {
                        reading.me = Some(name.clone());
                    }
                    reading.participants.push(name);
                }
            }
        }
    });
    reading
}

pub const MEET_TALKING_CLASSES: &[&str] = &["v5h6Xc"];
const TILE_UP: usize = 7;
const MEET_WORDS: &[&str] = &[
    "meet",
    "reframe",
    "captions",
    "chat",
    "people",
    "activities",
    "host controls",
    "present",
    "raise hand",
    "reactions",
    "recaptcha",
    "upgrade",
    "am",
    "pm",
];
const PARTICLES: &[&str] = &[
    "de", "del", "della", "der", "den", "di", "da", "das", "do", "dos", "du", "la", "le", "van",
    "von", "ter", "ten", "y", "e", "bin", "binti", "al", "el", "ibn", "zu",
];

fn meeting_code(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 3
        && parts
            .iter()
            .zip([3, 4, 3])
            .all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_lowercase()))
}

fn strip_meet_tag(text: &str) -> &str {
    let trimmed = text.trim_end();
    if let Some(open) = trimmed.rfind('(') {
        if trimmed.ends_with(')') {
            let inner = trimmed[open + 1..].to_lowercase();
            if ["presenting", "presentation", "you"]
                .iter()
                .any(|t| inner.starts_with(t))
            {
                return trimmed[..open].trim_end();
            }
        }
    }
    trimmed
}

fn meet_name(text: &str) -> Option<String> {
    let name = strip_meet_tag(text.trim()).trim();
    let count = name.chars().count();
    if !(2..=60).contains(&count) || !name.chars().any(char::is_alphabetic) {
        return None;
    }
    if name
        .chars()
        .any(|c| c.is_ascii_digit() || "?!…:/⌘|".contains(c))
        || name.contains("..")
        || [" - ", " – ", " — "].iter().any(|d| name.contains(d))
        || meeting_code(name)
        || low(name).contains("says in chat")
        || MEET_WORDS.contains(&low(name).as_str())
    {
        return None;
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    if name.ends_with('.')
        && words
            .last()
            .is_some_and(|w| w.trim_end_matches('.').chars().count() > 2)
    {
        return None;
    }
    if words.len() > 5 {
        return None;
    }
    if words.len() > 1
        && words
            .iter()
            .any(|w| w.chars().next().is_some_and(char::is_lowercase) && !PARTICLES.contains(w))
    {
        return None;
    }
    Some(name.to_owned())
}

fn live(node: &Node) -> bool {
    is(node, LIVE) || node.subrole.as_deref().is_some_and(|s| LIVE.contains(&s))
}

fn chat_panel(node: &Node) -> bool {
    !is(node, PAGES)
        && label(node).is_some_and(|l| {
            has_word(l, "chat") || has_word(l, "message") || has_word(l, "messages")
        })
}

fn meet(windows: &[Window]) -> Reading {
    let mut groups: Vec<(&Node, Vec<String>)> = Vec::new();
    for window in windows {
        let titled = window
            .title
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "Meet");
        if !window.title.is_empty() && !titled {
            continue;
        }
        let skip: Vec<String> = window
            .title
            .split(" - ")
            .flat_map(|p| p.split(" – "))
            .flat_map(|p| p.split(" — "))
            .map(low)
            .collect();
        walk(&window.tree, &mut Vec::new(), &mut |node, up| {
            if !is(node, TEXTS) || up.len() < TILE_UP || up.iter().any(|a| live(a) || chat_panel(a))
            {
                return;
            }
            let Some(name) = text(node).and_then(meet_name) else {
                return;
            };
            if skip.contains(&low(&name)) {
                return;
            }
            let root = up[up.len() - TILE_UP];
            match groups.iter_mut().find(|(k, _)| std::ptr::eq(*k, root)) {
                Some((_, names)) => names.push(name),
                None => groups.push((root, vec![name])),
            }
        });
    }
    let mut reading = Reading::default();
    for (root, names) in groups {
        let names = unique(&names);
        let [one] = names.as_slice() else {
            continue;
        };
        if norm(one) == "you" {
            reading.me = Some(one.clone());
        }
        let mut talking = false;
        walk(root, &mut Vec::new(), &mut |x, _| {
            if x.classes
                .iter()
                .any(|c| MEET_TALKING_CLASSES.contains(&c.as_str()))
            {
                talking = true;
            }
        });
        if talking {
            reading.speaking.push(one.clone());
        }
        reading.participants.push(one.clone());
    }
    reading
}

const TEAMS_STATUS: &[&str] = &[
    "muted",
    "unmuted",
    "mic on",
    "mic off",
    "microphone on",
    "microphone off",
    "video on",
    "video off",
    "video is on",
    "video is off",
    "camera on",
    "camera off",
    "camera is on",
    "camera is off",
];
const TEAMS_ROLES: &[&str] = &[
    "organizer",
    "presenter",
    "guest",
    "external",
    "co-organizer",
    "coorganizer",
    "attendee",
    "host",
    "co-host",
    "cohost",
    "interpreter",
];
const TEAMS_SAYS: &[&str] = &["speaking", "talking", "active speaker", "now speaking"];
const TEAMS_ROSTERS: &[&str] = &[
    "participants",
    "attendees",
    "people",
    "in this meeting",
    "in the meeting",
];
const TEAMS_TAGS: &[&str] = &["organizer", "presenter", "guest", "external", "you", "me"];
const TEAMS_ROWS: &[&str] = &["AXRow", "ListItem", "DataItem", "TreeItem"];

fn starts_word(text: &str, word: &str) -> bool {
    text.get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
        && text[word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric())
}

fn find_ascii(text: &str, needle: &str) -> Option<usize> {
    text.char_indices().map(|(at, _)| at).find(|&at| {
        text.get(at..at + needle.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(needle))
    })
}

fn clean_teams(text: &str) -> String {
    let mut name = text.to_owned();
    for tag in TEAMS_TAGS {
        let needle = format!("({tag})");
        while let Some(at) = find_ascii(&name, &needle) {
            name = format!("{}{}", name[..at].trim_end(), &name[at + needle.len()..]);
        }
    }
    name.trim()
        .trim_end_matches([',', '.', ':', ';'])
        .trim()
        .to_owned()
}

fn bounded(name: &str) -> Option<String> {
    let count = name.chars().count();
    (2..=60).contains(&count).then(|| name.to_owned())
}

fn teams_speaker(label: &str) -> Option<String> {
    for (at, _) in label.char_indices().skip(2) {
        if label[..at].chars().count() > 60 {
            break;
        }
        let rest = &label[at..];
        let after = if let Some(r) = rest.strip_prefix(',') {
            r.trim_start()
        } else if rest.starts_with(char::is_whitespace) && starts_word(rest.trim_start(), "is") {
            rest.trim_start()[2..].trim_start()
        } else {
            continue;
        };
        if starts_word(after, "speaking") || starts_word(after, "talking") {
            return Some(clean_teams(&label[..at]));
        }
    }
    for marker in ["active speaker", "now speaking", "speaking now"] {
        let Some(at) = find_ascii(label, marker) else {
            continue;
        };
        if at > 0
            && label[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        let rest = &label[at + marker.len()..];
        let rest = rest.strip_prefix([':', ',']).unwrap_or(rest);
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        return bounded(rest.trim()).map(|n| clean_teams(&n));
    }
    None
}

fn teams_person(label: &str) -> Option<(String, bool)> {
    let fields: Vec<&str> = label.split(',').map(str::trim).collect();
    let state = fields
        .iter()
        .position(|f| TEAMS_STATUS.contains(&low(f).as_str()))?;
    if state < 1 {
        return None;
    }
    let mut before = fields[..state].to_vec();
    while before.len() > 1 && TEAMS_ROLES.contains(&low(before[before.len() - 1]).as_str()) {
        before.pop();
    }
    let name = bounded(&clean_teams(&before.join(", ")))?;
    let speaking = fields.iter().any(|f| TEAMS_SAYS.contains(&low(f).as_str()));
    Some((name, speaking))
}

fn teams_me(label: &str) -> Option<String> {
    let at = find_ascii(label, "(you)").or_else(|| find_ascii(label, "(me)"))?;
    bounded(&clean_teams(&label[..at]))
}

fn in_roster(node: &Node, up: &[&Node]) -> bool {
    (is(node, TEAMS_ROWS) || up.iter().any(|a| is(a, TEAMS_ROWS)))
        && up.iter().any(|a| {
            is(a, LISTS)
                && label(a).is_some_and(|l| {
                    let l = l.trim();
                    let l = if starts_word(l, "meeting") {
                        l[7..].trim_start()
                    } else {
                        l
                    };
                    TEAMS_ROSTERS.iter().any(|r| starts_word(l, r))
                })
        })
}

fn teams<'a>(windows: impl IntoIterator<Item = &'a Window>) -> Reading {
    let mut reading = Reading::default();
    each(windows, &mut |node, up| {
        let roster = in_roster(node, up);
        for label in texts(node) {
            if let Some(name) = teams_speaker(label) {
                if name.chars().count() >= 2
                    && !starts_word(&name, "you")
                    && !starts_word(&name, "me")
                {
                    reading.speaking.push(name);
                }
            }
            if !roster {
                continue;
            }
            if let Some((name, speaking)) = teams_person(label) {
                if speaking {
                    reading.speaking.push(name.clone());
                }
                reading.participants.push(name);
            }
            if let Some(me) = teams_me(label) {
                reading.participants.push(me.clone());
                reading.me = Some(me);
            }
        }
    });
    reading
}
