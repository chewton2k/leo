use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::boards::{self, Board, Card};
use crate::tools::{Call, Done, Kind, Param, Proposal, Spec};

const MOST_SHOWN: usize = 12_000;
const DESCRIPTION_SHOWN: usize = 300;

const BOARD: Param = Param {
    name: "board",
    required: false,
    about: "Board id or exact name; optional with one board",
};

const CARD: Param = Param {
    name: "card",
    required: true,
    about: "Card id or exact title",
};

const PRIORITY: Param = Param {
    name: "priority",
    required: false,
    about: "none, low, medium or high",
};

pub const SPECS: [Spec; 9] = [
    Spec {
        name: "list_boards",
        kind: Kind::ReadsLists,
        step: "",
        purpose: "List Kanban boards (not calendar tasks).",
        params: &[],
        returns: "Board ids, names, columns and card counts.",
        example: r#"<tool>{"name":"list_boards"}</tool>"#,
    },
    Spec {
        name: "open_board",
        kind: Kind::ReadsLists,
        step: "",
        purpose: "Read a board's cards by column, or one card in full.",
        params: &[
            BOARD,
            Param {
                name: "card",
                required: false,
                about: "Card to read in full",
            },
        ],
        returns: "Cards by column with ids, priority and description starts.",
        example: r#"<tool>{"name":"open_board","board":"Sprint"}</tool>"#,
    },
    Spec {
        name: "add_card",
        kind: Kind::Organizes,
        step: "Tried to add a card",
        purpose: "Add a requested card.",
        params: &[
            Param {
                name: "title",
                required: true,
                about: "Card title",
            },
            BOARD,
            Param {
                name: "column",
                required: false,
                about: "Column id or name; default first",
            },
            Param {
                name: "subheading",
                required: false,
                about: "Line under the title",
            },
            Param {
                name: "description",
                required: false,
                about: "Plain text",
            },
            PRIORITY,
            Param {
                name: "top",
                required: false,
                about: "yes for the top of the column",
            },
        ],
        returns: "Suggested, or done with Undo in Auto.",
        example: r#"<tool>{"name":"add_card","board":"Sprint","column":"To do","title":"Write tests","priority":"high"}</tool>"#,
    },
    Spec {
        name: "update_card",
        kind: Kind::Organizes,
        step: "Tried to change a card",
        purpose: "Change card fields; omitted stay, empty clears.",
        params: &[
            CARD,
            BOARD,
            Param {
                name: "title",
                required: false,
                about: "New title",
            },
            Param {
                name: "subheading",
                required: false,
                about: "New subheading",
            },
            Param {
                name: "description",
                required: false,
                about: "New description",
            },
            PRIORITY,
        ],
        returns: "Suggested, or done with Undo in Auto.",
        example: r#"<tool>{"name":"update_card","card":"Write tests","priority":"low"}</tool>"#,
    },
    Spec {
        name: "move_card",
        kind: Kind::Organizes,
        step: "Tried to move a card",
        purpose: "Move a card to a column (status) or board, or reorder it.",
        params: &[
            CARD,
            Param {
                name: "column",
                required: false,
                about: "Target column id or name",
            },
            Param {
                name: "to_board",
                required: false,
                about: "Target board id or name",
            },
            Param {
                name: "position",
                required: false,
                about: "top, end (default) or a card to follow",
            },
        ],
        returns: "Suggested, or done with Undo in Auto.",
        example: r#"<tool>{"name":"move_card","card":"Write tests","column":"Done"}</tool>"#,
    },
    Spec {
        name: "delete_card",
        kind: Kind::Organizes,
        step: "Tried to delete a card",
        purpose: "Delete a requested card.",
        params: &[CARD, BOARD],
        returns: "Suggested, or done with Undo in Auto.",
        example: r#"<tool>{"name":"delete_card","card":"c1a2b3c4d5"}</tool>"#,
    },
    Spec {
        name: "add_board",
        kind: Kind::Organizes,
        step: "Tried to make a board",
        purpose: "Make a requested board.",
        params: &[
            Param {
                name: "title",
                required: true,
                about: "Board name",
            },
            Param {
                name: "columns",
                required: false,
                about: "Column names separated by |; default To do | In progress | Done",
            },
        ],
        returns: "Suggested, or done with Undo in Auto.",
        example: r#"<tool>{"name":"add_board","title":"Thesis","columns":"Ideas | Writing | Done"}</tool>"#,
    },
    Spec {
        name: "change_board",
        kind: Kind::Organizes,
        step: "Tried to change a board",
        purpose: "Rename a board or set its columns; same-named columns keep their cards.",
        params: &[
            BOARD,
            Param {
                name: "new_name",
                required: false,
                about: "New board name",
            },
            Param {
                name: "columns",
                required: false,
                about: "All columns in order separated by |; unlisted ones are removed",
            },
            Param {
                name: "move_to",
                required: false,
                about: "Kept column for removed columns' cards; default first",
            },
        ],
        returns: "Suggested, or done with Undo in Auto.",
        example: r#"<tool>{"name":"change_board","board":"Sprint","columns":"To do | Doing | Review | Done"}</tool>"#,
    },
    Spec {
        name: "delete_board",
        kind: Kind::Organizes,
        step: "Tried to delete a board",
        purpose: "Delete only the requested board and its cards. Always asks the user.",
        params: &[BOARD],
        returns: "Suggested; the user decides.",
        example: r#"<tool>{"name":"delete_board","board":"Old sprint"}</tool>"#,
    },
];

