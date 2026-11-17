pub(crate) mod office;

use anyhow::{bail, Context, Result};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::ai::budget::Budget;
use crate::ai::chat::{self, Prompt};
use crate::ai::provider::Image;
use crate::figures::{self, Figure};

const PAGES_PER_LOOK: usize = 6;
const MOST_IMAGES: usize = 40;
const NOTE_TOKENS: u32 = 8_000;
const MOST_PDF_PAGES: usize = 2_048;
const MOST_SOURCE_BYTES: usize = 90 * 1024 * 1024;
const MOST_PDF_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MOST_PDF_OPERATIONS: usize = 300_000;
const MOST_PDF_DEPTH: usize = 128;
const SCAN_TEXT_LETTERS: usize = 40;
const SCAN_PAGE_SHARE: f32 = 0.65;

#[derive(Debug, Clone)]
pub struct Upload {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Material {
    pub text: String,
    pub images: Vec<Image>,
    pub figures: Vec<Figure>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    pub title: String,
    pub body: String,
    pub figures: Vec<Figure>,
}

fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

fn full_page_image(doc: &lopdf::Document, page: lopdf::ObjectId) -> Result<bool> {
    let mut current = page;
    let mut bounds = None;
    for _ in 0..MOST_PDF_DEPTH {
        let Ok(dict) = doc.get_dictionary(current) else {
            break;
        };
        if let Ok(rect) = dict.get(b"MediaBox").and_then(lopdf::Object::as_array) {
            if let [x0, y0, x1, y1] = rect.as_slice() {
                bounds = x0
                    .as_float()
                    .ok()
                    .zip(y0.as_float().ok())
                    .zip(x1.as_float().ok().zip(y1.as_float().ok()))
                    .map(|((x0, y0), (x1, y1))| ((x1 - x0) * (y1 - y0)).abs());
            }
            break;
        }
        let Ok(parent) = dict.get(b"Parent").and_then(lopdf::Object::as_reference) else {
            break;
        };
        current = parent;
    }
    let Some(area) = bounds.filter(|area| area.is_finite() && *area > 0.0) else {
        return Ok(false);
    };
    let bytes = doc
        .get_page_content_with_limit(page, MOST_PDF_CONTENT_BYTES)
        .context("a PDF page expands to too much content; export a simpler PDF")?;
    let Ok(content) = lopdf::content::Content::decode(&bytes) else {
        return Ok(false);
    };
    if content.operations.len() > MOST_PDF_OPERATIONS {
        bail!("a PDF page is too complex; export a simpler PDF");
    }
    let mut scale = 1.0f32;
    let mut stack = Vec::new();
    for operation in content.operations {
        match operation.operator.as_str() {
            "q" => {
                if stack.len() >= MOST_PDF_DEPTH {
                    bail!("a PDF page is nested too deeply; export a simpler PDF");
                }
                stack.push(scale);
            }
            "Q" => scale = stack.pop().unwrap_or(1.0),
            "cm" => {
                if let [a, b, c, d, _, _] = operation.operands.as_slice() {
                    if let (Ok(a), Ok(b), Ok(c), Ok(d)) =
                        (a.as_float(), b.as_float(), c.as_float(), d.as_float())
                    {
                        scale *= (a * d - b * c).abs();
                    }
                }
            }
            "Do" if scale.is_finite() && scale >= area * SCAN_PAGE_SHARE => return Ok(true),
            _ => {}
        }
    }
    Ok(false)
}

fn pdf(bytes: &[u8]) -> Result<Material> {
    let doc = lopdf::Document::load_mem(bytes)
        .context("this PDF could not be opened (it may be damaged or password-protected)")?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        bail!("this PDF has no pages");
    }
    if pages.len() > MOST_PDF_PAGES {
        bail!("this PDF has too many pages; split it into smaller documents");
    }
    let mut text = String::new();
    let mut images = Vec::new();
    let mut prepared = std::collections::HashMap::new();
    let mut image_bytes = 0;
    for (&number, &page) in &pages {
        let extracted = match doc.extract_text_with_limit(&[number], MOST_PDF_CONTENT_BYTES) {
            Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
                ..
            })) => bail!("a PDF page expands to too much content; export a simpler PDF"),
            result => result.unwrap_or_default(),
        };
        let extracted = extracted.trim();
        if !extracted.is_empty() {
            text.push_str(&format!("[page {number}]\n{extracted}\n\n"));
            if text.len() > MOST_SOURCE_BYTES {
                bail!("this PDF expands to too much text; split it into smaller documents");
            }
            let letters = extracted
                .chars()
                .filter(|c| c.is_alphanumeric())
                .take(SCAN_TEXT_LETTERS)
                .count();
            if letters >= SCAN_TEXT_LETTERS || !full_page_image(&doc, page)? {
                continue;
            }
        }
        for image in doc.get_page_images(page).unwrap_or_default() {
            if image.width < 200 || image.height < 200 {
                continue;
            }
            if image
                .width
                .checked_mul(image.height)
                .is_none_or(|area| area > figures::MOST_PDF_PIXELS as i64)
            {
                bail!("page {number} contains a scan beyond the safe image limit; export that page at a lower resolution, then try again");
            }
            let bytes = figures::pdf_figure(&doc, &image).with_context(|| format!("page {number} contains a scanned image leo could not read safely; export that page as a standard JPEG or PNG, then try again"))?;
            if bytes.len() > MOST_SOURCE_BYTES - image_bytes {
                bail!("the PDF's reading images use too much memory; upload it in smaller page groups");
            }
            image_bytes += bytes.len();
            let mime = leo_core::attachments::kind_of(&bytes)
                .and_then(leo_core::attachments::mime_of)
                .unwrap_or("image/png");
            images.push(Image {
                mime: mime.into(),
                bytes: bytes.clone(),
            });
            prepared.insert(image.id, bytes);
            if images.len() > MOST_IMAGES {
                bail!("this PDF has too many scanned images; upload it in smaller groups");
            }
        }
    }
    if images.is_empty() && text.is_empty() {
        bail!("this PDF looks scanned, but leo cannot read its pages; take photos of them, or export them as images, and upload those");
    }
    Ok(Material {
        text: text.trim().to_string(),
        images,
        figures: figures::from_pdf_prepared(&doc, &mut prepared),
    })
}

