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
        ..Reading::default()
    }
}

fn look(app: &str, json: &str) -> Look {
    Look::Call {
        app: app.into(),
        windows: windows(json),
        seat: Seat::default(),
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
    let list = windows(include_str!("fixtures/meet-chrome.json"));
    assert!(list.iter().any(|w| is_call_tree(&w.tree)));
}

#[test]
fn meet_skips_windows_that_are_not_a_call() {
    let mut list = windows(include_str!("fixtures/meet-chrome.json"));
    list[0].title = "Meeting notes - Google Chrome".into();
    assert_eq!(
        read_windows(Kind::Meet, &list),
        Reading {
            unmarked_self: true,
            ..Reading::default()
        }
    );
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
fn the_call_chosen_first_stays_the_one_read() {
    let seen = |pid: u32, app: &str, title: &str, call: bool| Seen {
        pid,
        app: app.into(),
        title: title.into(),
        page: None,
        call,
    };
    let start = [
        seen(7, "com.google.Chrome", "Meet - abc-defg-hij", true),
        seen(7, "com.google.Chrome", "Meet the team - Docs", false),
        seen(9, "us.zoom.xos", "Zoom Workplace", false),
    ];
    let (seat, at) = pick(None, &start).unwrap();
    assert_eq!(at, [0]);
    assert_eq!(seat.title.as_deref(), Some("Meet - abc-defg-hij"));
    let other = [
        seen(7, "com.google.Chrome", "Meet - xyz-wxyz-xyz", true),
        seen(9, "us.zoom.xos", "Zoom Meeting", true),
    ];
    assert_eq!(pick(Some(&seat), &other), Err("the call window closed"));
    let back = [
        seen(7, "com.google.Chrome", "Meet - xyz-wxyz-xyz", true),
        seen(7, "com.google.Chrome", "Meet - abc-defg-hij", false),
    ];
    assert_eq!(pick(Some(&seat), &back).unwrap().1, [1]);
    assert_eq!(pick(None, &other), Err("more than one call is open"));
    let two_tabs = [
        seen(7, "com.google.Chrome", "Meet - abc-defg-hij", true),
        seen(7, "com.google.Chrome", "Meet - xyz-wxyz-xyz", true),
    ];
    assert_eq!(pick(None, &two_tabs), Err("more than one call is open"));
    let zoom = [
        seen(9, "us.zoom.xos", "Zoom Meeting", true),
        seen(9, "us.zoom.xos", "zoom floating video window", false),
    ];
    let (seat, at) = pick(None, &zoom).unwrap();
    assert_eq!((seat.title, at), (None, vec![0, 1]));
    assert_eq!(pick(None, &start[1..]), Err("no call window is open"));
}

#[test]
fn a_meet_call_stays_bound_when_its_window_title_changes_or_it_pops_out() {
    let seen = |title: &str, page: Option<&str>, call: bool| Seen {
        pid: 7,
        app: "com.google.Chrome".into(),
        title: title.into(),
        page: page.map(Into::into),
        call,
    };
    let call = "Meet - abc-defg-hij";
    let main = format!("{call} - Microphone recording - Google Chrome - Juno");
    let popped = format!("{call} - Juno");
    let (seat, at) = pick(None, &[seen(&main, Some(call), true)]).unwrap();
    assert_eq!((seat.title.as_deref(), at), (Some(call), vec![0]));
    for later in [
        format!("{call} - Camera and microphone recording - Google Chrome - Juno"),
        format!("{call} – Google Chrome"),
        format!("{call} — Mozilla Firefox"),
        popped.clone(),
        call.to_owned(),
    ] {
        let now = [
            seen("Docs - Google Chrome", None, false),
            seen(&later, None, false),
        ];
        assert_eq!(pick(Some(&seat), &now).unwrap().1, [1], "{later}");
    }
    for other in ["Meet - abc-defg-hijk - Juno", "Meet - xyz-wxyz-xyz - Juno"] {
        let now = [seen(other, None, false)];
        assert_eq!(pick(Some(&seat), &now), Err("the call window closed"));
    }
    let both = [seen(&popped, None, true), seen(&main, Some(call), true)];
    let (seat, at) = pick(None, &both).unwrap();
    assert_eq!((seat.title.as_deref(), at), (Some(call), vec![0, 1]));
    let vague = [seen("Meet - Google Chrome", Some("Meet"), true)];
    let (seat, _) = pick(None, &vague).unwrap();
    assert_eq!(seat.title.as_deref(), Some("Meet - Google Chrome"));
}

#[test]
fn a_meet_window_as_it_is_in_november_2026_gives_the_people_and_who_is_me() {
    for (fixture, me, shown) in [
        (include_str!("fixtures/meet-live-main.json"), true, None),
        (
            include_str!("fixtures/meet-live-people.json"),
            true,
            Some(2),
        ),
        (include_str!("fixtures/meet-live-popout.json"), false, None),
    ] {
        let list = windows(fixture);
        assert!(shows_call("com.google.Chrome", &list[0]));
        assert!(call_window(Kind::Meet, &list[0].title, ""));
        let read = read_windows(Kind::Meet, &list);
        assert_eq!(read.participants, ["Juno Halvorsen", "Rowan Ellery"]);
        assert_eq!(read.me.as_deref(), me.then_some("Juno Halvorsen"));
        assert_eq!(read.shown, shown);
    }
    let main = windows(include_str!("fixtures/meet-live-main.json")).remove(0);
    assert_eq!(page(&main).as_deref(), Some("Meet - abc-defg-hij"));
    let popped = windows(include_str!("fixtures/meet-live-popout.json")).remove(0);
    assert_eq!(page(&popped), None);
}

#[test]
fn a_mark_left_on_the_users_own_meet_tile_never_hides_who_is_talking() {
    let mut main = windows(include_str!("fixtures/meet-live-main.json"));
    fn mark(node: &mut Node) {
        if node.classes.iter().any(|c| c == "dkjMxf") {
            node.classes.push("v5h6Xc".into());
        }
        node.children.iter_mut().for_each(mark);
    }
    mark(&mut main[0].tree);
    let read = read_windows(Kind::Meet, &main);
    assert_eq!(read.speaking, ["Juno Halvorsen", "Rowan Ellery"]);
    let t = timeline(&every(0.0, 6.0, Some(read)));
    assert_eq!(t.name(1.0, 5.0).as_deref(), Some("Rowan Ellery"));
}

fn on(from: f64, to: f64, who: &str, mark: bool) -> Mark {
    Mark::Run {
        from,
        to,
        who: who.into(),
        mark,
    }
}

#[test]
fn a_mark_that_stays_on_through_silence_names_only_the_line_it_came_with() {
    let lines = [(4.2, 11.0), (11.0, 18.0), (23.0, 30.0), (31.0, 40.0)];
    let frozen = |mark: bool| {
        let mut t = Timeline::from_marks([on(4.0, 60.0, "Rowan Ellery", mark)]);
        t.doubt_marks_through_silence(&lines, &lines);
        lines.map(|(a, b)| t.name(a, b).is_some())
    };
    assert_eq!(frozen(true), [true, false, false, false]);
    assert_eq!(frozen(false), [true; 4]);
}

#[test]
fn silence_inside_one_long_row_still_gives_a_frozen_mark_away() {
    let mut t = Timeline::from_marks([on(4.0, 60.0, "Rowan Ellery", true)]);
    let heard = [(4.2, 11.0), (19.0, 30.0)];
    t.doubt_marks_through_silence(&heard, &[(4.2, 30.0), (30.0, 60.0)]);
    assert_eq!(t.name(30.0, 60.0), None);
}

#[test]
fn a_mark_is_believed_while_the_talking_goes_on() {
    let mut t = Timeline::from_marks([on(4.0, 41.0, "Rowan Ellery", true)]);
    let spoken = [(4.2, 11.0), (12.0, 18.0), (20.5, 30.0), (31.0, 40.0)];
    t.doubt_marks_through_silence(&spoken, &spoken);
    for (from, to) in spoken {
        assert_eq!(t.name(from, to).as_deref(), Some("Rowan Ellery"));
    }
}

#[test]
fn a_mark_that_was_on_long_before_anyone_spoke_names_nobody() {
    let mut t = Timeline::from_marks([on(0.0, 60.0, "Rowan Ellery", true)]);
    let spoken = [(8.0, 12.0), (13.0, 20.0)];
    t.doubt_marks_through_silence(&spoken, &spoken);
    assert_eq!(t.name(8.0, 12.0), None);
    let mut silent = Timeline::from_marks([on(0.0, 60.0, "Rowan Ellery", true)]);
    silent.doubt_marks_through_silence(&[], &[]);
    assert_eq!(silent.name(8.0, 12.0), None);
}

#[test]
fn a_short_line_is_named_only_well_inside_one_persons_marks() {
    let t = Timeline::from_marks([on(4.0, 10.0, "Rowan Ellery", true)]);
    assert_eq!(t.name(6.0, 6.8).as_deref(), Some("Rowan Ellery"));
    assert_eq!(t.name(4.2, 5.0), None);
    assert_eq!(t.name(9.2, 9.8), None);
    assert_eq!(t.name(9.6, 9.6), None);
    assert_eq!(t.name(6.0, 6.0).as_deref(), Some("Rowan Ellery"));
    let sole = Timeline::from_marks([on(4.0, 10.0, "Rowan Ellery", false)]);
    assert_eq!(sole.name(4.2, 5.0).as_deref(), Some("Rowan Ellery"));
}

#[test]
fn rows_written_before_marks_were_told_apart_are_read_as_they_were() {
    let mut t = Timeline::parse(r#"{"from":4.0,"to":60.0,"who":"Rowan Ellery"}"#);
    t.doubt_marks_through_silence(&[(4.2, 11.0), (30.0, 40.0)], &[(4.2, 11.0)]);
    assert_eq!(t.name(30.0, 40.0).as_deref(), Some("Rowan Ellery"));
}

#[test]
fn a_run_ends_when_the_two_person_rule_gives_way_to_a_mark() {
    let two = Reading {
        shown: Some(2),
        ..reading(
            &["Rowan Ellery"],
            &["Juno Halvorsen", "Rowan Ellery"],
            Some("Juno Halvorsen"),
        )
    };
    let three = reading(
        &["Rowan Ellery"],
        &["Juno Halvorsen", "Rowan Ellery", "Tamsin Okafor"],
        Some("Juno Halvorsen"),
    );
    let mut samples = every(0.0, 10.0, Some(two));
    samples.extend(every(10.0, 60.0, Some(three)));
    let mut t = timeline(&samples);
    t.doubt_marks_through_silence(&[(1.0, 14.0), (30.0, 40.0)], &[(1.0, 14.0), (30.0, 40.0)]);
    assert_eq!(t.name(2.0, 8.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(t.name(30.0, 40.0), None);
}

#[test]
fn a_zoom_mark_may_outlast_the_speech() {
    let zoom = read(&look("us.zoom.xos", include_str!("fixtures/zoom-mac.json"))).unwrap();
    let mut t = timeline(&every(0.0, 60.0, Some(zoom)));
    t.doubt_marks_through_silence(&[(1.0, 9.0), (30.0, 40.0)], &[(1.0, 9.0), (30.0, 40.0)]);
    assert_eq!(t.name(30.0, 40.0).as_deref(), Some("Rowan Ellery"));
}

#[test]
fn a_call_window_is_covered_only_when_no_part_of_it_can_be_seen() {
    let area = |x: f64, y: f64, w: f64, h: f64| Rect { x, y, w, h };
    let shown = |pid: i32, solid: bool, area: Rect| Shown { pid, solid, area };
    let screens = [area(0.0, 0.0, 2000.0, 1200.0)];
    let covered = |call: Rect, list: &[Shown]| covered(7, call, list, &screens);
    let call = area(100.0, 100.0, 800.0, 600.0);
    let mine = shown(7, true, area(101.0, 100.0, 800.0, 599.0));
    assert!(!covered(call, &[mine]));
    assert!(covered(call, &[]));
    assert!(covered(call, &[shown(8, true, call)]));
    let over = shown(8, true, area(0.0, 0.0, 2000.0, 2000.0));
    assert!(covered(call, &[over, mine]));
    assert!(!covered(call, &[mine, over]));
    let glass = Shown {
        solid: false,
        ..over
    };
    assert!(!covered(call, &[glass, mine]));
    let left = shown(8, true, area(0.0, 0.0, 500.0, 2000.0));
    let right = shown(9, true, area(500.0, 0.0, 500.0, 2000.0));
    assert!(!covered(call, &[left, mine]));
    assert!(covered(call, &[left, right, mine]));
    let twin = shown(7, true, call);
    assert!(
        covered(call, &[twin, twin]),
        "one of two alike may be behind"
    );
    let hanging = area(1600.0, 100.0, 800.0, 600.0);
    let edge = shown(8, true, area(1500.0, 0.0, 500.0, 1200.0));
    assert!(covered(hanging, &[edge, shown(7, true, hanging)]));
}

#[test]
fn two_meet_calls_whose_names_start_alike_are_never_read_as_one() {
    let seen = |title: &str, page: Option<&str>| Seen {
        pid: 7,
        app: "com.google.Chrome".into(),
        title: title.into(),
        page: page.map(Into::into),
        call: true,
    };
    let weekly = seen("Meet - Weekly - Google Chrome", Some("Meet - Weekly"));
    let design = seen(
        "Meet - Weekly - Design - Google Chrome",
        Some("Meet - Weekly - Design"),
    );
    assert_eq!(
        pick(None, &[weekly.clone(), design]),
        Err("more than one call is open")
    );
    let (seat, _) = pick(None, &[weekly]).unwrap();
    assert!(same_call(&seat, Some("Meet - Weekly")));
    assert!(same_call(&seat, None));
    assert!(!same_call(&seat, Some("Meet - Weekly - Design")));
}

fn tile(name: &str, talking: bool) -> Node {
    let text = Node {
        role: "AXStaticText".into(),
        value: Some(name.into()),
        ..Node::default()
    };
    let mut tile = (0..7).fold(text, |child, _| Node {
        role: "AXGroup".into(),
        children: vec![child],
        ..Node::default()
    });
    if talking {
        tile.classes = vec!["v5h6Xc".into()];
    }
    tile
}

fn stage(tiles: Vec<Node>) -> Vec<Window> {
    vec![Window {
        title: "Meet - abc-defg-hij".into(),
        id: String::new(),
        tree: Node {
            role: "AXGroup".into(),
            children: tiles,
            ..Node::default()
        },
    }]
}

#[test]
fn a_mark_on_a_tile_whose_name_cannot_be_read_leaves_the_line_unnamed() {
    let you = || tile("You", false);
    let both = read_windows(
        Kind::Meet,
        &stage(vec![
            you(),
            tile("Rowan Ellery", true),
            tile("tamsin okafor", true),
        ]),
    );
    assert!(both.speaking.is_empty());
    for odd in [
        "tamsin okafor",
        "Tamsin2",
        "Tamsin - Acme",
        "One Two Three Four Five Six",
    ] {
        let list = stage(vec![you(), tile("Rowan Ellery", false), tile(odd, true)]);
        assert!(read_windows(Kind::Meet, &list).speaking.is_empty(), "{odd}");
    }
    let plain = stage(vec![
        you(),
        tile("Rowan Ellery", true),
        tile("tamsin okafor", false),
    ]);
    assert_eq!(read_windows(Kind::Meet, &plain).speaking, ["Rowan Ellery"]);
}

#[test]
fn a_meet_speaker_is_named_only_once_the_user_is_known() {
    let unknown = stage(vec![
        tile("Juno Halvorsen", true),
        tile("Rowan Ellery", false),
    ]);
    let t = timeline(&every(0.0, 8.0, Some(read_windows(Kind::Meet, &unknown))));
    assert_eq!(t.name(1.0, 6.0), None);
    let known = stage(vec![tile("You", false), tile("Rowan Ellery", true)]);
    let t = timeline(&every(0.0, 8.0, Some(read_windows(Kind::Meet, &known))));
    assert_eq!(t.name(1.0, 6.0).as_deref(), Some("Rowan Ellery"));
}

#[test]
fn a_people_row_beside_the_stage_is_never_taken_for_the_users_tile() {
    let group = |children: Vec<Node>| Node {
        role: "AXGroup".into(),
        children,
        ..Node::default()
    };
    let text = |value: &str| Node {
        role: "AXStaticText".into(),
        value: Some(value.into()),
        ..Node::default()
    };
    let deep = |leaf: Node, by: usize| (0..by).fold(leaf, |node, _| group(vec![node]));
    let labelled = |role: &str, label: &str, children: Vec<Node>| Node {
        role: role.into(),
        description: Some(label.into()),
        children,
        ..Node::default()
    };
    let own = labelled("AXButton", "Backgrounds and effects", Vec::new());
    for by in 0..12 {
        let mut mine = deep(text("Juno Halvorsen"), 7);
        mine.children.push(own.clone());
        let mut other = deep(text("Rowan Ellery"), 7);
        other.classes = vec!["v5h6Xc".into()];
        let row = labelled(
            "AXGroup",
            "Rowan Ellery",
            vec![deep(text("Rowan Ellery"), by)],
        );
        let list = labelled("AXList", "Participants", vec![row]);
        let page = group(vec![group(vec![mine, other]), group(vec![list])]);
        let window = Window {
            title: "Meet - abc-defg-hij".into(),
            id: String::new(),
            tree: page,
        };
        let read = read_windows(Kind::Meet, &[window]);
        assert_eq!(read.me.as_deref(), Some("Juno Halvorsen"), "{by}");
        assert_eq!(read.speaking, ["Rowan Ellery"], "{by}");
    }
}

#[test]
fn a_meet_call_of_two_is_named_from_the_open_people_panel() {
    let read = read_windows(
        Kind::Meet,
        &windows(include_str!("fixtures/meet-live-people.json")),
    );
    assert!(read.speaking.is_empty());
    let t = timeline(&every(0.0, 6.0, Some(read)));
    assert_eq!(t.name(1.0, 5.0).as_deref(), Some("Rowan Ellery"));
}

#[test]
fn a_call_without_an_english_leave_button_can_still_be_chosen() {
    let window = |title: &str, tree: Node| Window {
        title: title.into(),
        id: String::new(),
        tree,
    };
    let tile = Node {
        role: "AXTabGroup".into(),
        description: Some(
            "Rowan Ellery, Bei Computeraudio Stummschaltung deaktiviert, Aktiver Sprecher".into(),
        ),
        ..Node::default()
    };
    let german = window("Zoom Meeting", tile);
    assert!(!is_call_tree(&german.tree));
    assert!(shows_call("us.zoom.xos", &german));
    let seen = [Seen {
        pid: 9,
        app: "us.zoom.xos".into(),
        title: german.title.clone(),
        page: page(&german),
        call: shows_call("us.zoom.xos", &german),
    }];
    assert!(pick(None, &seen).is_ok());
    let mut meet = windows(include_str!("fixtures/meet-chrome.json")).remove(0);
    assert!(shows_call("com.google.Chrome", &meet));
    meet.tree.children[0]
        .children
        .retain(|n| n.role != "AXButton");
    assert!(!is_call_tree(&meet.tree));
    assert!(shows_call("com.google.Chrome", &meet));
    let teams = windows(include_str!("fixtures/teams-mac.json")).remove(0);
    assert!(!shows_call("com.microsoft.teams2", &teams));
    let notes = window(
        "Meet the team - Docs",
        Node {
            role: "AXStaticText".into(),
            value: Some("Rowan Ellery".into()),
            ..Node::default()
        },
    );
    assert!(!shows_call("com.google.Chrome", &notes));
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
    let two = Reading {
        shown: Some(2),
        ..reading(
            &[],
            &["Juno Halvorsen", "Rowan Ellery"],
            Some("Juno Halvorsen"),
        )
    };
    let t = timeline(&every(0.0, 6.0, Some(two.clone())));
    assert_eq!(t.name(1.0, 5.0).as_deref(), Some("Rowan Ellery"));
    let unknown_me = Reading {
        me: None,
        ..two.clone()
    };
    assert_eq!(
        timeline(&every(0.0, 6.0, Some(unknown_me))).name(1.0, 5.0),
        None
    );
    for shown in [None, Some(3)] {
        let partial = Reading {
            shown,
            ..two.clone()
        };
        assert_eq!(
            timeline(&every(0.0, 6.0, Some(partial))).name(1.0, 5.0),
            None
        );
    }
}

#[test]
fn the_count_a_call_app_shows_is_read_only_when_it_agrees() {
    let list = |labels: &[&str]| {
        vec![Window {
            title: "Zoom Meeting".into(),
            id: String::new(),
            tree: Node {
                role: "AXWindow".into(),
                children: labels
                    .iter()
                    .map(|l| Node {
                        role: "AXStaticText".into(),
                        value: Some(l.to_string()),
                        ..Node::default()
                    })
                    .collect(),
                ..Node::default()
            },
        }]
    };
    assert_eq!(
        read_windows(Kind::Zoom, &list(&["Participants (2)"])).shown,
        Some(2)
    );
    assert_eq!(
        read_windows(Kind::Meet, &list(&["In this meeting (3)"])).shown,
        Some(3)
    );
    assert_eq!(
        read_windows(Kind::Zoom, &list(&["Participants (2)", "People (3)"])).shown,
        None
    );
    assert_eq!(
        read_windows(Kind::Zoom, &list(&["Talking points (2)"])).shown,
        None
    );
}

#[test]
fn a_zoom_speaker_is_named_only_once_the_user_is_known() {
    let tiles = |me_tagged: bool| {
        let mine = if me_tagged {
            "Juno Halvorsen (me), Computer audio muted"
        } else {
            "Juno Halvorsen, Computer audio muted"
        };
        vec![Window {
            title: "Zoom Meeting".into(),
            id: String::new(),
            tree: Node {
                role: "AXWindow".into(),
                children: [mine, "Rowan Ellery, Computer audio unmuted, Active speaker"]
                    .iter()
                    .map(|d| Node {
                        role: "AXTabGroup".into(),
                        description: Some(d.to_string()),
                        ..Node::default()
                    })
                    .collect(),
                ..Node::default()
            },
        }]
    };
    let unsure = read_windows(Kind::Zoom, &tiles(false));
    assert!(unsure.unmarked_self && unsure.me.is_none());
    assert_eq!(
        timeline(&every(0.0, 6.0, Some(unsure.clone()))).name(1.0, 5.0),
        None
    );
    let sure = read_windows(Kind::Zoom, &tiles(true));
    assert_eq!(sure.me.as_deref(), Some("Juno Halvorsen"));
    let mut samples = vec![(0.0, Some(sure))];
    samples.extend(every(0.4, 6.0, Some(unsure)));
    assert_eq!(
        timeline(&samples).name(1.0, 5.0).as_deref(),
        Some("Rowan Ellery")
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
fn the_computer_account_name_is_never_given_to_the_other_side() {
    let named = |own: Option<&str>, r: Reading| {
        let mut t = Timeline::owned_by(own);
        for (at, r) in every(0.0, 6.0, Some(r)) {
            t.heard(at, r.as_ref());
        }
        t.end();
        t.name(1.0, 5.0)
    };
    let juno = reading(&["Juno Halvorsen"], GROUP, None);
    assert_eq!(named(Some("juno  HALVORSEN"), juno.clone()), None);
    assert_eq!(
        named(Some("Juno"), juno.clone()).as_deref(),
        Some("Juno Halvorsen")
    );
    assert_eq!(named(None, juno).as_deref(), Some("Juno Halvorsen"));
    let two = |speaking: &[&str]| Reading {
        shown: Some(2),
        ..reading(speaking, &["Juno Halvorsen", "Rowan Ellery"], None)
    };
    assert_eq!(
        named(Some("Juno Halvorsen"), two(&["Juno Halvorsen"])),
        None,
        "the account name never stands in for the call app's own marker"
    );
    let marked = |me: &str| Reading {
        me: Some(me.into()),
        ..two(&["Juno Halvorsen"])
    };
    assert_eq!(
        named(Some("Juno Halvorsen"), marked("Juno Halvorsen")).as_deref(),
        Some("Rowan Ellery")
    );
    assert_eq!(
        named(Some("Rowan Ellery"), marked("Juno Halvorsen")),
        None,
        "the one other person is never the account's own name"
    );
    let unsure = Reading {
        unmarked_self: true,
        ..reading(&["Rowan Ellery"], GROUP, None)
    };
    assert_eq!(named(Some("Juno Halvorsen"), unsure), None);
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
fn a_run_still_open_when_leo_stops_is_kept_up_to_its_last_reading() {
    let mut t = Timeline::default();
    let mut text = String::new();
    for (at, r) in every(0.0, 6.0, Some(reading(&["Rowan Ellery"], GROUP, None))) {
        for mark in t.heard(at, r.as_ref()) {
            text += &(serde_json::to_string(&mark).unwrap() + "\n");
        }
    }
    let crashed = Timeline::parse(&text);
    assert_eq!(crashed.name(0.5, 5.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(crashed.name(0.0, 1.4).as_deref(), Some("Rowan Ellery"));
    let closed = t.end().unwrap();
    text += &(serde_json::to_string(&closed).unwrap() + "\n");
    let whole = Timeline::parse(&text);
    assert_eq!(whole.name(4.0, 7.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(whole.name(0.0, 4.0), t.name(0.0, 4.0));
}

#[test]
fn a_torn_last_line_does_not_lose_the_row_written_after_it() {
    let text = concat!(
        "{\"from\":1.0,\"to\":3.0,\"who\":\"Rowan Ellery\"}\n",
        "{\"from\":4.0,\"to\"",
        "{\"from\":5.0,\"to\":8.0,\"who\":\"Tamsin Okafor\"}\n",
    );
    let t = Timeline::parse(text);
    assert_eq!(t.name(1.0, 3.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(t.name(5.0, 8.0).as_deref(), Some("Tamsin Okafor"));
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
fn only_static_text_has_its_value_read() {
    assert!(keeps_value("AXStaticText"));
    for typed in [
        "AXTextField",
        "AXTextArea",
        "AXComboBox",
        "AXSecureTextField",
        "AXButton",
        "AXRow",
    ] {
        assert!(!keeps_value(typed), "{typed}");
    }
}

#[test]
fn long_text_is_clipped_on_a_character_boundary() {
    let long = "é".repeat(MOST_TEXT + 5);
    assert_eq!(clip(&long).unwrap().chars().count(), MOST_TEXT);
    assert_eq!(clip("   "), None);
}

#[test]
fn overlapping_rows_count_their_time_once_and_a_long_run_across_the_start_is_found() {
    let run = |from: f64, to: f64, who: &str| Mark::Run {
        from,
        to,
        who: who.into(),
        mark: false,
    };
    let rolled = Timeline::from_marks([
        run(0.0, 10.0, "Rowan Ellery"),
        run(5.0, 10.0, "Rowan Ellery"),
        run(10.0, 16.0, "Tamsin Okafor"),
    ]);
    assert_eq!(rolled.name_over(&[(0.0, 20.0)]), None);
    assert_eq!(
        rolled.name_over(&[(0.0, 8.0), (4.0, 9.0)]).as_deref(),
        Some("Rowan Ellery")
    );
    assert_eq!(
        rolled.name_over(&[(0.0, 6.0), (0.0, 6.0), (10.0, 16.0)]),
        None,
        "a span listed twice counts once"
    );
    let long = Timeline::from_marks([
        run(0.0, 30.0, "Rowan Ellery"),
        run(1.0, 2.0, "Tamsin Okafor"),
        run(3.0, 4.0, "Tamsin Okafor"),
        run(5.0, 6.0, "Tamsin Okafor"),
    ]);
    assert_eq!(long.name(20.0, 25.0).as_deref(), Some("Rowan Ellery"));
    assert_eq!(long.name(22.0, 22.0).as_deref(), Some("Rowan Ellery"));
}