pub fn changes(name: &str) -> bool {
    SPECS
        .iter()
        .any(|spec| spec.name == name && spec.kind == Kind::Organizes)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnPlan {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn quoted(text: &str) -> String {
    format!("“{text}”")
}

fn board_line(board: &Board, cards: usize) -> String {
    let columns: Vec<String> = board
        .columns
        .iter()
        .map(|c| format!("{} [{}]", c.name, c.id))
        .collect();
    format!(
        "- [{}] {} · columns: {} · {} card{}",
        board.id,
        quoted(&board.name),
        columns.join(", "),
        cards,
        if cards == 1 { "" } else { "s" }
    )
}

fn names(all: &[Board]) -> String {
    all.iter()
        .map(|b| format!("{} [{}]", quoted(&b.name), b.id))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn find_board(chats: &Path, wanted: &str) -> Result<Board, String> {
    let all = boards::boards(chats);
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return match all.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err("there are no boards yet; make one with add_board".into()),
            _ => Err(format!("say which board: {}", names(&all))),
        };
    }
    if let Some(board) = all.iter().find(|b| b.id == wanted) {
        return Ok(board.clone());
    }
    let found: Vec<&Board> = all.iter().filter(|b| same(&b.name, wanted)).collect();
    match found.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(if all.is_empty() {
            "there are no boards yet; make one with add_board".into()
        } else {
            format!(
                "no board is called \"{wanted}\"; the boards are {}",
                names(&all)
            )
        }),
        _ => Err(format!(
            "more than one board is called \"{wanted}\"; use its id: {}",
            names(&found.into_iter().cloned().collect::<Vec<_>>())
        )),
    }
}