pub fn extract(upload: &Upload) -> Result<Material> {
    if upload.bytes.len() > MOST_SOURCE_BYTES {
        bail!(
            "{} is too large to read; split it into smaller files",
            upload.name
        );
    }
    let ext = extension(&upload.name);
    let mime = upload.mime.to_lowercase();
    let text = |t: String| Material {
        text: t,
        ..Material::default()
    };
    let photo = |mime: String| Material {
        text: String::new(),
        images: vec![Image {
            mime: mime.clone(),
            bytes: upload.bytes.clone(),
        }],
        figures: vec![Figure {
            mime,
            bytes: upload.bytes.clone(),
            place: upload.name.clone(),
            photo: true,
        }],
    };
    if matches!(
        mime.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    ) {
        return Ok(photo(mime));
    }
    match ext.as_str() {
        "pdf" => pdf(&upload.bytes),
        "docx" => office::docx(&upload.bytes).map(|t| Material {
            figures: figures::from_docx(&upload.bytes),
            ..text(t)
        }),
        "pptx" => office::pptx(&upload.bytes).map(|t| Material {
            figures: figures::from_pptx(&upload.bytes),
            ..text(t)
        }),
        "txt" | "md" | "markdown" | "text" => Ok(text(String::from_utf8_lossy(&upload.bytes).into_owned())),
        "jpg" | "jpeg" | "png" | "webp" | "gif" => Ok(photo(format!(
            "image/{}",
            if ext == "jpg" { "jpeg" } else { ext.as_str() }
        ))),
        "heic" | "heif" => bail!("{} is a HEIC photo; upload it from the phone's photo picker, which converts it, or save it as JPEG", upload.name),
        "doc" | "ppt" => bail!("{} is an old Office format; save it as .{}x and upload that", upload.name, ext),
        _ => bail!("leo cannot read {} yet: upload a PDF, Word, PowerPoint, text file, or photos", upload.name),
    }
}

pub fn gather(uploads: &[Upload]) -> Result<Material> {
    let mut all = Material::default();
    for upload in uploads {
        let found = extract(upload)?;
        if !found.text.trim().is_empty() {
            if uploads.len() > 1 {
                all.text.push_str(&format!("# {}\n\n", upload.name));
            }
            all.text.push_str(found.text.trim());
            all.text.push_str("\n\n");
        }
        all.images.extend(found.images);
        let many = uploads.len() > 1;
        all.figures.extend(found.figures.into_iter().map(|mut f| {
            if many && !f.photo {
                f.place = format!("{} of {}", f.place, upload.name);
            }
            f
        }));
    }
    let mut documents = 0;
    all.figures.retain(|f| {
        documents += usize::from(!f.photo);
        f.photo || documents <= figures::MOST_FIGURES
    });
    if all.images.len() > MOST_IMAGES {
        bail!("that is {} pages of images; leo reads up to {MOST_IMAGES} at a time, so upload them in smaller groups", all.images.len());
    }
    if all.text.trim().is_empty() && all.images.is_empty() {
        bail!("there is no text in that file for leo to read");
    }
    Ok(all)
}

const DOC_RULES: &str = "\
You turn material a student uploaded (lecture slides, a handout, a paper, a worksheet, a textbook page, or photos of a whiteboard or handwritten notes) into study notes in Markdown.

- Use interpretable language: plain words someone new to the subject can follow, with each technical term explained the first time it appears.
- Keep everything that matters for learning: definitions, steps, formulas, worked examples, results, and what figures or diagrams show, described in words.
- Leave out page furniture: headers, footers, page numbers, repeated slide titles, copyright lines.
- Organise with ## headings and bullet points (- ); bold a term where it is defined; write math in LaTeX ($...$ inside a sentence, $$...$$ on lines of their own) and code in fenced code blocks.
- Unlike the rest, explanations, examples, diagrams and the Check yourself questions may go beyond the source to help the reader learn, as long as they are accurate.
- Put tasks or deadlines that the material states in a final \"## Action items\" section as checkboxes (- [ ] ); leave the section out otherwise. Keep only the fewest that matter: one checkbox per deliverable or date (a homework, an exam, a meeting), never its sub-steps, suggested readings or general advice like \"review the slides\".
- Do not add facts that are not in the source, except to explain. If something is unreadable, write [unreadable] rather than guessing.";

const WANTS: &str = "\
The user said what they want from this material in <what_the_user_wants>. Shape the notes around it wherever it fits (what to focus on, how to lay it out, how deep to go), without leaving out what matters for learning.";

const PLACING: &str = "\
Pictures from the material can go in the notes. They are listed in <figures> as figure:N, with the slide or page each comes from. Put a picture where it helps someone understand, on its own line, as ![what it shows, in a few words](figure:N), next to the notes for its slide or page. Use each one at most once and only the ones listed, and leave out pictures that are only decoration.";

const SEEING: &str = "\
The images are photos or scans of pages, in order. Read all of them, including handwriting, tables and diagrams.";

fn whole_shape() -> &'static str {
    "Shape of the reply:\n1. The first line is the title, as plain text: no \"Title:\", no #, no quotes, no bold.\n2. A blank line, then a 2-3 sentence summary.\n3. ## sections telling the story, ending with ## Check yourself.\n\nReply with the note only: no preamble, no remarks after it, and do not wrap it in a code block."
}

fn part_shape(part: usize, parts: usize) -> String {
    let check = if part == parts {
        "End with ## Check yourself for the whole material."
    } else {
        "Leave ## Check yourself to the last part."
    };
    format!("This is part {part} of {parts} of the material; the other parts are handled separately. Write ## sections for this part only: no title and no summary of the whole. {check}\n\nReply with the notes only: no preamble, no remarks after them, and do not wrap them in a code block.")
}

fn wants_block(wants: &str) -> (String, String, String) {
    let wants = wants.trim();
    if wants.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    (
        format!("\n\n{WANTS}"),
        format!("<what_the_user_wants>\n{wants}\n</what_the_user_wants>\n\n"),
        " Follow what the user wants from it.".to_string(),
    )
}

