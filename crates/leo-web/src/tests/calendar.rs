use super::*;
use crate::calendar::tests::Fake;

const LINK: &str = "https://calendar.google.com/calendar/ical/me/private-abc/basic.ics";

fn feed() -> String {
    let start = (chrono::Utc::now() + chrono::Duration::hours(1)).format("%Y%m%dT%H%M%SZ");
    format!("BEGIN:VCALENDAR\nX-WR-CALNAME:Classes\nBEGIN:VEVENT\nUID:a\nSUMMARY:Algorithms lecture\nDTSTART:{start}\nDURATION:PT1H\nDESCRIPTION:Shortest paths\nEND:VEVENT\nEND:VCALENDAR\n")
}

fn add(state: &AppState, link: &str) -> Response {
    run(crate::calendar::add(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({ "link": link })).unwrap()),
    ))
}

#[test]
fn a_pasted_calendar_link_shows_upcoming_events_and_is_never_sent_back() {
    let (mut state, dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    fake.pages.lock().unwrap().insert(LINK.into(), feed());
    state.calendar_access = Some(fake.clone());

    let wrong = add(&state, "https://example.com/not-a-calendar.ics");
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert!(json_of(wrong)["error"]
        .as_str()
        .unwrap()
        .contains("could not open"));

    let page = json_of(add(&state, &LINK.replace("https://", "webcal://")));
    assert_eq!(page["connected"], true);
    assert_eq!(page["calendars"][0]["name"], "Classes");
    assert_eq!(page["events"][0]["title"], "Algorithms lecture");
    assert!(page["events"][0]["context"]
        .as_str()
        .unwrap()
        .contains("Shortest paths"));
    let everything = format!(
        "{page}{}{}",
        std::fs::read_to_string(dir.path().join("calendars.json")).unwrap(),
        String::from_utf8_lossy(&std::fs::read(dir.path().join("leo.db")).unwrap_or_default())
    );
    assert!(!dir.path().join("calendar-cache.json").exists());
    assert!(
        !everything.contains("private-abc"),
        "the secret link stays in the secret store"
    );
    assert_eq!(fake.secrets.lock().unwrap().len(), 1);
    assert_eq!(
        page["offer"], true,
        "offering to record is on once connected"
    );
    let off = json_of(run(crate::calendar::change(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"offer": false})).unwrap()),
    )));
    assert_eq!(off["offer"], false);
    assert_eq!(off["calendars"][0]["name"], "Classes");

    fake.pages.lock().unwrap().clear();
    let refreshed = json_of(run(crate::calendar::refresh_now(State(state.clone()))));
    assert!(
        refreshed["calendars"][0]["problem"].is_string(),
        "an unreachable calendar says so"
    );

    let id = page["calendars"][0]["id"].as_str().unwrap().to_string();
    let gone = json_of(run(crate::calendar::remove(State(state.clone()), Path(id))));
    assert_eq!(gone["connected"], false);
    assert!(fake.secrets.lock().unwrap().is_empty());
}

