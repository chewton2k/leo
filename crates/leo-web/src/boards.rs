//! Boards: task boards with columns of cards, like Jira's. They are separate from
//! calendar tasks because cards have no dates.
//!
//! Each board and each card is its own file, `<notes>/.leo/boards/<id>.json` and
//! `<notes>/.leo/cards/<id>.json`, stored like standalone tasks: one JSON line per
//! save, written atomically, and read as the line with the newest `updated_at`.
//! Deleting writes a tombstone line; the file goes when the Trash period ends.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::jsonline::{load, read_all, write_file, Index, Line};

pub const MOST_COLUMNS: usize = 12;
pub const MOST_NAME: usize = 80;
pub const MOST_TITLE: usize = 300;
pub const MOST_SUBHEADING: usize = 200;
pub const MOST_DESCRIPTION: usize = 8_000;
pub const FACE_CHARS: usize = 240;
/// "" is no priority.
pub const PRIORITIES: [&str; 4] = ["", "low", "medium", "high"];
pub const DEFAULT_COLUMNS: [&str; 3] = ["To do", "In progress", "Done"];
/// The gap left between neighbouring cards, so most moves rewrite only the moved card.
const STEP: f64 = 1024.0;

/// One change at a time: a move reads a column's order and then writes it.
pub static WRITING: Mutex<()> = Mutex::new(());

pub fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Column {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub columns: Vec<Column>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Card {
    pub id: String,
    pub board: String,
    pub column: String,
    #[serde(default)]
    pub rank: f64,
    pub title: String,
    #[serde(default)]
    pub subheading: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

impl Line for Board {
    const DIR: &'static str = "boards";
    fn id(&self) -> &str {
        &self.id
    }
    fn stamp(&self) -> DateTime<Utc> {
        self.updated_at
    }
    fn deleted_at(&mut self, now: DateTime<Utc>) {
        self.deleted = true;
        self.updated_at = now;
    }
}

impl Line for Card {
    const DIR: &'static str = "cards";
    fn id(&self) -> &str {
        &self.id
    }
    fn stamp(&self) -> DateTime<Utc> {
        self.updated_at
    }
    fn deleted_at(&mut self, now: DateTime<Utc>) {
        self.deleted = true;
        self.updated_at = now;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(from = "Card")]
pub struct Placed {
    pub id: String,
    pub board: String,
    pub column: String,
    pub rank: f64,
    pub title: String,
    pub subheading: String,
    #[serde(rename = "description")]
    pub start: String,
    #[serde(rename = "description_more")]
    pub more: bool,
    pub priority: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip)]
    pub deleted: bool,
}

impl From<Card> for Placed {
    fn from(card: Card) -> Self {
        let mut rest = card.description.chars();
        let start: String = rest.by_ref().take(FACE_CHARS).collect();
        Self {
            id: card.id,
            board: card.board,
            column: card.column,
            rank: card.rank,
            title: card.title,
            subheading: card.subheading,
            start,
            more: rest.next().is_some(),
            priority: card.priority,
            created_at: card.created_at,
            updated_at: card.updated_at,
            deleted: card.deleted,
        }
    }
}

impl Line for Placed {
    const DIR: &'static str = "cards";
    fn id(&self) -> &str {
        &self.id
    }
    fn stamp(&self) -> DateTime<Utc> {
        self.updated_at
    }
    fn deleted_at(&mut self, now: DateTime<Utc>) {
        self.deleted = true;
        self.updated_at = now;
    }
}

pub trait Seat: Clone {
    fn seat_column(&self) -> &str;
    fn seat_on(&self) -> &str;
    fn seat_id(&self) -> &str;
    fn seat_rank(&self) -> f64;
    fn seat_born(&self) -> DateTime<Utc>;
    fn seat_move(&mut self, column: String);
}

macro_rules! seat {
    ($kind:ty) => {
        impl Seat for $kind {
            fn seat_column(&self) -> &str {
                &self.column
            }
            fn seat_on(&self) -> &str {
                &self.board
            }
            fn seat_id(&self) -> &str {
                &self.id
            }
            fn seat_rank(&self) -> f64 {
                self.rank
            }
            fn seat_born(&self) -> DateTime<Utc> {
                self.created_at
            }
            fn seat_move(&mut self, column: String) {
                self.column = column;
            }
        }
    };
}
seat!(Card);
seat!(Placed);

#[derive(Default)]
struct Tables {
    boards: Index<Board>,
    cards: Index<Placed>,
}

fn tables(chats: &Path) -> Arc<Tables> {
    static TABLES: OnceLock<Mutex<HashMap<PathBuf, Arc<Tables>>>> = OnceLock::new();
    let key = crate::db::beside_of(chats, None).to_path_buf();
    let mut all = TABLES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    Arc::clone(all.entry(key).or_default())
}

pub fn generation(chats: &Path) -> u64 {
    let held = tables(chats);
    held.boards
        .generation(chats)
        .wrapping_add(held.cards.generation(chats))
}

pub fn look_again(chats: &Path) {
    crate::jsonline::hub(chats).everything();
}

pub use crate::jsonline::valid_id;

pub fn new_id(prefix: char) -> String {
    format!("{prefix}{}", uuid::Uuid::new_v4().simple())
}

fn new_column_id() -> String {
    let id: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(12)
        .collect();
    format!("k{id}")
}

/// One line of text: runs of whitespace (line breaks too) become one space.
pub fn clean_line(text: &str, most: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(most)
        .collect()
}

pub fn description_of(text: &str) -> Result<String, &'static str> {
    let text = text.trim();
    if text.chars().count() > MOST_DESCRIPTION {
        return Err("Keep the description within 8,000 characters.");
    }
    Ok(text.to_string())
}

