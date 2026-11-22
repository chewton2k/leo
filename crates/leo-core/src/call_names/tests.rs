use super::*;

fn windows(json: &str) -> Vec<Window> {
    serde_json::from_str(json).unwrap()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn reading(speaking: &[&str], participants: &[&str], me: Option<&str>) -> Reading {
    Reading {
        speaking: names(speaking),
        participants: names(participants),
        me: me.map(String::from),
    }
}

fn look(app: &str, json: &str) -> Look {
    Look::Call {
        app: app.into(),
        windows: windows(json),
    }
}

#[test]
fn zoom_on_a_mac_gives_the_active_speaker_the_people_and_who_is_me() {
    let read = read(&look("us.zoom.xos", include_str!("fixtures/zoom-mac.json"))).unwrap();
    assert_eq!(read.speaking, ["Rowan Ellery"]);
    assert_eq!(
        read.participants,
        ["Rowan Ellery", "Tamsin Okafor", "Juno Halvorsen"]
    );
    assert_eq!(read.me.as_deref(), Some("Juno Halvorsen"));
}

#[test]
fn zoom_while_sharing_reads_the_floating_window() {
    let read = read(&look(
        "us.zoom.xos",
        include_str!("fixtures/zoom-floating.json"),
    ))
    .unwrap();
    assert_eq!(read.speaking, ["Tamsin Okafor"]);
}

#[test]
fn zoom_on_windows_reads_tile_names() {
    let read = read(&look(
        "Zoom.exe",
        include_str!("fixtures/zoom-windows.json"),
    ))
    .unwrap();
    assert_eq!(read.speaking, ["Rowan Ellery"]);
    assert_eq!(read.participants.len(), 3);
    assert_eq!(read.me, None);
}

#[test]
fn zoom_tiles_in_other_languages_and_tags() {
    let tree = |d: &str| {
        vec![Window {
            title: "Zoom Meeting".into(),
            id: String::new(),
            tree: Node {
                role: "AXTabGroup".into(),
                description: Some(d.into()),
                ..Node::default()
            },
        }]
    };
    let de = read_windows(
        Kind::Zoom,
        &tree("Rowan Ellery, Bei Computeraudio Stummschaltung deaktiviert, Aktiver Sprecher"),
    );
    assert_eq!(de.speaking, ["Rowan Ellery"]);
    let tagged = read_windows(
        Kind::Zoom,
        &tree("Ellery, Rowan (Co-host), Computer audio muted"),
    );
    assert_eq!(tagged.participants, ["Ellery, Rowan"]);
    assert!(tagged.speaking.is_empty());
    assert!(read_windows(Kind::Zoom, &tree("Gallery view"))
        .participants
        .is_empty());
}

#[test]
fn meet_reads_named_tiles_and_the_talking_class_only() {
    let read = read(&look(
        "com.google.Chrome",
        include_str!("fixtures/meet-chrome.json"),
    ))
    .unwrap();
    assert_eq!(read.speaking, ["Pim Castell"]);
    assert_eq!(read.participants, ["Pim Castell", "Oriel Danvers", "You"]);
    assert_eq!(read.me.as_deref(), Some("You"));
}

#[test]
fn meet_skips_windows_that_are_not_a_call() {
    let mut list = windows(include_str!("fixtures/meet-chrome.json"));
    list[0].title = "Meeting notes - Google Chrome".into();
    assert_eq!(read_windows(Kind::Meet, &list), Reading::default());
}

#[test]
fn teams_reads_only_the_call_window_roster_and_speaking_label() {
    let read = read(&look(
        "com.microsoft.teams2",
        include_str!("fixtures/teams-mac.json"),
    ))
    .unwrap();
    assert_eq!(read.speaking, ["Ada Brennick"]);
    assert_eq!(
        read.participants,
        ["Ada Brennick", "Lio Marchetti", "Mara Quist"]
    );
    assert_eq!(read.me.as_deref(), Some("Lio Marchetti"));
}

#[test]
fn teams_labels_whose_case_changes_their_length_are_read_without_a_panic() {
    let row = |d: &str| Node {
        role: "AXRow".into(),
        description: Some(d.into()),
        ..Node::default()
    };
    let list = vec![Window {
        title: "Weekly sync | Microsoft Teams".into(),
        id: String::new(),
        tree: Node {
            role: "AXWindow".into(),
            children: vec![
                Node {
                    role: "AXButton".into(),
                    id: Some("hangup-button".into()),
                    ..Node::default()
                },
                Node {
                    role: "AXOutline".into(),
                    description: Some("Participants".into()),
                    children: vec![row("İ(You)ẞ, Muted"), row("İrem Aksoy (You), Muted")],
                    ..Node::default()
                },
                Node {
                    role: "AXGroup".into(),
                    description: Some("Active speaker: ẞtefan İnce (Guest)".into()),
                    ..Node::default()
                },
            ],
            ..Node::default()
        },
    }];
    let read = read_windows(Kind::Teams, &list);
    assert_eq!(read.participants, ["İẞ", "İrem Aksoy"]);
    assert_eq!(read.me.as_deref(), Some("İrem Aksoy"));
    assert_eq!(read.speaking, ["ẞtefan İnce"]);
}

#[test]
fn teams_in_a_browser_is_read_as_teams() {
    let list = windows(include_str!("fixtures/teams-mac.json"));
    assert_eq!(kind_of("msedge.exe", &list), Some(Kind::Teams));
    assert_eq!(kind_of("msedge.exe", &[]), Some(Kind::Meet));
    assert_eq!(kind_of("notepad.exe", &list), None);
}

#[test]
fn call_windows_are_told_apart_by_title() {
    assert!(call_window(
        Kind::Meet,
        "Meet - abc-defg-hij - Google Chrome",
        ""
    ));
    assert!(!call_window(
        Kind::Meet,
        "Meeting notes - Google Chrome",
        ""
    ));
    assert!(call_window(Kind::Zoom, "", "zm.meeting.window.main"));
    assert!(call_window(Kind::Zoom, "Zoom Webinar", ""));
    assert!(!call_window(Kind::Zoom, "Zoom Workplace", ""));
}

#[test]
fn an_unreadable_app_reads_nothing() {
    assert_eq!(read(&Look::Unreadable("no permission".into())), None);
}

fn timeline(samples: &[(f64, Option<Reading>)]) -> Timeline {
    let mut t = Timeline::default();
    for (at, r) in samples {
        t.heard(*at, r.as_ref());
    }
    t.end();
    t
}

fn every(from: f64, to: f64, r: Option<Reading>) -> Vec<(f64, Option<Reading>)> {
    let mut out = Vec::new();
    let mut at = from;
    while at < to {
        out.push((at, r.clone()));
        at += 0.4;
    }
    out
}

const GROUP: &[&str] = &["Juno Halvorsen", "Rowan Ellery", "Tamsin Okafor"];

#[test]
fn a_line_gets_the_name_marked_for_most_of_it() {
    let mut samples = every(0.0, 6.0, Some(reading(&["Rowan Ellery"], GROUP, None)));
    samples.extend(every(6.0, 8.0, Some(reading(&[], GROUP, None))));
    let t = timeline(&samples);
    assert_eq!(t.name(1.0, 5.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(t.name(4.0, 10.0), None);
    assert_eq!(t.name(20.0, 24.0), None);
}

#[test]
fn several_talking_at_once_or_nobody_marked_stays_unnamed() {
    let both = reading(&["Rowan Ellery", "Tamsin Okafor"], GROUP, None);
    let t = timeline(&every(0.0, 6.0, Some(both)));
    assert_eq!(t.name(1.0, 5.0), None);
    let t = timeline(&every(0.0, 6.0, Some(reading(&[], GROUP, None))));
    assert_eq!(t.name(1.0, 5.0), None);
}

#[test]
fn a_call_with_one_other_person_names_them_whoever_is_marked() {
    let two = reading(
        &[],
        &["Juno Halvorsen", "Rowan Ellery"],
        Some("Juno Halvorsen"),
    );
    let t = timeline(&every(0.0, 6.0, Some(two)));
    assert_eq!(t.name(1.0, 5.0).as_deref(), Some("Rowan Ellery"));
    let unknown_me = reading(&[], &["Juno Halvorsen", "Rowan Ellery"], None);
    assert_eq!(
        timeline(&every(0.0, 6.0, Some(unknown_me))).name(1.0, 5.0),
        None
    );
}

#[test]
fn a_speaker_missing_from_the_roster_cancels_the_two_person_rule() {
    let r = reading(
        &["Oriel Danvers", "Rowan Ellery"],
        &["Juno Halvorsen", "Rowan Ellery"],
        Some("Juno Halvorsen"),
    );
    assert_eq!(timeline(&every(0.0, 6.0, Some(r))).name(1.0, 5.0), None);
}

#[test]
fn the_users_own_name_never_goes_to_the_other_side() {
    let mut samples = every(0.0, 6.0, Some(reading(&["Juno Halvorsen"], GROUP, None)));
    samples.push((6.0, Some(reading(&[], GROUP, Some("Juno Halvorsen")))));
    let t = timeline(&samples);
    assert_eq!(t.name(1.0, 5.0), None);
    let meet_self = reading(
        &["You"],
        &["You", "Pim Castell", "Oriel Danvers"],
        Some("You"),
    );
    assert_eq!(
        timeline(&every(0.0, 6.0, Some(meet_self))).name(1.0, 5.0),
        None
    );
}

#[test]
fn a_reading_stands_for_two_seconds_at_most() {
    let t = timeline(&[
        (0.0, Some(reading(&["Rowan Ellery"], GROUP, None))),
        (10.0, Some(reading(&[], GROUP, None))),
    ]);
    assert_eq!(t.name(0.0, 3.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(t.name(0.0, 5.0), None);
    assert_eq!(t.name(2.5, 9.0), None);
}

#[test]
fn an_unreadable_moment_ends_the_name() {
    let mut samples = every(0.0, 2.0, Some(reading(&["Rowan Ellery"], GROUP, None)));
    samples.extend(every(2.0, 8.0, None));
    let t = timeline(&samples);
    assert_eq!(t.name(1.0, 7.0), None);
    assert_eq!(t.name(0.0, 2.0).as_deref(), Some("Rowan Ellery"));
}

#[test]
fn a_line_too_short_to_measure_takes_the_name_at_its_start() {
    let t = timeline(&every(
        0.0,
        6.0,
        Some(reading(&["Tamsin Okafor"], GROUP, None)),
    ));
    assert_eq!(t.name(3.0, 3.0).as_deref(), Some("Tamsin Okafor"));
    assert_eq!(t.name(30.0, 30.0), None);
}

#[test]
fn marks_written_while_recording_give_the_same_names_when_read_back() {
    let mut t = Timeline::default();
    let mut lines = Vec::new();
    let mut samples = every(0.0, 4.0, Some(reading(&["Rowan Ellery"], GROUP, None)));
    samples.extend(every(
        4.0,
        9.0,
        Some(reading(&["Tamsin Okafor"], GROUP, None)),
    ));
    samples.push((9.0, Some(reading(&[], GROUP, Some("Juno Halvorsen")))));
    for (at, r) in &samples {
        lines.extend(t.heard(*at, r.as_ref()));
    }
    lines.extend(t.end());
    let text: String = lines
        .iter()
        .map(|m| serde_json::to_string(m).unwrap() + "\n")
        .collect();
    let back = Timeline::parse(&format!("{text}{{\"broken\n"));
    for (start, end) in [(0.0, 4.0), (4.0, 9.0), (3.0, 6.0), (5.0, 5.0)] {
        assert_eq!(back.name(start, end), t.name(start, end));
    }
    assert_eq!(back.name(4.5, 8.5).as_deref(), Some("Tamsin Okafor"));
    assert!(text.contains("\"me\":\"Juno Halvorsen\""));
}

#[test]
fn readings_out_of_order_are_ignored() {
    let mut t = Timeline::default();
    t.heard(5.0, Some(&reading(&["Rowan Ellery"], GROUP, None)));
    assert!(t
        .heard(1.0, Some(&reading(&["Tamsin Okafor"], GROUP, None)))
        .is_empty());
    t.end();
    assert_eq!(t.name(1.0, 2.0), None);
}

#[test]
fn long_text_is_clipped_on_a_character_boundary() {
    let long = "é".repeat(MOST_TEXT + 5);
    assert_eq!(clip(&long).unwrap().chars().count(), MOST_TEXT);
    assert_eq!(clip("   "), None);
}
