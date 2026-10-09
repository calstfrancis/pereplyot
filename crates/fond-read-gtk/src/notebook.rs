//! Notebooks: a plain Typst outline you write in, with annotations from any document dropped in
//! as quote blocks. The file is the truth — it compiles by itself (a small preamble draws each
//! quote), reads correctly with the reader closed (the quote text is cached inline) and each
//! quote carries the hash and id that find its passage again.
//!
//! This module is the file format, the shelf of notebooks on disk, and the export to Typst,
//! Markdown and LaTeX with every quote cited. The editor is `notebook_view`.

use std::path::{Path, PathBuf};

use fond_annot::cite::typst_citation;
use fond_annot::export::{latex_escape, typst_escape, Format};
use serde::{Deserialize, Serialize};

use crate::fsutil;

const HEADER: &str = "// pereplyot-notebook ";
const PREAMBLE_START: &str = "// --- pereplyot preamble: drawn by Pereplyot, safe to ignore ---";
const PREAMBLE_END: &str = "// --- end preamble ---";

/// Defines `#pquote`, so the stored file compiles on its own.
const PREAMBLE: &str = r#"#let pquote(hash: "", id: "", page: "", label: "", colour: "", title: "", key: "", text: "", note: "", area: false) = {
  let shade = if colour == "" { luma(160) } else { rgb(colour) }
  let where = if title != "" { title + ", p. " + label } else { "p. " + label }
  block(width: 100%, inset: (left: 10pt, y: 4pt), stroke: (left: 3pt + shade), breakable: false)[
    #quote(block: true, attribution: [#where])[#text]
    #if note != "" [#emph(note)]
  ]
}"#;

/// One annotation placed in a notebook: where it is, and what it said when it was dropped in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Quote {
    pub hash: String,
    pub id: String,
    /// 1-based file page.
    pub page: u32,
    /// The printed page ("xii", "42"), as it is cited.
    pub label: String,
    pub colour: String,
    pub title: String,
    pub key: String,
    pub text: String,
    pub note: String,
    pub area: bool,
}

impl Quote {
    /// The page as a person cites it.
    pub fn locator(&self) -> String {
        if self.label.is_empty() {
            self.page.to_string()
        } else {
            self.label.clone()
        }
    }

