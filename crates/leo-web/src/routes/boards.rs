use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;

use crate::boards::{self, Board, Card, ColumnInput, Spot};
use crate::reply::error;
use crate::routes::insert;
use crate::routes::tasks::present;
use crate::AppState;

async fn blocking(
    state: &AppState,
    work: impl FnOnce(&std::path::Path) -> Response + Send + 'static,
) -> Response {
    let chats = state.chats.clone();
    super::off(move || work(&chats))
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

fn not_saved() -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Leo could not save the board. Check that Leo can write to its notes folder, then try again.",
    )
}

fn no_board() -> Response {
    error(StatusCode::NOT_FOUND, "That board is not there any more.")
}

fn no_card() -> Response {
    error(StatusCode::NOT_FOUND, "That card is not there any more.")
}

pub(crate) async fn list_boards(State(state): State<AppState>) -> Response {
    blocking(&state, move |chats| {
        let all = boards::boards(chats);
        let counts = boards::live_counts(chats);
        let listed: Vec<serde_json::Value> = all
            .into_iter()
            .map(|board| {
                let cards = counts.get(&board.id).copied().unwrap_or(0);
                let mut value = json!(board);
                value["cards"] = json!(cards);
                value
            })
            .collect();
        Json(json!({ "boards": listed })).into_response()
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct NewBoard {
    #[serde(default)]
    name: String,
    #[serde(default)]
    request: Option<String>,
}

pub(crate) async fn create_board(
    State(state): State<AppState>,
    Json(body): Json<NewBoard>,
) -> Response {
    let name = boards::clean_line(&body.name);
    let request = insert::requested(body.request);
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        if let Some(first) = request
            .as_deref()
            .and_then(|r| insert::remembered_creation("board", r))
        {
            return Json(first).into_response();
        }
        let board = boards::new_board(
            if name.is_empty() {
                "New board"
            } else {
                name.as_str()
            },
            Utc::now(),
        );
        if boards::put_board(chats, &board).is_err() {
            return not_saved();
        }
        let made = json!(board);
        if let Some(r) = &request {
            insert::remember_creation("board", r, &made);
        }
        (StatusCode::CREATED, Json(made)).into_response()
    })
    .await
}

pub(crate) async fn get_board(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(&state, move |chats| {
        let Some(board) = boards::board(chats, &id) else {
            return no_board();
        };
        let cards = boards::places_of(chats, &board);
        Json(json!({ "board": board, "cards": cards })).into_response()
    })
    .await
}

pub(crate) async fn get_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(&state, move |chats| match boards::card(chats, &id) {
        Some(card) => Json(json!(card)).into_response(),
        None => no_card(),
    })
    .await
}

pub(crate) async fn undelete_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        match boards::undelete_card(chats, &id, Utc::now()) {
            Ok(Some(card)) => Json(json!(card)).into_response(),
            Ok(None) => error(
                StatusCode::NOT_FOUND,
                "That card is no longer in leo's trash.",
            ),
            Err(_) => not_saved(),
        }
    })
    .await
}

#[derive(Deserialize, Default)]
pub(crate) struct BoardChange {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    columns: Option<Vec<ColumnInput>>,
    /// Where the cards of deleted columns go; the first column when missing.
    #[serde(default)]
    move_to: Option<String>,
}

pub(crate) async fn change_board(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<BoardChange>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let Some(before) = boards::board(chats, &id) else {
            return no_board();
        };
        let mut board = before.clone();
        if let Some(name) = change.name {
            let name = boards::clean_line(&name);
            if name.is_empty() {
                return error(StatusCode::BAD_REQUEST, "Give the board a name.");
            }
            board.name = name;
        }
        let mut leaving: Option<(String, HashSet<String>)> = None;
        if let Some(columns) = change.columns {
            let columns = match boards::clean_columns(columns) {
                Ok(columns) => columns,
                Err(message) => return error(StatusCode::BAD_REQUEST, message),
            };
            let kept: HashSet<String> = columns.iter().map(|c| c.id.clone()).collect();
            let removed: HashSet<String> = before
                .columns
                .iter()
                .filter(|c| !kept.contains(&c.id))
                .map(|c| c.id.clone())
                .collect();
            if !removed.is_empty() {
                let target = match change.move_to {
                    Some(target) if kept.contains(&target) => target,
                    Some(_) => {
                        return error(
                            StatusCode::BAD_REQUEST,
                            "Move the cards to a column that stays.",
                        )
                    }
                    None => columns[0].id.clone(),
                };
                leaving = Some((target, removed));
            }
            board.columns = columns;
        }
        let cards = boards::places_of(chats, &before);
        let now = Utc::now();
        board.updated_at = now;
        if boards::put_board(chats, &board).is_err() {
            return not_saved();
        }
        if let Some((target, removed)) = leaving {
            let moving: Vec<boards::Placed> = cards
                .into_iter()
                .filter(|c| removed.contains(&c.column))
                .collect();
            if boards::append_all(chats, &board, &target, moving, now).is_err() {
                return not_saved();
            }
        }
        Json(json!(board)).into_response()
    })
    .await
}

