use super::*;
use crate::routes::boards::{
    change_board, change_card, create_board, create_card, delete_board, delete_card, get_board,
    get_card, list_boards, move_card, restore_board, undelete_board, undelete_card,
};

fn new_board(state: &AppState, name: &str) -> serde_json::Value {
    json_of(run(create_board(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({ "name": name })).unwrap()),
    )))
}

fn shown(state: &AppState, id: &str) -> serde_json::Value {
    json_of(run(get_board(State(state.clone()), Path(id.to_string()))))
}

fn add(state: &AppState, board: &str, body: serde_json::Value) -> Response {
    run(create_card(
        State(state.clone()),
        Path(board.to_string()),
        Json(serde_json::from_value(body).unwrap()),
    ))
}

fn moved(state: &AppState, card: &str, body: serde_json::Value) -> Response {
    run(move_card(
        State(state.clone()),
        Path(card.to_string()),
        Json(serde_json::from_value(body).unwrap()),
    ))
}

fn column(board: &serde_json::Value, at: usize) -> String {
    board["columns"][at]["id"].as_str().unwrap().to_string()
}

/// Titles of the cards in one column, in order.
fn titles_in(state: &AppState, board: &str, column: &str) -> Vec<String> {
    shown(state, board)["cards"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["column"] == column)
        .map(|c| c["title"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_new_board_has_three_columns_and_cards_go_where_they_are_dropped() {
    let (state, _d, _) = state_with(&[]);
    let board = new_board(&state, "  Coursework ");
    assert_eq!(board["name"], "Coursework");
    let names: Vec<&str> = board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["To do", "In progress", "Done"]);
    let id = board["id"].as_str().unwrap().to_string();
    let todo = column(&board, 0);
    let doing = column(&board, 1);

    let mut ids = Vec::new();
    for title in ["Read chapter 3", "Write lab report", "Email tutor"] {
        let response = add(
            &state,
            &id,
            serde_json::json!({ "title": title, "subheading": "Biology\nweek 4" }),
        );
        assert_eq!(response.status(), 201);
        ids.push(json_of(response)["id"].as_str().unwrap().to_string());
    }
    assert_eq!(
        titles_in(&state, &id, &todo),
        ["Read chapter 3", "Write lab report", "Email tutor"]
    );
    assert_eq!(
        shown(&state, &id)["cards"][0]["subheading"],
        "Biology week 4"
    );

    let response = moved(&state, &ids[2], serde_json::json!({ "after": null }));
    assert_eq!(response.status(), 200);
    assert_eq!(
        titles_in(&state, &id, &todo),
        ["Email tutor", "Read chapter 3", "Write lab report"]
    );
    moved(&state, &ids[1], serde_json::json!({ "column": doing }));
    moved(
        &state,
        &ids[0],
        serde_json::json!({ "column": doing, "after": ids[1] }),
    );
    assert_eq!(
        titles_in(&state, &id, &doing),
        ["Write lab report", "Read chapter 3"]
    );
    assert_eq!(titles_in(&state, &id, &todo), ["Email tutor"]);
    assert_eq!(
        moved(
            &state,
            &ids[0],
            serde_json::json!({ "column": "knotacolumn1" })
        )
        .status(),
        400
    );

    let listed = json_of(run(list_boards(State(state.clone()))));
    assert_eq!(listed["boards"][0]["cards"], 3);
}

#[test]
fn a_card_moved_to_another_board_keeps_a_column_of_the_same_name() {
    let (state, _d, _) = state_with(&[]);
    let home = new_board(&state, "Home");
    let work = new_board(&state, "Work");
    let (home_id, work_id) = (home["id"].as_str().unwrap(), work["id"].as_str().unwrap());
    let card = json_of(add(
        &state,
        home_id,
        serde_json::json!({ "title": "Fix the shelf", "column": column(&home, 1) }),
    ));
    let card_id = card["id"].as_str().unwrap();
    let there = json_of(moved(
        &state,
        card_id,
        serde_json::json!({ "board": work_id }),
    ));
    assert_eq!(there["board"], work_id);
    assert_eq!(
        there["column"],
        column(&work, 1).as_str(),
        "In progress stays In progress"
    );
    assert!(shown(&state, home_id)["cards"]
        .as_array()
        .unwrap()
        .is_empty());

    let renamed = json_of(run(change_board(
        State(state.clone()),
        Path(work_id.to_string()),
        Json(
            serde_json::from_value(
                serde_json::json!({ "columns": [{ "id": column(&work, 0), "name": "Backlog" }] }),
            )
            .unwrap(),
        ),
    )));
    let first = column(&work, 0);
    assert_eq!(renamed["columns"].as_array().unwrap().len(), 1);
    assert_eq!(
        titles_in(&state, work_id, &first),
        ["Fix the shelf"],
        "a deleted column's cards move to the column left"
    );
    let back = json_of(moved(
        &state,
        card_id,
        serde_json::json!({ "board": home_id }),
    ));
    assert_eq!(
        back["column"],
        column(&home, 0).as_str(),
        "no Backlog on Home, so the first column"
    );
}

#[test]
fn deleting_columns_moves_their_cards_and_a_board_needs_one_column() {
    let (state, _d, _) = state_with(&[]);
    let board = new_board(&state, "Plan");
    let id = board["id"].as_str().unwrap();
    for (title, at) in [("A", 0), ("B", 1), ("C", 1), ("D", 2)] {
        add(
            &state,
            id,
            serde_json::json!({ "title": title, "column": column(&board, at) }),
        );
    }
    let change = |body: serde_json::Value| {
        run(change_board(
            State(state.clone()),
            Path(id.to_string()),
            Json(serde_json::from_value(body).unwrap()),
        ))
    };
    assert_eq!(change(serde_json::json!({ "columns": [] })).status(), 400);
    assert_eq!(change(serde_json::json!({ "name": "  " })).status(), 400);
    let keep = |at: usize| serde_json::json!({ "id": column(&board, at), "name": board["columns"][at]["name"] });
    assert_eq!(
        change(serde_json::json!({ "columns": [keep(0), keep(2)], "move_to": column(&board, 1) }))
            .status(),
        400,
        "the cards cannot move to the column being deleted"
    );
    let changed = json_of(change(serde_json::json!({
        "name": "Plan B",
        "columns": [keep(2), keep(0), { "name": "Waiting" }],
        "move_to": column(&board, 2),
    })));
    assert_eq!(changed["name"], "Plan B");
    assert_eq!(changed["columns"][0]["name"], "Done");
    assert_eq!(changed["columns"][2]["name"], "Waiting");
    assert!(changed["columns"][2]["id"].as_str().unwrap().len() >= 8);
    assert_eq!(titles_in(&state, id, &column(&board, 2)), ["D", "B", "C"]);
}

#[test]
fn cards_are_edited_validated_and_deleted_with_undo() {
    let (state, _d, _) = state_with(&[]);
    let board = new_board(&state, "Edits");
    let id = board["id"].as_str().unwrap();
    assert_eq!(
        add(&state, id, serde_json::json!({ "title": "   " })).status(),
        400
    );
    assert_eq!(
        add(
            &state,
            id,
            serde_json::json!({ "title": "x", "column": "knotacolumn1" })
        )
        .status(),
        400
    );
    assert_eq!(
        add(
            &state,
            id,
            serde_json::json!({ "title": "x", "description": "y".repeat(8_001) })
        )
        .status(),
        400
    );
    assert_eq!(
        add(&state, "bmissing0000", serde_json::json!({ "title": "x" })).status(),
        404
    );
    let card = json_of(add(
        &state,
        id,
        serde_json::json!({ "title": "Draft", "priority": "HIGH" }),
    ));
    assert_eq!(card["priority"], "high");
    let card_id = card["id"].as_str().unwrap().to_string();
    let edit = |body: serde_json::Value| {
        run(change_card(
            State(state.clone()),
            Path(card_id.clone()),
            Json(serde_json::from_value(body).unwrap()),
        ))
    };
    let edited = json_of(edit(
        serde_json::json!({ "description": "  Steps:\n1. Outline\n2. Write  ", "priority": "urgent", "subheading": "Essay" }),
    ));
    assert_eq!(
        edited["description"], "Steps:\n1. Outline\n2. Write",
        "a description keeps its lines"
    );
    assert_eq!(edited["priority"], "", "an unknown priority means none");
    assert_eq!(edit(serde_json::json!({ "title": "" })).status(), 400);

    let gone = json_of(run(delete_card(
        State(state.clone()),
        Path(card_id.clone()),
    )));
    assert!(shown(&state, id)["cards"].as_array().unwrap().is_empty());
    let back = add(&state, id, gone);
    assert_eq!(back.status(), 201);
    let back = json_of(back);
    assert_eq!(
        (back["id"].as_str(), back["description"].as_str()),
        (Some(card_id.as_str()), Some("Steps:\n1. Outline\n2. Write"))
    );
    assert_eq!(
        run(delete_card(
            State(state.clone()),
            Path("cmissing0000".into())
        ))
        .status(),
        404
    );
}

#[test]
fn a_deleted_board_takes_its_cards_and_undo_brings_both_back() {
    let (state, _d, _) = state_with(&[]);
    let board = new_board(&state, "Trip");
    let id = board["id"].as_str().unwrap().to_string();
    add(
        &state,
        &id,
        serde_json::json!({ "title": "Book train", "column": column(&board, 1) }),
    );
    let card = shown(&state, &id)["cards"][0].clone();
    let gone = json_of(run(delete_board(State(state.clone()), Path(id.clone()))));
    assert_eq!(gone["undo"]["board"], id.as_str());
    assert!(gone.get("cards").is_none());
    assert_eq!(
        run(get_board(State(state.clone()), Path(id.clone()))).status(),
        404
    );
    assert!(json_of(run(list_boards(State(state.clone()))))["boards"]
        .as_array()
        .unwrap()
        .is_empty());

    let back = json_of(run(undelete_board(State(state.clone()), Path(id.clone()))));
    assert_eq!(back["board"]["name"], "Trip");
    assert_eq!(titles_in(&state, &id, &column(&board, 1)), ["Book train"]);

    run(delete_board(State(state.clone()), Path(id.clone())));
    let back = json_of(run(restore_board(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({ "board": gone["board"], "cards": [card] }))
                .unwrap(),
        ),
    )));
    assert_eq!(back["board"]["name"], "Trip");
    assert_eq!(titles_in(&state, &id, &column(&board, 1)), ["Book train"]);
    assert!(
        state
            .chats
            .parent()
            .unwrap()
            .join("notes/.leo/cards")
            .read_dir()
            .unwrap()
            .count()
            == 1,
        "the card's file is reused, not copied"
    );
}