    pub fn link(&self) -> String {
        crate::deeplink::build(&crate::deeplink::DeepLink {
            hash: self.hash.clone(),
            annotation: Some(self.id.clone()),
            page: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Typst markup, exactly as typed.
    Text(String),
    Quote(Quote),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notebook {
    /// The file's name without `.typ`.
    pub stem: String,
    pub title: String,
    /// The Library shelf this notebook belongs to; none for a stand-alone one.
    pub shelf: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize)]
struct Header {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shelf: Option<String>,
}

fn typst_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn quote_line(q: &Quote) -> String {
    let mut args: Vec<String> = Vec::new();
    let mut s = |name: &str, v: &str| {
        if !v.is_empty() {
            args.push(format!("{name}: {}", typst_string(v)));
        }
    };
    s("hash", &q.hash);
    s("id", &q.id);
    s("page", &q.page.to_string());
    s("label", &q.label);
    s("colour", &q.colour);
    s("title", &q.title);
    s("key", &q.key);
    s("text", &q.text);
    s("note", &q.note);
    if q.area {
        args.push("area: true".to_string());
    }
    format!("#pquote({})\n", args.join(", "))
}

/// The arguments of a `#pquote(...)` line, or `None` if the line isn't one that is wholly ours.
fn parse_quote_line(line: &str) -> Option<Quote> {
    let rest = line.trim().strip_prefix("#pquote(")?.strip_suffix(')')?;
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    let mut q = Quote::default();
    let mut seen_page = false;
    loop {
        while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && chars[i] != ':' {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        i += 1;
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let value = if chars.get(i) == Some(&'"') {
            i += 1;
            let mut v = String::new();
            loop {
                match chars.get(i)? {
                    '"' => {
                        i += 1;
                        break;
                    }
                    '\\' => {
                        i += 1;
                        v.push(match chars.get(i)? {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            c => *c,
                        });
                        i += 1;
                    }
                    c => {
                        v.push(*c);
                        i += 1;
                    }
                }
            }
            v
        } else {
            let start = i;
            while i < chars.len() && chars[i] != ',' {
                i += 1;
            }
            chars[start..i]
                .iter()
                .collect::<String>()
                .trim()
                .to_string()
        };
        match name.trim() {
            "hash" => q.hash = value,
            "id" => q.id = value,
            "page" => {
                q.page = value.parse().ok()?;
                seen_page = true;
            }
            "label" => q.label = value,
            "colour" => q.colour = value,
            "title" => q.title = value,
            "key" => q.key = value,
            "text" => q.text = value,
            "note" => q.note = value,
            "area" => q.area = value == "true",
            _ => return None,
        }
    }
    (seen_page && !q.hash.is_empty() && !q.id.is_empty()).then_some(q)
}

impl Notebook {
    pub fn new(stem: &str, title: &str, shelf: Option<String>) -> Notebook {
        Notebook {
            stem: stem.to_string(),
            title: title.to_string(),
            shelf,
            blocks: Vec::new(),
        }
    }

    /// The whole file: header, preamble, then the body.
    pub fn to_file(&self) -> String {
        let header = serde_json::to_string(&Header {
            title: self.title.clone(),
            shelf: self.shelf.clone(),
        })
        .unwrap_or_default();
        let mut out = format!("{HEADER}{header}\n{PREAMBLE_START}\n{PREAMBLE}\n{PREAMBLE_END}\n");
        out.push_str(&self.body());
        out
    }

    pub fn body(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            match b {
                Block::Text(t) => out.push_str(t),
                Block::Quote(q) => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&quote_line(q));
                }
            }
        }
        out
    }

    pub fn parse(stem: &str, text: &str) -> Notebook {
        let mut lines = text.split_inclusive('\n').peekable();
        let header: Option<Header> = lines
            .peek()
            .and_then(|l| l.trim_end().strip_prefix(HEADER.trim_end()))
            .and_then(|j| serde_json::from_str(j.trim()).ok());
        if header.is_some() {
            lines.next();
        }
        if lines.peek().is_some_and(|l| l.trim_end() == PREAMBLE_START) {
            for l in lines.by_ref() {
                if l.trim_end() == PREAMBLE_END {
                    break;
                }
            }
        }
        let mut blocks: Vec<Block> = Vec::new();
        let mut pending = String::new();
        for line in lines {
            match parse_quote_line(line) {
                Some(q) => {
                    if !pending.is_empty() {
                        blocks.push(Block::Text(std::mem::take(&mut pending)));
                    }
                    blocks.push(Block::Quote(q));
                }
                None => pending.push_str(line),
            }
        }
        if !pending.is_empty() {
            blocks.push(Block::Text(pending));
        }
        let (title, shelf) = match header {
            Some(h) => (h.title, h.shelf),
            None => (stem.replace(['-', '_'], " "), None),
        };
        Notebook {
            stem: stem.to_string(),
            title,
            shelf,
            blocks,
        }
    }

    pub fn quotes(&self) -> impl Iterator<Item = &Quote> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Quote(q) => Some(q),
            Block::Text(_) => None,
        })
    }

    pub fn push_quote(&mut self, q: Quote) {
        if let Some(Block::Text(t)) = self.blocks.last_mut() {
            if !t.ends_with('\n') {
                t.push('\n');
            }
        }
        self.blocks.push(Block::Quote(q));
    }
}

// ----------------------------------------------------------------- the shelf on disk

pub fn dir() -> PathBuf {
    glib::user_data_dir().join("pereplyot").join("notebooks")
}

pub fn path_of(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{stem}.typ"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub stem: String,
    pub title: String,
    pub shelf: Option<String>,
    pub quotes: usize,
    pub modified: std::time::SystemTime,
}