pub fn clean_priority(priority: &str) -> String {
    let priority = priority.trim().to_lowercase();
    if PRIORITIES.contains(&priority.as_str()) {
        priority
    } else {
        String::new()
    }
}

/// A column as the page sends it: a new one has no id yet.
#[derive(Debug, Clone, Deserialize)]
pub struct ColumnInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
}

pub fn clean_columns(input: Vec<ColumnInput>) -> Result<Vec<Column>, &'static str> {
    if input.is_empty() {
        return Err("A board needs at least one column.");
    }
    if input.len() > MOST_COLUMNS {
        return Err("A board can have up to 12 columns.");
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(input.len());
    for column in input {
        let id = match column.id {
            Some(id) if valid_id(&id) => id,
            Some(_) => return Err("That column id is not valid."),
            None => new_column_id(),
        };
        if !seen.insert(id.clone()) {
            return Err("Two columns have the same id.");
        }
        let name = clean_line(&column.name, MOST_NAME);
        out.push(Column {
            id,
            name: if name.is_empty() {
                "Untitled".into()
            } else {
                name
            },
        });
    }
    Ok(out)
}

pub fn new_board(name: &str, now: DateTime<Utc>) -> Board {
    Board {
        id: new_id('b'),
        name: name.to_string(),
        columns: DEFAULT_COLUMNS
            .iter()
            .map(|name| Column {
                id: new_column_id(),
                name: (*name).into(),
            })
            .collect(),
        created_at: now,
        updated_at: now,
        deleted: false,
    }
}

/// Boards, oldest first, so the tabs keep their places.
pub fn boards(chats: &Path) -> Vec<Board> {
    let mut all: Vec<Board> = tables(chats).boards.view(chats, |held| {
        held.values()
            .map(|(_, board)| board)
            .filter(|board| !board.deleted)
            .cloned()
            .collect()
    });
    all.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    all
}

pub fn forget_deleted(chats: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(days) = days else {
        return 0;
    };
    let before = now - chrono::Duration::days(i64::from(days));
    let held: HashSet<String> = tables(chats).cards.view(chats, |held| {
        held.values()
            .map(|(_, card)| card)
            .filter(|card| !card.deleted)
            .map(|card| card.board.clone())
            .collect()
    });
    let boards = crate::jsonline::forget_deleted::<Board>(chats, before, |b| {
        b.deleted && !held.contains(&b.id)
    });
    let waiting = deleted_boards(chats);
    boards
        + crate::jsonline::forget_deleted::<Card>(chats, before, |c| {
            c.deleted && !waiting.contains_key(&c.board)
        })
}

fn deleted_boards(chats: &Path) -> HashMap<String, DateTime<Utc>> {
    tables(chats).boards.view(chats, |held| {
        held.values()
            .map(|(_, board)| board)
            .filter(|board| board.deleted)
            .map(|board| (board.id.clone(), board.updated_at))
            .collect()
    })
}

