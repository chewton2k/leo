use axum::http::StatusCode;
use leo_core::notes::Note;

const MOST_SCAN: usize = 32_000;
const MOST_PREVIEW: usize = 1_600;
const MOST_SUMMARY: usize = 220;
const MOST_LINES: usize = 800;

pub(crate) fn order(notes: &mut [&Note], sort: Option<&str>) -> Result<(), StatusCode> {
    let sort = sort.unwrap_or("updated");
    if !matches!(
        sort,
        "updated" | "oldest" | "created" | "created-asc" | "title" | "title-desc"
    ) {
        return Err(StatusCode::BAD_REQUEST);
    }
    notes.sort_by(|a, b| {
        let title = |note: &Note| note.title.to_lowercase();
        let by = match sort {
            "oldest" => a.updated_at.cmp(&b.updated_at),
            "created" => b.created_at.cmp(&a.created_at),
            "created-asc" => a.created_at.cmp(&b.created_at),
            "title" => title(a).cmp(&title(b)),
            "title-desc" => title(b).cmp(&title(a)),
            _ => std::cmp::Ordering::Equal,
        };
        (!a.pinned)
            .cmp(&!b.pinned)
            .then(by)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.id.cmp(&b.id))
    });
    Ok(())
}

pub(crate) fn only(
    notes: &mut Vec<&Note>,
    show: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StatusCode> {
    match show.unwrap_or("all") {
        "all" => {}
        "pinned" => notes.retain(|n| n.pinned),
        "tasks" => notes.retain(|n| {
            let [done, all] = crate::routes::notes::tasks_in(&n.body);
            done < all
        }),
        "week" => notes.retain(|n| now - n.updated_at <= chrono::Duration::days(7)),
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    Ok(())
}

pub(crate) fn preview(body: &str) -> String {
    let scan: String = body.chars().take(MOST_SCAN).collect();
    let lines: Vec<&str> = scan.lines().take(MOST_LINES).collect();
    let mut fence: Option<(usize, char, usize)> = None;
    let mut visual = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some((start, mark, width)) = fence {
            if trimmed.chars().take_while(|c| *c == mark).count() >= width {
                let part = lines[start..=i].join("\n");
                if part.chars().count() <= MOST_PREVIEW && visual == Some(start) {
                    return part;
                }
                fence = None;
                visual = None;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let mark = trimmed.chars().next().unwrap_or('`');
            let width = trimmed.chars().take_while(|c| *c == mark).count();
            let kind = trimmed.trim_start_matches(mark).trim();
            fence = Some((i, mark, width));
            if matches!(kind, "mermaid" | "svg") {
                visual = Some(i);
            }
        } else if (trimmed.starts_with("![") || trimmed.starts_with("![["))
            && trimmed.chars().count() <= MOST_PREVIEW
        {
            return trimmed.to_string();
        } else if trimmed == "$$" {
            if let Some(end) = lines
                .iter()
                .skip(i + 1)
                .position(|line| line.trim() == "$$")
            {
                let part = lines[i..=i + end + 1].join("\n");
                if part.chars().count() <= MOST_PREVIEW {
                    return part;
                }
            }
        }
    }
    scan.chars().take(MOST_PREVIEW).collect()
}

pub(crate) fn summary(body: &str) -> String {
    let scan: String = body.chars().take(MOST_SCAN).collect();
    let mut fenced = false;
    let mut headings = Vec::new();
    for line in scan.lines().take(MOST_LINES) {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            let heading = line.trim_start_matches('#').trim();
            if headings.len() < 4 && !heading.is_empty() {
                headings.push(heading);
            }
            continue;
        }
        if line.starts_with(['>', '|', '$', '!', '<']) || line.starts_with("From ") {
            continue;
        }
        let text = line.trim_start_matches(['-', '*', '+', ' ']);
        if text.starts_with('[') || text.chars().count() < 20 {
            continue;
        }
        let sentence = text
            .split_inclusive(['.', '。', '!', '?'])
            .next()
            .unwrap_or(text);
        return sentence.chars().take(MOST_SUMMARY).collect();
    }
    headings.join(" · ").chars().take(MOST_SUMMARY).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn note(title: &str, edited_days_ago: i64, created_days_ago: i64) -> Note {
        let mut n = Note::new(title, "", vec![], "");
        n.updated_at = Utc::now() - Duration::days(edited_days_ago);
        n.created_at = Utc::now() - Duration::days(created_days_ago);
        n
    }

    fn titles(notes: &[&Note]) -> Vec<String> {
        notes.iter().map(|n| n.title.clone()).collect()
    }

    #[test]
    fn notes_are_newest_edit_first_by_default_with_pinned_on_top() {
        let a = note("Apple", 3, 9);
        let b = note("banana", 1, 1);
        let mut c = note("Cherry", 10, 5);
        let d = note("Date", 0, 20);
        let all = [&a, &b, &c, &d];
        let mut list = all.to_vec();
        order(&mut list, None).unwrap();
        assert_eq!(titles(&list), ["Date", "banana", "Apple", "Cherry"]);
        order(&mut list, Some("oldest")).unwrap();
        assert_eq!(titles(&list), ["Cherry", "Apple", "banana", "Date"]);
        order(&mut list, Some("created")).unwrap();
        assert_eq!(titles(&list), ["banana", "Cherry", "Apple", "Date"]);
        order(&mut list, Some("created-asc")).unwrap();
        assert_eq!(titles(&list), ["Date", "Apple", "Cherry", "banana"]);
        order(&mut list, Some("title")).unwrap();
        assert_eq!(titles(&list), ["Apple", "banana", "Cherry", "Date"]);
        order(&mut list, Some("title-desc")).unwrap();
        assert_eq!(titles(&list), ["Date", "Cherry", "banana", "Apple"]);
        c.pinned = true;
        let mut list = vec![&a, &b, &c, &d];
        order(&mut list, None).unwrap();
        assert_eq!(titles(&list), ["Cherry", "Date", "banana", "Apple"]);
        assert_eq!(order(&mut list, Some("size")), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn filters_keep_pinned_notes_open_tasks_or_this_weeks_edits() {
        let mut pinned = note("Pinned", 30, 30);
        pinned.pinned = true;
        let mut tasks = note("Tasks", 30, 30);
        tasks.body = "- [x] done\n- [ ] still to do".into();
        let mut finished = note("Finished", 30, 30);
        finished.body = "- [x] done".into();
        let recent = note("Recent", 2, 30);
        let all = vec![&pinned, &tasks, &finished, &recent];
        let shown = |show: &str| {
            let mut list = all.clone();
            only(&mut list, Some(show), Utc::now()).unwrap();
            titles(&list)
        };
        assert_eq!(shown("all").len(), 4);
        assert_eq!(shown("pinned"), ["Pinned"]);
        assert_eq!(shown("tasks"), ["Tasks"]);
        assert_eq!(shown("week"), ["Recent"]);
        let mut list = all.clone();
        assert_eq!(
            only(&mut list, Some("nope"), Utc::now()),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn a_visual_preview_is_complete_bounded_and_unicode_safe() {
        let body =
            "# Lecture\nAn opening paragraph.\n```mermaid\ngraph LR\n A --> B\n```\nMore text";
        assert_eq!(preview(body), "```mermaid\ngraph LR\n A --> B\n```");
        assert_eq!(
            preview("# Photos\n![A figure](picture.png)"),
            "![A figure](picture.png)"
        );
        assert_eq!(preview("# Math\n$$\nx^2\n$$"), "$$\nx^2\n$$");
        assert_eq!(preview(&"é🦊".repeat(2000)).chars().count(), MOST_PREVIEW);
        let oversized = format!("```svg\n{}\n```\n![Small](small.png)", "x".repeat(2000));
        assert_eq!(preview(&oversized), "![Small](small.png)");
    }

    #[test]
    fn summaries_skip_drawing_code_and_tasks_then_use_content_or_headings() {
        assert_eq!(
            summary(
                "# Lecture\n```svg\n<svg>hidden internals</svg>\n```\n- [ ] Submit homework tomorrow\nBreadth first search visits each layer. Another sentence."
            ),
            "Breadth first search visits each layer."
        );
        assert_eq!(
            summary("# Queues\n## Breadth first search\n- [ ] Finish"),
            "Queues · Breadth first search"
        );
        assert!(summary(&"🦊".repeat(1000)).chars().count() <= MOST_SUMMARY);
    }
}