/// Every notebook in `dir`, most recently changed first.
pub fn list(dir: &Path) -> Vec<Summary> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Summary> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("typ") {
                return None;
            }
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            let nb = Notebook::parse(&stem, &text);
            Some(Summary {
                quotes: nb.quotes().count(),
                title: nb.title,
                shelf: nb.shelf,
                stem,
                modified: e.metadata().ok()?.modified().ok()?,
            })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.title.cmp(&b.title)));
    out
}

pub fn load(dir: &Path, stem: &str) -> Option<Notebook> {
    let text = std::fs::read_to_string(path_of(dir, stem)).ok()?;
    Some(Notebook::parse(stem, &text))
}

pub fn save(dir: &Path, nb: &Notebook) -> std::io::Result<()> {
    fsutil::write_atomic(&path_of(dir, &nb.stem), nb.to_file().as_bytes())
}

pub fn delete(dir: &Path, stem: &str) {
    let _ = std::fs::remove_file(path_of(dir, stem));
}

fn slug(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "notebook".to_string()
    } else {
        s.chars().take(48).collect()
    }
}

/// A Library shelf was renamed (`to` is its new name) or deleted (`to` is `None`): move the
/// notebooks that were on it.
pub fn move_shelf(dir: &Path, from: &str, to: Option<&str>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("typ") {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(mut nb) = std::fs::read_to_string(&path)
            .ok()
            .map(|t| Notebook::parse(&stem, &t))
        else {
            continue;
        };
        if nb.shelf.as_deref() == Some(from) {
            nb.shelf = to.map(str::to_string);
            let _ = save(dir, &nb);
        }
    }
}

/// A new, empty notebook with a file name no other notebook has.
pub fn create(dir: &Path, title: &str, shelf: Option<String>) -> std::io::Result<Notebook> {
    let base = slug(title);
    let mut stem = base.clone();
    let mut n = 2;
    while path_of(dir, &stem).exists() {
        stem = format!("{base}-{n}");
        n += 1;
    }
    let nb = Notebook::new(&stem, title, shelf);
    save(dir, &nb)?;
    Ok(nb)
}

// --------------------------------------------------------------------------- export

/// A connection of a quote to another annotation, as an export sentence needs it.
#[derive(Debug, Clone)]
pub struct Linked {
    pub note: String,
    pub text: String,
    pub whence: String,
}

pub struct ExportOptions<'a> {
    pub format: Format,
    /// Typst only: `#bibliography(...)` for this file, so `@key` citations resolve.
    pub bibliography: Option<&'a str>,
    /// The bibliography key to cite a quote's document by; falls back to the key the quote carries.
    pub key_for: &'a dyn Fn(&Quote) -> Option<String>,
    /// A picture file the output can load, for a clipped area.
    pub image_for: &'a dyn Fn(&Quote) -> Option<String>,
    pub connections_of: &'a dyn Fn(&Quote) -> Vec<Linked>,
    /// Add a link back to the passage in Pereplyot.
    pub links: bool,
}

pub fn export(nb: &Notebook, opts: &ExportOptions) -> String {
    match opts.format {
        Format::Typst => to_typst(nb, opts),
        Format::Markdown => to_markdown(nb, opts),
        Format::Latex => to_latex(nb, opts),
    }
}

fn key_of(q: &Quote, opts: &ExportOptions) -> Option<String> {
    (opts.key_for)(q)
        .or_else(|| (!q.key.is_empty()).then(|| q.key.clone()))
        .map(|k| k.trim_start_matches('@').to_string())
        .filter(|k| !k.is_empty())
}

fn whence(q: &Quote) -> String {
    if q.title.is_empty() {
        format!("p. {}", q.locator())
    } else {
        format!("{}, p. {}", q.title, q.locator())
    }
}

fn paragraphs(s: &str) -> Vec<String> {
    s.split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect()
}