pub fn text_parts(text: &str, most: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        if current.chars().count() + paragraph.chars().count() > most && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        if paragraph.chars().count() > most {
            let chars: Vec<char> = paragraph.chars().collect();
            for chunk in chars.chunks(most.max(1)) {
                parts.push(chunk.iter().collect());
            }
            continue;
        }
        current.push_str(paragraph);
        current.push_str("\n\n");
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

pub fn offered(figures: &[Figure]) -> String {
    figures
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.photo)
        .map(|(i, f)| format!("figure:{} is from {}", i + 1, f.place))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn text_prompt(
    text: &str,
    name: &str,
    part: Option<(usize, usize)>,
    figures: &str,
    wants: &str,
) -> Prompt {
    let shape = match part {
        Some((p, n)) => part_shape(p, n),
        None => whole_shape().to_string(),
    };
    let (rules, listed) = if figures.is_empty() {
        (String::new(), String::new())
    } else {
        (
            format!("\n\n{PLACING}"),
            format!("\n\n<figures>\n{figures}\n</figures>"),
        )
    };
    let (asked, wanted, follow) = wants_block(wants);
    let learning = crate::ai::chat::LEARNING;
    Prompt {
        system: format!("{DOC_RULES}\n\n{learning}{rules}{asked}\n\n{shape}"),
        user: format!("{wanted}<material name=\"{name}\">\n{}\n</material>{listed}\n\nWrite the study notes for this material.{follow}", text.trim()),
    }
}

pub fn image_prompt(
    name: &str,
    part: Option<(usize, usize)>,
    pages: (usize, usize, usize),
    wants: &str,
) -> Prompt {
    let shape = match part {
        Some((p, n)) => part_shape(p, n),
        None => whole_shape().to_string(),
    };
    let (asked, wanted, follow) = wants_block(wants);
    let learning = crate::ai::chat::LEARNING;
    Prompt {
        system: format!("{DOC_RULES}\n\n{learning}\n\n{SEEING}{asked}\n\n{shape}"),
        user: format!(
            "{wanted}These are pages {}-{} of {} from \"{name}\". Write the study notes for them.{follow}",
            pages.0, pages.1, pages.2
        ),
    }
}

fn named(mut prompt: Prompt, label: Option<&str>) -> Prompt {
    if let Some(label) = label {
        prompt.user.push_str(&format!(" This is \"{label}\", the name the user files it under: start the title with that name as written, then a few words on the topic."));
    }
    prompt
}

pub type Write<'a> = &'a (dyn Fn(Prompt, u32) -> Result<String> + Sync);
pub type See<'a> = &'a (dyn Fn(Prompt, &[Image], u32) -> Result<String> + Sync);

pub const CHAT_DOC_CHARS: usize = 2_000_000;
const TRANSCRIBE_TOKENS: u32 = 6_000;

const TRANSCRIBING: &str = "\
You copy out what is on photographed or scanned pages so someone can ask questions about them later. Write the text exactly as it appears, in reading order, in Markdown: keep headings, lists, tables, and formulas written in LaTeX ($...$ inline, $$...$$ on their own lines). Describe each diagram or figure in one short sentence in [brackets]. Write [unreadable] for anything you cannot read. Reply with the text only.";

pub fn clip_document(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= CHAT_DOC_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(CHAT_DOC_CHARS).collect();
    out.push_str("\n\n[…the rest of this document is left out]");
    out
}

pub fn read_for_chat(
    upload: &Upload,
    see: See<'_>,
    progress: &mut dyn FnMut(&str),
) -> Result<String> {
    let material = extract(upload)?;
    let mut text = material.text.trim().to_string();
    let groups: Vec<&[Image]> = material.images.chunks(PAGES_PER_LOOK).collect();
    for (i, pages) in groups.iter().enumerate() {
        progress(&if groups.len() > 1 {
            format!("Reading the pages {}/{}", i + 1, groups.len())
        } else {
            "Reading the image".to_string()
        });
        let prompt = Prompt {
            system: TRANSCRIBING.to_string(),
            user: format!(
                "These are pages from \"{}\". Copy out what they say.",
                upload.name
            ),
        };
        let seen = see(prompt, pages, TRANSCRIBE_TOKENS)?;
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(seen.trim());
    }
    if text.trim().is_empty() {
        bail!("there is no text in {} for Felix to read", upload.name);
    }
    Ok(clip_document(&text))
}

#[allow(clippy::too_many_arguments)]
pub fn write_note(
    material: &Material,
    name: &str,
    label: Option<&str>,
    wants: &str,
    write: Write<'_>,
    see: See<'_>,
    progress: &mut dyn FnMut(&str, usize, usize),
    budget: Budget,
) -> Result<(String, String)> {
    let texts = if material.text.trim().is_empty() {
        Vec::new()
    } else {
        text_parts(&material.text, budget.chars())
    };
    let looks: Vec<&[Image]> = material.images.chunks(PAGES_PER_LOOK).collect();
    let figures = offered(&material.figures);
    let jobs = texts.len() + looks.len();
    if jobs == 0 {
        bail!("there is nothing in that file for leo to read");
    }
    if jobs == 1 {
        progress("Writing the note", 0, 1);
        let reply = if let Some(text) = texts.first() {
            write(
                named(text_prompt(text, name, None, &figures, wants), label),
                NOTE_TOKENS,
            )?
        } else {
            let n = material.images.len();
            see(
                named(image_prompt(name, None, (1, n, n), wants), label),
                looks[0],
                NOTE_TOKENS,
            )?
        };
        progress("Writing the note", 1, 1);
        let (title, body) = chat::split_title_body(&chat::clean_reply(&reply));
        return Ok((leo_core::filename::with_label(&title, label), body));
    }
    let total = jobs + 1;
    let n = material.images.len();
    let run = |i: usize| -> Result<String> {
        let reply = if let Some(text) = texts.get(i) {
            write(
                text_prompt(text, name, Some((i + 1, jobs)), &figures, wants),
                NOTE_TOKENS,
            )?
        } else {
            let j = i - texts.len();
            let batch = looks[j];
            let pages = (j * PAGES_PER_LOOK + 1, j * PAGES_PER_LOOK + batch.len(), n);
            see(
                image_prompt(name, Some((i + 1, jobs)), pages, wants),
                batch,
                NOTE_TOKENS,
            )?
        };
        Ok(chat::clean_reply(&reply))
    };
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<String>>>> = (0..jobs).map(|_| Mutex::new(None)).collect();
    let mut step = |done: usize| {
        progress(
            &format!("Writing part {} of {jobs}", (done + 1).min(jobs)),
            done,
            total,
        );
    };
    step(0);
    let stop = leo_core::children::current_stop();
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        for _ in 0..budget.at_once.max(1).min(jobs) {
            let tx = tx.clone();
            let (next, results, run) = (&next, &results, &run);
            let stop = stop.clone().unwrap_or_default();
            scope.spawn(move || {
                leo_core::children::with_stop(stop, || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= jobs {
                        return;
                    }
                    let written = run(i);
                    let failed = written.is_err();
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = Some(written);
                    }
                    if failed {
                        next.store(jobs, Ordering::Relaxed);
                        return;
                    }
                    let _ = tx.send(());
                })
            });
        }
        drop(tx);
        for (done, ()) in rx.iter().enumerate() {
            step(done + 1);
        }
    });
    let mut sections = Vec::with_capacity(jobs);
    for slot in results {
        match slot.into_inner().ok().flatten() {
            Some(Ok(text)) => sections.push(text),
            Some(Err(e)) => return Err(e),
            None => bail!("a part of the note was not written"),
        }
    }
    progress("Naming the note", jobs, total);
    let joined = sections.join("\n\n");
    let digest: String = joined.chars().take(30_000).collect();
    let (title, summary) = match write(chat::build_summary_prompt(&digest), 600) {
        Ok(reply) => chat::split_title_body(&reply),
        Err(_) => (String::new(), String::new()),
    };
    progress("Naming the note", total, total);
    let body = if summary.trim().is_empty() {
        joined
    } else {
        format!("{}\n\n{joined}", summary.trim())
    };
    Ok((leo_core::filename::with_label(&title, label), body))
}

