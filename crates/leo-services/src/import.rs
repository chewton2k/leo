pub(crate) mod office;

use anyhow::{bail, Context, Result};

use std::io::{Read, Seek, SeekFrom, Write as _};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::ai::budget::Budget;
use crate::ai::chat::{self, Prompt};
use crate::ai::provider::Image;
use crate::figures::{self, Figure};

const PAGES_PER_LOOK: usize = 6;
const NOTE_TOKENS: u32 = 8_000;
const MOST_PDF_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MOST_PDF_OPERATIONS: usize = 300_000;
const MOST_PDF_DEPTH: usize = 128;
const PDF_WORD_GAP: i64 = -100;
const SCAN_TEXT_LETTERS: usize = 40;
const SCAN_PAGE_SHARE: f32 = 0.65;
const SPILL_AT: usize = 8 * 1024 * 1024;
const FREE_DISK_FLOOR: u64 = 1024 * 1024 * 1024;
const DISK_CHECK_EVERY: u64 = 64 * 1024 * 1024;
const LABEL_LOOK: usize = 2_000;

#[derive(Debug, Clone)]
pub struct Upload {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct Folder {
    dir: Mutex<Option<tempfile::TempDir>>,
    next: AtomicUsize,
}

#[derive(Debug, Default, Clone)]
pub struct Scratch(Arc<Folder>);

impl Scratch {
    fn path(&self, folder: &str, extension: &str) -> Result<PathBuf> {
        let mut dir = self
            .0
            .dir
            .lock()
            .map_err(|_| anyhow::anyhow!("leo could not keep the file it is reading"))?;
        if dir.is_none() {
            *dir = Some(
                tempfile::Builder::new()
                    .prefix(crate::ai::IMPORT_ROOM)
                    .tempdir()
                    .context("leo could not make a folder to read this file in")?,
            );
        }
        let Some(root) = dir.as_ref().map(|d| d.path().join(folder)) else {
            bail!("leo could not keep the file it is reading");
        };
        std::fs::create_dir_all(&root)?;
        let number = self.0.next.fetch_add(1, Ordering::Relaxed);
        Ok(root.join(format!("{number:05}.{extension}")))
    }

    fn keep_image(&self, mime: &str, bytes: &[u8]) -> Result<ImageFile> {
        let kind = mime.rsplit('/').next().unwrap_or("png");
        let path = self.path("scans", kind)?;
        std::fs::write(&path, bytes).context("leo could not keep a page it is reading")?;
        Ok(ImageFile {
            mime: mime.to_string(),
            path,
            scratch: self.clone(),
        })
    }

    pub(crate) fn keep_figure(&self, kind: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.path("figures", kind)?;
        let folder = path.parent().unwrap_or(&path);
        if fs2::available_space(folder)
            .is_ok_and(|free| free < FREE_DISK_FLOOR.saturating_add(bytes.len() as u64))
        {
            bail!("There is not enough free disk space to read this file.");
        }
        std::fs::write(&path, bytes).context("leo could not keep a figure it is reading")?;
        Ok(path)
    }
}

#[derive(Debug, Clone)]
pub struct ImageFile {
    pub mime: String,
    path: PathBuf,
    scratch: Scratch,
}

impl ImageFile {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Image> {
        let _held = &self.scratch;
        Ok(Image {
            mime: self.mime.clone(),
            bytes: std::fs::read(&self.path).context("leo could not read back a page it kept")?,
        })
    }
}

#[derive(Debug, Default)]
pub struct Spill {
    scratch: Scratch,
    buffer: String,
    file: Option<(PathBuf, std::fs::File)>,
    length: u64,
    content: bool,
    #[cfg(test)]
    free: Option<u64>,
}

impl Spill {
    fn new(scratch: &Scratch) -> Spill {
        let mut spill = Spill::default();
        spill.scratch = scratch.clone();
        spill
    }

    pub fn push_str(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.content |= !text.trim().is_empty();
        let before = self.length;
        self.length += text.len() as u64;
        if self.file.is_none() && self.buffer.len() + text.len() <= SPILL_AT {
            self.buffer.push_str(text);
            return Ok(());
        }
        if self.file.is_none() || before / DISK_CHECK_EVERY != self.length / DISK_CHECK_EVERY {
            self.room()?;
        }
        let buffered = std::mem::take(&mut self.buffer);
        let file = self.open()?;
        file.write_all(buffered.as_bytes())?;
        file.write_all(text.as_bytes())?;
        Ok(())
    }

    fn open(&mut self) -> Result<&mut std::fs::File> {
        if self.file.is_none() {
            let path = self.scratch.path("text", "md")?;
            let file = std::fs::File::options()
                .create_new(true)
                .append(true)
                .open(&path)
                .context("leo could not keep the text it is reading")?;
            self.file = Some((path, file));
        }
        match self.file.as_mut() {
            Some((_, file)) => Ok(file),
            None => bail!("leo could not keep the text it is reading"),
        }
    }

    fn room(&mut self) -> Result<()> {
        let path = match &self.file {
            Some((path, _)) => path.clone(),
            None => self.scratch.path("text", "md")?,
        };
        let folder = path.parent().unwrap_or(&path);
        #[cfg(test)]
        let free = self.free.or_else(|| fs2::available_space(folder).ok());
        #[cfg(not(test))]
        let free = fs2::available_space(folder).ok();
        if free.is_some_and(|free| free < FREE_DISK_FLOOR + DISK_CHECK_EVERY) {
            bail!("There is not enough free disk space to read this file.");
        }
        Ok(())
    }

    fn append(&mut self, other: &Spill) -> Result<()> {
        self.content |= other.content;
        self.length += other.length;
        let buffered = std::mem::take(&mut self.buffer);
        if self.file.is_none()
            && buffered.len() + other.buffer.len() <= SPILL_AT
            && other.file.is_none()
        {
            self.buffer = buffered;
            self.buffer.push_str(&other.buffer);
            return Ok(());
        }
        self.room()?;
        let file = self.open()?;
        file.write_all(buffered.as_bytes())?;
        match &other.file {
            Some((path, _)) => {
                std::io::copy(&mut std::fs::File::open(path)?, file)?;
            }
            None => file.write_all(other.buffer.as_bytes())?,
        }
        Ok(())
    }

    pub fn is_blank(&self) -> bool {
        !self.content
    }

    pub fn len(&self) -> u64 {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    fn bytes(&self, range: Range<u64>) -> Result<Vec<u8>> {
        let Some((path, _)) = &self.file else {
            let (from, to) = (usize::try_from(range.start)?, usize::try_from(range.end)?);
            return self
                .buffer
                .as_bytes()
                .get(from..to)
                .map(<[u8]>::to_vec)
                .context("leo asked for text past what it kept");
        };
        let mut file = std::fs::File::open(path).context("leo could not read back its text")?;
        file.seek(SeekFrom::Start(range.start))?;
        let mut out = Vec::new();
        file.take(range.end.saturating_sub(range.start))
            .read_to_end(&mut out)?;
        Ok(out)
    }

    fn reader(&self) -> Result<Box<dyn std::io::BufRead + '_>> {
        Ok(match &self.file {
            Some((path, _)) => Box::new(std::io::BufReader::new(
                std::fs::File::open(path).context("leo could not read back its text")?,
            )),
            None => Box::new(self.buffer.as_bytes()),
        })
    }