#[test]
fn a_repeated_request_id_creates_one_card_and_a_move_ignores_it() {
    let (state, _d, _) = state_with(&[]);
    let board = new_board(&state, "Coursework");
    let id = board["id"].as_str().unwrap().to_string();
    let (todo, doing) = (column(&board, 0), column(&board, 1));
    let request = uuid::Uuid::new_v4().to_string();
    let first = json_of(add(
        &state,
        &id,
        serde_json::json!({"title": "Essay", "column": todo, "request": request}),
    ));
    let again = json_of(add(
        &state,
        &id,
        serde_json::json!({"title": "Essay", "column": todo, "request": request}),
    ));
    assert_eq!(first["id"], again["id"]);
    assert_eq!(titles_in(&state, &id, &todo), ["Essay"]);
    json_of(add(
        &state,
        &id,
        serde_json::json!({"title": "Reading", "column": todo}),
    ));
    let card = first["id"].as_str().unwrap();
    let to_doing =
        serde_json::json!({"column": doing, "request": uuid::Uuid::new_v4().to_string()});
    json_of(moved(&state, card, to_doing.clone()));
    moved(&state, card, serde_json::json!({"column": todo}));
    json_of(moved(&state, card, to_doing));
    assert_eq!(titles_in(&state, &id, &doing), ["Essay"]);
}

