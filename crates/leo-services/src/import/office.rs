use std::collections::HashMap;
use std::io::{BufRead, BufReader, Cursor, Read};

use anyhow::{bail, Context, Result};
use quick_xml::events::Event;
use roxmltree::{Document, Node, ParsingOptions};

use super::Spill;

const MOST_PART_BYTES: usize = 16 * 1024 * 1024;
const MOST_PART_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const MOST_NODES: u32 = 300_000;
const MOST_DEPTH: usize = 32;
const MOST_COLUMNS: usize = 16_384;
const MOST_LEVEL: usize = 8;
const MOST_STYLE_HOPS: usize = 16;
const KEEP_AT: usize = 64 * 1024;
const TOO_BIG: &str = "this Office file expands to more text than any document; export it again";
const TOO_MUCH_AT_ONCE: &str =
    "this Office file has too much text in one paragraph, table row or slide; export it again";
const DAMAGED: &str = "this Office file has damaged or unsupported XML; export it again";
const UNREADABLE: &str =
    "this Office file has unreadable text; export it again as a new Office file";

struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> Package<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).context(
            "this Office file could not be opened; export it again and upload the new file",
        )?;
        Ok(Self { zip })
    }

    fn part(&mut self, name: &str) -> Result<Option<zip::read::ZipFile<'_, Cursor<&'a [u8]>>>> {
        match self.zip.by_name(name) {
            Ok(file) => Ok(Some(file)),
            Err(zip::result::ZipError::FileNotFound) => Ok(None),
            Err(_) => bail!("a part of this Office file could not be read; export it again"),
        }
    }

    fn text(&mut self, name: &str) -> Result<Option<String>> {
        let Some(mut file) = self.part(name)? else {
            return Ok(None);
        };
        if file.size() > MOST_PART_BYTES as u64 {
            bail!("this Office file expands to too much text; split it into smaller documents");
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MOST_PART_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .context("a part of this Office file is damaged; export it again")?;
        if bytes.len() > MOST_PART_BYTES {
            bail!("this Office file expands to too much text; split it into smaller documents");
        }
        String::from_utf8(bytes).map(Some).context(UNREADABLE)
    }

    fn stream(
        &mut self,
        name: &str,
    ) -> Result<BufReader<zip::read::ZipFile<'_, Cursor<&'a [u8]>>>> {
        let file = self
            .part(name)?
            .context("this Office file is missing a required part; export it again")?;
        if file.size() > MOST_PART_SIZE {
            bail!(TOO_BIG);
        }
        Ok(BufReader::new(file))
    }

    fn required(&mut self, name: &str) -> Result<String> {
        self.text(name)?
            .context("this Office file is missing a required part; export it again")
    }
    fn relationships(&mut self, name: &str) -> Result<HashMap<String, Link>> {
        let parent = name.rsplit_once('/').map_or("", |(p, _)| p);
        let file = name.rsplit('/').next().unwrap_or(name);
        let rels = format!("{parent}/_rels/{file}.rels");
        let Some(xml) = self.text(&rels)? else {
            return Ok(HashMap::new());
        };
        let doc = parse(&xml)?;
        let mut links = HashMap::new();
        for node in doc.descendants().filter(|n| is(*n, "Relationship")) {
            let (Some(id), Some(kind), Some(target)) =
                (attr(node, "Id"), attr(node, "Type"), attr(node, "Target"))
            else {
                continue;
            };
            let external = attr(node, "TargetMode") == Some("External");
            let target = if external {
                target.to_string()
            } else {
                joined(parent, target)?
            };
            links.insert(
                id.to_string(),
                Link {
                    kind: kind.rsplit('/').next().unwrap_or(kind).to_string(),
                    target,
                    external,
                },
            );
        }
        Ok(links)
    }
}

struct Link {
    kind: String,
    target: String,
    external: bool,
}

pub(crate) fn joined(parent: &str, target: &str) -> Result<String> {
    if target.contains(['\\', ':', '\0', '?', '#']) {
        bail!("this Office file has an invalid internal link; export it again");
    }
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        parent.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    bail!("this Office file has a link outside the document; export it again");
                }
            }
            part => parts.push(part),
        }
    }
    if !matches!(parts.first(), Some(&"word" | &"ppt")) {
        bail!("this Office file has a link outside its document parts; export it again");
    }
    Ok(parts.join("/"))
}

fn parse(xml: &str) -> Result<Document<'_>> {
    if xml.len() > MOST_PART_BYTES {
        bail!("this Office file expands to too much text; split it into smaller documents");
    }
    let doc = Document::parse_with_options(
        xml,
        ParsingOptions {
            nodes_limit: MOST_NODES,
            ..ParsingOptions::default()
        },
    )
    .map_err(|_| {
        anyhow::anyhow!("this Office file has damaged or unsupported XML; export it again")
    })?;
    if doc
        .descendants()
        .any(|n| n.ancestors().take(MOST_DEPTH + 1).count() > MOST_DEPTH)
    {
        bail!("this Office file is nested too deeply; export a simpler document");
    }
    Ok(doc)
}

fn is(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    if name == "id" {
        if let Some(attribute) = node.attributes().find(|a| {
            a.name() == name
                && a.namespace()
                    .is_some_and(|ns| ns.ends_with("/relationships"))
        }) {
            return Some(attribute.value());
        }
    }
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|n| is(*n, name))
}

fn value<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|n| attr(n, "val"))
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn destination(target: &str) -> Option<String> {
    if target.starts_with('#') && !target.chars().any(char::is_control) {
        return Some(target.replace(['(', ')', ' ', '<', '>'], ""));
    }
    let url = url::Url::parse(target).ok()?;
    if !matches!(url.scheme(), "https" | "http" | "mailto") {
        return None;
    }
    Some(url.as_str().replace('(', "%28").replace(')', "%29"))
}

fn linked(text: String, target: Option<&str>) -> String {
    match target.and_then(destination) {
        Some(target) if !text.trim().is_empty() => format!("[{text}]({target})"),
        _ => text,
    }
}

fn enabled(node: Node<'_, '_>, name: &str) -> bool {
    child(node, name).is_some_and(|n| !matches!(attr(n, "val"), Some("0" | "false" | "off")))
}