fn typst_url(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn to_typst(nb: &Notebook, opts: &ExportOptions) -> String {
    let mut out = format!("= {}\n\n", typst_escape(&nb.title));
    for b in &nb.blocks {
        match b {
            Block::Text(t) => out.push_str(t),
            Block::Quote(q) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
                let mut attribution = match key_of(q, opts) {
                    Some(k) => typst_citation(&k, Some(&format!("p. {}", q.locator()))),
                    None => typst_escape(&whence(q)),
                };
                if opts.links {
                    attribution.push_str(&format!(" #link(\"{}\")[↗]", typst_url(&q.link())));
                }
                if let Some(image) = (opts.image_for)(q) {
                    out.push_str(&format!(
                        "#figure(image(\"{}\"), caption: [{attribution}])\n\n",
                        typst_url(&image)
                    ));
                } else if q.text.trim().is_empty() {
                    out.push_str(&format!("*{}*\n\n", typst_escape(&whence(q))));
                } else {
                    let body = paragraphs(&q.text)
                        .iter()
                        .map(|p| typst_escape(p))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    out.push_str(&format!(
                        "#quote(block: true, attribution: [{attribution}])[{body}]\n\n"
                    ));
                }
                for p in paragraphs(&q.note) {
                    out.push_str(&format!("{}\n\n", typst_escape(&p)));
                }
                for l in (opts.connections_of)(q) {
                    out.push_str(&format!(
                        "_Connected{}: {} ({})_\n\n",
                        if l.note.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", typst_escape(&l.note))
                        },
                        typst_escape(&l.text),
                        typst_escape(&l.whence)
                    ));
                }
            }
        }
    }
    if let Some(bib) = opts.bibliography.filter(|b| !b.trim().is_empty()) {
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("#bibliography(\"{}\")\n", typst_url(bib.trim())));
    }
    out
}

// ---- Typst markup → Markdown / LaTeX, for the text the notebook's author wrote

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Markdown,
    Latex,
}

fn is_key_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, ':' | '_' | '-' | '.')
}

fn plain(s: &str, t: Target) -> String {
    match t {
        Target::Markdown => s.to_string(),
        Target::Latex => latex_escape(s),
    }
}

/// One line of Typst markup, with emphasis, links, citations and escapes carried across and
/// everything else left as it was written.
fn convert_inline(line: &str, t: Target) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut bold = false;
    let mut emph = false;
    let word_before = |i: usize| i > 0 && chars[i - 1].is_alphanumeric();
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                out.push_str(&plain(&chars[i + 1].to_string(), t));
                i += 2;
            }
            '*' => {
                bold = !bold;
                out.push_str(match (t, bold) {
                    (Target::Markdown, _) => "**",
                    (Target::Latex, true) => "\\textbf{",
                    (Target::Latex, false) => "}",
                });
                i += 1;
            }
            '_' if !word_before(i) || emph => {
                emph = !emph;
                out.push_str(match (t, emph) {
                    (Target::Markdown, _) => "*",
                    (Target::Latex, true) => "\\emph{",
                    (Target::Latex, false) => "}",
                });
                i += 1;
            }
            '@' if !word_before(i) && chars.get(i + 1).is_some_and(|c| c.is_alphanumeric()) => {
                let mut j = i + 1;
                while j < chars.len() && is_key_char(chars[j]) {
                    j += 1;
                }
                while j > i + 1 && matches!(chars[j - 1], '.' | ':' | '-') {
                    j -= 1;
                }
                let key: String = chars[i + 1..j].iter().collect();
                let mut supplement = String::new();
                if chars.get(j) == Some(&'[') {
                    if let Some(end) = chars[j..].iter().position(|c| *c == ']') {
                        supplement = chars[j + 1..j + end].iter().collect();
                        j += end + 1;
                    }
                }
                out.push_str(&match (t, supplement.is_empty()) {
                    (Target::Markdown, true) => format!("[@{key}]"),
                    (Target::Markdown, false) => format!("[@{key}, {supplement}]"),
                    (Target::Latex, true) => format!("\\autocite{{{key}}}"),
                    (Target::Latex, false) => {
                        format!("\\autocite[{}]{{{key}}}", supplement.replace(". ", ".~"))
                    }
                });
                i = j;
            }
            '#' if line[line.char_indices().nth(i).map_or(0, |(b, _)| b)..]
                .starts_with("#link(\"") =>
            {
                let rest: String = chars[i + 7..].iter().collect();
                let parsed = rest.find("\")[").and_then(|u| {
                    let url = rest[..u].to_string();
                    let after = &rest[u + 3..];
                    after
                        .find(']')
                        .map(|e| (url, after[..e].to_string(), u + 3 + e + 1))
                });
                match parsed {
                    Some((url, label, used)) => {
                        out.push_str(&match t {
                            Target::Markdown => format!("[{label}]({url})"),
                            Target::Latex => {
                                format!("\\href{{{url}}}{{{}}}", latex_escape(&label))
                            }
                        });
                        i += 7 + rest[..used].chars().count();
                    }
                    None => {
                        out.push_str(&plain("#", t));
                        i += 1;
                    }
                }
            }
            c => {
                out.push_str(&plain(&c.to_string(), t));
                i += 1;
            }
        }
    }
    if bold && t == Target::Latex {
        out.push('}');
    }
    if emph && t == Target::Latex {
        out.push('}');
    }
    out
}