#[test]
fn an_expired_board_cannot_be_undone() {
    let (state, _d, _) = state_with(&[]);
    let board = new_board(&state, "Old");
    let id = board["id"].as_str().unwrap().to_string();
    run(delete_board(State(state.clone()), Path(id.clone())));
    crate::boards::forget_deleted(
        &state.chats,
        Some(30),
        chrono::Utc::now() + chrono::Duration::days(31),
    );
    let response = run(undelete_board(State(state.clone()), Path(id)));
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_of(response)["error"],
        "That board is no longer in leo's trash."
    );
}

#[test]
fn a_very_large_board_deletes_and_comes_back() {
    let (state, _d, _) = state_with(&[]);
    let board = super::felix::big_board(&state, 3);
    let gone = run(delete_board(State(state.clone()), Path(board.id.clone())));
    assert_eq!(gone.status(), StatusCode::OK);
    let gone = json_of(gone);
    assert!(gone.to_string().len() < 2048, "{}", gone.to_string().len());
    let back = run(undelete_board(State(state.clone()), Path(board.id.clone())));
    assert_eq!(back.status(), StatusCode::OK);
    let cards = shown(&state, &board.id)["cards"].as_array().unwrap().len();
    assert_eq!(cards, 300);
}

fn written(state: &AppState, board: &crate::boards::Board, count: usize, text: usize) {
    let now = chrono::Utc::now();
    for number in 0..count {
        let card = crate::boards::Card {
            id: format!("cface{number:05}"),
            board: board.id.clone(),
            column: board.columns[0].id.clone(),
            rank: (number + 1) as f64 * 1024.0,
            title: format!("Card {number}"),
            subheading: String::new(),
            description: format!("{number} ").repeat(text / 2),
            priority: String::new(),
            created_at: now,
            updated_at: now,
            deleted: false,
        };
        crate::boards::put_card(&state.chats, &card).unwrap();
    }
}