pub fn finish_deletes(chats: &Path) -> usize {
    let deleted = deleted_boards(chats);
    if deleted.is_empty() {
        return 0;
    }
    let waiting: Vec<(String, DateTime<Utc>)> = tables(chats).cards.view(chats, |held| {
        held.values()
            .map(|(_, card)| card)
            .filter(|card| !card.deleted)
            .filter_map(|card| deleted.get(&card.board).map(|at| (card.id.clone(), *at)))
            .collect()
    });
    waiting
        .into_iter()
        .filter_map(|(id, at)| card(chats, &id).map(|card| (card, at)))
        .filter(|(card, at)| remove_card(chats, card, *at).is_ok())
        .count()
}

pub fn undelete(chats: &Path, id: &str, now: DateTime<Utc>) -> anyhow::Result<Option<Board>> {
    let Some(mut board) = load::<Board>(chats, id) else {
        return Ok(None);
    };
    if !board.deleted {
        return Ok(Some(board));
    }
    finish_deletes(chats);
    let stamp = board.updated_at;
    let deleted: Vec<String> = tables(chats).cards.view(chats, |held| {
        held.values()
            .map(|(_, card)| card)
            .filter(|c| c.board == board.id && c.deleted && c.updated_at == stamp)
            .map(|c| c.id.clone())
            .collect()
    });
    for id in deleted {
        if let Some(mut card) = load::<Card>(chats, &id) {
            card.deleted = false;
            card.updated_at = now;
            put_card(chats, &card)?;
        }
    }
    board.deleted = false;
    board.updated_at = now;
    put_board(chats, &board)?;
    Ok(Some(board))
}

pub fn undelete_card(chats: &Path, id: &str, now: DateTime<Utc>) -> anyhow::Result<Option<Card>> {
    let Some(mut card) = load::<Card>(chats, id) else {
        return Ok(None);
    };
    if !card.deleted {
        return Ok(Some(card));
    }
    if board(chats, &card.board).is_none() {
        return Ok(None);
    }
    card.deleted = false;
    card.updated_at = now;
    put_card(chats, &card)?;
    Ok(Some(card))
}

#[derive(Serialize, Deserialize)]
struct Change {
    name: String,
    columns: Vec<Column>,
    cards: Vec<(String, String, f64)>,
}