fn math(node: Node<'_, '_>, depth: usize) -> String {
    if depth >= MOST_DEPTH {
        return String::new();
    }
    let children = || {
        node.children()
            .filter(Node::is_element)
            .map(|n| math(n, depth + 1))
            .collect::<String>()
    };
    let part = |name| {
        child(node, name)
            .map(|n| math(n, depth + 1))
            .unwrap_or_default()
    };
    match node.tag_name().name() {
        "t" => node.text().unwrap_or_default().to_string(),
        "f" => format!("\\frac{{{}}}{{{}}}", part("num"), part("den")),
        "sSup" => format!("{{{}}}^{{{}}}", part("e"), part("sup")),
        "sSub" => format!("{{{}}}_{{{}}}", part("e"), part("sub")),
        "sSubSup" => format!("{{{}}}_{{{}}}^{{{}}}", part("e"), part("sub"), part("sup")),
        "rad" => {
            let degree = part("deg");
            if degree.is_empty() {
                format!("\\sqrt{{{}}}", part("e"))
            } else {
                format!("\\sqrt[{degree}]{{{}}}", part("e"))
            }
        }
        name if name.ends_with("Pr") => String::new(),
        _ => children(),
    }
}

fn inline(node: Node<'_, '_>, links: &HashMap<String, Link>, depth: usize) -> String {
    if depth >= MOST_DEPTH {
        return String::new();
    }
    let content = || {
        node.children()
            .filter(Node::is_element)
            .map(|n| inline(n, links, depth + 1))
            .collect::<String>()
    };
    match node.tag_name().name() {
        "t" => escape(node.text().unwrap_or_default()),
        "tab" => "\t".to_string(),
        "br" | "cr" => "\n".to_string(),
        "del" | "delText" | "instrText" | "drawing" | "pict" => String::new(),
        "oMath" => format!("${}$", math(node, 0)),
        "r" => {
            let text = content();
            let Some(props) = child(node, "rPr") else {
                return text;
            };
            let bold = enabled(props, "b") || attr(props, "b") == Some("1");
            let italic = enabled(props, "i") || attr(props, "i") == Some("1");
            let text = if text.trim().is_empty() {
                text
            } else {
                let leading: String = text.chars().take_while(|c| c.is_whitespace()).collect();
                let trailing: String = text
                    .chars()
                    .rev()
                    .take_while(|c| c.is_whitespace())
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                let mark = match (bold, italic) {
                    (true, true) => "***",
                    (true, false) => "**",
                    (false, true) => "*",
                    _ => "",
                };
                format!("{leading}{mark}{}{mark}{trailing}", text.trim())
            };
            let target = child(props, "hlinkClick")
                .and_then(|n| attr(n, "id"))
                .and_then(|id| links.get(id))
                .filter(|l| l.kind == "hyperlink")
                .map(|l| l.target.as_str());
            linked(text, target)
        }
        "hyperlink" => {
            let anchor = attr(node, "anchor").map(|a| format!("#{a}"));
            let target = attr(node, "id")
                .and_then(|id| links.get(id))
                .filter(|l| l.kind == "hyperlink")
                .map(|l| l.target.as_str())
                .or(anchor.as_deref());
            linked(content(), target)
        }
        name if name.ends_with("Pr") => String::new(),
        _ => content(),
    }
}

fn row_line(row: &[String], columns: usize) -> String {
    let mut out = String::from("|");
    for col in 0..columns {
        let cell = row.get(col).map(String::as_str).unwrap_or_default();
        let cell = cell
            .trim()
            .replace('|', "\\|")
            .replace(['\n', '\r'], "<br>");
        out.push_str(&format!(" {cell} |"));
    }
    out
}

fn rule(columns: usize) -> String {
    format!("|{}", " --- |".repeat(columns))
}

fn table(rows: Vec<Vec<String>>) -> Result<String> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Ok(String::new());
    }
    if columns.saturating_mul(rows.len()) > MOST_NODES as usize {
        bail!("this document has a table inside a table that is too large; export it again");
    }
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        out.push_str(&row_line(row, columns));
        out.push('\n');
        if i == 0 {
            out.push_str(&rule(columns));
            out.push('\n');
        }
    }
    Ok(out.trim_end().to_string())
}

struct Text<'a> {
    spill: &'a mut Spill,
    kept: String,
    held: String,
    fresh: bool,
    first: bool,
    blank: bool,
    pending: Vec<(usize, String)>,
    frames: Vec<bool>,
}

impl<'a> Text<'a> {
    fn new(spill: &'a mut Spill) -> Self {
        Text {
            spill,
            kept: String::new(),
            held: String::new(),
            fresh: true,
            first: true,
            blank: false,
            pending: Vec::new(),
            frames: Vec::new(),
        }
    }

    fn push(&mut self, text: &str) -> Result<()> {
        let text = if self.fresh { text.trim_start() } else { text };
        let body = text.trim_end();
        if body.is_empty() {
            if !self.fresh {
                self.held.push_str(text);
            }
            return Ok(());
        }
        self.kept.push_str(&self.held);
        self.kept.push_str(body);
        self.held.clear();
        self.held
            .push_str(text.get(body.len()..).unwrap_or_default());
        self.fresh = false;
        if self.kept.len() >= KEEP_AT {
            self.spill.push_str(&self.kept)?;
            self.kept.clear();
        }
        Ok(())
    }

    fn trim_from_here(&mut self) {
        self.fresh = true;
    }

    fn trim_to_here(&mut self) {
        if !self.fresh {
            self.held.clear();
        }
        self.fresh = false;
    }

    fn item(&mut self, first: &mut bool, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if !std::mem::take(first) {
            self.push("\n")?;
        }
        self.push(text)
    }

    fn line(&mut self, line: &str) -> Result<()> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let empty = line.trim().is_empty();
        if empty && self.blank {
            return Ok(());
        }
        self.blank = empty;
        if !std::mem::take(&mut self.first) {
            self.push("\n")?;
        }
        self.push(line)
    }

    fn segment(&mut self, line: &str) -> Result<()> {
        if line.trim().is_empty() {
            self.hold(self.frames.len(), line.to_string());
            return Ok(());
        }
        for (_, blank) in std::mem::take(&mut self.pending) {
            self.line(&blank)?;
        }
        if let Some(top) = self.frames.last_mut() {
            *top = true;
        }
        self.line(line)
    }

    fn segments(&mut self, text: &str) -> Result<()> {
        text.split('\n').try_for_each(|line| self.segment(line))
    }

    fn hold(&mut self, depth: usize, line: String) {
        if let [.., (before, _), (at, last)] = &mut self.pending[..] {
            if *before == depth && *at == depth {
                *last = line;
                return;
            }
        }
        self.pending.push((depth, line));
    }

    fn open(&mut self) {
        self.frames.push(false);
    }

    fn close(&mut self) {
        let depth = self.frames.len();
        let had = self.frames.pop().unwrap_or(true);
        if self
            .pending
            .last()
            .is_some_and(|(at, line)| *at == depth && line.is_empty())
        {
            self.pending.pop();
        }
        let kept = match self.pending.iter().position(|(at, _)| *at == depth) {
            Some(at) => {
                self.pending.truncate(at + 1);
                self.pending.pop().map(|(_, line)| line)
            }
            None => None,
        };
        match kept {
            Some(line) => self.hold(depth.saturating_sub(1), line),
            None if !had => self.hold(depth.saturating_sub(1), String::new()),
            None => {}
        }
        if had {
            if let Some(top) = self.frames.last_mut() {
                *top = true;
            }
        }
    }

    fn finish(self) -> Result<()> {
        self.spill.push_str(&self.kept)
    }
}

