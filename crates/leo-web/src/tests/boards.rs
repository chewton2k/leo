use super::*;
use crate::routes::boards::{
    change_board, change_card, create_board, create_card, delete_board, delete_card, get_board,
    list_boards, move_card, restore_board, undelete_board,
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