fn heading_level(line: &str) -> Option<(usize, &str)> {
    let n = line.chars().take_while(|c| *c == '=').count();
    (n > 0 && line[n..].starts_with(' ')).then(|| (n, line[n..].trim()))
}

/// A block of Typst markup as Markdown or LaTeX: headings, emphasis, lists, links and citations.
fn convert_text(text: &str, t: Target) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum List {
        None,
        Bullets,
        Numbers,
    }
    let mut out = String::new();
    let mut list = List::None;
    let close = |list: List, out: &mut String| {
        if t == Target::Latex {
            match list {
                List::Bullets => out.push_str("\\end{itemize}\n"),
                List::Numbers => out.push_str("\\end{enumerate}\n"),
                List::None => {}
            }
        }
    };
    for line in text.lines() {
        let trimmed = line.trim_start();
        let item = if let Some(r) = trimmed.strip_prefix("- ") {
            Some((List::Bullets, r))
        } else {
            trimmed.strip_prefix("+ ").map(|r| (List::Numbers, r))
        };
        if let Some((kind, rest)) = item {
            if list != kind {
                close(list, &mut out);
                if t == Target::Latex {
                    out.push_str(if kind == List::Bullets {
                        "\\begin{itemize}\n"
                    } else {
                        "\\begin{enumerate}\n"
                    });
                }
                list = kind;
            }
            let body = convert_inline(rest, t);
            match (t, kind) {
                (Target::Markdown, List::Bullets) => out.push_str(&format!("- {body}\n")),
                (Target::Markdown, _) => out.push_str(&format!("1. {body}\n")),
                (Target::Latex, _) => out.push_str(&format!("  \\item {body}\n")),
            }
            continue;
        }
        close(list, &mut out);
        list = List::None;
        if let Some((level, title)) = heading_level(trimmed) {
            let body = convert_inline(title, t);
            match t {
                Target::Markdown => out.push_str(&format!("{} {body}\n", "#".repeat(level))),
                Target::Latex => {
                    let cmd = ["section", "subsection", "subsubsection"][(level - 1).min(2)];
                    out.push_str(&format!("\\{cmd}{{{body}}}\n"));
                }
            }
        } else {
            out.push_str(&convert_inline(line, t));
            out.push('\n');
        }
    }
    close(list, &mut out);
    out
}

fn md_cite(q: &Quote, opts: &ExportOptions) -> String {
    let base = match key_of(q, opts) {
        Some(k) => format!("[@{k}, p. {}]", q.locator()),
        None => format!("({})", whence(q)),
    };
    if opts.links {
        format!(
            "{base} [↗]({})",
            q.link().replace(' ', "%20").replace(')', "%29")
        )
    } else {
        base
    }
}