fn label_for(uploads: &[Upload], material: &Material) -> Option<String> {
    use leo_core::filename::{file_label, series_label};
    match uploads {
        [one] => file_label(&one.name)
            .or_else(|| series_label(&one.name))
            .or_else(|| series_label(&material.text)),
        _ => None,
    }
}

pub fn import(
    uploads: &[Upload],
    wants: &str,
    progress: &mut dyn FnMut(&str, usize, usize),
) -> Result<Imported> {
    progress("Reading the file", 0, 1);
    let material = gather(uploads)?;
    let name = uploads.first().map(|u| u.name.as_str()).unwrap_or("upload");
    let write = |prompt: Prompt, most: u32| -> Result<String> {
        Ok(crate::ai::chat_outcome(prompt, most)?.value)
    };
    let see = |prompt: Prompt, images: &[Image], most: u32| crate::ai::see(prompt, images, most);
    let budget = crate::ai::writing_budget();
    let label = label_for(uploads, &material);
    let (title, body) = write_note(
        &material,
        name,
        label.as_deref(),
        wants,
        &write,
        &see,
        progress,
        budget,
    )?;
    Ok(Imported {
        title,
        body,
        figures: material.figures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn a_stop_reaches_the_import_workers() {
        use std::sync::atomic::AtomicUsize;
        let caller = std::thread::current().id();
        let on_workers = AtomicUsize::new(0);
        let unstopped = AtomicUsize::new(0);
        let count = || {
            if std::thread::current().id() != caller {
                on_workers.fetch_add(1, Ordering::SeqCst);
            }
            if leo_core::children::current_stop().is_none() {
                unstopped.fetch_add(1, Ordering::SeqCst);
            }
        };
        let write = |_: Prompt, _: u32| -> Result<String> {
            count();
            Ok("Heaps\n\nHow heaps work.".into())
        };
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> {
            count();
            Ok("## Whiteboard\n- drawing".into())
        };
        let long = Material {
            text: vec!["word ".repeat(1000); 10].join("\n\n"),
            images: (0..8)
                .map(|i| Image {
                    mime: "image/jpeg".into(),
                    bytes: vec![i],
                })
                .collect(),
            figures: vec![],
        };
        leo_core::children::with_stop(leo_core::children::Stop::default(), || {
            write_note(
                &long,
                "handout.pdf",
                None,
                "",
                &write,
                &see,
                &mut |_, _, _| {},
                Budget::FREE,
            )
        })
        .unwrap();
        assert!(on_workers.load(Ordering::SeqCst) > 1);
        assert_eq!(unstopped.load(Ordering::SeqCst), 0);
    }

    fn never(_: Prompt, _: &[Image], _: u32) -> Result<String> {
        panic!("there are no images here")
    }

    #[test]
    fn notes_from_uploads_tell_a_story_and_follow_what_the_user_wants() {
        let plain = text_prompt("x", "interview.pdf", None, "", "");
        assert!(plain
            .system
            .contains("one checkbox per deliverable or date"));
        assert!(plain.system.contains("## Check yourself") && plain.system.contains("[!example]-"));
        assert!(plain.system.contains("break the problem down"));
        assert!(!plain.user.contains("what_the_user_wants"));
        let steered = text_prompt(
            "x",
            "interview.pdf",
            None,
            "",
            "  The problem, then the code, explained step by step.  ",
        );
        assert!(steered.system.contains("<what_the_user_wants>"));
        assert!(steered.user.starts_with("<what_the_user_wants>\nThe problem, then the code, explained step by step.\n</what_the_user_wants>"));
        assert!(steered
            .user
            .ends_with("Follow what the user wants from it."));
        let pages = image_prompt("board.jpg", Some((2, 2)), (3, 4, 4), "only the proofs");
        assert!(
            pages.user.contains("only the proofs")
                && pages.system.contains("End with ## Check yourself")
        );
        assert!(image_prompt("board.jpg", Some((1, 2)), (1, 2, 4), "")
            .system
            .contains("Leave ## Check yourself to the last part"));
    }

    #[test]
    fn photos_are_kept_as_pictures_and_document_figures_are_offered_by_place() {
        let photo = Upload {
            name: "board.png".into(),
            mime: "image/png".into(),
            bytes: b"\x89PNG\r\n\x1a\nrest".to_vec(),
        };
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps".to_vec(),
        };
        let gathered = gather(&[photo.clone(), text]).unwrap();
        assert_eq!(gathered.images.len(), 1);
        assert_eq!(gathered.figures.len(), 1);
        assert!(gathered.figures[0].photo);
        assert_eq!(gathered.figures[0].place, "board.png");
        let figures = vec![
            gathered.figures[0].clone(),
            Figure {
                mime: "image/png".into(),
                bytes: vec![],
                place: "slide 3".into(),
                photo: false,
            },
        ];
        assert_eq!(offered(&figures), "figure:2 is from slide 3");
        let with = text_prompt("x", "deck.pptx", None, &offered(&figures), "");
        assert!(with
            .system
            .contains("![what it shows, in a few words](figure:N)"));
        assert!(with
            .user
            .contains("<figures>\nfigure:2 is from slide 3\n</figures>"));
        let without = text_prompt("x", "notes.md", None, "", "");
        assert!(!without.system.contains("figure:N") && !without.user.contains("<figures>"));
    }

    #[test]
    fn notes_from_uploads_are_written_in_interpretable_language() {
        assert!(text_prompt("x", "a.pdf", None, "", "")
            .system
            .contains("Use interpretable language"));
        assert!(image_prompt("a.jpg", None, (1, 1, 1), "")
            .system
            .contains("Use interpretable language"));
    }

    #[test]
    fn a_document_for_felix_keeps_only_its_text_and_is_clipped() {
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps\n\nMinimum at the root.".to_vec(),
        };
        let read = read_for_chat(&text, &never, &mut |_| {}).unwrap();
        assert_eq!(read, "# Heaps\n\nMinimum at the root.");
        let long = Upload {
            name: "long.txt".into(),
            mime: "text/plain".into(),
            bytes: "word ".repeat(CHAT_DOC_CHARS / 5 + 100).into_bytes(),
        };
        let clipped = read_for_chat(&long, &never, &mut |_| {}).unwrap();
        assert!(clipped.chars().count() < CHAT_DOC_CHARS + 60);
        assert!(clipped.ends_with("[…the rest of this document is left out]"));
        let blank = Upload {
            name: "blank.txt".into(),
            mime: "text/plain".into(),
            bytes: b"   ".to_vec(),
        };
        assert!(read_for_chat(&blank, &never, &mut |_| {}).is_err());
    }

    #[test]
    fn a_photo_for_felix_is_copied_out_by_the_ai_that_can_see() {
        let looked = Mutex::new(0);
        let see = |prompt: Prompt, images: &[Image], _: u32| -> Result<String> {
            *looked.lock().unwrap() += 1;
            assert_eq!(images.len(), 1);
            assert!(prompt.system.contains("copy out"));
            Ok("Board: Dijkstra uses a heap".into())
        };
        let photo = Upload {
            name: "board.jpg".into(),
            mime: "image/jpeg".into(),
            bytes: vec![0xff, 0xd8, 0xff],
        };
        let mut steps = Vec::new();
        let read = read_for_chat(&photo, &see, &mut |s| steps.push(s.to_string())).unwrap();
        assert_eq!(read, "Board: Dijkstra uses a heap");
        assert_eq!(*looked.lock().unwrap(), 1);
        assert_eq!(steps, ["Reading the image"]);
    }

    fn office(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, text) in files {
                zip.start_file(*name, options).unwrap();
                let text = text.replacen('>', " xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">", 1);
                zip.write_all(text.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn upload(name: &str, mime: &str, bytes: Vec<u8>) -> Upload {
        Upload {
            name: name.into(),
            mime: mime.into(),
            bytes,
        }
    }

    #[test]
    fn native_sources_keep_word_structure_without_vision() {
        let xml = r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Café &amp; heaps</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Minimum</w:t></w:r><w:r><w:t> first. </w:t></w:r><w:hyperlink r:id="rId1"><w:r><w:t>Read more</w:t></w:r></w:hyperlink></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Insert</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Sift up</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Operation</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Cost</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Insert | push</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>O(log n)</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/hyperlink" Target="https://example.com/heaps" TargetMode="External"/></Relationships>"#;
        let numbering = r#"<w:numbering><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:start w:val="3"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;
        let source = upload(
            "notes.docx",
            "",
            office(&[
                ("word/document.xml", xml),
                ("word/_rels/document.xml.rels", rels),
                ("word/numbering.xml", numbering),
            ]),
        );
        let text = read_for_chat(&source, &never, &mut |_| {}).unwrap();
        assert!(!text.contains("# Café &amp;"));
        assert!(text.contains("# Café & heaps"), "{text}");
        assert!(
            text.contains("**Minimum** first. [Read more](https://example.com/heaps)"),
            "{text}"
        );
        assert!(text.contains("3. Insert\n  - Sift up"), "{text}");
        assert!(
            text.contains("| Operation | Cost |\n| --- | --- |\n| Insert \\| push | O(log n) |"),
            "{text}"
        );
    }

    #[test]
    fn native_sources_follow_slide_and_notes_relationships() {
        let presentation = r#"<p:presentation><p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/slide" Target="slides/slide1.xml"/><Relationship Id="rId2" Type="http://x/slide" Target="slides/slide2.xml"/></Relationships>"#;
        let slide = r#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Heaps</a:t></a:r></a:p><a:p><a:pPr lvl="0"><a:buChar char="•"/></a:pPr><a:r><a:t>Minimum first</a:t></a:r></a:p></p:txBody></p:sp><p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Cost</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr><a:tc><a:txBody><a:p><a:r><a:t>O(log n)</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame></p:spTree></p:cSld></p:sld>"#;
        let slide_rels = r#"<Relationships><Relationship Id="rId7" Type="http://x/notesSlide" Target="../notesSlides/notesSlide9.xml"/></Relationships>"#;
        let notes = r#"<p:notes><p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Keep this speaker note</a:t></a:r></a:p></p:txBody></p:sp></p:notes>"#;
        let source = upload(
            "deck.pptx",
            "",
            office(&[
                ("ppt/presentation.xml", presentation),
                ("ppt/_rels/presentation.xml.rels", rels),
                (
                    "ppt/slides/slide1.xml",
                    "<p:sld><a:p><a:r><a:t>Last</a:t></a:r></a:p></p:sld>",
                ),
                ("ppt/slides/slide2.xml", slide),
                ("ppt/slides/_rels/slide2.xml.rels", slide_rels),
                ("ppt/notesSlides/notesSlide9.xml", notes),
            ]),
        );
        let text = read_for_chat(&source, &never, &mut |_| {}).unwrap();
        assert!(text.starts_with("## Slide 1\n### Heaps"), "{text}");
        assert!(text.contains("- Minimum first"), "{text}");
        assert!(text.contains("| Cost |\n| --- |\n| O(log n) |"), "{text}");
        assert!(
            text.contains("Speaker notes: Keep this speaker note"),
            "{text}"
        );
        assert!(text.ends_with("## Slide 2\nLast"), "{text}");
    }

    #[test]
    fn native_sources_read_short_pdf_text_without_vision() {
        let source = upload(
            "short.pdf",
            "application/pdf",
            pdf_with(Some("Quiz"), Some(&[0xff, 0xd8, 0xff, 1])),
        );
        assert_eq!(
            read_for_chat(&source, &never, &mut |_| {}).unwrap(),
            "[page 1]\nQuiz"
        );
    }

    #[test]
    fn word_documents_keep_their_paragraphs_and_symbols() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Heaps &amp; queues</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Insert is </w:t></w:r><w:r><w:t>O(log n) &lt;fast&gt;</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>Caf&#233;</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = office(&[("word/document.xml", xml)]);
        let got = extract(&upload("notes.docx", "application/octet-stream", bytes)).unwrap();
        assert_eq!(
            got.text,
            "Heaps & queues\nInsert is O(log n) \\<fast\\>\n\nCafé"
        );
        assert!(got.images.is_empty());
    }

    #[test]
    fn slides_come_in_order_with_their_speaker_notes() {
        let slide = |t: &str| format!("<p:sld><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>");
        let bytes = office(&[
            ("ppt/slides/slide10.xml", &slide("Ten")),
            ("ppt/slides/slide2.xml", &slide("Two")),
            ("ppt/slides/slide1.xml", &slide("One")),
            (
                "ppt/notesSlides/notesSlide2.xml",
                &slide("Say this out loud"),
            ),
            ("ppt/notesSlides/notesSlide1.xml", &slide("1")),
        ]);
        let got = extract(&upload("deck.pptx", "", bytes)).unwrap().text;
        assert_eq!(got, "## Slide 1\nOne\n\n## Slide 2\nTwo\nSpeaker notes: Say this out loud\n\n## Slide 10\nTen");
    }

    #[test]
    fn photos_become_images_and_unknown_files_say_what_works() {
        let photo = extract(&upload("board.jpg", "image/jpeg", vec![1, 2])).unwrap();
        assert_eq!(photo.images.len(), 1);
        assert_eq!(photo.images[0].mime, "image/jpeg");
        let png = extract(&upload("scan.PNG", "", vec![1])).unwrap();
        assert_eq!(png.images[0].mime, "image/png");
        assert!(extract(&upload("x.heic", "", vec![]))
            .unwrap_err()
            .to_string()
            .contains("HEIC"));
        assert!(extract(&upload("old.doc", "", vec![]))
            .unwrap_err()
            .to_string()
            .contains(".docx"));
        assert!(extract(&upload("a.zip", "", vec![]))
            .unwrap_err()
            .to_string()
            .contains("PDF, Word"));
        assert!(extract(&upload("bad.docx", "", vec![1, 2, 3])).is_err());
        assert_eq!(
            extract(&upload("a.md", "text/markdown", b"# Hi".to_vec()))
                .unwrap()
                .text,
            "# Hi"
        );
    }

    fn pdf_with(text: Option<&str>, jpeg: Option<&[u8]>) -> Vec<u8> {
        pdf_with_pages(&[(text, jpeg, false)])
    }

    type PdfPart<'a> = (Option<&'a str>, Option<&'a [u8]>, bool);

    fn pdf_with_pages(parts: &[PdfPart<'_>]) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" },
        );
        let mut kids = Vec::new();
        for &(text, jpeg, full) in parts {
            let mut xobjects = lopdf::Dictionary::new();
            let mut ops = Vec::new();
            if let Some(text) = text {
                ops.extend([
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![50.into(), 700.into()]),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                ]);
            }
            if let Some(jpeg) = jpeg {
                let image = Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 400, "Height" => 500, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode" },
                    jpeg.to_vec(),
                );
                let image_id = doc.add_object(image);
                xobjects.set("Im1", image_id);
                ops.extend([
                    Operation::new("q", vec![]),
                    Operation::new(
                        "cm",
                        vec![
                            (if full { 612 } else { 400 }).into(),
                            0.into(),
                            0.into(),
                            (if full { 792 } else { 500 }).into(),
                            0.into(),
                            0.into(),
                        ],
                    ),
                    Operation::new("Do", vec!["Im1".into()]),
                    Operation::new("Q", vec![]),
                ]);
            }
            let content = Content { operations: ops };
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let resources =
                dictionary! { "Font" => dictionary! { "F1" => font_id }, "XObject" => xobjects };
            let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()] });
            kids.push(page_id.into());
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => parts.len() as i64 },
            ),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn a_pdf_with_text_is_read_and_a_scanned_one_gives_its_page_images() {
        let typed = pdf_with(
            Some("Dijkstra uses a priority queue to pick the closest vertex next"),
            None,
        );
        let got = extract(&upload("paper.pdf", "application/pdf", typed)).unwrap();
        assert!(got.text.contains("priority queue"), "{}", got.text);
        assert!(got.images.is_empty());
        let fake_jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
        let scanned = pdf_with(None, Some(&fake_jpeg));
        let got = extract(&upload("scan.pdf", "application/pdf", scanned)).unwrap();
        assert_eq!(got.images.len(), 1);
        assert_eq!(got.images[0].bytes, fake_jpeg);
        assert!(extract(&upload("empty.pdf", "", pdf_with(None, None))).is_err());
        assert!(extract(&upload("junk.pdf", "", b"not a pdf".to_vec())).is_err());
    }

    #[test]
    fn native_sources_only_send_scanned_pdf_pages_to_vision_in_either_order() {
        let jpeg = [0xff, 0xd8, 0xff, 1];
        for reversed in [false, true] {
            let mut parts = [(Some("Digital text that is long enough to pass the old whole-document heuristic."), None, false), (Some("2"), Some(jpeg.as_slice()), true), (Some("Quiz"), Some(jpeg.as_slice()), false)];
            if reversed {
                parts.reverse();
            }
            let source = upload("mixed.pdf", "", pdf_with_pages(&parts));
            let calls = Mutex::new(0);
            let see = |_: Prompt, images: &[Image], _: u32| {
                *calls.lock().unwrap() += 1;
                assert_eq!(images.len(), 1);
                Ok("Text from the scanned page".to_string())
            };
            let read = read_for_chat(&source, &see, &mut |_| {}).unwrap();
            assert!(
                read.contains("Digital text")
                    && read.contains("Quiz")
                    && read.contains("Text from the scanned page")
            );
            assert_eq!(*calls.lock().unwrap(), 1);
        }
    }

    #[test]
    fn native_sources_preserve_word_styles_equations_and_numbering_overrides() {
        let xml = r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Custom"/></w:pPr><w:r><w:t>Fractions</w:t></w:r></w:p><w:p><m:oMath><m:f><m:num><m:r><m:t>α</m:t></m:r></m:num><m:den><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:den></m:f></m:oMath></w:p><w:p><w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr><w:r><w:rPr><w:b w:val="0"/><w:i/></w:rPr><w:t>First</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p></w:body></w:document>"#;
        let styles = r#"<w:styles><w:style w:styleId="Custom"><w:basedOn w:val="Section"/></w:style><w:style w:styleId="Section"><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style></w:styles>"#;
        let numbers = r#"<w:numbering><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:start w:val="1"/></w:lvl></w:abstractNum><w:num w:numId="7"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride></w:num></w:numbering>"#;
        let source = upload(
            "math.docx",
            "",
            office(&[
                ("word/document.xml", xml),
                ("word/styles.xml", styles),
                ("word/numbering.xml", numbers),
            ]),
        );
        let text = read_for_chat(&source, &never, &mut |_| {}).unwrap();
        assert_eq!(
            text,
            "## Fractions\n$\\frac{α}{{x}^{2}}$\n5. *First*\n6. Second"
        );
    }

    #[test]
    fn native_sources_extract_chart_data_without_running_external_links() {
        let slide = r#"<p:sld><a:graphic><a:graphicData><c:chart r:id="rId1"/></a:graphicData></a:graphic></p:sld>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/chart" Target="../charts/chart1.xml"/><Relationship Id="rId2" Type="http://x/image" Target="http://127.0.0.1/private" TargetMode="External"/></Relationships>"#;
        let chart = r#"<c:chartSpace><c:chart><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Café</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="1"><c:v>B</c:v></c:pt><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt><c:pt idx="1"><c:v>7</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let source = upload(
            "chart.pptx",
            "",
            office(&[
                ("ppt/slides/slide1.xml", slide),
                ("ppt/slides/_rels/slide1.xml.rels", rels),
                ("ppt/charts/chart1.xml", chart),
            ]),
        );
        let text = read_for_chat(&source, &never, &mut |_| {}).unwrap();
        assert!(
            text.contains("| Category | Café |\n| --- | --- |\n| A | 3 |\n| B | 7 |"),
            "{text}"
        );
        assert!(!text.contains("127.0.0.1"));
    }

    #[test]
    fn native_sources_feed_markdown_to_one_study_note_request() {
        let xml = r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Heaps</w:t></w:r></w:p></w:body></w:document>"#;
        let material = gather(&[upload(
            "heaps.docx",
            "",
            office(&[("word/document.xml", xml)]),
        )])
        .unwrap();
        let calls = Mutex::new(0);
        let write = |p: Prompt, _: u32| {
            *calls.lock().unwrap() += 1;
            assert!(p.user.contains("# Heaps"));
            Ok("Heaps\n\nPolished notes".to_string())
        };
        let (title, body) = write_note(
            &material,
            "heaps.docx",
            None,
            "",
            &write,
            &never,
            &mut |_, _, _| {},
            Budget::FREE,
        )
        .unwrap();
        assert_eq!((title.as_str(), body.as_str()), ("Heaps", "Polished notes"));
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn high_resolution_scans_are_resized_without_rejecting_the_pdf() {
        use lopdf::dictionary;
        use std::io::Write;
        for predictor in [1, 15, 0] {
            let source = pdf_with(None, Some(&[0xff, 0xd8, 0xff, 1]));
            let mut doc = lopdf::Document::load_mem(&source).unwrap();
            let image = doc
                .objects
                .values_mut()
                .find_map(|o| {
                    o.as_stream_mut().ok().filter(|s| {
                        s.dict
                            .get(b"Subtype")
                            .is_ok_and(|v| v.as_name().is_ok_and(|n| n == b"Image"))
                    })
                })
                .unwrap();
            let (width, height) = (5000usize, 3600usize);
            let channels = if predictor == 15 { 3 } else { 1 };
            let raw = vec![160u8; width * height * channels];
            image.dict.set("Width", width as i64);
            image.dict.set("Height", height as i64);
            image.dict.set(
                "ColorSpace",
                if channels == 3 {
                    "DeviceRGB"
                } else {
                    "DeviceGray"
                },
            );
            image.dict.set("Filter", "FlateDecode");
            image.content = if predictor == 0 {
                image.dict.set("Filter", "DCTDecode");
                let mut jpeg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
                    .encode(
                        &raw,
                        width as u32,
                        height as u32,
                        image::ExtendedColorType::L8,
                    )
                    .unwrap();
                jpeg
            } else if predictor == 15 {
                image.dict.set("DecodeParms", dictionary! { "Predictor" => 15, "Colors" => channels as i64, "BitsPerComponent" => 8, "Columns" => width as i64 });
                figures::filtered_rows(&raw, width, height, channels).unwrap()
            } else {
                let mut encoder =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&raw).unwrap();
                encoder.finish().unwrap()
            };
            drop(raw);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            let original = bytes.clone();
            let material = extract(&upload("large-scan.pdf", "application/pdf", bytes)).unwrap();
            assert_eq!(material.images.len(), 1, "predictor {predictor}");
            let (w, h) = leo_core::attachments::dimensions(&material.images[0].bytes).unwrap();
            assert!(w <= 4096 && h <= 4096 && w < width as u32 && h < height as u32);
            assert!((f64::from(w) / f64::from(h) - width as f64 / height as f64).abs() < 0.001);
            assert_eq!(
                lopdf::Document::load_mem(&original)
                    .unwrap()
                    .get_page_images(*doc.get_pages().values().next().unwrap())
                    .unwrap()[0]
                    .width,
                width as i64
            );
            assert_eq!(material.images[0].mime, "image/png");
            let decoded = image::load_from_memory(&material.images[0].bytes).unwrap();
            assert_eq!(
                image::GenericImageView::get_pixel(&decoded, w / 2, h / 2).0,
                [160, 160, 160, 255]
            );
            drop(decoded);
            assert!(material
                .figures
                .iter()
                .all(|figure| figure.bytes == material.images[0].bytes));
            let file = upload("large-scan.pdf", "application/pdf", original);
            let read = read_for_chat(
                &file,
                &|_, images, _| {
                    assert_eq!(images.len(), 1);
                    let (w, h) = leo_core::attachments::dimensions(&images[0].bytes).unwrap();
                    assert!(w <= 4000 && h <= 4000);
                    Ok("Text from the attached high-resolution PDF".into())
                },
                &mut |_| {},
            )
            .unwrap();
            assert_eq!(read, "Text from the attached high-resolution PDF");
        }
    }

    #[test]
    fn native_sources_keep_png_scans_and_bound_pdf_decompression() {
        use lopdf::{dictionary, Object, Stream};
        let bytes = pdf_with(None, Some(&[0xff, 0xd8, 0xff, 1]));
        let mut doc = lopdf::Document::load_mem(&bytes).unwrap();
        let image = doc
            .objects
            .values_mut()
            .find_map(|o| {
                o.as_stream_mut().ok().filter(|s| {
                    s.dict
                        .get(b"Subtype")
                        .is_ok_and(|v| v.as_name().is_ok_and(|name| name == b"Image"))
                })
            })
            .unwrap();
        image.dict.set("Filter", "FlateDecode");
        image.content = figures::filtered_rows(&vec![50; 400 * 500 * 3], 400, 500, 3).unwrap();
        image.dict.set("DecodeParms", dictionary! { "Predictor" => 15, "Colors" => 3, "BitsPerComponent" => 8, "Columns" => 400 });
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        let material = extract(&upload("scan.pdf", "", out)).unwrap();
        assert_eq!(material.images.len(), 1);
        assert_eq!(material.images[0].mime, "image/png");
        assert_eq!(
            leo_core::attachments::dimensions(&material.images[0].bytes),
            Some((400, 500))
        );

        let page = *doc.get_pages().values().next().unwrap();
        let mut stream = Stream::new(dictionary! {}, vec![b' '; MOST_PDF_CONTENT_BYTES + 1]);
        stream.compress().unwrap();
        let id = doc.add_object(stream);
        doc.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Contents", Object::Reference(id));
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        let error = extract(&upload("bomb.pdf", "", out))
            .unwrap_err()
            .to_string();
        assert!(error.contains("too much content"), "{error}");
    }

    #[test]
    fn long_text_is_split_into_parts_without_losing_any() {
        let paragraph = "word ".repeat(1000);
        let text = [paragraph.as_str(); 12].join("\n\n");
        let most = Budget::FREE.chars();
        let parts = text_parts(&text, most);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.chars().count() <= most + 2));
        assert_eq!(parts.concat().matches("word").count(), 12_000);
        let huge = "x".repeat(most * 2 + 5);
        assert_eq!(text_parts(&huge, most).len(), 3);
        assert_eq!(
            text_parts(&text, Budget::AGENT.chars()).len(),
            1,
            "a big model reads it in one go"
        );
    }

    #[test]
    fn the_name_comes_from_the_file_then_a_lecture_or_week_number_in_it_or_on_its_first_page() {
        let upload = |name: &str| Upload {
            name: name.into(),
            mime: String::new(),
            bytes: vec![],
        };
        let text = |t: &str| Material {
            text: t.into(),
            ..Material::default()
        };
        let slides = text("# Lecture 1\nIntroduction to proofs");
        assert_eq!(
            label_for(&[upload("ma161_hw3.pdf")], &slides).as_deref(),
            Some("ma161_hw3")
        );
        assert_eq!(
            label_for(&[upload("Scan of Lecture 4.pdf")], &slides).as_deref(),
            Some("Lecture 4")
        );
        assert_eq!(
            label_for(&[upload("IMG_2034.jpg")], &slides).as_deref(),
            Some("Lecture 1")
        );
        assert_eq!(
            label_for(&[upload("slides.pdf")], &text("Graphs and trees")),
            None
        );
        assert_eq!(
            label_for(&[upload("a.pdf"), upload("b.pdf")], &slides),
            None
        );
    }

    #[test]
    fn a_file_named_like_ma161_hw3_keeps_that_name_in_its_title_short_or_long() {
        let users = Mutex::new(Vec::new());
        let write = |p: Prompt, _: u32| -> Result<String> {
            users.lock().unwrap().push(p.user.clone());
            if p.system.contains("name and summarize") {
                Ok("Eigenvalues and diagonalisation\n\nA problem set.".into())
            } else if p.system.contains("This is part") {
                Ok("## A section\n- point".into())
            } else {
                Ok("Eigenvalues\n\nFind them.\n\n## Problem 1\n- det".into())
            }
        };
        let short = Material {
            text: "Find the eigenvalues of A.".into(),
            ..Material::default()
        };
        let (title, _) = write_note(
            &short,
            "ma161_hw3.pdf",
            Some("ma161_hw3"),
            "",
            &write,
            &never,
            &mut |_, _, _| {},
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "ma161_hw3 – Eigenvalues");
        assert!(users.lock().unwrap()[0]
            .contains("This is \"ma161_hw3\", the name the user files it under"));
        let long = Material {
            text: vec!["word ".repeat(1000); 10].join("\n\n"),
            ..Material::default()
        };
        let (title, _) = write_note(
            &long,
            "ma161_hw3.pdf",
            Some("ma161_hw3"),
            "",
            &write,
            &never,
            &mut |_, _, _| {},
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "ma161_hw3 – Eigenvalues and diagonalisation");
        let (title, _) = write_note(
            &short,
            "IMG_2034.jpg",
            None,
            "",
            &write,
            &never,
            &mut |_, _, _| {},
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "Eigenvalues");
        assert!(!users
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains("the name the user files it under"));
    }

    #[test]
    fn a_short_file_is_one_request_and_a_long_one_is_written_in_parts_then_named() {
        let asked = Mutex::new(Vec::new());
        let write = |p: Prompt, _: u32| -> Result<String> {
            asked.lock().unwrap().push(p.system.clone());
            if p.system.contains("name and summarize") {
                Ok("Graphs and queues\n\nA long handout about graphs.".into())
            } else if p.system.contains("This is part") {
                Ok("## A section\n- point".into())
            } else {
                Ok("Heaps\n\nHow heaps work.\n\n## Insert\n- sift up".into())
            }
        };
        let saw = Mutex::new(0);
        let see = |p: Prompt, images: &[Image], _: u32| -> Result<String> {
            assert!(p.system.contains("photos or scans"));
            *saw.lock().unwrap() += images.len();
            Ok("## Whiteboard\n- drawing".into())
        };
        let short = Material {
            text: "Heaps keep the minimum on top.".into(),
            ..Material::default()
        };
        let mut steps = Vec::new();
        let (title, body) = write_note(
            &short,
            "heaps.txt",
            None,
            "",
            &write,
            &see,
            &mut |s, d, t| steps.push((s.to_string(), d, t)),
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "Heaps");
        assert!(body.starts_with("How heaps work."));
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert_eq!(steps.last().unwrap().1, steps.last().unwrap().2);

        asked.lock().unwrap().clear();
        let long = Material {
            text: vec!["word ".repeat(1000); 10].join("\n\n"),
            images: (0..8)
                .map(|i| Image {
                    mime: "image/jpeg".into(),
                    bytes: vec![i],
                })
                .collect(),
            figures: vec![],
        };
        let (title, body) = write_note(
            &long,
            "handout.pdf",
            None,
            "",
            &write,
            &see,
            &mut |_, _, _| {},
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "Graphs and queues");
        assert!(body.starts_with("A long handout about graphs."));
        assert!(body.contains("## Whiteboard"));
        assert_eq!(
            *saw.lock().unwrap(),
            8,
            "every page image is looked at once"
        );
        assert!(asked
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains("name and summarize"));
    }

    #[test]
    fn parts_are_written_at_the_same_time_and_kept_in_order() {
        use std::sync::atomic::AtomicUsize;
        let now = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let write = |p: Prompt, _: u32| -> Result<String> {
            if p.system.contains("name and summarize") {
                return Ok("Title\n\nSummary.".into());
            }
            let busy = now.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(busy, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(40));
            now.fetch_sub(1, Ordering::SeqCst);
            let part = p
                .system
                .split("This is part ")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .to_string();
            Ok(format!("## Part {part}"))
        };
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> { unreachable!() };
        let material = Material {
            text: vec!["word ".repeat(1000); 6].join("\n\n"),
            ..Material::default()
        };
        let budget = Budget {
            words: 1000,
            at_once: 3,
        };
        let mut steps = Vec::new();
        let (_, body) = write_note(
            &material,
            "a.pdf",
            None,
            "",
            &write,
            &see,
            &mut |_, d, t| steps.push((d, t)),
            budget,
        )
        .unwrap();
        let order: Vec<&str> = body.lines().filter(|l| l.starts_with("## Part")).collect();
        assert_eq!(
            order,
            [
                "## Part 1",
                "## Part 2",
                "## Part 3",
                "## Part 4",
                "## Part 5",
                "## Part 6"
            ]
        );
        assert_eq!(most.load(Ordering::SeqCst), 3);
        assert_eq!(steps.last(), Some(&(7, 7)));
        assert!(
            steps.windows(2).all(|w| w[0].0 <= w[1].0),
            "progress only goes forward"
        );

        let failing = |p: Prompt, _: u32| -> Result<String> {
            if p.system.contains("part 2 of") {
                anyhow::bail!("rate limited")
            }
            Ok("## fine".into())
        };
        let err = write_note(
            &material,
            "a.pdf",
            None,
            "",
            &failing,
            &see,
            &mut |_, _, _| {},
            budget,
        )
        .unwrap_err();
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn too_many_pages_or_nothing_to_read_is_said_plainly() {
        let many: Vec<Upload> = (0..41)
            .map(|i| upload(&format!("{i}.jpg"), "image/jpeg", vec![1]))
            .collect();
        assert!(gather(&many)
            .unwrap_err()
            .to_string()
            .contains("smaller groups"));
        assert!(gather(&[upload("blank.txt", "", b"   ".to_vec())]).is_err());
        let both = gather(&[
            upload("a.txt", "", b"alpha".to_vec()),
            upload("b.jpg", "image/jpeg", vec![9]),
        ])
        .unwrap();
        assert!(both.text.starts_with("# a.txt\n\nalpha"));
        assert_eq!(both.images.len(), 1);
    }
}