struct Counted<R> {
    inner: R,
    since: usize,
    total: u64,
    refused: Option<&'static str>,
}

impl<R: BufRead> Read for Counted<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let got = self.fill_buf()?;
        let n = got.len().min(out.len());
        if let (Some(to), Some(from)) = (out.get_mut(..n), got.get(..n)) {
            to.copy_from_slice(from);
        }
        self.consume(n);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for Counted<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.since > MOST_PART_BYTES {
            self.refused = Some(TOO_MUCH_AT_ONCE);
        } else if self.total > MOST_PART_SIZE {
            self.refused = Some(TOO_BIG);
        }
        if let Some(said) = self.refused {
            return Err(std::io::Error::other(said));
        }
        self.inner.fill_buf()
    }

    fn consume(&mut self, n: usize) {
        self.since = self.since.saturating_add(n);
        self.total = self.total.saturating_add(n as u64);
        self.inner.consume(n);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Outside,
    Body,
    Holder,
    Table,
    Skip,
}

struct Open {
    tag: String,
    name: String,
    role: Role,
}

enum Piece<'a> {
    Block {
        xml: &'a str,
        levels: usize,
        depth: usize,
    },
    Open,
    Close,
    Table,
    Row {
        xml: &'a str,
        levels: usize,
        depth: usize,
    },
    TableEnd,
}

fn wrapped(stack: &[Open], piece: &[u8]) -> Result<String> {
    let mut xml = String::new();
    for open in stack {
        xml.push('<');
        xml.push_str(&open.tag);
        xml.push('>');
    }
    xml.push_str(std::str::from_utf8(piece).context(UNREADABLE)?);
    for open in stack.iter().rev() {
        xml.push_str("</");
        xml.push_str(&open.name);
        xml.push('>');
    }
    Ok(xml)
}

fn pieces(
    part: impl BufRead,
    rows_only: bool,
    mut each: impl FnMut(Piece<'_>) -> Result<()>,
) -> Result<bool> {
    let mut reader = quick_xml::Reader::from_reader(Counted {
        inner: part,
        since: 0,
        total: 0,
        refused: None,
    });
    let mut stack: Vec<Open> = Vec::new();
    let mut found = false;
    let mut buf = Vec::new();
    let mut piece = Vec::new();
    let mut inside = 0usize;
    let mut row = false;
    loop {
        if inside == 0 {
            reader.get_mut().since = 0;
        }
        buf.clear();
        let event = match reader.read_event_into(&mut buf) {
            Ok(event) => event,
            Err(quick_xml::Error::Io(_)) => match reader.get_ref().refused {
                Some(said) => bail!(said),
                None => bail!("a part of this Office file is damaged; export it again"),
            },
            Err(quick_xml::Error::Encoding(_)) => bail!(UNREADABLE),
            Err(_) => bail!(DAMAGED),
        };
        let ended = if inside > 0 {
            match &event {
                Event::Start(_) => inside += 1,
                Event::End(_) => inside -= 1,
                Event::Eof => bail!(DAMAGED),
                _ => {}
            }
            quick_xml::Writer::new(&mut piece).write_event(event)?;
            inside == 0
        } else {
            let (tag, empty) = match &event {
                Event::Eof => break,
                Event::DocType(_) => bail!(DAMAGED),
                Event::End(_) => {
                    let open = stack.pop().context(DAMAGED)?;
                    match open.role {
                        Role::Holder => each(Piece::Close)?,
                        Role::Table => each(Piece::TableEnd)?,
                        _ => {}
                    }
                    continue;
                }
                Event::Start(tag) => (tag, false),
                Event::Empty(tag) => (tag, true),
                _ => continue,
            };
            let local = tag.local_name();
            let parent = stack.last().map_or(Role::Outside, |open| open.role);
            let role = match (parent, local.as_ref()) {
                (Role::Outside, "body") if !found => {
                    found = true;
                    Some(Role::Body)
                }
                (Role::Outside, _) => Some(Role::Outside),
                (Role::Body | Role::Holder, "sdt" | "sdtContent" | "ins" | "customXml") => {
                    Some(Role::Holder)
                }
                (Role::Body | Role::Holder, "tbl") => Some(Role::Table),
                (Role::Body | Role::Holder, _) if rows_only => Some(Role::Skip),
                (Role::Body | Role::Holder, _) => None,
                (Role::Table, "tr") => None,
                (Role::Table | Role::Skip, _) => Some(Role::Skip),
            };
            match role {
                Some(role) => {
                    match role {
                        Role::Holder => each(Piece::Open)?,
                        Role::Table => each(Piece::Table)?,
                        _ => {}
                    }
                    if empty {
                        match role {
                            Role::Holder => each(Piece::Close)?,
                            Role::Table => each(Piece::TableEnd)?,
                            _ => {}
                        }
                    } else {
                        if stack.len() >= MOST_DEPTH {
                            bail!(
                                "this Office file is nested too deeply; export a simpler document"
                            );
                        }
                        stack.push(Open {
                            tag: tag.as_ref().to_string(),
                            name: tag.name().as_ref().to_string(),
                            role,
                        });
                    }
                    continue;
                }
                None => {
                    row = parent == Role::Table;
                    piece.clear();
                    inside = usize::from(!empty);
                    quick_xml::Writer::new(&mut piece).write_event(event)?;
                    empty
                }
            }
        };
        if ended {
            let xml = wrapped(&stack, &piece)?;
            let levels = stack.len();
            let depth = stack.iter().filter(|o| o.role == Role::Holder).count();
            let piece = if row {
                Piece::Row {
                    xml: &xml,
                    levels,
                    depth,
                }
            } else {
                Piece::Block {
                    xml: &xml,
                    levels,
                    depth,
                }
            };
            each(piece)?;
        }
    }
    if !stack.is_empty() {
        bail!(DAMAGED);
    }
    Ok(found)
}

fn node_at<'a, 'input>(doc: &'a Document<'input>, levels: usize) -> Result<Node<'a, 'input>> {
    let mut node = doc.root_element();
    for _ in 0..levels {
        node = node.first_element_child().context(DAMAGED)?;
    }
    Ok(node)
}

#[derive(Default)]
struct Style {
    base: String,
    heading: Option<usize>,
}

fn heading_name(name: &str) -> Option<usize> {
    name.to_ascii_lowercase()
        .replace(' ', "")
        .strip_prefix("heading")?
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=6).contains(n))
}