fn stamp_of(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn remember_change(
    chats: &Path,
    before: &Board,
    moved: &[(String, String, f64)],
    now: DateTime<Utc>,
) -> anyhow::Result<String> {
    let id = new_id('u');
    let data = serde_json::to_string(&Change {
        name: before.name.clone(),
        columns: before.columns.clone(),
        cards: moved.to_vec(),
    })?;
    crate::db::try_for_chats(chats)?.with(|c| {
        c.execute(
            "INSERT INTO board_undos (id, board, at, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, before.id, stamp_of(now), data],
        )
    })?;
    Ok(id)
}

pub fn undo_change(
    chats: &Path,
    board: &str,
    undo: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<Board>> {
    use rusqlite::OptionalExtension;
    let db = crate::db::try_for_chats(chats)?;
    let data: Option<String> = db.with(|c| {
        c.query_row(
            "SELECT data FROM board_undos WHERE id = ?1 AND board = ?2",
            [undo, board],
            |r| r.get(0),
        )
        .optional()
    })?;
    let (Some(data), Some(mut live)) = (data, self::board(chats, board)) else {
        return Ok(None);
    };
    let change: Change = serde_json::from_str(&data)?;
    live.name = change.name;
    live.columns = change.columns;
    live.updated_at = now;
    put_board(chats, &live)?;
    for (id, column, rank) in change.cards {
        if let Some(mut moved) = card(chats, &id).filter(|c| c.board == live.id) {
            moved.column = column;
            moved.rank = rank;
            moved.updated_at = now;
            put_card(chats, &moved)?;
        }
    }
    db.with(|c| c.execute("DELETE FROM board_undos WHERE id = ?1", [undo]))?;
    Ok(Some(live))
}

pub fn forget_changes(chats: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let days = days.unwrap_or(leo_core::keep::DEFAULT_TRASH_DAYS);
    let before = stamp_of(now - chrono::Duration::days(i64::from(days)));
    let Ok(db) = crate::db::try_for_chats(chats) else {
        return 0;
    };
    let Ok(named) = db.with(|c| {
        c.prepare("SELECT DISTINCT board FROM board_undos")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()
    }) else {
        return 0;
    };
    let gone: Vec<String> = named
        .into_iter()
        .filter(|b| load::<Board>(chats, b).is_none())
        .collect();
    db.with(|c| {
        let mut forgotten = c.execute("DELETE FROM board_undos WHERE at < ?1", [&before])?;
        for board in &gone {
            forgotten += c.execute("DELETE FROM board_undos WHERE board = ?1", [board])?;
        }
        Ok(forgotten)
    })
    .unwrap_or(0)
}

pub fn board(chats: &Path, id: &str) -> Option<Board> {
    tables(chats).boards.view(chats, |held| {
        held.get(id)
            .map(|(_, board)| board)
            .filter(|board| !board.deleted)
            .cloned()
    })
}

pub fn card(chats: &Path, id: &str) -> Option<Card> {
    load::<Card>(chats, id).filter(|c| !c.deleted)
}

pub fn all_cards(chats: &Path) -> Vec<Card> {
    read_all::<Card>(chats, None)
        .into_iter()
        .filter(|c| !c.deleted)
        .collect()
}

pub fn live_counts(chats: &Path) -> HashMap<String, usize> {
    tables(chats).cards.view(chats, |held| {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, card) in held.values().filter(|(_, card)| !card.deleted) {
            *counts.entry(card.board.clone()).or_default() += 1;
        }
        counts
    })
}

fn live_where(chats: &Path, wanted: impl Fn(&Placed) -> bool) -> Vec<Placed> {
    tables(chats).cards.view(chats, |held| {
        held.values()
            .map(|(_, card)| card)
            .filter(|card| !card.deleted && wanted(card))
            .cloned()
            .collect()
    })
}

/// A board's cards in order, as their faces. A card whose column is gone
/// (deleted on another computer) shows in the first column.
pub fn places_of(chats: &Path, board: &Board) -> Vec<Placed> {
    arranged(board, live_where(chats, |card| card.board == board.id))
}

/// A board's cards in order. A card whose column is gone (deleted on another
/// computer) shows in the first column; it is only rewritten when it moves.
pub fn cards_of(chats: &Path, board: &Board) -> Vec<Card> {
    cards_in(board, &all_cards(chats))
}

/// `cards_of` over cards already read, so a caller going through several boards
/// reads the card files once.
pub fn cards_in<S: Seat>(board: &Board, all: &[S]) -> Vec<S> {
    arranged(
        board,
        all.iter()
            .filter(|c| c.seat_on() == board.id)
            .cloned()
            .collect(),
    )
}

fn arranged<S: Seat>(board: &Board, mut cards: Vec<S>) -> Vec<S> {
    let first = board
        .columns
        .first()
        .map(|c| c.id.clone())
        .unwrap_or_default();
    for card in &mut cards {
        if !board.columns.iter().any(|c| c.id == card.seat_column()) {
            card.seat_move(first.clone());
        }
    }
    cards.sort_by(|a, b| {
        a.seat_rank()
            .total_cmp(&b.seat_rank())
            .then(a.seat_born().cmp(&b.seat_born()))
            .then(a.seat_id().cmp(b.seat_id()))
    });
    cards
}

fn column_places(chats: &Path, board: &Board, column: &str) -> Vec<Placed> {
    let first = board.columns.first().map(|c| c.id.as_str());
    let here = |card: &Placed| {
        card.board == board.id
            && (card.column == column
                || (first == Some(column) && !board.columns.iter().any(|c| c.id == card.column)))
    };
    arranged(board, live_where(chats, here))
}

pub fn put_board(chats: &Path, board: &Board) -> anyhow::Result<()> {
    write_file(chats, board)
}

pub fn put_card(chats: &Path, card: &Card) -> anyhow::Result<()> {
    anyhow::ensure!(card.rank.is_finite(), "That card's place is not valid.");
    write_file(chats, card)
}

/// Where a moved card goes within its column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spot<'a> {
    Top,
    End,
    After(&'a str),
}

/// A rank between two neighbours, or None when there is no room left between them.
fn between(prev: Option<f64>, next: Option<f64>) -> Option<f64> {
    let rank = match (prev, next) {
        (None, None) => STEP,
        (Some(p), None) => p + STEP,
        (None, Some(n)) => n - STEP,
        (Some(p), Some(n)) => (p + n) / 2.0,
    };
    let fits = match (prev, next) {
        (Some(p), Some(n)) => rank > p && rank < n,
        _ => true,
    };
    (fits && rank.is_finite() && rank.abs() < 1e12).then_some(rank)
}

/// Put `card` (already in its new board and column) at `spot` and save it. When
/// its neighbours are too close to split, the column is numbered afresh.
pub fn settle(
    chats: &Path,
    board: &Board,
    mut card: Card,
    spot: Spot,
    now: DateTime<Utc>,
) -> anyhow::Result<Card> {
    let others: Vec<Placed> = column_places(chats, board, &card.column)
        .into_iter()
        .filter(|c| c.id != card.id)
        .collect();
    let at = match spot {
        Spot::Top => 0,
        Spot::End => others.len(),
        Spot::After(id) => others
            .iter()
            .position(|c| c.id == id)
            .map_or(others.len(), |i| i + 1),
    };
    let prev = at.checked_sub(1).map(|i| others[i].rank);
    let next = others.get(at).map(|c| c.rank);
    card.updated_at = now;
    match between(prev, next) {
        Some(rank) => card.rank = rank,
        None => {
            for (i, other) in others.iter().enumerate() {
                let place = if i < at { i } else { i + 1 };
                let rank = (place as f64 + 1.0) * STEP;
                if other.rank != rank {
                    if let Some(mut other) = self::card(chats, &other.id) {
                        other.rank = rank;
                        other.updated_at = now;
                        put_card(chats, &other)?;
                    }
                }
            }
            card.rank = (at as f64 + 1.0) * STEP;
        }
    }
    put_card(chats, &card)?;
    Ok(card)
}

/// Move `cards` (in their current order) to the end of `column`, as when the
/// column they were in is deleted.
pub fn append_all(
    chats: &Path,
    board: &Board,
    column: &str,
    cards: Vec<Placed>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let moving: HashSet<String> = cards.iter().map(|c| c.id.clone()).collect();
    let last = column_places(chats, board, column)
        .iter()
        .filter(|c| !moving.contains(&c.id))
        .map(|c| c.rank)
        .fold(0.0_f64, f64::max);
    for (i, moved) in cards.into_iter().enumerate() {
        if let Some(mut card) = self::card(chats, &moved.id) {
            card.column = column.to_string();
            card.rank = last + (i as f64 + 1.0) * STEP;
            card.updated_at = now;
            put_card(chats, &card)?;
        }
    }
    Ok(())
}

/// The column a card lands in on another board: one with the same name, else the first.
pub fn matching_column(board: &Board, name: Option<&str>) -> Option<String> {
    let wanted = name.map(|n| n.trim().to_lowercase());
    board
        .columns
        .iter()
        .find(|c| wanted.as_deref() == Some(c.name.trim().to_lowercase().as_str()))
        .or_else(|| board.columns.first())
        .map(|c| c.id.clone())
}

pub fn remove_board(chats: &Path, board: &Board, now: DateTime<Utc>) -> anyhow::Result<()> {
    crate::jsonline::write_gone(chats, board, now)?;
    for placed in live_where(chats, |card| card.board == board.id) {
        if let Some(card) = card(chats, &placed.id) {
            let _ = remove_card(chats, &card, now);
        }
    }
    Ok(())
}

pub fn remove_card(chats: &Path, card: &Card, now: DateTime<Utc>) -> anyhow::Result<()> {
    anyhow::ensure!(card.rank.is_finite(), "That card's place is not valid.");
    crate::jsonline::write_gone(chats, card, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_in(board: &Board, column: usize, id: &str, rank: f64, now: DateTime<Utc>) -> Card {
        Card {
            id: id.into(),
            board: board.id.clone(),
            column: board.columns[column].id.clone(),
            rank,
            title: id.into(),
            subheading: String::new(),
            description: String::new(),
            priority: String::new(),
            created_at: now,
            updated_at: now,
            deleted: false,
        }
    }

    fn order(chats: &Path, board: &Board, column: usize) -> Vec<String> {
        cards_of(chats, board)
            .into_iter()
            .filter(|c| c.column == board.columns[column].id)
            .map(|c| c.id)
            .collect()
    }

    #[test]
    fn ranks_split_neighbours_and_run_out_only_when_they_meet() {
        assert_eq!(between(None, None), Some(STEP));
        assert_eq!(between(Some(1024.0), None), Some(2048.0));
        assert_eq!(between(None, Some(1024.0)), Some(0.0));
        assert_eq!(between(Some(1.0), Some(2.0)), Some(1.5));
        assert_eq!(between(Some(1.0), Some(1.0)), None);
        assert_eq!(between(Some(1.0), Some(1.0 + f64::EPSILON)), None);
    }

    #[test]
    fn a_move_lands_after_its_neighbour_and_renumbers_a_full_column() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = new_board("Work", now);
        put_board(&chats, &board).unwrap();
        for (id, rank) in [("card-0001", 1.0), ("card-0002", 1.0), ("card-0003", 3.0)] {
            put_card(&chats, &card_in(&board, 0, id, rank, now)).unwrap();
        }
        let moving = card(&chats, "card-0003").unwrap();
        settle(&chats, &board, moving, Spot::After("card-0001"), now).unwrap();
        assert_eq!(
            order(&chats, &board, 0),
            ["card-0001", "card-0003", "card-0002"],
            "equal neighbours are renumbered so the card still lands between them"
        );
        let mut across = card(&chats, "card-0001").unwrap();
        across.column = board.columns[2].id.clone();
        settle(&chats, &board, across, Spot::Top, now).unwrap();
        assert_eq!(order(&chats, &board, 2), ["card-0001"]);
        assert_eq!(order(&chats, &board, 0), ["card-0003", "card-0002"]);
    }

    #[test]
    fn a_card_settled_after_another_lands_just_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = new_board("Work", now);
        put_board(&chats, &board).unwrap();
        for (id, rank) in [("card-0001", 1.0), ("card-0002", 1.0)] {
            put_card(&chats, &card_in(&board, 0, id, rank, now)).unwrap();
        }
        let fresh = card_in(&board, 0, "card-0003", 0.0, now);
        settle(&chats, &board, fresh, Spot::After("card-0001"), now).unwrap();
        assert_eq!(
            order(&chats, &board, 0),
            ["card-0001", "card-0003", "card-0002"]
        );
    }

    #[test]
    fn files_merge_by_newest_line_and_lost_columns_fall_back_to_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = new_board("Home", now);
        put_board(&chats, &board).unwrap();
        let older = card_in(&board, 1, "card-0001", 1.0, now);
        let mut newer = older.clone();
        newer.title = "Renamed elsewhere".into();
        newer.updated_at = now + chrono::Duration::seconds(5);
        let file = dir.path().join("notes/.leo/cards/card-0001.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&newer).unwrap(),
                serde_json::to_string(&older).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(
            card(&chats, "card-0001").unwrap().title,
            "Renamed elsewhere"
        );

        let mut orphan = card_in(&board, 0, "card-0002", 2.0, now);
        orphan.column = "kgone0000000".into();
        put_card(&chats, &orphan).unwrap();
        assert_eq!(
            cards_of(&chats, &board)
                .iter()
                .find(|c| c.id == "card-0002")
                .unwrap()
                .column,
            board.columns[0].id
        );

        std::fs::write(
            dir.path().join("notes/.leo/cards/card-0009.json"),
            serde_json::to_string(&older).unwrap(),
        )
        .unwrap();
        assert!(
            all_cards(&chats)
                .iter()
                .all(|c| c.id != "card-0001" || c.title == "Renamed elsewhere"),
            "a file whose name is not its id is ignored"
        );
        remove_card(
            &chats,
            &card(&chats, "card-0002").unwrap(),
            now + chrono::Duration::seconds(9),
        )
        .unwrap();
        assert!(card(&chats, "card-0002").is_none());
        assert!(file.exists());
    }

    #[test]
    fn tombstones_never_crowd_out_live_items_and_temp_files_stay_out_of_backup() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        // More deleted boards than an earlier scan cap allowed files to be read.
        for _ in 0..210 {
            let mut gone = new_board("Gone", now);
            gone.deleted = true;
            put_board(&chats, &gone).unwrap();
        }
        let live = new_board("Still here", now);
        put_board(&chats, &live).unwrap();
        assert_eq!(boards(&chats).len(), 1);
        assert_eq!(boards(&chats)[0].id, live.id);

        let names: Vec<String> = std::fs::read_dir(dir.path().join("notes/.leo/boards"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| n.ends_with(".json") && !n.starts_with('.')),
            "no temporary file is left behind: {names:?}"
        );
    }

    #[test]
    fn old_board_and_card_tombstones_go_with_the_trash_period_and_live_ones_stay() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let live = new_board("Live", now);
        put_board(&chats, &live).unwrap();
        let mut gone = new_board("Gone", now);
        gone.deleted = true;
        put_board(&chats, &gone).unwrap();
        put_card(&chats, &card_in(&live, 0, "card-0001", 1.0, now)).unwrap();
        let mut dead = card_in(&live, 0, "card-0002", 2.0, now);
        dead.deleted = true;
        put_card(&chats, &dead).unwrap();
        let tomb = |kind: &str, id: &str| dir.path().join(format!("notes/.leo/{kind}/{id}.json"));
        assert_eq!(forget_deleted(&chats, None, now), 0);
        assert_eq!(forget_deleted(&chats, Some(30), now), 0);
        assert!(tomb("boards", &gone.id).exists() && tomb("cards", "card-0002").exists());
        let later = now + chrono::Duration::days(31);
        assert_eq!(forget_deleted(&chats, Some(30), later), 2);
        assert!(!tomb("boards", &gone.id).exists() && !tomb("cards", "card-0002").exists());
        assert!(tomb("boards", &live.id).exists() && tomb("cards", "card-0001").exists());
        assert!(board(&chats, &live.id).is_some() && card(&chats, "card-0001").is_some());
    }

    #[test]
    fn columns_are_cleaned_and_bounded() {
        let input = |names: &[&str]| -> Vec<ColumnInput> {
            names
                .iter()
                .map(|n| ColumnInput {
                    id: None,
                    name: (*n).into(),
                })
                .collect()
        };
        let cleaned = clean_columns(input(&["  Doing \n now ", ""])).unwrap();
        assert_eq!(cleaned[0].name, "Doing now");
        assert_eq!(cleaned[1].name, "Untitled");
        assert!(clean_columns(Vec::new()).is_err());
        assert!(clean_columns(input(&["a"; 13])).is_err());
        let twice = vec![
            ColumnInput {
                id: Some("kcolumn00001".into()),
                name: "A".into(),
            },
            ColumnInput {
                id: Some("kcolumn00001".into()),
                name: "B".into(),
            },
        ];
        assert!(clean_columns(twice).is_err());
        assert_eq!(clean_priority("HIGH"), "high");
        assert_eq!(clean_priority("urgent"), "");
    }

    fn trip(chats: &Path, cards: usize, now: DateTime<Utc>) -> Board {
        let board = new_board("Trip", now);
        put_board(chats, &board).unwrap();
        for i in 0..cards {
            let mut card = card_in(&board, 0, &format!("card-{i:04}"), i as f64, now);
            card.description = format!("Notes for card {i}");
            put_card(chats, &card).unwrap();
        }
        board
    }

    fn live_titles(chats: &Path, board: &Board) -> Vec<String> {
        let mut titles: Vec<String> = all_cards(chats)
            .into_iter()
            .filter(|c| c.board == board.id)
            .map(|c| c.title)
            .collect();
        titles.sort();
        titles
    }

    #[test]
    fn undo_brings_back_the_board_and_only_the_cards_deleted_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 3, now);
        remove_card(
            &chats,
            &card(&chats, "card-0000").unwrap(),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
        remove_board(&chats, &board, now + chrono::Duration::seconds(2)).unwrap();
        assert!(self::board(&chats, &board.id).is_none());
        assert!(live_titles(&chats, &board).is_empty());
        let back = undelete(&chats, &board.id, now + chrono::Duration::seconds(3))
            .unwrap()
            .unwrap();
        assert!(!back.deleted);
        assert!(self::board(&chats, &board.id).is_some());
        assert_eq!(live_titles(&chats, &board), ["card-0001", "card-0002"]);
        assert!(card(&chats, "card-0000").is_none());
    }

    #[test]
    fn a_second_undo_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 2, now);
        remove_board(&chats, &board, now).unwrap();
        let first = undelete(&chats, &board.id, now + chrono::Duration::seconds(1))
            .unwrap()
            .unwrap();
        let cards = all_cards(&chats);
        let second = undelete(&chats, &board.id, now + chrono::Duration::seconds(2))
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(all_cards(&chats).len(), cards.len());
        assert!(all_cards(&chats).iter().all(|c| cards.contains(c)));
        assert!(undelete(&chats, "b-not-there-at-all", now)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_delete_stopped_after_some_cards_is_finished_and_can_be_undone() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 5, now);
        let at = now + chrono::Duration::seconds(1);
        let mut gone = board.clone();
        gone.deleted = true;
        gone.updated_at = at;
        put_board(&chats, &gone).unwrap();
        for id in ["card-0000", "card-0001"] {
            remove_card(&chats, &card(&chats, id).unwrap(), at).unwrap();
        }
        assert_eq!(finish_deletes(&chats), 3);
        assert!(live_titles(&chats, &board).is_empty());
        assert_eq!(finish_deletes(&chats), 0);
        undelete(&chats, &board.id, at + chrono::Duration::seconds(1))
            .unwrap()
            .unwrap();
        let back = all_cards(&chats);
        assert_eq!(back.len(), 5);
        assert!(back
            .iter()
            .all(|c| c.description == format!("Notes for card {}", c.rank)));
    }

    #[cfg(unix)]
    #[test]
    fn a_card_that_cannot_be_marked_does_not_fail_the_delete() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 3, now);
        let cards = dir.path().join("notes/.leo/cards");
        std::fs::set_permissions(&cards, std::fs::Permissions::from_mode(0o555)).unwrap();
        let removed = remove_board(&chats, &board, now + chrono::Duration::seconds(1));
        let still = live_titles(&chats, &board);
        let back = undelete(&chats, &board.id, now + chrono::Duration::seconds(2));
        std::fs::set_permissions(&cards, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(removed.is_ok());
        assert_eq!(still.len(), 3);
        assert!(back.unwrap().is_some());
        assert!(self::board(&chats, &board.id).is_some());
        assert_eq!(live_titles(&chats, &board).len(), 3);
    }

    #[test]
    fn an_unfinished_delete_is_never_expired() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let long_ago = now - chrono::Duration::days(400);
        let board = trip(&chats, 2, long_ago);
        let mut gone = board.clone();
        gone.deleted = true;
        gone.updated_at = long_ago;
        put_board(&chats, &gone).unwrap();
        remove_card(&chats, &card(&chats, "card-0000").unwrap(), long_ago).unwrap();
        assert_eq!(forget_deleted(&chats, Some(30), now), 0);
        assert!(load::<Board>(&chats, &board.id).is_some());
        assert!(load::<Card>(&chats, "card-0000").is_some());
        assert_eq!(finish_deletes(&chats), 1);
        assert_eq!(forget_deleted(&chats, Some(30), now), 3);
        assert!(load::<Board>(&chats, &board.id).is_none());
        assert!(read_all::<Card>(&chats, None).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_delete_whose_first_write_fails_changes_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 2, now);
        let boards = dir.path().join("notes/.leo/boards");
        std::fs::set_permissions(&boards, std::fs::Permissions::from_mode(0o555)).unwrap();
        let removed = remove_board(&chats, &board, now + chrono::Duration::seconds(1));
        std::fs::set_permissions(&boards, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(removed.is_err());
        assert!(self::board(&chats, &board.id).is_some());
        assert_eq!(live_titles(&chats, &board).len(), 2);
    }

    #[test]
    fn undo_leaves_cards_added_since_where_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 1, now);
        let undo = remember_change(
            &chats,
            &board,
            &[("card-0000".into(), board.columns[0].id.clone(), 0.0)],
            now,
        )
        .unwrap();
        let mut changed = board.clone();
        changed.name = "Holiday".into();
        changed.columns.truncate(2);
        put_board(&chats, &changed).unwrap();
        let mut moved = card(&chats, "card-0000").unwrap();
        moved.column = board.columns[1].id.clone();
        moved.rank = 9.0;
        put_card(&chats, &moved).unwrap();
        put_card(&chats, &card_in(&board, 1, "card-late", 5.0, now)).unwrap();
        let back = undo_change(&chats, &board.id, &undo, now).unwrap().unwrap();
        assert_eq!(
            (back.name.as_str(), &back.columns),
            ("Trip", &board.columns)
        );
        let first = card(&chats, "card-0000").unwrap();
        assert_eq!(
            (first.column.as_str(), first.rank),
            (board.columns[0].id.as_str(), 0.0)
        );
        assert_eq!(
            card(&chats, "card-late").unwrap().column,
            board.columns[1].id
        );
    }

    #[test]
    fn an_undo_record_is_used_once() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = trip(&chats, 1, now);
        let undo = remember_change(&chats, &board, &[], now).unwrap();
        assert!(undo_change(&chats, "b-some-other-board", &undo, now)
            .unwrap()
            .is_none());
        assert!(undo_change(&chats, &board.id, &undo, now)
            .unwrap()
            .is_some());
        assert!(undo_change(&chats, &board.id, &undo, now)
            .unwrap()
            .is_none());
    }
}