    pub fn read(&self, range: Range<u64>) -> Result<String> {
        String::from_utf8(self.bytes(range)?).context("leo kept text that is not text")
    }

    pub fn head(&self, chars: usize) -> Result<String> {
        let end = (chars as u64).saturating_mul(4).min(self.length);
        let bytes = self.bytes(0..end)?;
        Ok(String::from_utf8_lossy(&bytes)
            .chars()
            .take(chars)
            .collect())
    }

    pub fn parts(&self, most_chars: usize) -> Result<Vec<Range<u64>>> {
        match &self.file {
            Some((path, _)) => part_ranges(
                std::io::BufReader::new(std::fs::File::open(path)?),
                most_chars,
            ),
            None => part_ranges(self.buffer.as_bytes(), most_chars),
        }
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        if let Some((path, _)) = self.file.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Pieces {
    most: usize,
    ranges: Vec<Range<u64>>,
    start: u64,
    paragraphs: usize,
    chars: usize,
    text: bool,
}

impl Pieces {
    fn paragraph(&mut self, at: Range<u64>, next: u64, chars: usize, text: bool, marks: &[u64]) {
        if self.paragraphs > 0 && self.chars + chars > self.most {
            self.ranges.push(self.start..at.start);
            self.start = at.start;
            (self.paragraphs, self.chars, self.text) = (0, 0, false);
        }
        if chars > self.most {
            let mut starts = vec![at.start];
            starts.extend(marks.iter().copied().filter(|m| *m < at.end));
            for (i, from) in starts.iter().enumerate() {
                let to = starts.get(i + 1).copied().unwrap_or(at.end);
                self.ranges.push(*from..to);
            }
            self.start = next;
        } else {
            self.paragraphs += 1;
            self.chars += chars + 2;
            self.text |= text;
        }
    }
}

fn part_ranges<R: Read>(mut reader: R, most: usize) -> Result<Vec<Range<u64>>> {
    let mut pieces = Pieces {
        most: most.max(1),
        ranges: Vec::new(),
        start: 0,
        paragraphs: 0,
        chars: 0,
        text: false,
    };
    let mut buffer = vec![0u8; 64 * 1024];
    let mut position = 0u64;
    let mut newline = false;
    let (mut para_start, mut para_chars, mut para_text) = (0u64, 0usize, false);
    let mut marks: Vec<u64> = Vec::new();
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for &byte in &buffer[..read] {
            if byte & 0xC0 != 0x80 {
                if para_chars > 0 && para_chars % pieces.most == 0 {
                    marks.push(position);
                }
                para_chars += 1;
            }
            para_text |= !byte.is_ascii_whitespace();
            if byte == b'\n' && newline {
                pieces.paragraph(
                    para_start..position - 1,
                    position + 1,
                    para_chars - 2,
                    para_text,
                    &marks,
                );
                (para_start, para_chars, para_text) = (position + 1, 0, false);
                marks.clear();
                newline = false;
            } else {
                newline = byte == b'\n';
            }
            position += 1;
        }
    }
    pieces.paragraph(
        para_start..position,
        position,
        para_chars,
        para_text,
        &marks,
    );
    if pieces.paragraphs > 0 && pieces.text {
        pieces.ranges.push(pieces.start..position);
    }
    Ok(pieces.ranges)
}

#[derive(Debug, Default)]
pub struct Material {
    pub text: Spill,
    pub images: Vec<ImageFile>,
    pub figures: Vec<Figure>,
    pub scratch: Scratch,
}

impl Material {
    fn new(scratch: &Scratch) -> Material {
        Material {
            text: Spill::new(scratch),
            scratch: scratch.clone(),
            ..Material::default()
        }
    }