#[test]
fn the_schedule_shows_a_month_of_events_with_all_day_items_and_tasks_linked_by_marker() {
    let (mut state, _dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    let task = json_of(run(crate::routes::tasks::create_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"title": "Submit lab", "date": "2026-12-03", "time": "23:59", "kind": "assignment"})).unwrap()),
    )));
    let marker: String = task["id"].as_str().unwrap().chars().take(12).collect();
    let feed = format!("BEGIN:VCALENDAR\nX-WR-CALNAME:Classes\nBEGIN:VEVENT\nUID:final\nSUMMARY:Biology final\nDTSTART;VALUE=DATE:20261216\nEND:VEVENT\nBEGIN:VEVENT\nUID:lab\nSUMMARY:Due: Submit lab\nDTSTART:20261204T073000Z\nDURATION:PT30M\nDESCRIPTION:From leo · leo-task:{marker}\nEND:VEVENT\nEND:VCALENDAR\n");
    fake.pages.lock().unwrap().insert(LINK.into(), feed);
    state.calendar_access = Some(fake.clone());
    assert_eq!(add(&state, LINK).status(), StatusCode::OK);
    let window = |from: &str, to: &str| {
        run(crate::calendar::schedule(
            State(state.clone()),
            Query(
                serde_json::from_value(serde_json::json!({"from": from, "to": to, "live": "1"}))
                    .unwrap(),
            ),
        ))
    };
    let month = json_of(window(
        "2026-12-01T00:00:00-08:00",
        "2027-01-01T00:00:00-08:00",
    ));
    let events = month["events"].as_array().unwrap();
    let final_exam = events
        .iter()
        .find(|e| e["title"] == "Biology final")
        .unwrap();
    assert_eq!(final_exam["all_day"], true);
    assert_eq!(final_exam["date"], "2026-12-16");
    let lab = events
        .iter()
        .find(|e| e["title"] == "Due: Submit lab")
        .unwrap();
    assert_eq!(
        lab["task"], task["id"],
        "the event Leo opened in Google links back to its task"
    );
    assert!(
        lab["id"].as_str().unwrap().contains(':'),
        "event ids are scoped by calendar"
    );
    assert_eq!(month["tasks"][0]["title"], "Submit lab");
    assert_eq!(
        window("2026-12-01T00:00:00Z", "2027-03-01T00:00:00Z").status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        window("nonsense", "2027-01-01T00:00:00Z").status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn the_schedule_carries_every_task_of_its_window_and_the_count_of_them_all() {
    let (mut state, _dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    let make = |body: serde_json::Value| {
        json_of(run(crate::routes::tasks::create_task(
            State(state.clone()),
            Json(serde_json::from_value(body).unwrap()),
        )))
    };
    for i in 0..600 {
        make(serde_json::json!({"title": format!("Old {i:03}"), "date": "2020-01-01"}));
    }
    let repeating =
        make(serde_json::json!({"title": "Every week", "date": "2021-01-04", "repeat": [0, 2]}));
    let inside = make(serde_json::json!({"title": "In the window", "date": "2026-12-03"}));
    let next_day = make(serde_json::json!({"title": "Just after", "date": "2026-12-31"}));
    let beyond = make(serde_json::json!({"title": "Far off", "date": "2027-06-01"}));
    let marker: String = inside["id"].as_str().unwrap().chars().take(12).collect();
    let feed = format!("BEGIN:VCALENDAR\nX-WR-CALNAME:Classes\nBEGIN:VEVENT\nUID:lab\nSUMMARY:Due\nDTSTART:20261204T073000Z\nDURATION:PT30M\nDESCRIPTION:From leo · leo-task:{marker}\nEND:VEVENT\nEND:VCALENDAR\n");
    fake.pages.lock().unwrap().insert(LINK.into(), feed);
    state.calendar_access = Some(fake.clone());
    assert_eq!(add(&state, LINK).status(), StatusCode::OK);
    let month = json_of(run(crate::calendar::schedule(
        State(state.clone()),
        Query(
            serde_json::from_value(serde_json::json!({
                "from": "2026-12-01T00:00:00-08:00", "to": "2026-12-31T00:00:00-08:00", "live": "1"
            }))
            .unwrap(),
        ),
    )));
    assert_eq!(month["total"], 604);
    assert_eq!(month["tasks"].as_array().unwrap().len(), 500);
    let window: Vec<&str> = month["window"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    let ids = |t: &serde_json::Value| t["id"].as_str().unwrap().to_string();
    assert!(
        window.contains(&ids(&repeating).as_str()),
        "a repeating task"
    );
    assert!(
        window.contains(&ids(&inside).as_str()),
        "a task dated in it"
    );
    assert!(
        window.contains(&ids(&next_day).as_str()),
        "a day either side stays"
    );
    assert!(
        !window.contains(&ids(&beyond).as_str()),
        "a task far outside it"
    );
    assert_eq!(window.len(), 3);
    assert_eq!(
        month["events"][0]["task"], inside["id"],
        "an event links to a task past the first page"
    );
    let longer = json_of(run(crate::calendar::schedule(
        State(state.clone()),
        Query(
            serde_json::from_value(serde_json::json!({
                "from": "2026-12-01T00:00:00-08:00", "to": "2026-12-31T00:00:00-08:00", "limit": 601
            }))
            .unwrap(),
        ),
    )));
    assert_eq!(longer["tasks"].as_array().unwrap().len(), 601);
    assert_eq!(longer["window"].as_array().unwrap().len(), 2);
}

struct Flipping {
    inside: Arc<Fake>,
    settings: std::path::PathBuf,
}

impl crate::CalendarAccess for Flipping {
    fn fetch(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        self.inside.fetch(url)
    }
    fn get(&self, account: &str) -> anyhow::Result<Option<String>> {
        self.inside.get(account)
    }
    fn set(&self, account: &str, value: &str) -> anyhow::Result<()> {
        self.inside.set(account, value)
    }
    fn delete(&self, account: &str) -> anyhow::Result<()> {
        let mut saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.settings).unwrap()).unwrap();
        saved["offer"] = serde_json::json!(false);
        std::fs::write(&self.settings, saved.to_string()).unwrap();
        self.inside.delete(account)
    }
}

#[test]
fn removing_a_calendar_does_not_undo_a_switch_flipped_while_it_was_removed() {
    let (mut state, dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    fake.pages.lock().unwrap().insert(LINK.into(), feed());
    state.calendar_access = Some(fake.clone());
    let page = json_of(add(&state, LINK));
    let id = page["calendars"][0]["id"].as_str().unwrap().to_string();
    let settings = dir.path().join("calendars.json");
    state.calendar_access = Some(Arc::new(Flipping {
        inside: fake,
        settings: settings.clone(),
    }));
    let gone = json_of(run(crate::calendar::remove(State(state.clone()), Path(id))));
    assert_eq!(gone["connected"], false);
    assert_eq!(gone["offer"], false);
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(saved["offer"], false);
}

#[test]
fn eight_calendars_can_be_linked() {
    let (mut state, _dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    state.calendar_access = Some(fake.clone());
    let mut last = None;
    for n in 0..8 {
        let link = format!("https://calendar.example.com/ical/{n}/basic.ics");
        fake.pages.lock().unwrap().insert(link.clone(), feed());
        last = Some(json_of(add(&state, &link)));
    }
    assert_eq!(last.unwrap()["calendars"].as_array().unwrap().len(), 8);
}