fn styles(xml: Option<&str>) -> Result<HashMap<String, Style>> {
    let mut styles = HashMap::new();
    if let Some(xml) = xml {
        let doc = parse(xml)?;
        for n in doc.descendants().filter(|n| is(*n, "style")) {
            let Some(id) = attr(n, "styleId") else {
                continue;
            };
            let heading = child(n, "pPr")
                .and_then(|p| value(p, "outlineLvl"))
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n < 6)
                .map(|n| n + 1)
                .or_else(|| value(n, "name").and_then(heading_name))
                .or_else(|| heading_name(id));
            styles.insert(
                id.to_string(),
                Style {
                    base: value(n, "basedOn").unwrap_or_default().to_string(),
                    heading,
                },
            );
        }
    }
    Ok(styles)
}

#[derive(Clone)]
struct Level {
    ordered: bool,
    start: usize,
}

fn numbering(xml: Option<&str>) -> Result<HashMap<(String, usize), Level>> {
    let mut levels = HashMap::new();
    let Some(xml) = xml else {
        return Ok(levels);
    };
    let doc = parse(xml)?;
    let mut abstracts = HashMap::new();
    for n in doc.descendants().filter(|n| is(*n, "abstractNum")) {
        let Some(id) = attr(n, "abstractNumId") else {
            continue;
        };
        for lvl in n.children().filter(|n| is(*n, "lvl")) {
            let depth = attr(lvl, "ilvl")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
                .min(MOST_LEVEL);
            let ordered = !matches!(value(lvl, "numFmt"), Some("bullet" | "none"));
            let start = value(lvl, "start")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            abstracts.insert((id.to_string(), depth), Level { ordered, start });
        }
    }
    for n in doc.descendants().filter(|n| is(*n, "num")) {
        let (Some(id), Some(base)) = (attr(n, "numId"), value(n, "abstractNumId")) else {
            continue;
        };
        for depth in 0..=MOST_LEVEL {
            if let Some(level) = abstracts.get(&(base.to_string(), depth)) {
                let mut level = level.clone();
                if let Some(over) = n.children().find(|n| {
                    is(*n, "lvlOverride")
                        && attr(*n, "ilvl").and_then(|v| v.parse::<usize>().ok()) == Some(depth)
                }) {
                    if let Some(start) = value(over, "startOverride").and_then(|v| v.parse().ok()) {
                        level.start = start;
                    }
                }
                levels.insert((id.to_string(), depth), level);
            }
        }
    }
    Ok(levels)
}

struct Word {
    links: HashMap<String, Link>,
    styles: HashMap<String, Style>,
    levels: HashMap<(String, usize), Level>,
    counts: HashMap<(String, usize), usize>,
}