    fn with_text(scratch: &Scratch, text: &str) -> Result<Material> {
        let mut material = Material::new(scratch);
        material.text.push_str(text.trim())?;
        Ok(material)
    }
}

#[derive(Debug, Default)]
pub struct Imported {
    pub title: String,
    pub body: String,
    pub figures: Vec<Figure>,
    pub scratch: Scratch,
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
        return Ok(false);
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

fn pdf_text_operands(
    text: &mut String,
    encoding: &lopdf::Encoding<'_>,
    operands: &[lopdf::Object],
) -> lopdf::Result<()> {
    for operand in operands {
        match operand {
            lopdf::Object::String(bytes, _) => encoding.write_to_string(bytes, text)?,
            lopdf::Object::Array(items) => {
                pdf_text_operands(text, encoding, items)?;
                text.push(' ');
            }
            lopdf::Object::Integer(spacing) if *spacing < PDF_WORD_GAP => text.push(' '),
            _ => {}
        }
    }
    Ok(())
}

fn pdf_page_text(doc: &lopdf::Document, page: lopdf::ObjectId) -> lopdf::Result<String> {
    let mut first_error = None;
    let mut encodings = std::collections::BTreeMap::new();
    for (name, font) in doc.get_page_fonts(page)? {
        match font.get_font_encoding_with_limit(doc, MOST_PDF_CONTENT_BYTES) {
            Ok(encoding) => {
                encodings.insert(name, encoding);
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    let content = lopdf::content::Content::decode(
        &doc.get_page_content_with_limit(page, MOST_PDF_CONTENT_BYTES)?,
    )?;
    let mut encoding = None;
    let mut text = String::new();
    let mut chunk_start = 0;
    for operation in content.operations {
        let operator = operation.operator.as_str();
        let needs_newline = text.len() == chunk_start || !text.ends_with('\n');
        match (operator, encoding) {
            ("Tf", _) => {
                let font = operation
                    .operands
                    .first()
                    .ok_or_else(|| lopdf::Error::Syntax("missing font operand".to_string()))?;
                encoding = match font.as_name() {
                    Ok(name) => encodings.get(name),
                    Err(error) => {
                        first_error.get_or_insert(error);
                        None
                    }
                };
                chunk_start = text.len();
            }
            ("Tj" | "TJ" | "'" | "\"", Some(encoding)) => {
                if needs_newline && matches!(operator, "'" | "\"") {
                    text.push('\n');
                }
                let operands = if operator == "\"" {
                    operation.operands.get(2..3).unwrap_or_default()
                } else {
                    &operation.operands
                };
                if let Err(error) = pdf_text_operands(&mut text, encoding, operands) {
                    first_error.get_or_insert(error);
                }
            }
            ("T*" | "ET", _) if needs_newline => text.push('\n'),
            _ => {}
        }
    }
    first_error.map_or(Ok(text), Err)
}

fn pdf(bytes: &[u8], scratch: &Scratch) -> Result<Material> {
    let doc = lopdf::Document::load_mem(bytes)
        .context("this PDF could not be opened (it may be damaged or password-protected)")?;
    let mut pages = doc.page_iter().peekable();
    if pages.peek().is_none() {
        bail!("this PDF has no pages");
    }
    let mut material = Material::new(scratch);
    let mut prepared = std::collections::HashMap::new();
    for (index, page) in pages.enumerate() {
        let number = (index + 1) as u32;
        let extracted = match pdf_page_text(&doc, page) {
            Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
                ..
            })) => bail!("a PDF page expands to too much content; export a simpler PDF"),
            result => result.unwrap_or_default(),
        };
        let extracted = extracted.trim();
        if !extracted.is_empty() {
            if !material.text.is_empty() {
                material.text.push_str("\n\n")?;
            }
            material
                .text
                .push_str(&format!("[page {number}]\n{extracted}"))?;
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
            let (width, height) = (image.width, image.height);
            if width
                .checked_mul(height)
                .is_none_or(|area| area > figures::MOST_PDF_PIXELS as i64)
            {
                bail!("page {number} holds a scan of {width} by {height} pixels, more than the {} million pixels leo reads; export that page at a lower resolution, then try again", figures::MOST_PDF_PIXELS / 1_000_000);
            }
            let bytes = figures::pdf_figure(&doc, &image).with_context(|| format!("page {number} contains a scanned image of {width} by {height} pixels that leo could not read safely; export that page as a standard JPEG or PNG, then try again"))?;
            let mime = leo_core::attachments::kind_of(&bytes)
                .and_then(leo_core::attachments::mime_of)
                .unwrap_or("image/png");
            let file = scratch.keep_image(mime, &bytes)?;
            prepared.insert(image.id, file.clone());
            material.images.push(file);
        }
    }
    if material.images.is_empty() && material.text.is_blank() {
        bail!("this PDF looks scanned, but leo cannot read its pages; take photos of them, or export them as images, and upload those");
    }
    material.figures = figures::from_pdf_prepared(&doc, &prepared, scratch)?;
    Ok(material)
}

pub fn extract(upload: &Upload) -> Result<Material> {
    read_into(upload, &Scratch::default())
}

struct SpillWriter<'a>(&'a mut Spill);

impl std::io::Write for SpillWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.0.push_str(text).map_err(std::io::Error::other)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn read_whole(path: &Path, name: &str) -> Result<Vec<u8>> {
    let mut file = leo_core::paths::open_regular(path)
        .with_context(|| format!("leo could not open {name}"))?;
    let mut bytes = Vec::with_capacity(file.metadata().map_or(0, |m| m.len() as usize));
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_into(upload: &Upload, scratch: &Scratch) -> Result<Material> {
    let ext = extension(&upload.name);
    let mime = upload.mime.to_lowercase();
    let is_photo = matches!(
        mime.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    );
    let streamed = match &upload.path {
        Some(path) if !is_photo && matches!(ext.as_str(), "txt" | "md" | "markdown" | "text") => {
            let mut file = leo_core::paths::open_regular(path)
                .with_context(|| format!("leo could not open {}", upload.name))?;
            let mut material = Material::new(scratch);
            copy_text(&mut file, &mut SpillWriter(&mut material.text))?;
            return Ok(material);
        }
        Some(path) => Some(read_whole(path, &upload.name)?),
        None => None,
    };
    let bytes: &[u8] = streamed.as_deref().unwrap_or(&upload.bytes);
    let text = |t: &str| Material::with_text(scratch, t);
    let photo = |mime: String| -> Result<Material> {
        let image = scratch.keep_image(&mime, bytes)?;
        Ok(Material {
            figures: vec![Figure {
                mime,
                path: image.path().to_path_buf(),
                place: upload.name.clone(),
                photo: true,
            }],
            images: vec![image],
            ..Material::new(scratch)
        })
    };
    if is_photo {
        return photo(mime);
    }
    match ext.as_str() {
        "pdf" => pdf(bytes, scratch),
        "docx" => {
            let mut material = Material::new(scratch);
            office::docx(bytes, &mut material.text)?;
            material.figures = figures::from_docx(bytes, scratch)?;
            Ok(material)
        }
        "pptx" => {
            let mut material = Material::new(scratch);
            office::pptx(bytes, &mut material.text)?;
            material.figures = figures::from_pptx(bytes, scratch)?;
            Ok(material)
        }
        "txt" | "md" | "markdown" | "text" => text(&String::from_utf8_lossy(bytes)),
        "jpg" | "jpeg" | "png" | "webp" | "gif" => photo(format!(
            "image/{}",
            if ext == "jpg" { "jpeg" } else { ext.as_str() }
        )),
        "heic" | "heif" => bail!("{} is a HEIC photo; upload it from the phone's photo picker, which converts it, or save it as JPEG", upload.name),
        "doc" | "ppt" => bail!("{} is an old Office format; save it as .{}x and upload that", upload.name, ext),
        _ => bail!("leo cannot read {} yet: upload a PDF, Word, PowerPoint, text file, or photos", upload.name),
    }
}

pub fn gather(uploads: &[Upload]) -> Result<Material> {
    let scratch = Scratch::default();
    let mut all = Material::new(&scratch);
    for upload in uploads {
        let found = read_into(upload, &scratch)?;
        if !found.text.is_blank() {
            if uploads.len() > 1 {
                all.text.push_str(&format!("# {}\n\n", upload.name))?;
            }
            all.text.append(&found.text)?;
            all.text.push_str("\n\n")?;
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
    if all.text.is_blank() && all.images.is_empty() {
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

#[cfg(test)]
fn text_parts(text: &str, most: usize) -> Vec<String> {
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

fn marker(place: &str) -> Option<String> {
    let (kind, rest) = place.split_once(' ')?;
    let number: u64 = rest.split(' ').next()?.parse().ok()?;
    match kind {
        "page" => Some(format!("[page {number}]")),
        "slide" => Some(format!("## Slide {number}\n")),
        _ => None,
    }
}

pub fn offered(figures: &[Figure], text: &str, part: usize, parts: usize) -> String {
    let listed: Vec<(usize, &Figure)> = figures
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.photo)
        .collect();
    let by_position = listed
        .iter()
        .filter(|(_, f)| marker(&f.place).is_none())
        .count();
    let mut seen = 0;
    listed
        .into_iter()
        .filter(|(_, f)| match marker(&f.place) {
            _ if parts <= 1 => true,
            Some(marker) => text.contains(&marker),
            None => {
                seen += 1;
                (seen - 1) * parts / by_position == part
            }
        })
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

const TRANSCRIBE_TOKENS: u32 = 6_000;

const TRANSCRIBING: &str = "\
You copy out what is on photographed or scanned pages so someone can ask questions about them later. Write the text exactly as it appears, in reading order, in Markdown: keep headings, lists, tables, and formulas written in LaTeX ($...$ inline, $$...$$ on their own lines). Describe each diagram or figure in one short sentence in [brackets]. Write [unreadable] for anything you cannot read. Reply with the text only.";

fn seen_pages(
    images: &[ImageFile],
    name: &str,
    see: See<'_>,
    progress: &mut dyn FnMut(&str),
    mut each: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let groups: Vec<&[ImageFile]> = images.chunks(PAGES_PER_LOOK).collect();
    for (i, pages) in groups.iter().enumerate() {
        progress(&if groups.len() > 1 {
            format!("Reading the pages {}/{}", i + 1, groups.len())
        } else {
            "Reading the image".to_string()
        });
        let prompt = Prompt {
            system: TRANSCRIBING.to_string(),
            user: format!("These are pages from \"{name}\". Copy out what they say."),
        };
        let pages = pages
            .iter()
            .map(ImageFile::load)
            .collect::<Result<Vec<_>>>()?;
        each(see(prompt, &pages, TRANSCRIBE_TOKENS)?.trim())?;
    }
    Ok(())
}

pub fn read_for_chat(
    upload: &Upload,
    see: See<'_>,
    progress: &mut dyn FnMut(&str),
) -> Result<String> {
    let material = extract(upload)?;
    let mut text = material
        .text
        .read(0..material.text.len())?
        .trim()
        .to_string();
    seen_pages(&material.images, &upload.name, see, progress, |seen| {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(seen);
        Ok(())
    })?;
    if text.trim().is_empty() {
        bail!("there is no text in {} for Felix to read", upload.name);
    }
    Ok(text)
}

const TEXT_PIECE: usize = 64 * 1024;

fn incomplete_tail(bytes: &[u8]) -> usize {
    for back in 1..=bytes.len().min(3) {
        let byte = bytes[bytes.len() - back];
        if byte & 0xC0 == 0x80 {
            continue;
        }
        let needs = match byte {
            0xF0..=0xF7 => 4,
            0xE0..=0xEF => 3,
            0xC0..=0xDF => 2,
            _ => 1,
        };
        return if needs > back { back } else { 0 };
    }
    0
}

fn copy_text(file: &mut impl Read, out: &mut dyn std::io::Write) -> Result<bool> {
    let mut buffer = vec![0u8; TEXT_PIECE + 3];
    let mut held = 0;
    let mut content = false;
    loop {
        let got = file.read(&mut buffer[held..held + TEXT_PIECE])?;
        if got == 0 {
            break;
        }
        let end = held + got;
        let whole = end - incomplete_tail(&buffer[..end]);
        let text = String::from_utf8_lossy(&buffer[..whole]);
        content = content || !text.trim().is_empty();
        out.write_all(text.as_bytes())?;
        buffer.copy_within(whole..end, 0);
        held = end - whole;
    }
    if held > 0 {
        out.write_all(String::from_utf8_lossy(&buffer[..held]).as_bytes())?;
    }
    Ok(content)
}

pub fn read_to(
    path: &Path,
    name: &str,
    mime: &str,
    see: See<'_>,
    progress: &mut dyn FnMut(&str),
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let text_file = !mime.to_lowercase().starts_with("image/")
        && matches!(extension(name).as_str(), "txt" | "md" | "markdown" | "text");
    let wrote = if text_file {
        let mut file = leo_core::paths::open_regular(path)
            .with_context(|| format!("leo could not open {name}"))?;
        copy_text(&mut file, out)?
    } else {
        let upload = Upload {
            name: name.to_string(),
            mime: mime.to_string(),
            bytes: Vec::new(),
            path: Some(path.to_path_buf()),
        };
        let material = read_into(&upload, &Scratch::default())?;
        drop(upload);
        let mut wrote = !material.text.is_blank();
        if !material.text.is_empty() {
            std::io::copy(&mut material.text.reader()?, out)?;
        }
        seen_pages(&material.images, name, see, progress, |seen| {
            if wrote {
                out.write_all(b"\n\n")?;
            }
            out.write_all(seen.as_bytes())?;
            wrote = wrote || !seen.is_empty();
            Ok(())
        })?;
        wrote
    };
    if !wrote {
        bail!("there is no text in {name} for Felix to read");
    }
    Ok(())
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
    let texts = if material.text.is_blank() {
        Vec::new()
    } else {
        material.text.parts(budget.chars())?
    };
    let looks: Vec<&[ImageFile]> = material.images.chunks(PAGES_PER_LOOK).collect();
    let load = |batch: &[ImageFile]| {
        batch
            .iter()
            .map(ImageFile::load)
            .collect::<Result<Vec<_>>>()
    };
    let jobs = texts.len() + looks.len();
    if jobs == 0 {
        bail!("there is nothing in that file for leo to read");
    }
    if jobs == 1 {
        progress("Writing the note", 0, 1);
        let reply = if let Some(range) = texts.first() {
            let text = material.text.read(range.clone())?;
            let figures = offered(&material.figures, &text, 0, 1);
            write(
                named(text_prompt(&text, name, None, &figures, wants), label),
                NOTE_TOKENS,
            )?
        } else {
            let n = material.images.len();
            see(
                named(image_prompt(name, None, (1, n, n), wants), label),
                &load(looks[0])?,
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
        let reply = if let Some(range) = texts.get(i) {
            let text = material.text.read(range.clone())?;
            let figures = offered(&material.figures, &text, i, texts.len());
            write(
                text_prompt(&text, name, Some((i + 1, jobs)), &figures, wants),
                NOTE_TOKENS,
            )?
        } else {
            let j = i - texts.len();
            let batch = looks[j];
            let pages = (j * PAGES_PER_LOOK + 1, j * PAGES_PER_LOOK + batch.len(), n);
            see(
                image_prompt(name, Some((i + 1, jobs)), pages, wants),
                &load(batch)?,
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

fn label_for(uploads: &[Upload], material: &Material) -> Result<Option<String>> {
    use leo_core::filename::{file_label, series_label};
    Ok(match uploads {
        [one] => match file_label(&one.name).or_else(|| series_label(&one.name)) {
            Some(label) => Some(label),
            None => series_label(&material.text.head(LABEL_LOOK)?),
        },
        _ => None,
    })
}

fn said(e: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(crate::ai::writing_error(&e))
}

pub fn import(
    uploads: &[Upload],
    wants: &str,
    write: Write<'_>,
    see: See<'_>,
    progress: &mut dyn FnMut(&str, usize, usize),
) -> Result<Imported> {
    progress("Reading the file", 0, 1);
    let material = gather(uploads)?;
    let name = uploads.first().map(|u| u.name.as_str()).unwrap_or("upload");
    let write = |prompt: Prompt, most: u32| write(prompt, most).map_err(said);
    let see = |prompt: Prompt, images: &[Image], most: u32| see(prompt, images, most).map_err(said);
    let budget = crate::ai::writing_budget();
    let label = label_for(uploads, &material)?;
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
        scratch: material.scratch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn made(text: &str, images: Vec<Image>) -> Material {
        let scratch = Scratch::default();
        let mut material = Material::with_text(&scratch, text).unwrap();
        for image in images {
            material
                .images
                .push(scratch.keep_image(&image.mime, &image.bytes).unwrap());
        }
        material
    }

    fn text_of(material: &Material) -> String {
        material.text.read(0..material.text.len()).unwrap()
    }

    fn pixels(file: &ImageFile) -> Vec<u8> {
        file.load().unwrap().bytes
    }

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
        let long = made(
            &vec!["word ".repeat(1000); 10].join("\n\n"),
            (0..8)
                .map(|i| Image {
                    mime: "image/jpeg".into(),
                    bytes: vec![i],
                })
                .collect(),
        );
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
            path: None,
        };
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps".to_vec(),
            path: None,
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
                path: PathBuf::new(),
                place: "slide 3".into(),
                photo: false,
            },
        ];
        let listed = offered(&figures, "## Slide 3\ntext", 0, 1);
        assert_eq!(listed, "figure:2 is from slide 3");
        let with = text_prompt("x", "deck.pptx", None, &listed, "");
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
    fn a_document_for_felix_keeps_only_its_text_and_is_kept_whole() {
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps\n\nMinimum at the root.".to_vec(),
            path: None,
        };
        let read = read_for_chat(&text, &never, &mut |_| {}).unwrap();
        assert_eq!(read, "# Heaps\n\nMinimum at the root.");
        let long = Upload {
            name: "long.txt".into(),
            bytes: "word ".repeat(420_000).into_bytes(),
            mime: "text/plain".into(),
            path: None,
        };
        let whole = read_for_chat(&long, &never, &mut |_| {}).unwrap();
        assert_eq!(whole.chars().count(), 5 * 420_000 - 1);
        assert!(!whole.contains("left out"));
        let blank = Upload {
            name: "blank.txt".into(),
            mime: "text/plain".into(),
            bytes: b"   ".to_vec(),
            path: None,
        };
        assert!(read_for_chat(&blank, &never, &mut |_| {}).is_err());
    }

    #[test]
    fn a_provider_rejection_reaches_import_and_the_reader_in_the_shared_words() {
        use std::io::Read;
        const SENTINEL: &str = "SENTINEL-reflected-source-excerpt";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
                    let mut buffer = [0u8; 65536];
                    let _ = stream.read(&mut buffer);
                    let body =
                        format!("{{\"error\":{{\"message\":\"bad request near: {SENTINEL}\"}}}}");
                    let _ = write!(
                        stream,
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        let mut cfg = crate::config::Config::default();
        cfg.chat.chain = vec!["ollama".into()];
        cfg.providers.insert(
            "ollama".into(),
            crate::config::provider::ProviderConfig {
                kind: Some(crate::config::provider::ProviderKind::Openai),
                base_url: Some(format!("http://127.0.0.1:{port}/v1")),
                model: Some("llama3.2".into()),
                ..Default::default()
            },
        );
        let store = crate::config::secret::MemoryStore::default();
        let write = |prompt: Prompt, most: u32| -> Result<String> {
            crate::ai::chat::complete(&cfg, &store, prompt, most)
                .map(|outcome| outcome.value)
                .map_err(said)
        };
        let see = |prompt: Prompt, images: &[Image], most: u32| -> Result<String> {
            let req = crate::ai::provider::ChatRequest {
                system: Some(prompt.system),
                prompt: prompt.user,
                temperature: 0.2,
                max_tokens: most,
            };
            let providers = crate::ai::provider::build_chat_chain(&cfg, &store);
            crate::ai::chain::run_vision_chain(providers, &req, images)
                .map(|outcome| outcome.value)
                .map_err(said)
        };
        let ask = Prompt {
            system: "s".into(),
            user: "u".into(),
        };
        let shared = write(ask, 10).unwrap_err().to_string();
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps\nThe minimum sits at the root.".to_vec(),
            path: None,
        };
        let material = gather(&[text]).unwrap();
        let imported = write_note(
            &material,
            "notes.md",
            None,
            "",
            &write,
            &see,
            &mut |_, _, _| {},
            crate::ai::budget::writing_budget(&cfg),
        )
        .unwrap_err()
        .to_string();
        let photo = Upload {
            name: "board.jpg".into(),
            mime: "image/jpeg".into(),
            bytes: vec![0xff, 0xd8, 0xff],
            path: None,
        };
        let read = read_for_chat(&photo, &see, &mut |_| {})
            .unwrap_err()
            .to_string();
        assert!(!shared.contains(SENTINEL), "{shared}");
        assert_eq!(imported, shared);
        assert_eq!(read, shared);
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
            path: None,
        };
        let mut steps = Vec::new();
        let read = read_for_chat(&photo, &see, &mut |s| steps.push(s.to_string())).unwrap();
        assert_eq!(read, "Board: Dijkstra uses a heap");
        assert_eq!(*looked.lock().unwrap(), 1);
        assert_eq!(steps, ["Reading the image"]);
    }

    fn read_file(name: &str, mime: &str, bytes: &[u8]) -> Result<String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> { Ok("Board text".into()) };
        let mut out = Vec::new();
        read_to(&path, name, mime, &see, &mut |_| {}, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_text_file_is_copied_whole_across_a_multibyte_join() {
        let mut text = "a".repeat(TEXT_PIECE - 1);
        text.push('é');
        text.push_str(&"漢字 and more\n".repeat(30_000));
        assert!(text.len() > 3 * TEXT_PIECE);
        assert_eq!(
            read_file("long.md", "text/markdown", text.as_bytes()).unwrap(),
            text
        );
        let mut lossy = b"caf".to_vec();
        lossy.extend([0xe9, b' ', 0xe2, 0x82]);
        assert_eq!(
            read_file("old.txt", "text/plain", &lossy).unwrap(),
            "caf\u{fffd} \u{fffd}"
        );
    }

    #[test]
    fn a_blank_text_file_or_one_that_is_not_a_file_cannot_be_read() {
        assert!(read_file("empty.txt", "text/plain", b" \n").is_err());
        let dir = tempfile::tempdir().unwrap();
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> { Ok(String::new()) };
        assert!(read_to(
            dir.path(),
            "x.txt",
            "text/plain",
            &see,
            &mut |_| {},
            &mut Vec::new()
        )
        .is_err());
    }

    #[test]
    fn a_photo_and_an_office_file_are_written_to_the_stream() {
        assert_eq!(
            read_file("board.jpg", "image/jpeg", &[0xff, 0xd8, 0xff]).unwrap(),
            "Board text"
        );
        let docx = office(&[(
            "word/document.xml",
            "<w:document><w:body><w:p><w:r><w:t>Heaps</w:t></w:r></w:p></w:body></w:document>",
        )]);
        let read = read_file(
            "notes.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &docx,
        )
        .unwrap();
        assert!(read.contains("Heaps"), "{read}");
    }

    #[test]
    #[ignore = "writes a 100 MB sparse file"]
    fn a_text_upload_over_ninety_megabytes_is_read_for_chat() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"first line\n").unwrap();
        file.set_len(100 * 1024 * 1024).unwrap();
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> { Ok(String::new()) };
        let mut counted = 0u64;
        struct Count<'a>(&'a mut u64);
        impl std::io::Write for Count<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                *self.0 += bytes.len() as u64;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        read_to(
            &path,
            "big.txt",
            "text/plain",
            &see,
            &mut |_| {},
            &mut Count(&mut counted),
        )
        .unwrap();
        assert_eq!(counted, 100 * 1024 * 1024);
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
            path: None,
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
        let source = pdf_upload(
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
            text_of(&got),
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
        let got = text_of(&extract(&upload("deck.pptx", "", bytes)).unwrap());
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
            text_of(&extract(&upload("a.md", "text/markdown", b"# Hi".to_vec())).unwrap()),
            "# Hi"
        );
    }

    fn pdf_with(text: Option<&str>, jpeg: Option<&[u8]>) -> Vec<u8> {
        pdf_with_pages(&[(text, jpeg, false)])
    }

    fn pdf_upload(name: &str, mime: &str, bytes: Vec<u8>) -> Upload {
        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        for (number, page) in doc.get_pages() {
            let old = doc.extract_text_with_limit(&[number], MOST_PDF_CONTENT_BYTES);
            assert_eq!(
                pdf_page_text(&doc, page).map_err(|error| format!("{error:?}")),
                old.map_err(|error| format!("{error:?}")),
                "{name}, page {number}"
            );
        }
        upload(name, mime, bytes)
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
        let got = extract(&pdf_upload("paper.pdf", "application/pdf", typed)).unwrap();
        assert!(text_of(&got).contains("priority queue"));
        assert!(got.images.is_empty());
        let fake_jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
        let scanned = pdf_with(None, Some(&fake_jpeg));
        let got = extract(&pdf_upload("scan.pdf", "application/pdf", scanned)).unwrap();
        assert_eq!(got.images.len(), 1);
        assert_eq!(pixels(&got.images[0]), fake_jpeg);
        assert!(extract(&pdf_upload("empty.pdf", "", pdf_with(None, None))).is_err());
        assert!(extract(&upload("junk.pdf", "", b"not a pdf".to_vec())).is_err());
    }

    #[test]
    fn pdf_pages_keep_their_own_font_and_text_spacing() {
        use lopdf::{dictionary, Document, Stream};

        let mut doc = Document::load_mem(&pdf_with_pages(&[(None, None, false); 2])).unwrap();
        for ((_, page), encoding) in doc
            .get_pages()
            .into_iter()
            .zip(["WinAnsiEncoding", "MacRomanEncoding"])
        {
            let font = doc.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier", "Encoding" => encoding,
            });
            let content = doc.add_object(Stream::new(
                dictionary! {},
                br#"BT (ignored) Tj /F1 12 Tf (\200) Tj [[(A) -101 (B)] -100 (C)] TJ (next) ' 1 2 (last) " T* /F1 12 Tf T* (tail) Tj ET"#.to_vec(),
            ));
            let page = doc.get_dictionary_mut(page).unwrap();
            page.set(
                "Resources",
                dictionary! { "Font" => dictionary! { "F1" => font } },
            );
            page.set("Contents", content);
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let material = extract(&pdf_upload("fonts.pdf", "", bytes)).unwrap();
        assert_eq!(
            text_of(&material),
            "[page 1]\n€A B C \nnext\nlast\n\ntail\n\n[page 2]\nÄA B C \nnext\nlast\n\ntail"
        );
    }

    #[test]
    fn native_sources_only_send_scanned_pdf_pages_to_vision_in_either_order() {
        let jpeg = [0xff, 0xd8, 0xff, 1];
        for reversed in [false, true] {
            let mut parts = [(Some("Digital text that is long enough to pass the old whole-document heuristic."), None, false), (Some("2"), Some(jpeg.as_slice()), true), (Some("Quiz"), Some(jpeg.as_slice()), false)];
            if reversed {
                parts.reverse();
            }
            let source = pdf_upload("mixed.pdf", "", pdf_with_pages(&parts));
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
            let material =
                extract(&pdf_upload("large-scan.pdf", "application/pdf", bytes)).unwrap();
            assert_eq!(material.images.len(), 1, "predictor {predictor}");
            let (w, h) = leo_core::attachments::dimensions(&pixels(&material.images[0])).unwrap();
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
            let decoded = image::load_from_memory(&pixels(&material.images[0])).unwrap();
            assert_eq!(
                image::GenericImageView::get_pixel(&decoded, w / 2, h / 2).0,
                [160, 160, 160, 255]
            );
            drop(decoded);
            assert!(material
                .figures
                .iter()
                .all(|figure| figure.path == material.images[0].path()));
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
    fn a_scan_past_four_thousand_megapixels_is_refused_with_its_size() {
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
        image.dict.set("Width", 70_000);
        image.dict.set("Height", 70_000);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let refused = extract(&pdf_upload("huge.pdf", "application/pdf", bytes)).unwrap_err();
        let said = format!("{refused:#}");
        assert!(
            said.contains("page 1") && said.contains("70000 by 70000 pixels"),
            "{said}"
        );
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
        let material = extract(&pdf_upload("scan.pdf", "", out)).unwrap();
        assert_eq!(material.images.len(), 1);
        assert_eq!(material.images[0].mime, "image/png");
        assert_eq!(
            leo_core::attachments::dimensions(&pixels(&material.images[0])),
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
        let error = extract(&pdf_upload("bomb.pdf", "", out))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "a PDF page expands to too much content; export a simpler PDF"
        );

        let font = doc
            .objects
            .values_mut()
            .filter_map(|object| object.as_dict_mut().ok())
            .find(|dict| dict.has_type(b"Font"))
            .unwrap();
        font.set("ToUnicode", id);
        for (content, refused) in [(b"/F1 12 Tf".as_slice(), true), (b"Tf".as_slice(), false)] {
            let content = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("Contents", content);
            let mut out = Vec::new();
            doc.save_to(&mut out).unwrap();
            let result = extract(&pdf_upload("font-limit.pdf", "", out));
            if refused {
                assert_eq!(result.unwrap_err().to_string(), error);
            } else {
                assert_eq!(result.unwrap().images.len(), 1);
            }
        }
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

    fn same_parts(text: &str, most: usize) {
        let mut spill = Spill::default();
        spill.push_str(text).unwrap();
        let kept: Vec<String> = spill
            .parts(most)
            .unwrap()
            .into_iter()
            .map(|range| spill.read(range).unwrap().trim_end().to_string())
            .collect();
        let expected: Vec<String> = text_parts(text, most)
            .iter()
            .map(|part| part.trim_end().to_string())
            .collect();
        assert_eq!(kept, expected, "{text:?} at {most}");
    }

    #[test]
    fn long_text_is_spilled_and_read_back_in_parts() {
        let paragraph = "word ".repeat(1000);
        let mut spill = Spill::default();
        let mut text = String::new();
        for i in 0..1_900 {
            if i > 0 {
                text.push_str("\n\n");
            }
            text.push_str(&paragraph);
        }
        for piece in text.as_bytes().chunks(1_000_000) {
            spill.push_str(std::str::from_utf8(piece).unwrap()).unwrap();
        }
        assert!(text.len() > SPILL_AT);
        assert!(
            spill.file.is_some(),
            "text past 8 MB goes to the scratch folder"
        );
        assert!(spill.buffer.is_empty());
        let most = Budget::FREE.chars();
        let ranges = spill.parts(most).unwrap();
        let expected = text_parts(&text, most);
        assert_eq!(ranges.len(), expected.len());
        for (range, part) in ranges.into_iter().zip(&expected) {
            assert_eq!(spill.read(range).unwrap().trim_end(), part.trim_end());
        }
        let path = spill.file.as_ref().unwrap().0.clone();
        drop(spill);
        assert!(!path.exists());
        for most in [1, 3, 7, 10, 40] {
            for sample in [
                "",
                "   \n\n  ",
                "ab",
                "ab\n\ncdé\n\n\nxyzxyzxyzxyzxyz\nq\n",
                "é€𝄞é€𝄞é€𝄞é€𝄞\n\nz",
                "\n\n\n\na\n\n",
                "aaaa bbbb\n\ncc\n\n\n\ndddddddddddddddddddddd\n\nee",
                "abcdefghij",
                "abcdefghij\n\nk",
            ] {
                same_parts(sample, most);
            }
        }
    }

    #[test]
    fn a_pdf_past_two_thousand_and_forty_eight_pages_is_read() {
        let parts: Vec<PdfPart<'_>> = (0..2_100)
            .map(|_| {
                (
                    Some("The scheduler picks the next ready process"),
                    None,
                    false,
                )
            })
            .collect();
        let got = extract(&pdf_upload(
            "long.pdf",
            "application/pdf",
            pdf_with_pages(&parts),
        ))
        .unwrap();
        let text = text_of(&got);
        assert!(text.starts_with("[page 1]\n"));
        assert!(text.contains("[page 2048]\n") && text.contains("[page 2100]\n"));
        assert_eq!(text.matches("[page ").count(), 2_100);
    }

    #[test]
    fn a_pdf_with_forty_one_scanned_pages_is_read_six_at_a_time() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
        let parts: Vec<PdfPart<'_>> = (0..41).map(|_| (None, Some(&jpeg[..]), true)).collect();
        let material = extract(&pdf_upload(
            "scan.pdf",
            "application/pdf",
            pdf_with_pages(&parts),
        ))
        .unwrap();
        assert_eq!(material.images.len(), 41);
        let folder = material.images[0].path.parent().unwrap().to_path_buf();
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 41);
        let looks = Mutex::new(Vec::new());
        let write = |_: Prompt, _: u32| -> Result<String> { Ok("Scans\n\nPages.".into()) };
        let see = |_: Prompt, images: &[Image], _: u32| -> Result<String> {
            looks.lock().unwrap().push(images.len());
            Ok("## Pages\n- text".into())
        };
        write_note(
            &material,
            "scan.pdf",
            None,
            "",
            &write,
            &see,
            &mut |_, _, _| {},
            Budget::LOCAL,
        )
        .unwrap();
        assert_eq!(*looks.lock().unwrap(), [6, 6, 6, 6, 6, 6, 5]);
        drop(material);
        assert!(!folder.exists(), "the scratch folder goes with the import");
    }

    #[test]
    fn a_drawing_heavy_page_no_longer_refuses_the_pdf() {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Object, Stream};
        let mut doc = lopdf::Document::load_mem(&pdf_with(Some("Fig 1"), None)).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        let mut operations = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.into()]),
            Operation::new("Tj", vec![Object::string_literal("Fig 1")]),
            Operation::new("ET", vec![]),
        ];
        operations.extend((0..MOST_PDF_OPERATIONS + 1).map(|_| Operation::new("n", vec![])));
        let content = Content { operations };
        let id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        doc.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Contents", Object::Reference(id));
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        let got = extract(&pdf_upload("drawing.pdf", "", out)).unwrap();
        assert!(text_of(&got).contains("Fig 1"));
    }

    #[test]
    fn each_part_is_offered_its_own_figures() {
        let figure = |place: &str| Figure {
            mime: "image/png".into(),
            path: PathBuf::new(),
            place: place.into(),
            photo: false,
        };
        let figures = vec![
            figure("page 1"),
            figure("page 900"),
            figure("the document"),
            figure("the document"),
        ];
        let first = offered(&figures, "[page 1]\nHeaps", 0, 2);
        let second = offered(&figures, "[page 900]\nTries", 1, 2);
        assert_eq!(
            first,
            "figure:1 is from page 1\nfigure:3 is from the document"
        );
        assert_eq!(
            second,
            "figure:2 is from page 900\nfigure:4 is from the document"
        );
        assert_eq!(offered(&figures, "no markers", 0, 1).lines().count(), 4);
        assert_eq!(offered(&[figure("slide 1")], "## Slide 10\ntext", 0, 2), "");
    }

    #[test]
    fn forty_one_photos_are_read() {
        let many: Vec<Upload> = (0..41)
            .map(|i| upload(&format!("{i}.jpg"), "image/jpeg", vec![1]))
            .collect();
        let gathered = gather(&many).unwrap();
        assert_eq!(gathered.images.len(), 41);
        assert_eq!(gathered.figures.len(), 41);
        assert_eq!(pixels(&gathered.images[40]), [1]);
    }

    #[test]
    fn the_name_comes_from_the_file_then_a_lecture_or_week_number_in_it_or_on_its_first_page() {
        let upload = |name: &str| Upload {
            name: name.into(),
            mime: String::new(),
            bytes: vec![],
            path: None,
        };
        let text = |t: &str| made(t, vec![]);
        let slides = text("# Lecture 1\nIntroduction to proofs");
        assert_eq!(
            label_for(&[upload("ma161_hw3.pdf")], &slides)
                .unwrap()
                .as_deref(),
            Some("ma161_hw3")
        );
        assert_eq!(
            label_for(&[upload("Scan of Lecture 4.pdf")], &slides)
                .unwrap()
                .as_deref(),
            Some("Lecture 4")
        );
        assert_eq!(
            label_for(&[upload("IMG_2034.jpg")], &slides)
                .unwrap()
                .as_deref(),
            Some("Lecture 1")
        );
        assert_eq!(
            label_for(&[upload("slides.pdf")], &text("Graphs and trees")).unwrap(),
            None
        );
        assert_eq!(
            label_for(&[upload("a.pdf"), upload("b.pdf")], &slides).unwrap(),
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
        let short = made("Find the eigenvalues of A.", vec![]);
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
        let long = made(&vec!["word ".repeat(1000); 10].join("\n\n"), vec![]);
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
        let short = made("Heaps keep the minimum on top.", vec![]);
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
        let long = made(
            &vec!["word ".repeat(1000); 10].join("\n\n"),
            (0..8)
                .map(|i| Image {
                    mime: "image/jpeg".into(),
                    bytes: vec![i],
                })
                .collect(),
        );
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
        let material = made(&vec!["word ".repeat(1000); 6].join("\n\n"), vec![]);
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
    fn nothing_to_read_is_said_plainly() {
        assert!(gather(&[upload("blank.txt", "", b"   ".to_vec())]).is_err());
        let both = gather(&[
            upload("a.txt", "", b"alpha".to_vec()),
            upload("b.jpg", "image/jpeg", vec![9]),
        ])
        .unwrap();
        assert!(text_of(&both).starts_with("# a.txt\n\nalpha"));
        assert_eq!(both.images.len(), 1);
    }

    fn resident_kb() -> u64 {
        leo_core::children::quiet_command("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
            .unwrap_or(0)
    }

    fn scan_jpeg() -> Vec<u8> {
        let mut seed = 0x2545_f491_u32;
        let picture = image::RgbImage::from_fn(400, 500, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            image::Rgb([
                (x / 2) as u8 ^ (seed & 255) as u8,
                (y / 2) as u8,
                ((x * y) >> 6) as u8,
            ])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75)
            .encode_image(&picture)
            .unwrap();
        out
    }

    fn long_pdf(kind: &str, pages: usize) -> Vec<u8> {
        let words: Vec<&str> =
            "heap queue priority sift down insert extract root parent child array index"
                .split(' ')
                .collect();
        let page: String = (0..400)
            .map(|i| words[i * 7 % words.len()])
            .collect::<Vec<_>>()
            .join(" ");
        let jpeg = scan_jpeg();
        let parts: Vec<PdfPart<'_>> = (0..pages)
            .map(|_| {
                if kind == "scan" {
                    (None, Some(jpeg.as_slice()), true)
                } else {
                    (Some(page.as_str()), None, false)
                }
            })
            .collect();
        pdf_with_pages(&parts)
    }

    pub(super) fn measured_read(file: &str, name: &str) {
        let bytes = std::fs::read(file).unwrap();
        let baseline = resident_kb();
        let peak = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(baseline));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sampler = {
            let (peak, done) = (peak.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    peak.fetch_max(resident_kb(), Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            })
        };
        let started = std::time::Instant::now();
        let outcome = (|| -> Result<(usize, usize, usize)> {
            let material = extract(&Upload {
                name: name.into(),
                mime: String::new(),
                bytes,
                path: None,
            })?;
            let parts = std::sync::atomic::AtomicUsize::new(0);
            let looks = std::sync::atomic::AtomicUsize::new(0);
            let write = |_: Prompt, _: u32| -> Result<String> {
                parts.fetch_add(1, Ordering::SeqCst);
                Ok("Heaps\n\nHow heaps work.".into())
            };
            let see = |_: Prompt, images: &[Image], _: u32| -> Result<String> {
                looks.fetch_add(images.len(), Ordering::SeqCst);
                Ok("## Page\n- text".into())
            };
            write_note(
                &material,
                name,
                None,
                "",
                &write,
                &see,
                &mut |_, _, _| {},
                Budget::AGENT,
            )?;
            Ok((
                parts.load(Ordering::SeqCst),
                looks.load(Ordering::SeqCst),
                0,
            ))
        })();
        let seconds = started.elapsed().as_secs_f64();
        done.store(true, Ordering::SeqCst);
        sampler.join().unwrap();
        println!(
            "RESULT file {} MB: {} in {seconds:.1} s, peak {} MB beyond the file",
            std::fs::metadata(file).unwrap().len() / 1_048_576,
            match outcome {
                Ok((parts, pages, _)) =>
                    format!("read, {parts} text parts, {pages} scanned pages seen"),
                Err(e) => format!("refused ({e})"),
            },
            (peak.load(Ordering::SeqCst).saturating_sub(baseline)) / 1024,
        );
    }

    #[test]
    #[ignore]
    fn benchmark_long_pdf() {
        if let Ok(file) = std::env::var("LEO_BENCH_PDF_FILE") {
            measured_read(&file, "long.pdf");
            return;
        }
        let cases = std::env::var("LEO_BENCH_CASES")
            .unwrap_or_else(|_| "text:2000,text:10000,scan:40,scan:400".into());
        let dir = tempfile::tempdir().unwrap();
        for case in cases.split(',') {
            let (kind, pages) = case.split_once(':').unwrap();
            let file = dir.path().join("long.pdf");
            std::fs::write(&file, long_pdf(kind, pages.parse().unwrap())).unwrap();
            let out = leo_core::children::quiet_command(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "--nocapture",
                    "import::tests::benchmark_long_pdf",
                ])
                .env("LEO_BENCH_PDF_FILE", &file)
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            println!(
                "{case}: {}",
                text.lines()
                    .find(|l| l.starts_with("RESULT"))
                    .unwrap_or("no result")
            );
        }
    }

    fn on_disk(dir: &Path, name: &str, bytes: &[u8]) -> Upload {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        Upload {
            name: name.into(),
            mime: String::new(),
            bytes: Vec::new(),
            path: Some(path),
        }
    }

    #[test]
    fn a_file_on_disk_is_read_from_its_path_and_no_size_refuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let line = "A line of a long lecture.\n";
        let text = line.repeat((95 * 1024 * 1024) / line.len() + 1);
        assert!(text.len() > 90 * 1024 * 1024);
        let upload = on_disk(dir.path(), "lecture.txt", text.as_bytes());
        let material = extract(&upload).unwrap();
        assert_eq!(material.text.len(), text.len() as u64);
        assert_eq!(
            material.text.head(26).unwrap(),
            "A line of a long lecture.\n"
        );
        let pdf = on_disk(
            dir.path(),
            "paper.pdf",
            &pdf_with_pages(&[(Some("Heaps are trees."), None, false)]),
        );
        assert!(extract(&pdf)
            .unwrap()
            .text
            .head(100)
            .unwrap()
            .contains("Heaps are trees."));
    }

    #[test]
    #[ignore = "writes 200 MB and prints the memory it took"]
    fn a_large_text_file_on_disk_is_read_in_little_memory() {
        let dir = tempfile::tempdir().unwrap();
        let line = "A line of a long lecture about heaps and queues, kept short.\n";
        let upload = {
            let path = dir.path().join("book.txt");
            let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            for _ in 0..(200 * 1024 * 1024 / line.len()) {
                file.write_all(line.as_bytes()).unwrap();
            }
            drop(file);
            Upload {
                name: "book.txt".into(),
                mime: "text/plain".into(),
                bytes: Vec::new(),
                path: Some(path),
            }
        };
        let baseline = resident_kb();
        let peak = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(baseline));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sampler = {
            let (peak, done) = (peak.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    peak.fetch_max(resident_kb(), Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            })
        };
        let started = std::time::Instant::now();
        let material = extract(&upload).unwrap();
        let parts = std::sync::atomic::AtomicUsize::new(0);
        let write = |_: Prompt, _: u32| -> Result<String> {
            parts.fetch_add(1, Ordering::SeqCst);
            Ok("Heaps\n\nHow heaps work.".into())
        };
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> { Ok(String::new()) };
        write_note(
            &material,
            "book.txt",
            None,
            "",
            &write,
            &see,
            &mut |_, _, _| {},
            Budget::AGENT,
        )
        .unwrap();
        done.store(true, Ordering::SeqCst);
        sampler.join().unwrap();
        println!(
            "200 MB text from disk: {} parts, {:?}, memory beyond the start {} KB",
            parts.load(Ordering::SeqCst),
            started.elapsed(),
            peak.load(Ordering::SeqCst).saturating_sub(baseline)
        );
    }
}
