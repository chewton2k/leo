use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::notes::Note;

/// Export a note to the specified format. Returns the path of the exported file.
pub fn export_note(note: &Note, format: &str, output_dir: &Path) -> Result<PathBuf> {
    let safe_title: String = note
        .title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_title = safe_title.trim().replace(' ', "-");

    let filename = format!("{safe_title}.{format}");
    let output_path = output_dir.join(&filename);

    match format {
        "txt" => export_txt(note, &output_path)?,
        "md" => export_md(note, &output_path)?,
        "html" => export_html(note, &output_path)?,
        "doc" | "docx" | "pdf" | "rtf" | "odt" => export_via_pandoc(note, &output_path)?,
        _ => bail!("Unsupported format: {format}. Supported: txt, md, html, docx, pdf, rtf, odt"),
    }

    Ok(output_path)
}

fn export_txt(note: &Note, path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "{}", note.title)?;
    writeln!(f, "{}", "=".repeat(note.title.len()))?;
    writeln!(f)?;
    if !note.tags.is_empty() {
        writeln!(f, "Tags: {}", note.tags.join(", "))?;
        writeln!(f)?;
    }
    writeln!(
        f,
        "Created: {}",
        note.created_at.format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(
        f,
        "Updated: {}",
        note.updated_at.format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(f)?;
    write!(f, "{}", note.body)?;
    writeln!(f)?;
    Ok(())
}

fn export_md(note: &Note, path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "# {}", note.title)?;
    writeln!(f)?;
    if !note.tags.is_empty() {
        let tags: Vec<String> = note.tags.iter().map(|t| format!("`#{t}`")).collect();
        writeln!(f, "{}", tags.join(" "))?;
        writeln!(f)?;
    }
    write!(f, "{}", note.body)?;
    writeln!(f)?;
    Ok(())
}

fn export_html(note: &Note, path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;

    let body_html = markdown_to_html(&note.body);
    let tags_html = if note.tags.is_empty() {
        String::new()
    } else {
        format!(
            "<p class=\"tags\">{}</p>",
            note.tags
                .iter()
                .map(|t| format!("<span class=\"tag\">#{t}</span>"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    write!(
        f,
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  body {{ font-family: -apple-system, sans-serif; max-width: 700px; margin: 2em auto; padding: 0 1em; color: #333; }}
  h1 {{ border-bottom: 1px solid #eee; padding-bottom: 0.3em; }}
  h2 {{ color: #555; }}
  .tags {{ color: #666; }}
  .tag {{ background: #f0f0f0; padding: 2px 8px; border-radius: 3px; margin-right: 4px; }}
  .meta {{ color: #999; font-size: 0.85em; }}
  ul {{ padding-left: 1.5em; }}
  li {{ margin: 0.3em 0; }}
</style>
</head>
<body>
<h1>{title}</h1>
{tags}
<p class="meta">Created: {created} &middot; Updated: {updated}</p>
{body}
</body>
</html>
"#,
        title = html_escape(&note.title),
        tags = tags_html,
        created = note.created_at.format("%Y-%m-%d %H:%M UTC"),
        updated = note.updated_at.format("%Y-%m-%d %H:%M UTC"),
        body = body_html,
    )?;

    Ok(())
}

fn export_via_pandoc(note: &Note, output_path: &Path) -> Result<()> {
    if Command::new("pandoc").arg("--version").output().is_err() {
        bail!(
            "Exporting to this format requires pandoc. Install it:\n  \
             macOS:   brew install pandoc\n  \
             Linux:   sudo apt install pandoc\n  \
             Windows: choco install pandoc"
        );
    }

    let tmp = std::env::temp_dir().join("leo-export.md");
    {
        let mut f = std::fs::File::create(&tmp)?;
        writeln!(f, "# {}", note.title)?;
        writeln!(f)?;
        write!(f, "{}", note.body)?;
        writeln!(f)?;
    }

    let status = Command::new("pandoc")
        .args([
            tmp.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to run pandoc")?;

    let _ = std::fs::remove_file(&tmp);

    if !status.success() {
        bail!("pandoc exited with an error");
    }

    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn markdown_to_html(md: &str) -> String {
    let mut html = String::new();
    let mut in_list = false;

    for line in md.lines() {
        let trimmed = line.trim();

        if let Some(heading) = trimmed.strip_prefix("## ") {
            if in_list {
                html.push_str("</ul>\n");
                in_list = false;
            }
            html.push_str(&format!("<h2>{}</h2>\n", html_escape(heading)));
        } else if let Some(heading) = trimmed.strip_prefix("### ") {
            if in_list {
                html.push_str("</ul>\n");
                in_list = false;
            }
            html.push_str(&format!("<h3>{}</h3>\n", html_escape(heading)));
        } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            html.push_str(&format!(
                "<li>&#9745; {}</li>\n",
                html_escape(&trimmed[6..])
            ));
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            html.push_str(&format!("<li>&#9744; {}</li>\n", html_escape(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            html.push_str(&format!("<li>{}</li>\n", html_escape(rest)));
        } else {
            if in_list {
                html.push_str("</ul>\n");
                in_list = false;
            }
            if trimmed.is_empty() {
                html.push('\n');
            } else {
                html.push_str(&format!("<p>{}</p>\n", html_escape(trimmed)));
            }
        }
    }

    if in_list {
        html.push_str("</ul>\n");
    }
    html
}