fn to_markdown(nb: &Notebook, opts: &ExportOptions) -> String {
    let mut out = format!("# {}\n\n", nb.title);
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut String| {
        if !text.is_empty() {
            out.push_str(&convert_text(text, Target::Markdown));
            text.clear();
        }
    };
    for b in &nb.blocks {
        match b {
            Block::Text(t) => text.push_str(t),
            Block::Quote(q) => {
                flush(&mut text, &mut out);
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
                if let Some(image) = (opts.image_for)(q) {
                    out.push_str(&format!(
                        "![{}]({})\n\n{}\n\n",
                        q.text.replace(['[', ']', '\n'], " ").trim(),
                        image.replace(' ', "%20"),
                        md_cite(q, opts)
                    ));
                } else if q.text.trim().is_empty() {
                    out.push_str(&format!("**{}**\n\n", whence(q)));
                } else {
                    for p in paragraphs(&q.text) {
                        out.push_str(&format!("> {p}\n>\n"));
                    }
                    out.push_str(&format!("> — {}\n\n", md_cite(q, opts)));
                }
                for p in paragraphs(&q.note) {
                    out.push_str(&format!("{p}\n\n"));
                }
                for l in (opts.connections_of)(q) {
                    out.push_str(&format!(
                        "*Connected{}: {} ({})*\n\n",
                        if l.note.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", l.note)
                        },
                        l.text,
                        l.whence
                    ));
                }
            }
        }
    }
    flush(&mut text, &mut out);
    out
}

