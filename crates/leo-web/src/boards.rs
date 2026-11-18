//! Boards: task boards with columns of cards, like Jira's. They are separate from
//! calendar tasks because cards have no dates.
//!
//! Each board and each card is its own file, `<notes>/.leo/boards/<id>.json` and
//! `<notes>/.leo/cards/<id>.json`, stored like standalone tasks: one JSON line per
//! save, written atomically, and read as the line with the newest `updated_at`.
//! Deleting writes a tombstone line; the file goes when the Trash period ends.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::jsonline::{load, read_all, write_file, Line};

pub const MOST_BOARDS: usize = 100;
pub const MOST_COLUMNS: usize = 12;
pub const MOST_CARDS: usize = 5_000;
pub const MOST_NAME: usize = 80;
pub const MOST_TITLE: usize = 300;
pub const MOST_SUBHEADING: usize = 200;
pub const MOST_DESCRIPTION: usize = 8_000;
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
}

impl Line for Card {
    const DIR: &'static str = "cards";
    fn id(&self) -> &str {
        &self.id
    }
    fn stamp(&self) -> DateTime<Utc> {
        self.updated_at
    }
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
    let mut all: Vec<Board> = read_all::<Board>(chats, None)
        .into_iter()
        .filter(|b| !b.deleted)
        .collect();
    all.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    all.truncate(MOST_BOARDS);
    all
}

pub fn forget_deleted(chats: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(days) = days else {
        return 0;
    };
    let before = now - chrono::Duration::days(i64::from(days));
    crate::jsonline::forget_deleted::<Board>(chats, before, |b| b.deleted)
        + crate::jsonline::forget_deleted::<Card>(chats, before, |c| c.deleted)
}

pub fn board(chats: &Path, id: &str) -> Option<Board> {
    load::<Board>(chats, id).filter(|b| !b.deleted)
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

/// A board's cards in order. A card whose column is gone (deleted on another
/// computer) shows in the first column; it is only rewritten when it moves.
pub fn cards_of(chats: &Path, board: &Board) -> Vec<Card> {
    cards_in(board, &all_cards(chats))
}

/// `cards_of` over cards already read, so a caller going through several boards
/// reads the card files once.
pub fn cards_in(board: &Board, all: &[Card]) -> Vec<Card> {
    let first = board
        .columns
        .first()
        .map(|c| c.id.clone())
        .unwrap_or_default();
    let mut cards: Vec<Card> = all
        .iter()
        .filter(|c| c.board == board.id)
        .cloned()
        .map(|mut card| {
            if !board.columns.iter().any(|c| c.id == card.column) {
                card.column = first.clone();
            }
            card
        })
        .collect();
    cards.sort_by(|a, b| {
        a.rank
            .total_cmp(&b.rank)
            .then(a.created_at.cmp(&b.created_at))
            .then(a.id.cmp(&b.id))
    });
    cards
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
    card: Card,
    spot: Spot,
    now: DateTime<Utc>,
) -> anyhow::Result<Card> {
    settle_in(chats, board, card, spot, now, &all_cards(chats))
}

/// `settle` over cards already read, so a request that has read them does not read them again.
pub fn settle_in(
    chats: &Path,
    board: &Board,
    mut card: Card,
    spot: Spot,
    now: DateTime<Utc>,
    all: &[Card],
) -> anyhow::Result<Card> {
    let others: Vec<Card> = cards_in(board, all)
        .into_iter()
        .filter(|c| c.column == card.column && c.id != card.id)
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
                    let mut other = other.clone();
                    other.rank = rank;
                    other.updated_at = now;
                    put_card(chats, &other)?;
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
    cards: Vec<Card>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let moving: HashSet<String> = cards.iter().map(|c| c.id.clone()).collect();
    let last = cards_of(chats, board)
        .iter()
        .filter(|c| c.column == column && !moving.contains(&c.id))
        .map(|c| c.rank)
        .fold(0.0_f64, f64::max);
    for (i, mut card) in cards.into_iter().enumerate() {
        card.column = column.to_string();
        card.rank = last + (i as f64 + 1.0) * STEP;
        card.updated_at = now;
        put_card(chats, &card)?;
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

/// Delete a board and its cards (tombstones); returns them for Undo.
pub fn remove_board(chats: &Path, board: &Board, now: DateTime<Utc>) -> anyhow::Result<Vec<Card>> {
    let cards = cards_of(chats, board);
    for card in &cards {
        let mut gone = card.clone();
        gone.deleted = true;
        gone.updated_at = now;
        put_card(chats, &gone)?;
    }
    let mut gone = board.clone();
    gone.deleted = true;
    gone.updated_at = now;
    put_board(chats, &gone)?;
    Ok(cards)
}

pub fn remove_card(chats: &Path, card: &Card, now: DateTime<Utc>) -> anyhow::Result<()> {
    let mut gone = card.clone();
    gone.deleted = true;
    gone.updated_at = now;
    put_card(chats, &gone)
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
    fn settling_in_cards_already_read_lands_where_settle_does() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = Utc::now();
        let board = new_board("Work", now);
        put_board(&chats, &board).unwrap();
        for (id, rank) in [("card-0001", 1.0), ("card-0002", 1.0)] {
            put_card(&chats, &card_in(&board, 0, id, rank, now)).unwrap();
        }
        let all = all_cards(&chats);
        let fresh = card_in(&board, 0, "card-0003", 0.0, now);
        settle_in(&chats, &board, fresh, Spot::After("card-0001"), now, &all).unwrap();
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
        for _ in 0..MOST_BOARDS * 2 + 10 {
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
}