pub(crate) async fn delete_board(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let Some(board) = boards::board(chats, &id) else {
            return no_board();
        };
        match boards::remove_board(chats, &board, Utc::now()) {
            Ok(()) => {
                Json(json!({ "board": board, "undo": { "board": board.id } })).into_response()
            }
            Err(_) => not_saved(),
        }
    })
    .await
}

pub(crate) async fn undelete_board(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        match boards::undelete(chats, &id, Utc::now()) {
            Ok(Some(board)) => Json(json!({ "board": board })).into_response(),
            Ok(None) => error(
                StatusCode::NOT_FOUND,
                "That board is no longer in leo's trash.",
            ),
            Err(_) => not_saved(),
        }
    })
    .await
}

pub(crate) async fn undo_board_change(
    State(state): State<AppState>,
    Path((id, undo)): Path<(String, String)>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        match boards::undo_change(chats, &id, &undo, Utc::now()) {
            Ok(Some(board)) => Json(json!({ "board": board })).into_response(),
            Ok(None) => error(
                StatusCode::NOT_FOUND,
                "That change can no longer be undone.",
            ),
            Err(_) => not_saved(),
        }
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct Restore {
    board: Board,
    #[serde(default)]
    cards: Vec<Card>,
}

/// Undo of a board delete: the board and its cards come back as they were.
pub(crate) async fn restore_board(
    State(state): State<AppState>,
    Json(body): Json<Restore>,
) -> Response {
    let Restore { board, cards } = body;
    if !boards::valid_id(&board.id) {
        return error(StatusCode::BAD_REQUEST, "That board cannot be restored.");
    }
    let columns = match boards::clean_columns(
        board
            .columns
            .into_iter()
            .map(|c| ColumnInput {
                id: Some(c.id),
                name: c.name,
            })
            .collect(),
    ) {
        Ok(columns) => columns,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let now = Utc::now();
        let name = boards::clean_line(&board.name);
        let restored = Board {
            id: board.id,
            name: if name.is_empty() {
                "New board".into()
            } else {
                name
            },
            columns,
            created_at: board.created_at,
            updated_at: now,
            deleted: false,
        };
        if boards::put_board(chats, &restored).is_err() {
            return not_saved();
        }
        let first = restored.columns[0].id.clone();
        for card in cards {
            if card.board != restored.id || !boards::valid_id(&card.id) {
                continue;
            }
            let description = card.description.trim().to_string();
            let column = if restored.columns.iter().any(|c| c.id == card.column) {
                card.column
            } else {
                first.clone()
            };
            let back = Card {
                title: boards::clean_line(&card.title),
                subheading: boards::clean_line(&card.subheading),
                description,
                priority: boards::clean_priority(&card.priority),
                column,
                rank: if card.rank.is_finite() {
                    card.rank
                } else {
                    0.0
                },
                updated_at: now,
                deleted: false,
                ..card
            };
            if back.title.is_empty() {
                continue;
            }
            if boards::put_card(chats, &back).is_err() {
                return not_saved();
            }
        }
        let cards = boards::places_of(chats, &restored);
        Json(json!({ "board": restored, "cards": cards })).into_response()
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct NewCard {
    /// Given when Undo brings a deleted card back.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    column: Option<String>,
    title: String,
    #[serde(default)]
    subheading: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: String,
    /// Undo puts a card back exactly where it was.
    #[serde(default)]
    rank: Option<f64>,
    /// New cards go to the end of the column unless this is set.
    #[serde(default)]
    top: bool,
    #[serde(default)]
    request: Option<String>,
}

pub(crate) async fn create_card(
    State(state): State<AppState>,
    Path(board_id): Path<String>,
    Json(body): Json<NewCard>,
) -> Response {
    let title = boards::clean_line(&body.title);
    if title.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Give the card a title.");
    }
    let description = body.description.trim().to_string();
    let id = match body.id {
        Some(id) if boards::valid_id(&id) && id.starts_with('c') => id,
        Some(_) => return error(StatusCode::BAD_REQUEST, "That card id is not valid."),
        None => boards::new_id('c'),
    };
    let request = insert::requested(body.request).map(|r| (format!("card:{board_id}"), r));
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let Some(board) = boards::board(chats, &board_id) else {
            return no_board();
        };
        if let Some(first) = request
            .as_ref()
            .and_then(|(scope, r)| insert::remembered_creation(scope, r))
        {
            return Json(first).into_response();
        }
        if let Some(existing) = boards::card(chats, &id) {
            return Json(json!(existing)).into_response();
        }
        let column = match body.column {
            Some(column) if board.columns.iter().any(|c| c.id == column) => column,
            Some(_) => return error(StatusCode::BAD_REQUEST, "That column is not on this board."),
            None => match board.columns.first() {
                Some(column) => column.id.clone(),
                None => return error(StatusCode::BAD_REQUEST, "That board has no columns."),
            },
        };
        let now = Utc::now();
        let card = Card {
            id,
            board: board.id.clone(),
            column,
            rank: 0.0,
            title,
            subheading: boards::clean_line(&body.subheading),
            description,
            priority: boards::clean_priority(&body.priority),
            created_at: now,
            updated_at: now,
            deleted: false,
        };
        let saved = match body.rank.filter(|r| r.is_finite()) {
            Some(rank) => {
                let card = Card { rank, ..card };
                boards::put_card(chats, &card).map(|_| card)
            }
            None => {
                let spot = if body.top { Spot::Top } else { Spot::End };
                boards::settle(chats, &board, card, spot, now)
            }
        };
        match saved {
            Ok(card) => {
                let made = json!(card);
                if let Some((scope, r)) = &request {
                    insert::remember_creation(scope, r, &made);
                }
                (StatusCode::CREATED, Json(made)).into_response()
            }
            Err(_) => not_saved(),
        }
    })
    .await
}