fn to_latex(nb: &Notebook, opts: &ExportOptions) -> String {
    let mut out = format!("\\section*{{{}}}\n\n", latex_escape(&nb.title));
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut String| {
        if !text.is_empty() {
            out.push_str(&convert_text(text, Target::Latex));
            text.clear();
        }
    };
    for b in &nb.blocks {
        match b {
            Block::Text(t) => text.push_str(t),
            Block::Quote(q) => {
                flush(&mut text, &mut out);
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
                let cite = match key_of(q, opts) {
                    Some(k) => format!("\\autocite[p.~{}]{{{k}}}", latex_escape(&q.locator())),
                    None => format!("({})", latex_escape(&whence(q))),
                };
                if let Some(image) = (opts.image_for)(q) {
                    out.push_str(&format!(
                        "\\begin{{figure}}[h]\n\\centering\n\\includegraphics[width=\\linewidth]{{{image}}}\n\\caption{{{cite}}}\n\\end{{figure}}\n\n"
                    ));
                } else if q.text.trim().is_empty() {
                    out.push_str(&format!("\\textbf{{{}}}\n\n", latex_escape(&whence(q))));
                } else {
                    let body = paragraphs(&q.text)
                        .iter()
                        .map(|p| latex_escape(p))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    out.push_str(&format!(
                        "\\begin{{quote}}\n{body}\n{cite}\n\\end{{quote}}\n\n"
                    ));
                }
                for p in paragraphs(&q.note) {
                    out.push_str(&format!("{}\n\n", latex_escape(&p)));
                }
                for l in (opts.connections_of)(q) {
                    out.push_str(&format!(
                        "\\emph{{Connected{}: {} ({})}}\n\n",
                        if l.note.is_empty() {
                            String::new()
                        } else {
                            format!(" --- {}", latex_escape(&l.note))
                        },
                        latex_escape(&l.text),
                        latex_escape(&l.whence)
                    ));
                }
            }
        }
    }
    flush(&mut text, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(id: &str, page: u32, text: &str) -> Quote {
        Quote {
            hash: "abc123".into(),
            id: id.into(),
            page,
            label: page.to_string(),
            colour: "#ffd966".into(),
            title: "Smith, On Things".into(),
            key: String::new(),
            text: text.into(),
            note: String::new(),
            area: false,
        }
    }

    fn plain_opts(format: Format) -> ExportOptions<'static> {
        ExportOptions {
            format,
            bibliography: None,
            key_for: &|_| None,
            image_for: &|_| None,
            connections_of: &|_| Vec::new(),
            links: false,
        }
    }

    #[test]
    fn a_notebook_survives_a_trip_through_its_file() {
        let mut nb = Notebook::new("thesis", "Thesis \"notes\"", Some("PhD".into()));
        nb.blocks
            .push(Block::Text("= Plan\nFirst, a thought.\n".into()));
        let mut q = quote("hl-1", 12, "He said \"no\"\nand left \\ quickly.");
        q.note = "Check this against Jones".into();
        nb.blocks.push(Block::Quote(q));
        nb.blocks
            .push(Block::Text("\nAnd then a paragraph.\n".into()));
        let back = Notebook::parse("thesis", &nb.to_file());
        assert_eq!(back, nb);
    }

    #[test]
    fn a_hand_written_file_with_no_header_still_opens() {
        let nb = Notebook::parse("reading-list", "= Hello\n\nSome text.\n");
        assert_eq!(nb.title, "reading list");
        assert_eq!(nb.shelf, None);
        assert_eq!(
            nb.blocks,
            vec![Block::Text("= Hello\n\nSome text.\n".into())]
        );
        assert_eq!(nb.body(), "= Hello\n\nSome text.\n");
    }

    #[test]
    fn a_damaged_quote_line_stays_as_text_instead_of_being_lost() {
        let text = "#pquote(hash: \"a\", oops\n";
        let nb = Notebook::parse("x", text);
        assert_eq!(nb.blocks, vec![Block::Text(text.into())]);
    }

    #[test]
    fn a_quote_after_text_with_no_newline_starts_its_own_line() {
        let mut nb = Notebook::new("x", "X", None);
        nb.blocks.push(Block::Text("no newline".into()));
        nb.blocks.push(Block::Quote(quote("a", 1, "t")));
        assert!(nb.body().starts_with("no newline\n#pquote("));
    }

    #[test]
    fn the_shelf_creates_lists_and_deletes_notebooks() {
        let dir = std::env::temp_dir().join(format!("pereplyot-nb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = create(&dir, "Chapter 3: Q & A", None).unwrap();
        let b = create(&dir, "Chapter 3: Q & A", Some("Thesis".into())).unwrap();
        assert_eq!(a.stem, "chapter-3-q-a");
        assert_eq!(b.stem, "chapter-3-q-a-2");
        let mut b = b;
        b.push_quote(quote("a", 3, "x"));
        save(&dir, &b).unwrap();
        let listed = list(&dir);
        assert_eq!(listed.len(), 2);
        let found = listed.iter().find(|s| s.stem == b.stem).unwrap();
        assert_eq!(found.shelf.as_deref(), Some("Thesis"));
        assert_eq!(found.quotes, 1);
        delete(&dir, &a.stem);
        assert_eq!(list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_or_deleting_a_shelf_moves_its_notebooks() {
        let dir = std::env::temp_dir().join(format!("pereplyot-nb-shelf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = create(&dir, "A", Some("Old".into())).unwrap();
        let b = create(&dir, "B", Some("Other".into())).unwrap();
        move_shelf(&dir, "Old", Some("New"));
        assert_eq!(load(&dir, &a.stem).unwrap().shelf.as_deref(), Some("New"));
        assert_eq!(load(&dir, &b.stem).unwrap().shelf.as_deref(), Some("Other"));
        move_shelf(&dir, "New", None);
        assert_eq!(load(&dir, &a.stem).unwrap().shelf, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typst_export_cites_with_the_key_and_links_back() {
        let mut nb = Notebook::new("x", "My essay", None);
        nb.blocks.push(Block::Text("Intro with *weight*.\n".into()));
        nb.push_quote(quote("hl-1", 12, "A line [with] brackets."));
        let opts = ExportOptions {
            key_for: &|_| Some("smith2020".into()),
            links: true,
            bibliography: Some("refs.bib"),
            ..plain_opts(Format::Typst)
        };
        let out = export(&nb, &opts);
        assert!(
            out.starts_with("= My essay\n\nIntro with *weight*.\n"),
            "{out}"
        );
        assert!(out.contains("attribution: [@smith2020[p. 12] #link(\"pereplyot://open?hash=abc123&annotation=hl-1\")[↗]]"), "{out}");
        assert!(out.contains("[A line \\[with\\] brackets.]"), "{out}");
        assert!(out.ends_with("#bibliography(\"refs.bib\")\n"), "{out}");
    }

    #[test]
    fn without_a_key_the_source_is_named_in_words() {
        let mut nb = Notebook::new("x", "E", None);
        nb.push_quote(quote("a", 4, "Words."));
        let out = export(&nb, &plain_opts(Format::Typst));
        assert!(
            out.contains("attribution: [Smith, On Things, p. 4]"),
            "{out}"
        );
    }

    #[test]
    fn markdown_and_latex_carry_the_authors_text_and_cite_every_quote() {
        let mut nb = Notebook::new("x", "E", None);
        nb.blocks.push(Block::Text(
            "== Part\nSee @jones2019[p. 3] and _this_ *that*.\n- one\n- two\n".into(),
        ));
        let mut q = quote("a", 4, "Words & more.");
        q.note = "My thought.".into();
        nb.push_quote(q);
        let with_key = |f| ExportOptions {
            key_for: &|_| Some("smith2020".into()),
            ..plain_opts(f)
        };
        let md = export(&nb, &with_key(Format::Markdown));
        assert!(md.contains("## Part\n"), "{md}");
        assert!(
            md.contains("See [@jones2019, p. 3] and *this* **that**."),
            "{md}"
        );
        assert!(md.contains("- one\n- two\n"), "{md}");
        assert!(
            md.contains("> Words & more.\n>\n> — [@smith2020, p. 4]"),
            "{md}"
        );
        assert!(md.contains("My thought."), "{md}");
        let tex = export(&nb, &with_key(Format::Latex));
        assert!(tex.contains("\\subsection{Part}"), "{tex}");
        assert!(tex.contains("\\autocite[p.~3]{jones2019}"), "{tex}");
        assert!(tex.contains("\\emph{this} \\textbf{that}."), "{tex}");
        assert!(
            tex.contains("\\begin{itemize}\n  \\item one\n  \\item two\n\\end{itemize}"),
            "{tex}"
        );
        assert!(
            tex.contains("Words \\& more.\n\\autocite[p.~4]{smith2020}\n\\end{quote}"),
            "{tex}"
        );
    }

    #[test]
    fn connections_are_mentioned_beside_the_quote() {
        let mut nb = Notebook::new("x", "E", None);
        nb.push_quote(quote("a", 4, "Words."));
        let opts = ExportOptions {
            connections_of: &|_| {
                vec![Linked {
                    note: "contradicts".into(),
                    text: "Other words".into(),
                    whence: "Jones, p. 12".into(),
                }]
            },
            ..plain_opts(Format::Typst)
        };
        let out = export(&nb, &opts);
        assert!(
            out.contains("_Connected — contradicts: Other words (Jones, p. 12)_"),
            "{out}"
        );
    }

    fn on_path(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .find(|f| f.is_file())
        })
    }

    #[test]
    fn the_stored_file_and_the_typst_export_both_compile_with_citations() {
        let Some(typst) = on_path("typst") else {
            eprintln!("no typst on PATH; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pereplyot-nb-typst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("refs.bib"),
            "@book{smith2020,\n  author = {Smith, Anna},\n  title = {On Things},\n  year = {2020},\n  publisher = {Press},\n}\n",
        )
        .unwrap();
        let mut nb = Notebook::new("essay", "An essay", None);
        nb.blocks
            .push(Block::Text("= Argument\nSmith disagrees.\n".into()));
        let mut q = quote(
            "hl-1",
            12,
            "Quote with \"quotes\", [brackets], #hash and $dollars$.",
        );
        q.note = "A note: with *stars* and _underscores_.".into();
        nb.push_quote(q);
        nb.push_quote(quote("hl-2", 14, ""));
        nb.blocks.push(Block::Text("\nThe end.\n".into()));

        let stored = dir.join("stored.typ");
        std::fs::write(&stored, nb.to_file()).unwrap();
        let exported = dir.join("export.typ");
        let opts = ExportOptions {
            key_for: &|_| Some("smith2020".into()),
            links: true,
            bibliography: Some("refs.bib"),
            ..plain_opts(Format::Typst)
        };
        std::fs::write(&exported, export(&nb, &opts)).unwrap();
        for (file, pdf) in [(&stored, "stored.pdf"), (&exported, "export.pdf")] {
            let out = std::process::Command::new(&typst)
                .arg("compile")
                .arg(file)
                .arg(dir.join(pdf))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{} did not compile:\n{}",
                file.display(),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