pub fn find_column(board: &Board, wanted: &str) -> Result<String, String> {
    let wanted = wanted.trim();
    if let Some(column) = board.columns.iter().find(|c| c.id == wanted) {
        return Ok(column.id.clone());
    }
    let found: Vec<_> = board
        .columns
        .iter()
        .filter(|c| same(&c.name, wanted))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(format!(
            "{} has no column called \"{wanted}\"; its columns are {}",
            quoted(&board.name),
            board
                .columns
                .iter()
                .map(|c| quoted(&c.name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => Err(format!(
            "{} has more than one column called \"{wanted}\"; use its id from list_boards",
            quoted(&board.name)
        )),
    }
}

fn column_name(board: &Board, id: &str) -> String {
    board
        .columns
        .iter()
        .find(|c| c.id == id)
        .or(board.columns.first())
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

pub fn find_card(chats: &Path, wanted: &str, board: &str) -> Result<(Card, Board), String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return Err("say which card, by its id from open_board or its exact title".into());
    }
    let within = if board.trim().is_empty() {
        None
    } else {
        Some(find_board(chats, board)?)
    };
    let all = boards::all_cards(chats);
    let live: Vec<Board> = boards::boards(chats);
    let placed = |card: &Card| -> Option<(Card, Board)> {
        let home = live.iter().find(|b| b.id == card.board)?;
        if within.as_ref().is_some_and(|w| w.id != home.id) {
            return None;
        }
        let shown = boards::cards_in(home, std::slice::from_ref(card))
            .pop()
            .unwrap_or_else(|| card.clone());
        Some((shown, home.clone()))
    };
    if let Some(found) = all.iter().find(|c| c.id == wanted).and_then(placed) {
        return Ok(found);
    }
    let found: Vec<(Card, Board)> = all
        .iter()
        .filter(|c| same(&c.title, wanted))
        .filter_map(placed)
        .collect();
    match found.len() {
        1 => Ok(found.into_iter().next().unwrap_or_else(|| unreachable!())),
        0 => Err(format!(
            "no card is called \"{wanted}\"{}; use open_board to see the cards",
            within
                .as_ref()
                .map(|b| format!(" on {}", quoted(&b.name)))
                .unwrap_or_default()
        )),
        _ => Err(format!(
            "more than one card is called \"{wanted}\"; use its id: {}",
            found
                .iter()
                .map(|(c, b)| format!("[{}] on {}", c.id, quoted(&b.name)))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn priority_of(text: &str) -> Result<String, String> {
    let wanted = text.trim().to_lowercase();
    match wanted.as_str() {
        "" | "none" | "no" | "clear" => Ok(String::new()),
        "low" | "medium" | "high" => Ok(wanted),
        "med" | "normal" => Ok("medium".into()),
        "urgent" | "highest" | "critical" => Ok("high".into()),
        "lowest" | "minor" => Ok("low".into()),
        _ => Err(format!(
            "priority \"{}\" is not one of none, low, medium, high",
            text.trim()
        )),
    }
}

fn yes(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "yes" | "true" | "1" | "top"
    )
}

fn given(call: &Call, key: &str) -> Option<String> {
    call.args
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

pub fn column_names(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim();
    let listed: Vec<String> = if text.starts_with('[') {
        serde_json::from_str::<Vec<String>>(text)
            .map_err(|_| "write columns as names separated by |".to_string())?
    } else if text.contains('|') {
        text.split('|').map(str::to_string).collect()
    } else if text.contains('\n') {
        text.lines().map(str::to_string).collect()
    } else {
        text.split(',').map(str::to_string).collect()
    };
    let listed: Vec<String> = listed
        .into_iter()
        .map(|n| boards::clean_line(&n))
        .filter(|n| !n.is_empty())
        .collect();
    if listed.is_empty() {
        return Err("a board needs at least one column".into());
    }
    if listed.len() > boards::MOST_COLUMNS {
        return Err(format!(
            "a board can have up to {} columns",
            boards::MOST_COLUMNS
        ));
    }
    for (i, name) in listed.iter().enumerate() {
        if listed[..i].iter().any(|n| same(n, name)) {
            return Err(format!("the column \"{name}\" is listed twice"));
        }
    }
    Ok(listed)
}

fn card_line(card: &Card, full: bool) -> String {
    let mut line = format!("- [{}] {}", card.id, quoted(&card.title));
    if !card.priority.is_empty() {
        line.push_str(&format!(" · priority {}", card.priority));
    }
    if !card.subheading.is_empty() {
        line.push_str(&format!(" · subheading {}", quoted(&card.subheading)));
    }
    if !card.description.is_empty() {
        let shown = if full {
            card.description.clone()
        } else {
            crate::chats::clipped(&card.description, DESCRIPTION_SHOWN)
        };
        line.push_str(&format!("\n  description: {}", quoted(&shown)));
    }
    line
}

fn read_done(step: String, result: String) -> Done {
    Done {
        step,
        result,
        proposal: None,
        found: Vec::new(),
    }
}

pub fn read(chats: &Path, call: &Call) -> Done {
    if call.name == "list_boards" {
        let all = boards::boards(chats);
        let cards = boards::all_cards(chats);
        if all.is_empty() {
            return read_done(
                "Looked at your boards".into(),
                "There are no boards yet. Make one with add_board when the user asks.".into(),
            );
        }
        let lines: Vec<String> = all
            .iter()
            .map(|b| board_line(b, cards.iter().filter(|c| c.board == b.id).count()))
            .collect();
        return read_done(
            format!(
                "Looked at your {} board{}",
                all.len(),
                if all.len() == 1 { "" } else { "s" }
            ),
            format!("Boards:\n{}", lines.join("\n")),
        );
    }
    let fail = |why: String| {
        read_done(
            "Tried to open a board".into(),
            format!("That did not work: {why}."),
        )
    };
    let wanted_card = call.text("card");
    if !wanted_card.trim().is_empty() {
        return match find_card(chats, &wanted_card, &call.text("board")) {
            Ok((card, board)) => read_done(
                format!(
                    "Opened the card {}",
                    quoted(&crate::chats::clipped(&card.title, 60))
                ),
                format!(
                    "Card on {} [{}], column {} [{}]:\n{}",
                    quoted(&board.name),
                    board.id,
                    quoted(&column_name(&board, &card.column)),
                    card.column,
                    card_line(&card, true)
                ),
            ),
            Err(why) => fail(why),
        };
    }
    let board = match find_board(chats, &call.text("board")) {
        Ok(board) => board,
        Err(why) => return fail(why),
    };
    let cards = boards::cards_of(chats, &board);
    let mut out = format!("Board [{}] {}\n", board.id, quoted(&board.name));
    let mut cut = false;
    for column in &board.columns {
        let inside: Vec<&Card> = cards.iter().filter(|c| c.column == column.id).collect();
        out.push_str(&format!(
            "\n## {} [{}] · {} card{}\n",
            column.name,
            column.id,
            inside.len(),
            if inside.len() == 1 { "" } else { "s" }
        ));
        for card in inside {
            let line = card_line(card, false);
            if out.len() + line.len() > MOST_SHOWN {
                cut = true;
                break;
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
    if cut {
        out.push_str("\nSome cards are left out; open one by its card id or title.");
    }
    read_done(format!("Opened the board {}", quoted(&board.name)), out)
}

pub fn propose(chats: &Path, call: &Call) -> Result<Proposal, String> {
    match call.name.as_str() {
        "add_card" => {
            let board = find_board(chats, &call.text("board"))?;
            let title = boards::clean_line(&call.text("title"));
            if title.is_empty() {
                return Err("give the card a title".into());
            }
            let column = match call.text("column").trim() {
                "" => board
                    .columns
                    .first()
                    .map(|c| c.id.clone())
                    .ok_or_else(|| format!("{} has no columns", quoted(&board.name)))?,
                wanted => find_column(&board, wanted)?,
            };
            let description = call.text("description").trim().to_string();
            Ok(Proposal::AddCard {
                summary: format!(
                    "Add the card {} to {} on {}",
                    quoted(&title),
                    quoted(&column_name(&board, &column)),
                    quoted(&board.name)
                ),
                board: board.id,
                column,
                title,
                subheading: boards::clean_line(&call.text("subheading")),
                description,
                priority: priority_of(&call.text("priority"))?,
                top: yes(&call.text("top")),
            })
        }
        "update_card" => {
            let (card, _) = find_card(chats, &call.text("card"), &call.text("board"))?;
            let title = match given(call, "title") {
                Some(t) if t.trim().is_empty() => return Err("a card needs a title".into()),
                Some(t) => Some(boards::clean_line(&t)),
                None => None,
            };
            let subheading = given(call, "subheading").map(|s| boards::clean_line(&s));
            let description = given(call, "description").map(|d| d.trim().to_string());
            let priority = match given(call, "priority") {
                Some(p) => Some(priority_of(&p)?),
                None => None,
            };
            if title.is_none()
                && subheading.is_none()
                && description.is_none()
                && priority.is_none()
            {
                return Err(
                    "say what to change: title, subheading, description or priority".into(),
                );
            }
            let mut what = Vec::new();
            if title.is_some() {
                what.push("title");
            }
            if subheading.is_some() {
                what.push("subheading");
            }
            if description.is_some() {
                what.push("description");
            }
            if priority.is_some() {
                what.push("priority");
            }
            Ok(Proposal::UpdateCard {
                summary: format!("Change the {} of {}", what.join(", "), quoted(&card.title)),
                card: card.id,
                title,
                subheading,
                description,
                priority,
            })
        }
        "move_card" => {
            let (card, from) = find_card(chats, &call.text("card"), "")?;
            let to = match call.text("to_board").trim() {
                "" => from.clone(),
                wanted => find_board(chats, wanted)?,
            };
            let column = match call.text("column").trim() {
                "" if to.id == from.id => card.column.clone(),
                "" => boards::matching_column(&to, Some(&column_name(&from, &card.column)))
                    .ok_or_else(|| format!("{} has no columns", quoted(&to.name)))?,
                wanted => find_column(&to, wanted)?,
            };
            let position = call.text("position");
            let (top, after) = match position.trim().to_lowercase().as_str() {
                "" | "end" | "bottom" | "last" => (false, None),
                "top" | "first" | "start" => (true, None),
                _ => {
                    let (other, _) = find_card(chats, position.trim(), &to.id)?;
                    if other.id == card.id {
                        return Err("a card cannot go after itself".into());
                    }
                    if other.column != column {
                        return Err(format!(
                            "{} is in another column; give that column too, or use top or end",
                            quoted(&other.title)
                        ));
                    }
                    (false, Some(other.id))
                }
            };
            if to.id == from.id
                && column == card.column
                && !top
                && after.is_none()
                && position.trim().is_empty()
            {
                return Err(format!(
                    "{} is already in {}; give another column, board or position",
                    quoted(&card.title),
                    quoted(&column_name(&from, &card.column))
                ));
            }
            let place = if to.id == from.id {
                quoted(&column_name(&to, &column))
            } else {
                format!(
                    "{} on {}",
                    quoted(&column_name(&to, &column)),
                    quoted(&to.name)
                )
            };
            Ok(Proposal::MoveCard {
                summary: format!("Move {} to {place}", quoted(&card.title)),
                card: card.id,
                board: to.id,
                column,
                after,
                top,
            })
        }
        "delete_card" => {
            let (card, board) = find_card(chats, &call.text("card"), &call.text("board"))?;
            Ok(Proposal::DeleteCard {
                summary: format!(
                    "Delete the card {} from {}",
                    quoted(&card.title),
                    quoted(&board.name)
                ),
                card: card.id,
                title: card.title,
            })
        }
        "add_board" => {
            let name = boards::clean_line(&call.text("title"));
            if name.is_empty() {
                return Err("give the board a name".into());
            }
            let columns = match call.text("columns").trim() {
                "" => Vec::new(),
                text => column_names(text)?,
            };
            Ok(Proposal::AddBoard {
                summary: if columns.is_empty() {
                    format!("Make the board {}", quoted(&name))
                } else {
                    format!(
                        "Make the board {} with {}",
                        quoted(&name),
                        columns.join(", ")
                    )
                },
                name,
                columns,
            })
        }
        "change_board" => {
            let board = find_board(chats, &call.text("board"))?;
            let name = match given(call, "new_name") {
                Some(n) if n.trim().is_empty() => None,
                Some(n) => Some(boards::clean_line(&n)),
                None => None,
            };
            let columns = match call.text("columns").trim() {
                "" => None,
                text => {
                    let wanted = column_names(text)?;
                    Some(
                        wanted
                            .into_iter()
                            .map(|n| ColumnPlan {
                                id: board
                                    .columns
                                    .iter()
                                    .find(|c| same(&c.name, &n) || c.id == n)
                                    .map(|c| c.id.clone()),
                                name: board
                                    .columns
                                    .iter()
                                    .find(|c| c.id == n)
                                    .map_or(n, |c| c.name.clone()),
                            })
                            .collect::<Vec<_>>(),
                    )
                }
            };
            if name.is_none() && columns.is_none() {
                return Err("say what to change: new_name or columns".into());
            }
            let removed: Vec<&boards::Column> = columns.as_ref().map_or_else(Vec::new, |cols| {
                board
                    .columns
                    .iter()
                    .filter(|c| !cols.iter().any(|p| p.id.as_deref() == Some(c.id.as_str())))
                    .collect()
            });
            let move_to = match call.text("move_to").trim() {
                "" => None,
                wanted => {
                    let cols = columns.as_ref().ok_or("move_to needs columns")?;
                    let found = cols
                        .iter()
                        .find(|p| same(&p.name, wanted) || p.id.as_deref() == Some(wanted));
                    match found {
                        Some(ColumnPlan { id: Some(id), .. }) => Some(id.clone()),
                        Some(_) => {
                            return Err(
                                "move_to must be a column that already exists and stays".into()
                            )
                        }
                        None => {
                            return Err(format!(
                                "move_to \"{wanted}\" is not one of the new columns"
                            ))
                        }
                    }
                }
            };
            let mut what = Vec::new();
            if let Some(n) = &name {
                what.push(format!("rename it to {}", quoted(n)));
            }
            if let Some(cols) = &columns {
                what.push(format!(
                    "set its columns to {}",
                    cols.iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !removed.is_empty() {
                what.push(format!(
                    "remove {}",
                    removed
                        .iter()
                        .map(|c| quoted(&c.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Ok(Proposal::ChangeBoard {
                summary: format!(
                    "Change the board {}: {}",
                    quoted(&board.name),
                    what.join("; ")
                ),
                board: board.id,
                name,
                columns,
                move_to,
            })
        }
        "delete_board" => {
            let board = find_board(chats, &call.text("board"))?;
            let cards = boards::cards_of(chats, &board).len();
            Ok(Proposal::DeleteBoard {
                summary: format!(
                    "Delete the board {} and its {} card{}",
                    quoted(&board.name),
                    cards,
                    if cards == 1 { "" } else { "s" }
                ),
                board: board.id,
            })
        }
        other => Err(format!("{other} is not a board tool")),
    }
}

pub fn snapshot(chats: &Path, board: &str) -> Option<(Board, Vec<Card>)> {
    let board = boards::board(chats, board)?;
    let cards = boards::cards_of(chats, &board);
    Some((board, cards))
}

pub fn neighbour_before(chats: &Path, card: &Card) -> Option<Option<String>> {
    let board = boards::board(chats, &card.board)?;
    let column: Vec<Card> = boards::cards_of(chats, &board)
        .into_iter()
        .filter(|c| c.column == card.column)
        .collect();
    let at = column.iter().position(|c| c.id == card.id)?;
    Some(at.checked_sub(1).map(|i| column[i].id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_without_columns_is_refused_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        let now = chrono::Utc::now();
        let mut from = boards::new_board("Sprint", now);
        let card = Card {
            id: boards::new_id('c'),
            board: from.id.clone(),
            column: from.columns[0].id.clone(),
            rank: 1.0,
            title: "Write it".into(),
            subheading: String::new(),
            description: String::new(),
            priority: String::new(),
            created_at: now,
            updated_at: now,
            deleted: false,
        };
        boards::put_card(&chats, &card).unwrap();
        from.columns.clear();
        boards::put_board(&chats, &from).unwrap();
        let mut empty = boards::new_board("Empty", now);
        empty.columns.clear();
        boards::put_board(&chats, &empty).unwrap();
        let add = Call {
            name: "add_card".into(),
            args: serde_json::json!({"board": "Sprint", "title": "Read"}),
        };
        assert!(propose(&chats, &add).unwrap_err().contains("no columns"));
        let moved = Call {
            name: "move_card".into(),
            args: serde_json::json!({"card": "Write it", "to_board": "Empty"}),
        };
        assert!(propose(&chats, &moved).unwrap_err().contains("no columns"));
    }
}