fn reads(work: impl FnOnce()) -> usize {
    crate::jsonline::READS.with(|reads| reads.set(0));
    work();
    crate::jsonline::READS.with(|reads| reads.get())
}

#[test]
fn a_move_reads_one_card_file() {
    let (state, _d, _) = state_with(&[]);
    let board = crate::boards::new_board("Many", chrono::Utc::now());
    crate::boards::put_board(&state.chats, &board).unwrap();
    written(&state, &board, 200, 40);
    let _ = crate::boards::places_of(&state.chats, &board);
    let read = reads(|| {
        let card = crate::boards::card(&state.chats, "cface00100").unwrap();
        crate::boards::settle(
            &state.chats,
            &board,
            card,
            crate::boards::Spot::After("cface00150"),
            chrono::Utc::now(),
        )
        .unwrap();
    });
    assert_eq!(read, 1);
    let order: Vec<String> = crate::boards::places_of(&state.chats, &board)
        .into_iter()
        .map(|c| c.id)
        .collect();
    let at = order.iter().position(|id| id == "cface00100").unwrap();
    assert_eq!(order[at - 1], "cface00150");
}

#[test]
fn a_board_answer_carries_card_faces_not_descriptions() {
    let (state, _d, _) = state_with(&[]);
    let board = crate::boards::new_board("Faces", chrono::Utc::now());
    crate::boards::put_board(&state.chats, &board).unwrap();
    written(&state, &board, 50, 8_000);
    let shown = shown(&state, &board.id);
    let cards = shown["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 50);
    for card in cards {
        assert_eq!(card["description"].as_str().unwrap().chars().count(), 240);
        assert_eq!(card["description_more"], true);
    }
    let read = reads(|| {
        let _ = crate::boards::places_of(&state.chats, &board);
    });
    assert_eq!(read, 0);
}

#[test]
fn opening_a_card_reads_its_whole_description() {
    let (state, _d, _) = state_with(&[]);
    let board = crate::boards::new_board("Open", chrono::Utc::now());
    crate::boards::put_board(&state.chats, &board).unwrap();
    written(&state, &board, 1, 8_000);
    let opened = json_of(run(get_card(
        State(state.clone()),
        Path("cface00000".into()),
    )));
    assert_eq!(
        opened["description"].as_str().unwrap().chars().count(),
        8_000 / 2 * 2
    );
    let missing = run(get_card(State(state.clone()), Path("cnone000000".into())));
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[test]
fn a_deleted_card_comes_back_whole() {
    let (state, _d, _) = state_with(&[]);
    let board = crate::boards::new_board("Back", chrono::Utc::now());
    crate::boards::put_board(&state.chats, &board).unwrap();
    written(&state, &board, 1, 8_000);
    let gone = run(delete_card(State(state.clone()), Path("cface00000".into())));
    assert_eq!(gone.status(), StatusCode::OK);
    assert!(shown(&state, &board.id)["cards"]
        .as_array()
        .unwrap()
        .is_empty());
    let back = run(undelete_card(
        State(state.clone()),
        Path("cface00000".into()),
    ));
    assert_eq!(back.status(), StatusCode::OK);
    let card = crate::boards::card(&state.chats, "cface00000").unwrap();
    assert_eq!(card.description.chars().count(), 8_000 / 2 * 2);
    assert_eq!(
        shown(&state, &board.id)["cards"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn a_board_deleted_and_undone_keeps_every_cards_title_and_description() {
    let (state, _d, _) = state_with(&[]);
    let board = crate::boards::new_board("Round", chrono::Utc::now());
    crate::boards::put_board(&state.chats, &board).unwrap();
    written(&state, &board, 20, 8_000);
    let before = crate::boards::all_cards(&state.chats);
    let gone = run(delete_board(State(state.clone()), Path(board.id.clone())));
    assert_eq!(gone.status(), StatusCode::OK);
    let back = run(undelete_board(State(state.clone()), Path(board.id.clone())));
    assert_eq!(back.status(), StatusCode::OK);
    let mut after = crate::boards::all_cards(&state.chats);
    let mut before = before;
    after.sort_by(|a, b| a.id.cmp(&b.id));
    before.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(after.len(), 20);
    for (was, now) in before.iter().zip(&after) {
        assert_eq!(
            (&was.title, &was.description),
            (&now.title, &now.description)
        );
    }
}

#[test]
fn the_hundred_and_first_board_and_a_card_past_five_thousand_are_accepted() {
    let (state, _d, _) = state_with(&[]);
    let mut first = serde_json::Value::Null;
    for number in 0..101 {
        let made = run(create_board(
            State(state.clone()),
            Json(
                serde_json::from_value(serde_json::json!({ "name": format!("Board {number}") }))
                    .unwrap(),
            ),
        ));
        assert_eq!(made.status(), StatusCode::CREATED, "board {number}");
        if number == 0 {
            first = json_of(made);
        }
    }
    assert_eq!(crate::boards::boards(&state.chats).len(), 101);
    let board = crate::boards::board(&state.chats, first["id"].as_str().unwrap()).unwrap();
    let folder = _d.path().join("notes/.leo/cards");
    std::fs::create_dir_all(&folder).unwrap();
    for number in 0..5_000 {
        let id = format!("cmany{number:05}");
        let line = bench_card_line(
            &id,
            &board.id,
            &board.columns[0].id,
            number as f64,
            false,
            10,
        );
        std::fs::write(folder.join(format!("{id}.json")), line).unwrap();
    }
    let last = add(
        &state,
        &board.id,
        serde_json::json!({ "title": "Number 5001" }),
    );
    assert_eq!(last.status(), StatusCode::CREATED);
    assert_eq!(crate::boards::live_counts(&state.chats)[&board.id], 5_001);
}

fn bench_card_line(
    id: &str,
    board: &str,
    column: &str,
    rank: f64,
    gone: bool,
    size: usize,
) -> String {
    let now = chrono::Utc::now().to_rfc3339();
    serde_json::json!({
        "id": id, "board": board, "column": column, "rank": rank,
        "title": format!("Card {id}"), "subheading": "Sub", "description": "d".repeat(size),
        "priority": "", "created_at": now, "updated_at": now, "deleted": gone,
    })
    .to_string()
        + "\n"
}

#[test]
#[ignore = "benchmark: run by hand with --release --ignored --nocapture"]
fn benchmark_card_moves_at_scale() {
    let sizes: Vec<usize> = std::env::var("LEO_BENCH_CARDS")
        .unwrap_or_else(|_| "5000,50000".into())
        .split(',')
        .map(|n| n.parse().unwrap())
        .collect();
    let size: usize = std::env::var("LEO_BENCH_DESC").map_or(1000, |n| n.parse().unwrap());
    let watching = std::env::var("LEO_BENCH_WATCH").is_ok_and(|v| v == "1");
    let time = |runs: usize, work: &mut dyn FnMut()| {
        let start = std::time::Instant::now();
        for _ in 0..runs {
            work();
        }
        start.elapsed().as_secs_f64() * 1000.0 / runs as f64
    };
    for n in sizes {
        let (state, dir, _) = state_with(&[]);
        let small = new_board(&state, "Small");
        let big = new_board(&state, "Big");
        let (small_id, big_id) = (
            small["id"].as_str().unwrap().to_string(),
            big["id"].as_str().unwrap().to_string(),
        );
        let cols = [column(&small, 0), column(&small, 1)];
        let big_col = column(&big, 0);
        let cards = dir.path().join("notes/.leo/cards");
        std::fs::create_dir_all(&cards).unwrap();
        for i in 0..n {
            let id = format!("cbench{i:06}");
            let line = if i < 10 {
                bench_card_line(
                    &id,
                    &small_id,
                    &cols[0],
                    (i + 1) as f64 * 1024.0,
                    false,
                    size,
                )
            } else {
                bench_card_line(&id, &big_id, &big_col, i as f64, i % 5 == 0, size)
            };
            std::fs::write(cards.join(format!("{id}.json")), line).unwrap();
        }
        let _changes = watching.then(|| crate::store_refresh::Changes::new(&state.notes_dir));
        let mut moves = 0usize;
        let mut move_once = || {
            let to = &cols[(moves + 1) % 2];
            moves += 1;
            let response = moved(&state, "cbench000003", serde_json::json!({ "column": to }));
            assert_eq!(response.status(), 200);
        };
        let first_move = time(1, &mut move_once);
        let move_ms = time(5, &mut move_once);
        let mut made = 0usize;
        let mut created = 0u16;
        let mut create_once = || {
            made += 1;
            let response = add(
                &state,
                &small_id,
                serde_json::json!({ "title": format!("New {made}") }),
            );
            created = response.status().as_u16();
        };
        let create_ms = time(5, &mut create_once);
        let mut answer = 0usize;
        let mut board_once = || {
            let response = run(get_board(State(state.clone()), Path(big_id.clone())));
            answer = run(axum::body::to_bytes(response.into_body(), usize::MAX))
                .unwrap()
                .len();
        };
        let board_ms = time(3, &mut board_once);
        println!(
            "bench watcher={watching} cards={n} desc={size} first_move_ms={first_move:.2} move_ms={move_ms:.2} create_card_ms={create_ms:.2} create_status={created} board_answer_ms={board_ms:.2} board_answer_bytes={answer}"
        );
    }
}