fn span(cell: Node<'_, '_>) -> usize {
    child(cell, "tcPr")
        .and_then(|p| value(p, "gridSpan"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
}

fn width(row: Node<'_, '_>) -> Result<usize> {
    let mut width = 0usize;
    for cell in row.children().filter(|n| is(*n, "tc")) {
        let span = span(cell);
        width = width.saturating_add(span.max(1));
        if span > MOST_COLUMNS || width > MOST_COLUMNS {
            bail!("this Word table has too many columns; split it before uploading");
        }
    }
    Ok(width)
}

impl Word {
    fn paragraph(&mut self, node: Node<'_, '_>) -> String {
        let text = inline(node, &self.links, 0);
        let Some(props) = child(node, "pPr") else {
            return text;
        };
        let style = value(props, "pStyle").unwrap_or_default();
        let mut heading = value(props, "outlineLvl")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n < 6)
            .map(|n| n + 1)
            .or_else(|| heading_name(style));
        let mut current = style;
        for _ in 0..MOST_STYLE_HOPS {
            let Some(style) = self.styles.get(current) else {
                break;
            };
            heading = heading.or(style.heading);
            if heading.is_some() {
                break;
            }
            current = &style.base;
        }
        if let Some(level) = heading {
            return format!("{} {}", "#".repeat(level), text.trim());
        }
        if let Some(num) = child(props, "numPr") {
            let id = value(num, "numId").unwrap_or("0");
            if id != "0" {
                let depth = value(num, "ilvl")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(MOST_LEVEL);
                let key = (id.to_string(), depth);
                let level = self.levels.get(&key);
                let mark = if level.is_some_and(|l| l.ordered) {
                    let count = self
                        .counts
                        .entry(key)
                        .or_insert_with(|| level.map_or(1, |l| l.start));
                    let mark = format!("{count}.");
                    *count = count.saturating_add(1);
                    mark
                } else {
                    "-".to_string()
                };
                self.counts
                    .retain(|(other, d), _| other != id || *d <= depth);
                return format!("{}{mark} {}", "  ".repeat(depth), text.trim());
            }
        }
        text
    }

    fn row(&mut self, row: Node<'_, '_>, depth: usize) -> Result<Vec<String>> {
        width(row)?;
        let mut cells = Vec::new();
        for cell in row.children().filter(|n| is(*n, "tc")) {
            cells.push(self.blocks(cell, depth)?);
            cells.extend((1..span(cell)).map(|_| String::new()));
        }
        Ok(cells)
    }

    fn block(&mut self, n: Node<'_, '_>, depth: usize) -> Result<Option<String>> {
        Ok(Some(match n.tag_name().name() {
            "p" => self.paragraph(n),
            "tbl" => {
                let mut rows = Vec::new();
                let mut cells = 0usize;
                for row in n.children().filter(|n| is(*n, "tr")) {
                    let row = self.row(row, depth + 1)?;
                    cells = cells.saturating_add(row.len());
                    if cells > MOST_NODES as usize {
                        bail!("this document has a table inside a table that is too large; export it again");
                    }
                    rows.push(row);
                }
                format!("\n{}\n", table(rows)?)
            }
            "oMathPara" => format!("\n$$\n{}\n$$\n", math(n, 0)),
            "sdt" | "sdtContent" | "ins" | "customXml" => self.blocks(n, depth + 1)?,
            _ => return Ok(None),
        }))
    }

    fn blocks(&mut self, node: Node<'_, '_>, depth: usize) -> Result<String> {
        if depth >= MOST_DEPTH {
            bail!("this Word file is nested too deeply; export a simpler document");
        }
        let mut out = Vec::new();
        for n in node.children().filter(Node::is_element) {
            if let Some(text) = self.block(n, depth)? {
                out.push(text);
            }
        }
        let mut blank = false;
        Ok(out
            .join("\n")
            .lines()
            .filter(|line| {
                let empty = line.trim().is_empty();
                let keep = !empty || !blank;
                blank = empty;
                keep
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

pub(super) fn docx(bytes: &[u8], spill: &mut Spill) -> Result<()> {
    let mut package = Package::open(bytes)?;
    let mut word = Word {
        links: package.relationships("word/document.xml")?,
        styles: styles(package.text("word/styles.xml")?.as_deref())?,
        levels: numbering(package.text("word/numbering.xml")?.as_deref())?,
        counts: HashMap::new(),
    };
    let mut widths = Spill::new(&spill.scratch);
    let mut widest = 0;
    let found = pieces(package.stream("word/document.xml")?, true, |piece| {
        match piece {
            Piece::Table => widest = 0,
            Piece::Row { xml, levels, .. } => {
                widest = widest.max(width(node_at(&parse(xml)?, levels)?)?);
            }
            Piece::TableEnd => widths.push_str(&format!("{widest:05}"))?,
            Piece::Block { .. } | Piece::Open | Piece::Close => {}
        }
        Ok(())
    })?;
    if !found {
        bail!("this Word file has no document body; export it again");
    }
    let mut text = Text::new(spill);
    let mut stored = widths.reader()?;
    let mut columns = 0;
    let mut rows = 0usize;
    pieces(package.stream("word/document.xml")?, false, |piece| {
        match piece {
            Piece::Block { xml, levels, depth } => {
                let doc = parse(xml)?;
                if let Some(block) = word.block(node_at(&doc, levels)?, depth)? {
                    text.segments(&block)?;
                }
            }
            Piece::Open => text.open(),
            Piece::Close => text.close(),
            Piece::TableEnd => text.segment("")?,
            Piece::Table => {
                let mut record = [0u8; 5];
                stored
                    .read_exact(&mut record)
                    .context("leo could not read back the tables it measured")?;
                columns = std::str::from_utf8(&record)?.parse()?;
                rows = 0;
                text.segment("")?;
            }
            Piece::Row { xml, levels, depth } => {
                let doc = parse(xml)?;
                let cells = word.row(node_at(&doc, levels)?, depth + 1)?;
                if columns > 0 {
                    text.segment(&row_line(&cells, columns))?;
                    if rows == 0 {
                        text.segment(&rule(columns))?;
                    }
                }
                rows += 1;
            }
        }
        Ok(())
    })?;
    text.finish()
}

fn slides(package: &mut Package<'_>) -> Result<Vec<String>> {
    if let Some(xml) = package.text("ppt/presentation.xml")? {
        let doc = parse(&xml)?;
        let links = package.relationships("ppt/presentation.xml")?;
        let mut slides = Vec::new();
        for n in doc.descendants().filter(|n| is(*n, "sldId")) {
            let link = attr(n, "id")
                .and_then(|id| links.get(id))
                .filter(|l| !l.external && l.kind == "slide")
                .context("this PowerPoint file has a missing slide link; export it again")?;
            slides.push(link.target.clone());
        }
        return Ok(slides);
    }
    let mut slides: Vec<(u32, String)> = Vec::new();
    for name in package.zip.file_names() {
        let name = name.context("this Office file has an unreadable file name; export it again")?;
        let Some(number) = name
            .strip_prefix("ppt/slides/slide")
            .and_then(|name| name.strip_suffix(".xml"))
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        slides.push((number, name.into_owned()));
    }
    slides.sort();
    Ok(slides.into_iter().map(|(_, name)| name).collect())
}

pub(crate) fn slide_order(bytes: &[u8]) -> Result<Vec<String>> {
    slides(&mut Package::open(bytes)?)
}

fn slide_paragraph(node: Node<'_, '_>, links: &HashMap<String, Link>) -> String {
    let text = inline(node, links, 0);
    let Some(props) = child(node, "pPr") else {
        return text;
    };
    let depth = attr(props, "lvl")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MOST_LEVEL);
    if child(props, "buChar").is_some() || child(props, "buAutoNum").is_some() {
        let mark = child(props, "buAutoNum")
            .map(|n| {
                format!(
                    "{}.",
                    attr(n, "startAt")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                )
            })
            .unwrap_or_else(|| "-".to_string());
        format!("{}{mark} {}", "  ".repeat(depth), text.trim())
    } else {
        text
    }
}

fn slide_table(
    node: Node<'_, '_>,
    links: &HashMap<String, Link>,
    text: &mut Text<'_>,
    first: &mut bool,
) -> Result<()> {
    let rows = || node.children().filter(|n| is(*n, "tr"));
    let columns = rows()
        .map(|row| row.children().filter(|n| is(*n, "tc")).count())
        .max()
        .unwrap_or(0);
    if columns > MOST_COLUMNS {
        bail!("this slide has a table with too many columns; split it before uploading");
    }
    text.item(first, "\n")?;
    for (i, row) in rows().enumerate() {
        if columns == 0 {
            break;
        }
        let cells: Vec<String> = row
            .children()
            .filter(|n| is(*n, "tc"))
            .map(|cell| {
                cell.descendants()
                    .filter(|n| is(*n, "p"))
                    .map(|p| slide_paragraph(p, links))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect();
        if i > 0 {
            text.push("\n")?;
        }
        text.push(&row_line(&cells, columns))?;
        if i == 0 {
            text.push("\n")?;
            text.push(&rule(columns))?;
        }
    }
    text.push("\n")
}

fn chart(
    package: &mut Package<'_>,
    path: &str,
    text: &mut Text<'_>,
    first: &mut bool,
) -> Result<()> {
    let xml = package.required(path)?;
    let doc = parse(&xml)?;
    let mut series = Vec::new();
    for node in doc.descendants().filter(|n| is(*n, "ser")) {
        let name = child(node, "tx")
            .and_then(|n| n.descendants().find(|n| is(*n, "v")))
            .and_then(|n| n.text())
            .unwrap_or("Series");
        let points = |field| {
            let mut points = HashMap::new();
            if let Some(n) = child(node, field) {
                for point in n.descendants().filter(|n| is(*n, "pt")) {
                    if let Some(index) = attr(point, "idx").and_then(|s| s.parse::<usize>().ok()) {
                        let text = child(point, "v").and_then(|n| n.text()).unwrap_or_default();
                        points.insert(index, escape(text));
                    }
                }
            }
            points
        };
        series.push((escape(name), points("cat"), points("val")));
    }
    text.item(first, "\n")?;
    if series.is_empty() {
        return text.push("\n");
    }
    if series.len() >= MOST_COLUMNS {
        bail!("this chart has too many series; split it before uploading");
    }
    let mut indices: Vec<usize> = series
        .iter()
        .flat_map(|(_, cats, vals)| cats.keys().chain(vals.keys()).copied())
        .collect();
    indices.sort_unstable();
    indices.dedup();
    let columns = series.len() + 1;
    let header: Vec<String> = std::iter::once("Category".to_string())
        .chain(series.iter().map(|s| s.0.clone()))
        .collect();
    text.push("### Chart\n\n")?;
    text.push(&row_line(&header, columns))?;
    text.push("\n")?;
    text.push(&rule(columns))?;
    for i in indices {
        let label = series
            .iter()
            .find_map(|s| s.1.get(&i))
            .cloned()
            .unwrap_or_else(|| (i.saturating_add(1)).to_string());
        let row: Vec<String> = std::iter::once(label)
            .chain(
                series
                    .iter()
                    .map(|s| s.2.get(&i).cloned().unwrap_or_default()),
            )
            .collect();
        text.push("\n")?;
        text.push(&row_line(&row, columns))?;
    }
    text.push("\n")
}

fn slide_blocks(
    node: Node<'_, '_>,
    links: &HashMap<String, Link>,
    package: &mut Package<'_>,
    depth: usize,
    text: &mut Text<'_>,
    first: &mut bool,
) -> Result<()> {
    if depth >= MOST_DEPTH {
        bail!("this slide is nested too deeply; export a simpler presentation");
    }
    let mut nodes: Vec<Node<'_, '_>> = node.children().filter(Node::is_element).collect();
    if is(node, "spTree") || is(node, "grpSp") {
        nodes.sort_by_key(|n| {
            let off = n.descendants().find(|n| is(*n, "off"));
            let coordinate = |key| {
                off.and_then(|n| attr(n, key))
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(i64::MAX)
            };
            (coordinate("y"), coordinate("x"))
        });
    }
    for n in nodes {
        match n.tag_name().name() {
            "sp" => {
                let title = n
                    .descendants()
                    .any(|n| is(n, "ph") && matches!(attr(n, "type"), Some("title" | "ctrTitle")));
                if let Some(body) = child(n, "txBody") {
                    for p in body.children().filter(|n| is(*n, "p")) {
                        let said = slide_paragraph(p, links);
                        text.item(
                            first,
                            &if title
                                && !said.trim().is_empty()
                                && child(p, "pPr").is_none_or(|pr| {
                                    child(pr, "buChar").is_none()
                                        && child(pr, "buAutoNum").is_none()
                                })
                            {
                                format!("### {}", said.trim())
                            } else {
                                said
                            },
                        )?;
                    }
                }
            }
            "p" => text.item(first, &slide_paragraph(n, links))?,
            "tbl" => slide_table(n, links, text, first)?,
            "chart" => {
                if let Some(link) = attr(n, "id")
                    .and_then(|id| links.get(id))
                    .filter(|l| !l.external && l.kind == "chart")
                {
                    chart(package, &link.target, text, first)?;
                }
            }
            "pic" | "nvSpPr" | "nvGrpSpPr" | "spPr" | "grpSpPr" => {}
            _ => slide_blocks(n, links, package, depth + 1, text, first)?,
        }
    }
    Ok(())
}

fn speaker_notes(xml: &str) -> Result<String> {
    let doc = parse(xml)?;
    let bodies: Vec<Node<'_, '_>> = doc
        .descendants()
        .filter(|n| {
            is(*n, "sp")
                && n.descendants()
                    .any(|n| is(n, "ph") && attr(n, "type") == Some("body"))
        })
        .collect();
    let roots = if bodies.is_empty() {
        vec![doc.root_element()]
    } else {
        bodies
    };
    let links = HashMap::new();
    Ok(roots
        .into_iter()
        .flat_map(|n| n.descendants())
        .filter(|n| is(*n, "p"))
        .map(|n| inline(n, &links, 0))
        .filter(|s| !s.trim().is_empty() && !s.chars().all(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(super) fn pptx(bytes: &[u8], spill: &mut Spill) -> Result<()> {
    let mut package = Package::open(bytes)?;
    let slides = slides(&mut package)?;
    if slides.is_empty() {
        bail!("this PowerPoint file has no slides in it");
    }
    let legacy = !package
        .zip
        .file_names()
        .any(|name| name.is_ok_and(|name| name == "ppt/presentation.xml"));
    let mut text = Text::new(spill);
    for (index, path) in slides.iter().enumerate() {
        let number = if legacy {
            path.strip_prefix("ppt/slides/slide")
                .and_then(|s| s.strip_suffix(".xml"))
                .and_then(|s| s.parse().ok())
                .unwrap_or(index + 1)
        } else {
            index + 1
        };
        let xml = package.required(path)?;
        let doc = parse(&xml)?;
        let links = package.relationships(path)?;
        text.push(&format!("## Slide {number}\n"))?;
        text.trim_from_here();
        slide_blocks(
            doc.root_element(),
            &links,
            &mut package,
            0,
            &mut text,
            &mut true,
        )?;
        text.trim_to_here();
        text.push("\n")?;
        let notes = links
            .values()
            .find(|l| !l.external && l.kind == "notesSlide")
            .map(|l| l.target.clone())
            .or_else(|| legacy.then(|| format!("ppt/notesSlides/notesSlide{number}.xml")));
        if let Some(path) = notes {
            if let Some(xml) = package.text(&path)? {
                let said = speaker_notes(&xml)?;
                if !said.is_empty() {
                    text.push(&format!("Speaker notes: {said}\n"))?;
                }
            }
        }
        text.push("\n")?;
    }
    text.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::Scratch;
    use std::io::Write;

    fn package(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated)
                        .compression_level(Some(9)),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn damaged_xml_entities_and_deep_documents_are_rejected() {
        for xml in [
            "<body><p></body>",
            "<!DOCTYPE body [<!ENTITY secret SYSTEM 'file:///private/key'>]><body>&secret;</body>",
            "<!DOCTYPE body [<!ENTITY x 'expanded'>]><body>&x;</body>",
        ] {
            assert!(parse(xml).is_err(), "{xml}");
        }
        let xml = format!(
            "{}text{}",
            "<p>".repeat(MOST_DEPTH + 1),
            "</p>".repeat(MOST_DEPTH + 1)
        );
        assert!(parse(&xml).unwrap_err().to_string().contains("deeply"));
        let bytes = package(&[("word/document.xml", b"\xff")]);
        assert!(read_docx(&bytes)
            .unwrap_err()
            .to_string()
            .contains("unreadable text"));
        assert!(read_docx(&package(&[])).is_err());
        assert!(read_pptx(&package(&[])).is_err());
    }

    #[test]
    fn internal_links_stay_inside_the_archive_and_external_links_are_data() {
        assert_eq!(
            joined("ppt/slides", "../notesSlides/notesSlide9.xml").unwrap(),
            "ppt/notesSlides/notesSlide9.xml"
        );
        assert_eq!(
            joined("word", "/word/media/image.png").unwrap(),
            "word/media/image.png"
        );
        for target in [
            "../../../private/key",
            "../../word/key",
            "file:///private/key",
            "C:\\key",
            "//private/key",
            "../media/../..",
        ] {
            assert!(joined("word", target).is_err(), "{target}");
        }
        for target in [
            "javascript:alert(1)",
            "file:///private/key",
            "data:text/html,bad",
            "https://example.com/a(b)",
        ] {
            let text = linked("Read".to_string(), Some(target));
            if target.starts_with("https") {
                assert!(text.contains("a%28b%29"));
            } else {
                assert_eq!(text, "Read");
            }
        }
        let bytes = package(&[("word/_rels/document.xml.rels", br#"<Relationships><Relationship Id="r1" Type="http://x/image" Target="../../../secret"/></Relationships>"#)]);
        assert!(Package::open(&bytes)
            .unwrap()
            .relationships("word/document.xml")
            .is_err());
    }

    #[test]
    fn one_piece_has_limits_and_tables_keep_their_markdown() {
        let large = vec![b'x'; MOST_PART_BYTES + 1];
        let bytes = package(&[("word/document.xml", &large)]);
        assert!(bytes.len() < MOST_PART_BYTES);
        assert!(read_docx(&bytes)
            .unwrap_err()
            .to_string()
            .contains("too much text"));
        assert!(read_docx(&word(&format!(
            "<w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"{}\"/></w:tcPr></w:tc></w:tr></w:tbl>",
            MOST_COLUMNS + 1
        )))
        .is_err());
        assert_eq!(
            table(vec![vec!["A | B".into(), "C\nD".into()], vec!["é".into()]]).unwrap(),
            "| A \\| B | C<br>D |\n| --- | --- |\n| é |  |"
        );
    }

    #[test]
    fn namespace_prefixes_and_literal_markdown_do_not_change_the_source() {
        let xml = r#"<x:document xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><x:body><x:p><x:pPr><x:pStyle x:val="Heading2"/></x:pPr><x:r><x:t>Café *[literal]* &lt;tag&gt;</x:t></x:r></x:p></x:body></x:document>"#;
        assert_eq!(
            read_docx(&package(&[("word/document.xml", xml.as_bytes())])).unwrap(),
            "## Café \\*\\[literal\\]\\* \\<tag\\>"
        );
    }

    #[test]
    fn positioned_slide_shapes_are_read_top_to_bottom_then_left_to_right() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:spTree><p:sp><p:spPr><a:xfrm><a:off x="100" y="200"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Bottom</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:spPr><a:xfrm><a:off x="200" y="100"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Right</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:spPr><a:xfrm><a:off x="100" y="100"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Left</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:sld>"#;
        assert_eq!(
            read_pptx(&package(&[("ppt/slides/slide1.xml", xml.as_bytes())])).unwrap(),
            "## Slide 1\nLeft\nRight\nBottom"
        );
    }

    fn read_with(bytes: &[u8], read: fn(&[u8], &mut Spill) -> Result<()>) -> Result<String> {
        let mut spill = Spill::new(&Scratch::default());
        read(bytes, &mut spill)?;
        spill.read(0..spill.len())
    }

    fn read_docx(bytes: &[u8]) -> Result<String> {
        read_with(bytes, docx)
    }

    fn read_pptx(bytes: &[u8]) -> Result<String> {
        read_with(bytes, pptx)
    }

    fn declared(xml: &str, size: u64) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default().large_file(true),
            )
            .unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        let mut bytes = writer.finish().unwrap().into_inner();
        let at = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        let name = usize::from(u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]));
        let extra = at + 46 + name;
        assert_eq!(bytes[extra..extra + 2], [1, 0]);
        bytes[extra + 4..extra + 12].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    fn word(body: &str) -> Vec<u8> {
        package(&[(
            "word/document.xml",
            format!("{WORD}{body}</w:body></w:document>").as_bytes(),
        )])
    }

    #[test]
    fn a_word_file_past_sixteen_megabytes_of_text_is_read() {
        let mut body = String::new();
        let mut i = 0;
        while body.len() < 20 * 1024 * 1024 {
            i += 1;
            body.push_str(&paragraph(i));
        }
        body.push_str("<w:p><w:r><w:t>The last page ends here.</w:t></w:r></w:p>");
        let text = read_docx(&word(&body)).unwrap();
        assert!(text.starts_with("Paragraph 1 explains"));
        assert!(text.contains(&format!("\nParagraph {i} explains")));
        assert!(text.ends_with("\nThe last page ends here."));
    }

    #[test]
    fn a_table_of_a_million_cells_is_read_row_by_row() {
        let row = format!(
            "<w:tr>{}</w:tr>",
            "<w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc>".repeat(300)
        );
        let text = read_docx(&word(&format!("<w:tbl>{}</w:tbl>", row.repeat(4_000)))).unwrap();
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows.len(), 4_001);
        assert_eq!(rows[1], format!("|{}", " --- |".repeat(300)));
        let line = format!("|{}", " x |".repeat(300));
        assert!(rows
            .iter()
            .enumerate()
            .all(|(i, row)| i == 1 || *row == line));
    }

    #[test]
    fn a_deck_of_five_thousand_slides_is_read() {
        let slides: Vec<(String, String)> = (1..=5_000)
            .map(|i| (format!("ppt/slides/slide{i}.xml"), slide(i)))
            .collect();
        let files: Vec<(&str, &[u8])> = slides
            .iter()
            .map(|(name, xml)| (name.as_str(), xml.as_bytes()))
            .collect();
        let text = read_pptx(&package(&files)).unwrap();
        assert!(text.starts_with("## Slide 1\n### Slide 1\nHeaps keep"));
        assert!(text.ends_with(
            "\n\n## Slide 5000\n### Slide 5000\nHeaps keep the smallest key at the root.\nInsert sifts up, extract sifts down."
        ));
    }

    #[test]
    fn a_highly_compressible_valid_document_is_read() {
        let row = format!("<w:tr>{}</w:tr>", "<w:tc><w:p/></w:tc>".repeat(40));
        let xml = format!(
            "{WORD}{}<w:tbl>{}</w:tbl></w:body></w:document>",
            "<w:p><w:r><w:t>Same line.</w:t></w:r></w:p>".repeat(200_000),
            row.repeat(5_000)
        );
        let bytes = package(&[("word/document.xml", xml.as_bytes())]);
        assert!(xml.len() / bytes.len() > 300, "{}", xml.len() / bytes.len());
        let text = read_docx(&bytes).unwrap();
        assert_eq!(text.lines().filter(|l| *l == "Same line.").count(), 200_000);
        let empty = format!("|{}", "  |".repeat(40));
        assert_eq!(text.lines().filter(|l| *l == empty).count(), 5_000);
    }

    #[test]
    fn a_part_past_four_gigabytes_is_refused() {
        let xml = format!("{WORD}<w:p/></w:body></w:document>");
        let err = read_docx(&declared(&xml, MOST_PART_SIZE + 1))
            .unwrap_err()
            .to_string();
        assert!(err.contains("more text than any document"), "{err}");
        let mut counted = Counted {
            inner: &b"<w:p/>"[..],
            since: 0,
            total: MOST_PART_SIZE + 1,
            refused: None,
        };
        assert_eq!(counted.fill_buf().unwrap_err().to_string(), TOO_BIG);
    }

    #[test]
    fn the_spill_stops_before_the_disk_fills() {
        let bytes = word(&(1..=100_000).map(paragraph).collect::<String>());
        let mut spill = Spill::new(&Scratch::default());
        spill.free = Some(512 * 1024 * 1024);
        assert_eq!(
            docx(&bytes, &mut spill).unwrap_err().to_string(),
            "There is not enough free disk space to read this file."
        );
        let mut spill = Spill::new(&Scratch::default());
        spill.free = Some(4 * 1024 * 1024 * 1024);
        docx(&bytes, &mut spill).unwrap();
        assert!(spill.len() > 8 * 1024 * 1024);
    }

    #[test]
    fn empty_paragraphs_controls_and_tables_hold_no_memory() {
        let mut spill = Spill::new(&Scratch::default());
        let mut text = Text::new(&mut spill);
        text.segment("a").unwrap();
        for _ in 0..300_000 {
            text.segment("").unwrap();
            text.open();
            text.close();
        }
        assert!(text.pending.len() <= 2);
        text.open();
        for _ in 0..1_000 {
            text.segment(" ").unwrap();
        }
        assert!(text.pending.len() <= 4);
        text.close();
        assert!(text.pending.len() <= 2);
        text.segment("b").unwrap();
        text.finish().unwrap();
        assert_eq!(spill.read(0..spill.len()).unwrap(), "a\n\nb");
        let body = format!(
            "{}<w:p><w:r><w:t>end</w:t></w:r></w:p>",
            "<w:p/><w:sdt/><w:tbl/>".repeat(100_000)
        );
        assert_eq!(read_docx(&word(&body)).unwrap(), "end");
    }

    #[test]
    fn deep_nesting_is_still_refused() {
        let body = format!(
            "<w:p>{}<w:t>deep</w:t>{}</w:p>",
            "<w:r>".repeat(33),
            "</w:r>".repeat(33)
        );
        let err = read_docx(&word(&body)).unwrap_err().to_string();
        assert!(err.contains("deeply"), "{err}");
    }

    const WORD: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#;

    fn paragraph(i: usize) -> String {
        format!("<w:p><w:r><w:t>Paragraph {i} explains how a heap keeps its smallest key at the root and sifts down after every extraction.</w:t></w:r></w:p>")
    }

    fn write_docx(path: &std::path::Path, megabytes: usize) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .large_file(true),
            )
            .unwrap();
        writer.write_all(WORD.as_bytes()).unwrap();
        let mut written = 0;
        let mut i = 0;
        while written < megabytes * 1024 * 1024 {
            let mut chunk = String::new();
            for _ in 0..1_000 {
                i += 1;
                chunk.push_str(&paragraph(i));
                if i % 500 == 0 {
                    chunk.push_str("<w:tbl>");
                    for row in 0..4 {
                        chunk.push_str("<w:tr>");
                        for col in 0..3 {
                            chunk.push_str(&format!(
                                "<w:tc><w:p><w:r><w:t>{row}:{col}</w:t></w:r></w:p></w:tc>"
                            ));
                        }
                        chunk.push_str("</w:tr>");
                    }
                    chunk.push_str("</w:tbl>");
                }
            }
            writer.write_all(chunk.as_bytes()).unwrap();
            written += chunk.len();
        }
        writer.write_all(b"</w:body></w:document>").unwrap();
        writer.finish().unwrap();
    }

    fn slide(i: usize) -> String {
        format!(
            r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Slide {i}</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:txBody><a:p><a:r><a:t>Heaps keep the smallest key at the root.</a:t></a:r></a:p><a:p><a:r><a:t>Insert sifts up, extract sifts down.</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    }

    fn write_pptx(path: &std::path::Path, slides: usize) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for i in 1..=slides {
            writer
                .start_file(
                    format!("ppt/slides/slide{i}.xml"),
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(slide(i).as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    #[ignore]
    fn benchmark_large_office_files() {
        if let Ok(file) = std::env::var("LEO_BENCH_OFFICE_FILE") {
            let name = std::env::var("LEO_BENCH_OFFICE_NAME").unwrap();
            crate::import::tests::measured_read(&file, &name);
            return;
        }
        let cases = std::env::var("LEO_BENCH_CASES")
            .unwrap_or_else(|_| "docx:5,docx:15,docx:200,docx:1000,pptx:300,pptx:5000".into());
        let dir = tempfile::tempdir().unwrap();
        for case in cases.split(',') {
            let (kind, size) = case.split_once(':').unwrap();
            let size: usize = size.parse().unwrap();
            let name = format!("large.{kind}");
            let file = dir.path().join(&name);
            if kind == "docx" {
                write_docx(&file, size);
            } else {
                write_pptx(&file, size);
            }
            let out = leo_core::children::quiet_command(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "--nocapture",
                    "import::office::tests::benchmark_large_office_files",
                ])
                .env("LEO_BENCH_OFFICE_FILE", &file)
                .env("LEO_BENCH_OFFICE_NAME", &name)
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
}