#[derive(Deserialize, Default)]
pub(crate) struct CardChange {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    subheading: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<String>,
}

pub(crate) async fn change_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<CardChange>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let Some(mut card) = boards::card(chats, &id) else {
            return no_card();
        };
        if let Some(title) = change.title {
            let title = boards::clean_line(&title);
            if title.is_empty() {
                return error(StatusCode::BAD_REQUEST, "Give the card a title.");
            }
            card.title = title;
        }
        if let Some(subheading) = change.subheading {
            card.subheading = boards::clean_line(&subheading);
        }
        if let Some(description) = change.description {
            card.description = description.trim().to_string();
        }
        if let Some(priority) = change.priority {
            card.priority = boards::clean_priority(&priority);
        }
        card.updated_at = Utc::now();
        if boards::put_card(chats, &card).is_err() {
            return not_saved();
        }
        Json(json!(card)).into_response()
    })
    .await
}

#[derive(Deserialize, Default)]
pub(crate) struct MoveCard {
    #[serde(default)]
    board: Option<String>,
    #[serde(default)]
    column: Option<String>,
    /// Missing: the end of the column. null: the top. An id: just after that card.
    #[serde(default, deserialize_with = "present")]
    after: Option<Option<String>>,
}

pub(crate) async fn move_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<MoveCard>,
) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let Some(mut card) = boards::card(chats, &id) else {
            return no_card();
        };
        let Some(from) = boards::board(chats, &card.board) else {
            return no_board();
        };
        let to = match change.board.as_deref() {
            Some(other) if other != from.id => match boards::board(chats, other) {
                Some(board) => board,
                None => return no_board(),
            },
            _ => from.clone(),
        };
        let column = match change.column {
            Some(column) if to.columns.iter().any(|c| c.id == column) => column,
            Some(_) => return error(StatusCode::BAD_REQUEST, "That column is not on this board."),
            None if to.id == from.id && to.columns.iter().any(|c| c.id == card.column) => {
                card.column.clone()
            }
            None => {
                let name = from
                    .columns
                    .iter()
                    .find(|c| c.id == card.column)
                    .map(|c| c.name.as_str());
                match boards::matching_column(&to, name) {
                    Some(column) => column,
                    None => return no_board(),
                }
            }
        };
        card.board = to.id.clone();
        card.column = column;
        let spot = match &change.after {
            None => Spot::End,
            Some(None) => Spot::Top,
            Some(Some(after)) => Spot::After(after.as_str()),
        };
        match boards::settle(chats, &to, card, spot, Utc::now()) {
            Ok(card) => Json(json!(card)).into_response(),
            Err(_) => not_saved(),
        }
    })
    .await
}

pub(crate) async fn delete_card(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(&state, move |chats| {
        let _writing = boards::writing(chats);
        let Some(card) = boards::card(chats, &id) else {
            return no_card();
        };
        match boards::remove_card(chats, &card, Utc::now()) {
            Ok(()) => Json(json!(card)).into_response(),
            Err(_) => not_saved(),
        }
    })
    .await
}
