//! Markdown blocks: CommonMark with GitHub's tables and task lists.
//!
//! Lines are read the way the CommonMark reference parsers read them. Each
//! line first continues the open containers (quotes, list items), may then
//! open new blocks, and what is left is text for the innermost open block,
//! or lazily for an open paragraph. Inline content is parsed at the end,
//! once every link reference definition is known. Raw HTML blocks go
//! through the HTML reader, so a README's `<p align="center">` logo shows
//! as a picture.

use super::inline::{self, Refs, Unclosed};
use std::cell::Cell;

use super::{first_title, html, table, Align, Block, Document, Item, MAX_NODES};

/// Quotes and list items nested deeper than this read as text. Each level
/// of a list takes two (the list and its item).
const MAX_CONTAINERS: usize = 2 * super::MAX_NESTING;

/// Tags that start an HTML block which ends at a blank line.
const BLOCK_TAGS: &[&str] = &[
    "address", "article", "aside", "base", "basefont", "blockquote", "body", "caption", "center", "col", "colgroup",
    "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset", "figcaption", "figure", "footer", "form", "frame",
    "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr", "html", "iframe", "legend", "li", "link",
    "main", "menu", "menuitem", "nav", "noframes", "ol", "optgroup", "option", "p", "param", "search", "section",
    "summary", "table", "tbody", "td", "tfoot", "th", "thead", "title", "tr", "track", "ul",
];

/// Tags whose HTML block runs to their closing tag, blank lines and all.
const RAW_TAGS: [&str; 4] = ["pre", "script", "style", "textarea"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Bullet(u8),
    /// An ordered item's delimiter, `.` or `)`.
    Ordered(u8),
}

#[derive(Clone, Copy, Debug)]
struct ListData {
    marker: Marker,
    start: u32,
    /// Columns before the marker.
    marker_offset: usize,
    /// Columns from the marker to the item's content.
    padding: usize,
}

#[derive(Clone, Copy, Debug)]
struct Fence {
    ch: u8,
    len: usize,
    /// The opening fence's indentation, removed from each line of code.
    indent: usize,
}

#[derive(Debug)]
enum Kind {
    Document,
    Quote,
    List(ListData),
    Item(ListData),
    Paragraph,
    Heading(u8),
    /// `None` for indented code.
    Code(Option<Fence>),
    /// CommonMark's HTML block conditions 1 to 7, which end differently.
    Html(u8),
    Rule,
    Table { align: Vec<Align>, header: String, rows: Vec<String> },
}

struct Node {
    kind: Kind,
    parent: usize,
    children: Vec<usize>,
    open: bool,
    /// Lines of text, each ending in `\n`; for code, the code itself once
    /// the block is closed.
    content: String,
    /// A fenced code block's info string.
    info: String,
    depth: usize,
}

/// What a line did to an open block.
enum Continued {
    Matched,
    Failed,
    /// A closing code fence: nothing more to do with this line.
    Consumed,
}

/// Whether a line started a block.
enum Started {
    No,
    /// A container, which may contain more blocks on the same line.
    Container,
    /// A leaf, which takes the rest of the line.
    Leaf,
}

struct Parser {
    nodes: Vec<Node>,
    tip: usize,
    old_tip: usize,
    last_matched: usize,
    all_closed: bool,
    refs: Refs,
    line: String,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
    /// An HTML block was too long to read whole.
    truncated: Cell<bool>,
}

pub(super) fn parse(text: &str) -> Document {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let (front_matter, body) = split_front_matter(text);
    let mut parser = Parser::new();
    let mut truncated = false;
    for line in lines(body) {
        // A line adds at most a few dozen nodes; past the limit the rest of
        // the text is left out.
        if parser.nodes.len() >= MAX_NODES {
            truncated = true;
            break;
        }
        parser.incorporate(line);
    }
    parser.finish();
    let mut blocks = Vec::new();
    if let Some(yaml) = front_matter {
        blocks.push(Block::Code {
            language: "yaml".into(),
            text: yaml.to_owned(),
        });
    }
    blocks.extend(parser.render(&parser.nodes[0].children));
    Document {
        title: first_title(&blocks),
        blocks,
        truncated: truncated || parser.truncated.get(),
    }
}

/// Lines without their endings (`\n`, `\r\n` or `\r`).
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(['\n', '\r']).unwrap_or(rest.len());
        let line = &rest[..end];
        rest = if rest[end..].starts_with("\r\n") {
            &rest[end + 2..]
        } else {
            &rest[(end + 1).min(rest.len())..]
        };
        Some(line)
    })
}

/// YAML front matter (`---` lines around `key: value` pairs, as Jekyll and
/// Hugo pages start), shown as code rather than as a rule and a heading.
fn split_front_matter(text: &str) -> (Option<&str>, &str) {
    let Some(after) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (None, text);
    };
    let first = after.lines().next().unwrap_or("");
    let key = first.split_once(':').map_or("", |(k, _)| k);
    if key.is_empty() || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-') {
        return (None, text);
    }
    let mut offset = 0;
    for line in after.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare == "---" || bare == "..." {
            return (Some(after[..offset].trim_end_matches(['\n', '\r'])), &after[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

fn is_space_or_tab(c: Option<u8>) -> bool {
    matches!(c, Some(b' ' | b'\t'))
}

/// Characters that may start a block other than a paragraph.
fn maybe_special(c: u8) -> bool {
    matches!(c, b'#' | b'`' | b'~' | b'*' | b'+' | b'_' | b'=' | b'<' | b'>' | b'-' | b'|' | b':' | b'0'..=b'9')
}

/// `#` to `######` and the heading's text, closing hashes removed.
fn atx_heading(s: &str) -> Option<(u8, &str)> {
    let hashes = s.bytes().take_while(|&c| c == b'#').count();
    if !(1..=6).contains(&hashes) || !matches!(s.as_bytes().get(hashes), None | Some(b' ' | b'\t')) {
        return None;
    }
    let mut text = s[hashes..].trim_matches([' ', '\t']);
    let without = text.trim_end_matches('#');
    if without.is_empty() {
        text = "";
    } else if without.ends_with([' ', '\t']) {
        text = without.trim_end_matches([' ', '\t']);
    }
    Some((hashes as u8, text))
}

/// The length of an opening code fence: three or more backticks (with no
/// backtick in the info string after them) or tildes.
fn opening_fence(s: &str) -> Option<usize> {
    let ch = *s.as_bytes().first()?;
    let len = s.bytes().take_while(|&c| c == ch).count();
    if len < 3 || (ch == b'`' && s[len..].contains('`')) {
        return None;
    }
    Some(len)
}

fn closes_fence(s: &str, fence: Fence) -> bool {
    let len = s.bytes().take_while(|&c| c == fence.ch).count();
    len >= fence.len && s[len..].bytes().all(|c| c == b' ' || c == b'\t')
}

fn is_thematic_break(s: &str) -> bool {
    let Some(&ch) = s.as_bytes().first() else {
        return false;
    };
    matches!(ch, b'*' | b'-' | b'_')
        && s.bytes().filter(|&c| c == ch).count() >= 3
        && s.bytes().all(|c| c == ch || c == b' ' || c == b'\t')
}

fn setext_level(s: &str) -> Option<u8> {
    let ch = *s.as_bytes().first()?;
    let len = s.bytes().take_while(|&c| c == ch).count();
    if !s[len..].bytes().all(|c| c == b' ' || c == b'\t') {
        return None;
    }
    match ch {
        b'=' => Some(1),
        b'-' => Some(2),
        _ => None,
    }
}

/// The tag name after `<` or `</`, lower-cased.
fn tag_name(s: &str) -> Option<(String, &str)> {
    let s = s.strip_prefix('<')?;
    let s = s.strip_prefix('/').unwrap_or(s);
    let len = s.bytes().take_while(|c| c.is_ascii_alphanumeric()).count();
    if len == 0 || !s.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    Some((s[..len].to_ascii_lowercase(), &s[len..]))
}

/// Which HTML block, if any, a line starts.
fn html_block_start(s: &str) -> Option<u8> {
    let lower = s.get(..9).unwrap_or(s).to_ascii_lowercase();
    if let Some((name, rest)) = tag_name(s) {
        let next = rest.as_bytes().first().copied();
        let opening = !s.starts_with("</");
        if opening && RAW_TAGS.contains(&name.as_str()) && matches!(next, None | Some(b' ' | b'\t' | b'>')) {
            return Some(1);
        }
        if BLOCK_TAGS.contains(&name.as_str())
            && (matches!(next, None | Some(b' ' | b'\t' | b'>')) || rest.starts_with("/>"))
        {
            return Some(6);
        }
    }
    if lower.starts_with("<!--") {
        return Some(2);
    }
    if lower.starts_with("<?") {
        return Some(3);
    }
    if lower.starts_with("<![cdata[") {
        return Some(5);
    }
    if lower.starts_with("<!") && s.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) {
        return Some(4);
    }
    // A complete tag alone on its line.
    let (len, tag) = inline::html_tag(s, 0, &mut Unclosed::new())?;
    (!RAW_TAGS.contains(&tag.name.as_str()) && s[len..].bytes().all(|c| c == b' ' || c == b'\t')).then_some(7)
}

fn html_block_ends(kind: u8, line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    match kind {
        1 => RAW_TAGS.iter().any(|tag| lower.contains(&format!("</{tag}>"))),
        2 => line.contains("-->"),
        3 => line.contains("?>"),
        4 => line.contains('>'),
        5 => line.contains("]]>"),
        _ => false,
    }
}

/// A table row's cells, without the outer pipes, trimmed, `\|` unescaped.
fn split_cells(line: &str) -> Vec<String> {
    let line = line.trim_matches([' ', '\t']);
    let b = line.as_bytes();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let start = usize::from(b.first() == Some(&b'|'));
    let mut flushed = start;
    let mut i = start;
    let mut ended_on_pipe = false;
    while i < b.len() {
        match b[i] {
            b'\\' if b.get(i + 1) == Some(&b'|') => {
                cell.push_str(&line[flushed..i]);
                cell.push('|');
                i += 2;
                flushed = i;
            }
            b'\\' if b.get(i + 1).is_some_and(u8::is_ascii) => i += 2,
            b'|' => {
                cell.push_str(&line[flushed..i]);
                cells.push(std::mem::take(&mut cell).trim_matches([' ', '\t']).to_owned());
                i += 1;
                flushed = i;
                ended_on_pipe = i >= b.len();
                continue;
            }
            _ => i += 1,
        }
        ended_on_pipe = false;
    }
    if !ended_on_pipe && (flushed < b.len() || !cell.is_empty() || cells.is_empty() && start == 0) {
        cell.push_str(&line[flushed.min(b.len())..]);
        cells.push(cell.trim_matches([' ', '\t']).to_owned());
    }
    cells
}

/// A table's delimiter row (`| :-- | :-: | --: |`): the columns' alignment.
fn delimiter_row(line: &str) -> Option<Vec<Align>> {
    if !line.bytes().all(|c| matches!(c, b'|' | b':' | b'-' | b' ' | b'\t')) || !line.contains('-') {
        return None;
    }
    split_cells(line)
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let inner = cell.strip_prefix(':').unwrap_or(cell);
            let (inner, right) = match inner.strip_suffix(':') {
                Some(inner) => (inner, true),
                None => (inner, false),
            };
            if inner.is_empty() || !inner.bytes().all(|c| c == b'-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                (false, false) => Align::None,
            })
        })
        .collect()
}

/// `[ ]`, `[x]` or `[X]` starting a list item's text, and the text after.
fn task_marker(text: &str) -> Option<(bool, &str)> {
    let checked = match text.get(..3)? {
        "[ ]" => false,
        "[x]" | "[X]" => true,
        _ => return None,
    };
    let rest = &text[3..];
    (rest.is_empty() || rest.starts_with([' ', '\t', '\n'])).then_some((checked, rest))
}

impl Parser {
    fn new() -> Self {
        Parser {
            nodes: vec![Node {
                kind: Kind::Document,
                parent: 0,
                children: Vec::new(),
                open: true,
                content: String::new(),
                info: String::new(),
                depth: 0,
            }],
            tip: 0,
            old_tip: 0,
            last_matched: 0,
            all_closed: true,
            refs: Refs::new(),
            line: String::new(),
            offset: 0,
            column: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            partially_consumed_tab: false,
            truncated: Cell::new(false),
        }
    }

    fn peek(&self, at: usize) -> Option<u8> {
        self.line.as_bytes().get(at).copied()
    }

    fn find_next_nonspace(&mut self) {
        let b = self.line.as_bytes();
        let mut i = self.offset;
        let mut column = self.column;
        while let Some(&c) = b.get(i) {
            match c {
                b' ' => column += 1,
                b'\t' => column += 4 - column % 4,
                _ => break,
            }
            i += 1;
        }
        self.blank = i >= b.len();
        self.next_nonspace = i;
        self.next_nonspace_column = column;
        self.indent = column - self.column;
        self.indented = self.indent >= 4;
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    /// Move over `count` characters, or with `columns` over `count` columns,
    /// where a tab may be consumed only partly. Only ever crosses ASCII.
    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        while count > 0 {
            let Some(c) = self.peek(self.offset) else {
                break;
            };
            if c == b'\t' {
                let to_tab = 4 - self.column % 4;
                if columns {
                    self.partially_consumed_tab = to_tab > count;
                    let advance = to_tab.min(count);
                    self.column += advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += 1;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn advance_to_end(&mut self) {
        self.offset = self.line.len();
        self.partially_consumed_tab = false;
    }

    fn accepts_lines(&self, id: usize) -> bool {
        matches!(self.nodes[id].kind, Kind::Paragraph | Kind::Code(_) | Kind::Html(_) | Kind::Table { .. })
    }

    fn add_child(&mut self, kind: Kind) -> usize {
        loop {
            let can_contain = match self.nodes[self.tip].kind {
                Kind::Document | Kind::Quote | Kind::Item(_) => !matches!(kind, Kind::Item(_)),
                Kind::List(_) => matches!(kind, Kind::Item(_)),
                _ => false,
            };
            if can_contain || self.tip == 0 {
                break;
            }
            self.finalize(self.tip);
        }
        let id = self.nodes.len();
        let depth = self.nodes[self.tip].depth + 1;
        self.nodes.push(Node {
            kind,
            parent: self.tip,
            children: Vec::new(),
            open: true,
            content: String::new(),
            info: String::new(),
            depth,
        });
        self.nodes[self.tip].children.push(id);
        self.tip = id;
        id
    }

    fn add_line(&mut self) {
        let node = &mut self.nodes[self.tip];
        if self.partially_consumed_tab {
            // Skip the tab, but keep the columns it had left.
            self.offset += 1;
            let to_tab = 4 - self.column % 4;
            node.content.push_str(&"    "[..to_tab]);
        }
        let rest = &self.line[self.offset.min(self.line.len())..];
        match &mut node.kind {
            Kind::Table { rows, .. } => {
                if !rest.trim_matches([' ', '\t']).is_empty() {
                    rows.push(rest.to_owned());
                }
            }
            _ => {
                node.content.push_str(rest);
                node.content.push('\n');
            }
        }
    }

    /// Link reference definitions at the start of a paragraph's text go to
    /// the table; returns what is left.
    fn take_definitions(&mut self, content: &str) -> String {
        let mut pos = 0;
        while content[pos..].starts_with('[') {
            let used = inline::parse_reference(&content[pos..], &mut self.refs);
            if used == 0 {
                break;
            }
            pos += used;
        }
        content[pos..].to_owned()
    }

    fn finalize(&mut self, id: usize) {
        let parent = self.nodes[id].parent;
        self.nodes[id].open = false;
        match self.nodes[id].kind {
            Kind::Paragraph => {
                let content = std::mem::take(&mut self.nodes[id].content);
                let rest = self.take_definitions(&content);
                if rest.trim().is_empty() && id != 0 {
                    let siblings = &mut self.nodes[parent].children;
                    if siblings.last() == Some(&id) {
                        siblings.pop();
                    } else {
                        siblings.retain(|&c| c != id);
                    }
                }
                self.nodes[id].content = rest;
            }
            Kind::Code(Some(_)) => {
                let node = &mut self.nodes[id];
                let content = std::mem::take(&mut node.content);
                let (info, code) = content.split_once('\n').unwrap_or((content.as_str(), ""));
                node.info = inline::unescape(info.trim());
                node.content = code.strip_suffix('\n').unwrap_or(code).to_owned();
            }
            Kind::Code(None) => {
                let node = &mut self.nodes[id];
                let mut lines: Vec<&str> = node.content.split('\n').collect();
                while lines.last().is_some_and(|l| l.trim_matches([' ', '\t']).is_empty()) {
                    lines.pop();
                }
                node.content = lines.join("\n");
            }
            _ => {}
        }
        self.tip = parent;
    }

    fn close_unmatched(&mut self) {
        if self.all_closed {
            return;
        }
        while self.old_tip != self.last_matched {
            let parent = self.nodes[self.old_tip].parent;
            self.finalize(self.old_tip);
            self.old_tip = parent;
        }
        self.all_closed = true;
    }

    fn finish(&mut self) {
        loop {
            let tip = self.tip;
            self.finalize(tip);
            if tip == 0 {
                break;
            }
        }
    }

    fn incorporate(&mut self, line: &str) {
        self.line.clear();
        if line.contains('\0') {
            self.line.push_str(&line.replace('\0', "\u{FFFD}"));
        } else {
            self.line.push_str(line);
        }
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.old_tip = self.tip;

        // Continue the open blocks, outermost first.
        let mut container = 0;
        while let Some(&last) = self.nodes[container].children.last() {
            if !self.nodes[last].open {
                break;
            }
            container = last;
            self.find_next_nonspace();
            match self.continue_block(container) {
                Continued::Matched => {}
                Continued::Failed => {
                    container = self.nodes[container].parent;
                    break;
                }
                Continued::Consumed => return,
            }
        }
        self.all_closed = container == self.old_tip;
        self.last_matched = container;

        // Open new blocks, unless the line belongs to a code or HTML block.
        let mut matched_leaf =
            self.accepts_lines(container) && !matches!(self.nodes[container].kind, Kind::Paragraph | Kind::Table { .. });
        while !matched_leaf {
            self.find_next_nonspace();
            if !self.indented && !self.peek(self.next_nonspace).is_some_and(maybe_special) {
                self.advance_next_nonspace();
                break;
            }
            match self.block_start(container) {
                Started::No => {
                    self.advance_next_nonspace();
                    break;
                }
                Started::Container => container = self.tip,
                Started::Leaf => {
                    container = self.tip;
                    matched_leaf = true;
                }
            }
        }

        // The rest is text: a lazy paragraph continuation, a line of the
        // current leaf, or a new paragraph.
        if !self.all_closed && !self.blank && matches!(self.nodes[self.tip].kind, Kind::Paragraph) {
            self.add_line();
            return;
        }
        self.close_unmatched();
        if self.accepts_lines(container) {
            self.add_line();
            if let Kind::Html(kind @ 1..=5) = self.nodes[container].kind {
                if html_block_ends(kind, &self.line[self.offset.min(self.line.len())..]) {
                    self.finalize(container);
                }
            }
        } else if self.offset < self.line.len() && !self.blank {
            self.add_child(Kind::Paragraph);
            self.advance_next_nonspace();
            self.add_line();
        }
    }

    fn continue_block(&mut self, id: usize) -> Continued {
        match self.nodes[id].kind {
            Kind::Document | Kind::List(_) => Continued::Matched,
            Kind::Quote => {
                if self.indented || self.peek(self.next_nonspace) != Some(b'>') {
                    return Continued::Failed;
                }
                self.advance_next_nonspace();
                self.advance_offset(1, false);
                if is_space_or_tab(self.peek(self.offset)) {
                    self.advance_offset(1, true);
                }
                Continued::Matched
            }
            Kind::Item(data) => {
                let needed = data.marker_offset + data.padding;
                if self.blank {
                    if self.nodes[id].children.is_empty() {
                        // An item can start with at most one blank line.
                        return Continued::Failed;
                    }
                    self.advance_next_nonspace();
                } else if self.indent >= needed {
                    self.advance_offset(needed, true);
                } else {
                    return Continued::Failed;
                }
                Continued::Matched
            }
            Kind::Heading(_) | Kind::Rule => Continued::Failed,
            Kind::Code(Some(fence)) => {
                if self.indent <= 3 && closes_fence(&self.line[self.next_nonspace..], fence) {
                    self.finalize(id);
                    return Continued::Consumed;
                }
                let mut strip = fence.indent;
                while strip > 0 && is_space_or_tab(self.peek(self.offset)) {
                    self.advance_offset(1, true);
                    strip -= 1;
                }
                Continued::Matched
            }
            Kind::Code(None) => {
                if self.indent >= 4 {
                    self.advance_offset(4, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return Continued::Failed;
                }
                Continued::Matched
            }
            Kind::Html(kind) => {
                if self.blank && kind >= 6 {
                    Continued::Failed
                } else {
                    Continued::Matched
                }
            }
            Kind::Paragraph | Kind::Table { .. } => {
                if self.blank {
                    Continued::Failed
                } else {
                    Continued::Matched
                }
            }
        }
    }

    fn block_start(&mut self, container: usize) -> Started {
        let room = self.nodes[container].depth < MAX_CONTAINERS;
        let in_paragraph = matches!(self.nodes[container].kind, Kind::Paragraph);
        let rest_start = self.next_nonspace;
        if !self.indented {
            let first = self.peek(rest_start);
            // Block quote.
            if first == Some(b'>') && room {
                self.advance_next_nonspace();
                self.advance_offset(1, false);
                if is_space_or_tab(self.peek(self.offset)) {
                    self.advance_offset(1, true);
                }
                self.close_unmatched();
                self.add_child(Kind::Quote);
                return Started::Container;
            }
            // ATX heading.
            if let Some((level, text)) = atx_heading(&self.line[rest_start..]) {
                let text = text.to_owned();
                self.close_unmatched();
                let id = self.add_child(Kind::Heading(level));
                self.nodes[id].content = text;
                self.advance_to_end();
                return Started::Leaf;
            }
            // Fenced code.
            if matches!(first, Some(b'`' | b'~')) {
                if let Some(len) = opening_fence(&self.line[rest_start..]) {
                    let fence = Fence {
                        ch: first.unwrap_or(b'`'),
                        len,
                        indent: self.indent,
                    };
                    self.close_unmatched();
                    self.add_child(Kind::Code(Some(fence)));
                    self.advance_next_nonspace();
                    self.advance_offset(len, false);
                    return Started::Leaf;
                }
            }
            // HTML block; the 7th kind cannot interrupt a paragraph.
            if first == Some(b'<') {
                if let Some(kind) = html_block_start(&self.line[rest_start..]) {
                    let lazy = !self.all_closed && !self.blank && matches!(self.nodes[self.tip].kind, Kind::Paragraph);
                    if kind < 7 || (!in_paragraph && !lazy) {
                        self.close_unmatched();
                        // The indentation belongs to the block.
                        self.add_child(Kind::Html(kind));
                        return Started::Leaf;
                    }
                }
            }
            // Setext heading underline.
            if in_paragraph && matches!(first, Some(b'=' | b'-')) {
                if let Some(level) = setext_level(&self.line[rest_start..]) {
                    self.close_unmatched();
                    let content = std::mem::take(&mut self.nodes[container].content);
                    let rest = self.take_definitions(&content);
                    let has_text = !rest.trim().is_empty();
                    self.nodes[container].content = rest;
                    if has_text {
                        self.nodes[container].kind = Kind::Heading(level);
                        self.advance_to_end();
                        return Started::Leaf;
                    }
                }
            }
            // Thematic break.
            if is_thematic_break(&self.line[rest_start..]) {
                self.close_unmatched();
                self.add_child(Kind::Rule);
                self.advance_to_end();
                return Started::Leaf;
            }
        }
        // List item.
        if (!self.indented || matches!(self.nodes[container].kind, Kind::List(_))) && room {
            if let Some(data) = self.list_marker(in_paragraph) {
                self.close_unmatched();
                let continues = matches!(self.nodes[self.tip].kind, Kind::List(list) if list.marker == data.marker);
                if !continues {
                    self.add_child(Kind::List(data));
                }
                self.add_child(Kind::Item(data));
                return Started::Container;
            }
        }
        // Indented code.
        if self.indented && !self.blank && !matches!(self.nodes[self.tip].kind, Kind::Paragraph | Kind::Table { .. }) {
            self.advance_offset(4, true);
            self.close_unmatched();
            self.add_child(Kind::Code(None));
            return Started::Leaf;
        }
        // Table: a delimiter row under a paragraph's last line, its header.
        if !self.indented && in_paragraph {
            if let Some(started) = self.table_start(container) {
                return started;
            }
        }
        Started::No
    }

    fn table_start(&mut self, paragraph: usize) -> Option<Started> {
        let delimiter = &self.line[self.next_nonspace..];
        let align = delimiter_row(delimiter)?;
        let content = &self.nodes[paragraph].content;
        let text = content.strip_suffix('\n').unwrap_or(content);
        let (before, header) = match text.rfind('\n') {
            Some(i) => (&text[..=i], &text[i + 1..]),
            None => ("", text),
        };
        if !(header.contains('|') || delimiter.contains('|')) || split_cells(header).len() != align.len() {
            return None;
        }
        let align: Vec<Align> = align.into_iter().take(super::MAX_COLUMNS).collect();
        let kind = Kind::Table {
            align,
            header: header.to_owned(),
            rows: Vec::new(),
        };
        let before = before.to_owned();
        self.close_unmatched();
        if before.trim().is_empty() {
            self.nodes[paragraph].kind = kind;
            self.nodes[paragraph].content.clear();
        } else {
            self.nodes[paragraph].content = before;
            self.finalize(paragraph);
            self.add_child(kind);
        }
        self.advance_to_end();
        Some(Started::Leaf)
    }

    /// A list marker at the next non-space character: `-`, `+`, `*`, or up
    /// to nine digits and `.` or `)`. Leaves the offset where the item's
    /// content starts.
    fn list_marker(&mut self, in_paragraph: bool) -> Option<ListData> {
        if self.indent >= 4 {
            return None;
        }
        let rest = &self.line.as_bytes()[self.next_nonspace..];
        let (marker, start, len) = match *rest.first()? {
            c @ (b'*' | b'+' | b'-') => (Marker::Bullet(c), 1, 1),
            b'0'..=b'9' => {
                let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
                let delimiter = *rest.get(digits)?;
                if digits > 9 || !matches!(delimiter, b'.' | b')') {
                    return None;
                }
                let start = self.line[self.next_nonspace..self.next_nonspace + digits].parse::<u32>().unwrap_or(1);
                // Only a list starting at 1 may interrupt a paragraph.
                if in_paragraph && start != 1 {
                    return None;
                }
                (Marker::Ordered(delimiter), start, digits + 1)
            }
            _ => return None,
        };
        if !matches!(rest.get(len), None | Some(b' ' | b'\t')) {
            return None;
        }
        // Nor may an empty item.
        if in_paragraph && rest[len..].iter().all(|&c| c == b' ' || c == b'\t') {
            return None;
        }
        let marker_offset = self.indent;
        self.advance_next_nonspace();
        self.advance_offset(len, true);
        let spaces_column = self.column;
        let spaces_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            if !(self.column - spaces_column < 5 && is_space_or_tab(self.peek(self.offset))) {
                break;
            }
        }
        let spaces = self.column - spaces_column;
        let blank_item = self.peek(self.offset).is_none();
        let padding = if !(1..5).contains(&spaces) || blank_item {
            // Content indented 5 or more is code inside the item: the item
            // itself starts one space after the marker.
            self.column = spaces_column;
            self.offset = spaces_offset;
            self.partially_consumed_tab = false;
            if is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            len + 1
        } else {
            len + spaces
        };
        Some(ListData {
            marker,
            start,
            marker_offset,
            padding,
        })
    }

    fn render(&self, children: &[usize]) -> Vec<Block> {
        let mut blocks = Vec::new();
        // HTML blocks in a row, separated only by blank lines, are read as
        // one piece of HTML, so that a table written across them stays one.
        let mut html = String::new();
        for (i, &id) in children.iter().enumerate() {
            let node = &self.nodes[id];
            if let Kind::Html(_) = node.kind {
                html.push_str(&node.content);
                let next = children.get(i + 1).map(|&next| &self.nodes[next].kind);
                if !matches!(next, Some(Kind::Html(_))) {
                    let (fragment, cut) = html::fragment(&std::mem::take(&mut html));
                    if cut {
                        self.truncated.set(true);
                    }
                    blocks.extend(fragment);
                }
                continue;
            }
            match &node.kind {
                Kind::Paragraph => {
                    let inlines = inline::parse(node.content.trim_matches([' ', '\t', '\n']), &self.refs);
                    if !inlines.is_empty() {
                        blocks.push(Block::Paragraph { inlines });
                    }
                }
                // Empty headings and quotes show nothing, as in HTML.
                Kind::Heading(level) => {
                    let inlines = inline::parse(node.content.trim_matches([' ', '\t', '\n']), &self.refs);
                    if !inlines.is_empty() {
                        blocks.push(Block::Heading { level: *level, inlines });
                    }
                }
                Kind::Code(_) => {
                    // The first word of the info string, also as Pandoc and
                    // R Markdown (`{.python}`, `{r, echo=FALSE}`) and rustdoc
                    // (`rust,ignore`) write it.
                    let info = node.info.trim_start().trim_start_matches(['{', '.']);
                    let language = info.split(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '}')).next();
                    blocks.push(Block::Code {
                        language: language.unwrap_or("").to_owned(),
                        text: node.content.clone(),
                    });
                }
                Kind::Rule => blocks.push(Block::Rule),
                Kind::Quote => {
                    let inner = self.render(&node.children);
                    if !inner.is_empty() {
                        blocks.push(Block::Quote { blocks: inner });
                    }
                }
                Kind::List(data) => blocks.push(Block::List {
                    ordered: matches!(data.marker, Marker::Ordered(_)),
                    start: i64::from(data.start),
                    items: node.children.iter().map(|&item| self.render_item(item)).collect(),
                }),
                Kind::Table { align, header, rows } => {
                    let cells = |line: &str| -> Vec<Vec<super::Inline>> {
                        split_cells(line).iter().map(|cell| inline::parse(cell, &self.refs)).collect()
                    };
                    blocks.extend(table(align.clone(), cells(header), rows.iter().map(|row| cells(row)).collect()));
                }
                Kind::Document | Kind::Item(_) | Kind::Html(_) => {}
            }
        }
        blocks
    }

    fn render_item(&self, id: usize) -> Item {
        let children = &self.nodes[id].children;
        let first = children.first().map(|&c| &self.nodes[c]);
        if let Some(Node {
            kind: Kind::Paragraph,
            content,
            ..
        }) = first
        {
            if let Some((checked, text)) = task_marker(content) {
                let mut blocks = Vec::new();
                let inlines = inline::parse(text.trim_matches([' ', '\t', '\n']), &self.refs);
                if !inlines.is_empty() {
                    blocks.push(Block::Paragraph { inlines });
                }
                blocks.extend(self.render(&children[1..]));
                return Item {
                    checked: Some(checked),
                    blocks,
                };
            }
        }
        Item {
            checked: None,
            blocks: self.render(children),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{outline, show};
    use super::super::{Inline, Style};
    use super::*;

    fn t(text: &str) -> Inline {
        Inline::Text {
            text: text.into(),
            style: Style::default(),
        }
    }

    fn para(text: &str) -> Block {
        Block::Paragraph { inlines: vec![t(text)] }
    }

    fn blocks(text: &str) -> Vec<Block> {
        parse(text).blocks
    }

    fn heading(level: u8, text: &str) -> Block {
        Block::Heading {
            level,
            inlines: vec![t(text)],
        }
    }

    fn code(language: &str, text: &str) -> Block {
        Block::Code {
            language: language.into(),
            text: text.into(),
        }
    }

    fn list(ordered: bool, start: i64, items: Vec<Vec<Block>>) -> Block {
        Block::List {
            ordered,
            start,
            items: items.into_iter().map(|blocks| Item { checked: None, blocks }).collect(),
        }
    }

    #[test]
    fn headings() {
        assert_eq!(
            blocks("# One\n## Two ##\n###### Six\n####### seven\n#hash\n# \n### closing #s ###   \n# not \\#"),
            [
                heading(1, "One"),
                heading(2, "Two"),
                heading(6, "Six"),
                para("####### seven #hash"),
                heading(3, "closing #s"),
                heading(1, "not #"),
            ]
        );
        assert_eq!(blocks("Title\n===\nSub\n---\n  multi\nline\n  ==="), [heading(1, "Title"), heading(2, "Sub"), heading(1, "multi line")]);
        assert_eq!(parse("intro\n\n# The Title\n\n# Second").title.as_deref(), Some("The Title"));
        assert_eq!(parse("## only two").title, None);
    }

    #[test]
    fn paragraphs_and_breaks() {
        assert_eq!(blocks("a\nb\n\n\nc"), [para("a b"), para("c")]);
        assert_eq!(
            blocks("one  \ntwo\\\nthree"),
            [Block::Paragraph { inlines: vec![t("one"), Inline::Break, t("two"), Inline::Break, t("three")] }]
        );
        assert_eq!(blocks("   indented three\n\tnot code after para"), [para("indented three not code after para")]);
        assert_eq!(blocks(""), []);
        assert_eq!(blocks("\n\n  \n"), []);
    }

    #[test]
    fn thematic_breaks() {
        assert_eq!(blocks("***\n- - -\n___\n--\n**"), [Block::Rule, Block::Rule, Block::Rule, para("-- **")]);
        assert_eq!(blocks("a\n***\nb"), [para("a"), Block::Rule, para("b")]);
        assert_eq!(blocks("* * *\n* a"), [Block::Rule, list(false, 1, vec![vec![para("a")]])]);
    }

    #[test]
    fn code_blocks() {
        assert_eq!(blocks("```rust ignore\nfn main() {}\n\n  x\n```\nafter"), [code("rust", "fn main() {}\n\n  x"), para("after")]);
        assert_eq!(blocks("~~~~\n```\nstill code\n~~~~~"), [code("", "```\nstill code")]);
        assert_eq!(blocks("  ```\n  a\n b\nc\n  ```"), [code("", "a\nb\nc")]);
        assert_eq!(blocks("```\nunclosed"), [code("", "unclosed")]);
        assert_eq!(blocks("``` a`b\n"), [Block::Paragraph { inlines: vec![t("``` a`b")] }]);
        assert_eq!(blocks("```{r}\nx\n```"), [code("r", "x")]);
        assert_eq!(blocks("    code\n      more\n\n    end\n\npara"), [code("", "code\n  more\n\nend"), para("para")]);
        assert_eq!(blocks("para\n    not code"), [para("para not code")]);
        assert_eq!(blocks("\tcode with tab"), [code("", "code with tab")]);
    }

    #[test]
    fn block_quotes() {
        assert_eq!(blocks("> a\n> b\n>\n> c"), [Block::Quote { blocks: vec![para("a b"), para("c")] }]);
        assert_eq!(blocks("> lazy\ncontinuation"), [Block::Quote { blocks: vec![para("lazy continuation")] }]);
        assert_eq!(
            blocks(">> nested\n> outer"),
            [Block::Quote { blocks: vec![Block::Quote { blocks: vec![para("nested outer")] }] }]
        );
        assert_eq!(blocks("> a\n\n> b"), [Block::Quote { blocks: vec![para("a")] }, Block::Quote { blocks: vec![para("b")] }]);
        assert_eq!(blocks("> # Head\n> ---"), [Block::Quote { blocks: vec![heading(1, "Head"), Block::Rule] }]);
        assert_eq!(blocks("> a\n---"), [Block::Quote { blocks: vec![para("a")] }, Block::Rule]);
    }

    #[test]
    fn lists() {
        assert_eq!(blocks("- a\n- b\n+ c"), [list(false, 1, vec![vec![para("a")], vec![para("b")]]), list(false, 1, vec![vec![para("c")]])]);
        assert_eq!(blocks("3. three\n4. four\n5) other"), [list(true, 3, vec![vec![para("three")], vec![para("four")]]), list(true, 5, vec![vec![para("other")]])]);
        assert_eq!(
            blocks("- a\n  - b\n    - c\n- d"),
            [list(
                false,
                1,
                vec![
                    vec![para("a"), list(false, 1, vec![vec![para("b"), list(false, 1, vec![vec![para("c")]])]])],
                    vec![para("d")],
                ]
            )]
        );
        assert_eq!(blocks("- a\n\n  b\n\n- c"), [list(false, 1, vec![vec![para("a"), para("b")], vec![para("c")]])]);
        assert_eq!(blocks("1. a\n\n       code\n2. b"), [list(true, 1, vec![vec![para("a"), code("", "code")], vec![para("b")]])]);
        assert_eq!(blocks("-\n  foo\n-\n\n  bar"), [list(false, 1, vec![vec![para("foo")], vec![]]), para("bar")]);
        assert_eq!(blocks("para\n2. not a list\n- but this is"), [para("para 2. not a list"), list(false, 1, vec![vec![para("but this is")]])]);
        assert_eq!(blocks("- lazy\ncontinuation"), [list(false, 1, vec![vec![para("lazy continuation")]])]);
        assert_eq!(blocks("-one\n2.two"), [para("-one 2.two")]);
        assert_eq!(blocks("- a\n - b\n  - c\n   - d\n    - e"), [list(
            false,
            1,
            vec![vec![para("a")], vec![para("b")], vec![para("c")], vec![para("d - e")]]
        )]);
        assert_eq!(blocks("1234567890. too long"), [para("1234567890. too long")]);
        assert_eq!(blocks("- > quoted\n  > more"), [list(false, 1, vec![vec![Block::Quote { blocks: vec![para("quoted more")] }]])]);
    }

    #[test]
    fn task_lists() {
        let items = |doc: Vec<Block>| match &doc[0] {
            Block::List { items, .. } => items.iter().map(|i| i.checked).collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(items(blocks("- [ ] todo\n- [x] done\n- [X] Done\n- [y] no\n- [ ]no\n- plain")), [
            Some(false),
            Some(true),
            Some(true),
            None,
            None,
            None
        ]);
        assert_eq!(blocks("- [x] done")[0], Block::List {
            ordered: false,
            start: 1,
            items: vec![Item { checked: Some(true), blocks: vec![para("done")] }]
        });
        assert_eq!(items(blocks("1. [ ]\n   more")), [Some(false)]);
    }

    #[test]
    fn tables() {
        let doc = blocks("intro\n| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 |\n| x \\| y | `\\|` | z | extra |\nno pipes\n\nafter");
        assert_eq!(doc[0], para("intro"));
        let Block::Table { align, header, rows } = &doc[1] else { panic!("{doc:?}") };
        assert_eq!(align, &[Align::Left, Align::Center, Align::Right]);
        assert_eq!(header, &[vec![t("a")], vec![t("b")], vec![t("c")]]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], [vec![t("1")], vec![t("2")], vec![]]);
        assert_eq!(rows[1][0], [t("x | y")]);
        assert_eq!(rows[1][1], [Inline::Text { text: "|".into(), style: Style { code: true, ..Style::default() } }]);
        assert_eq!(rows[2], [vec![t("no pipes")], vec![], vec![]]);
        assert_eq!(doc[2], para("after"));

        assert!(matches!(&blocks("a | b\n--|--\n1 | 2")[0], Block::Table { rows, .. } if rows.len() == 1));
        // The header must have as many cells as the delimiter row.
        assert_eq!(blocks("| a | b |\n|---|\n"), [para("| a | b | |---|")]);
        // Without any pipe, it is a setext heading.
        assert_eq!(blocks("a\n---"), [heading(2, "a")]);
        // A new block ends the table.
        assert!(matches!(&blocks("|a|\n|-|\n|1|\n> q")[..], [Block::Table { rows, .. }, Block::Quote { .. }] if rows.len() == 1));
        assert!(matches!(&blocks("> |a|\n> |-|\n> |1|")[..], [Block::Quote { blocks }] if matches!(blocks[0], Block::Table { .. })));
    }

    #[test]
    fn link_reference_definitions() {
        let doc = blocks("[home]: https://example.com \"Home\"\n[Docs]:\n  /docs\n\nSee [home] and [the docs][docs].\n\n[unused]: /x");
        assert_eq!(doc.len(), 1);
        let Block::Paragraph { inlines } = &doc[0] else { panic!() };
        let links: Vec<_> = inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Text { style: Style { link: Some(l), .. }, .. } => Some(l.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(links, ["https://example.com", "/docs"]);
        assert_eq!(blocks("[foo]: /url\n==="), [para("===")]);
        assert_eq!(blocks("[foo]: /url\nbar\n==="), [heading(1, "bar")]);
    }

    #[test]
    fn html_blocks() {
        let doc = blocks("<p align=\"center\">\n  <img src=\"logo.png\" alt=\"Logo\" width=\"200\">\n</p>\n\n# Name\n\n<!-- comment\n\nstill comment -->\ntext\n\n<details>\n<summary>More</summary>\n\nHidden *text*\n\n</details>");
        assert_eq!(doc[0], Block::Paragraph {
            inlines: vec![Inline::Image { alt: "Logo".into(), src: "logo.png".into(), link: None }]
        });
        assert_eq!(doc[1], heading(1, "Name"));
        assert_eq!(doc[2], para("text"));
        assert_eq!(doc[3], para("More"));
        assert!(matches!(&doc[4], Block::Paragraph { inlines } if super::super::plain_text(inlines) == "Hidden text"));
        assert_eq!(doc.len(), 5);
        assert_eq!(blocks("<pre>\nkeep\n\n  this\n</pre>"), [code("", "keep\n\n  this")]);
        // An inline tag alone on a line cannot interrupt a paragraph.
        assert_eq!(blocks("a\n<span>\nb</span>"), [para("a b")]);
    }

    #[test]
    fn front_matter() {
        let doc = parse("---\ntitle: Notes\ntags: [a, b]\n---\n# Heading\n");
        assert_eq!(doc.blocks, [code("yaml", "title: Notes\ntags: [a, b]"), heading(1, "Heading")]);
        assert_eq!(blocks("---\nnot front matter\n---"), [Block::Rule, heading(2, "not front matter")]);
    }

    #[test]
    fn tabs_and_odd_input() {
        assert_eq!(blocks("-\tfoo\n\n\tbar"), [list(false, 1, vec![vec![para("foo"), para("bar")]])]);
        assert_eq!(blocks(">\t\tfoo"), [Block::Quote { blocks: vec![code("", "  foo")] }]);
        assert_eq!(blocks("a\r\nb\rc\0d"), [para("a b c\u{FFFD}d")]);
        assert_eq!(blocks("\u{FEFF}# BOM"), [heading(1, "BOM")]);
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let quotes = ">".repeat(10_000) + " deep";
        let doc = blocks(&quotes);
        let mut depth = 0;
        let mut level = &doc;
        while let Some(Block::Quote { blocks }) = level.first() {
            depth += 1;
            level = blocks;
        }
        assert_eq!(depth, MAX_CONTAINERS);
        let lists: String = (0..500).map(|i| format!("{}- item\n", "  ".repeat(i))).collect();
        assert!(!blocks(&lists).is_empty());
    }

    /// Each input with its blocks written out by [`outline`].
    fn cases(cases: &[(&str, &[&str])]) {
        for (input, expected) in cases {
            assert_eq!(outline(&blocks(input)), *expected, "{input:?}");
        }
    }

    #[test]
    fn atx_and_setext_heading_forms() {
        cases(&[
            ("   # three spaces", &["h1 three spaces"]),
            ("    # four spaces", &["code[] # four spaces"]),
            ("#\tTab", &["h1 Tab"]),
            ("## *styled* `code`", &["h2 styled code"]),
            ("Foo\n# Bar\nBaz", &["p Foo", "h1 Bar", "p Baz"]),
            ("# Heading #5", &["h1 Heading #5"]),
            ("# Heading#", &["h1 Heading#"]),
            ("### foo \\###", &["h3 foo ###"]),
            ("# \n#\n### ###", &[]),
            ("Foo *bar*\n=========", &["h1 Foo bar"]),
            ("Foo\n    ---", &["p Foo ---"]),
            ("Foo\n= =", &["p Foo = ="]),
            ("Foo  \n-----", &["h2 Foo"]),
            ("> Foo\n---", &["> p Foo", "hr"]),
            ("- Foo\n---", &["- p Foo", "hr"]),
            ("Foo\nBar\n---", &["h2 Foo Bar"]),
            ("---\nFoo\n---\nBar\n---\nBaz", &["hr", "h2 Foo", "h2 Bar", "p Baz"]),
            ("\n====", &["p ===="]),
            ("Foo\n---\n---", &["h2 Foo", "hr"]),
            ("   Foo\n   ===", &["h1 Foo"]),
            ("\\> foo\n------", &["h2 > foo"]),
        ]);
    }

    #[test]
    fn paragraph_and_break_forms() {
        cases(&[
            ("aaa\n\nbbb", &["p aaa", "p bbb"]),
            ("  aaa\n bbb", &["p aaa bbb"]),
            ("aaa\n             bbb\n                                       ccc", &["p aaa bbb ccc"]),
            ("foo  ", &["p foo"]),
            ("foo\\", &["p foo\\"]),
            ("### foo\\", &["h3 foo\\"]),
            ("foo  \n     bar", &["p foo bar"]),
            ("\\# not heading\n\\- not list\n\\> not quote\n1\\. not ordered", &["p # not heading - not list > not quote 1. not ordered"]),
            ("&#35; entity\n&#42; star", &["p # entity * star"]),
        ]);
        let Block::Paragraph { inlines } = &blocks("foo  \n     bar")[0] else { panic!() };
        assert_eq!(inlines, &[t("foo"), Inline::Break, t("bar")]);
    }

    #[test]
    fn thematic_break_forms() {
        cases(&[
            (" ***\n  ***\n   ***", &["hr", "hr", "hr"]),
            ("    ***", &["code[] ***"]),
            ("Foo\n    ***", &["p Foo ***"]),
            ("_____________________________________", &["hr"]),
            (" - - -", &["hr"]),
            (" **  * ** * ** * **", &["hr"]),
            ("-     -      -      -", &["hr"]),
            ("- - - -    ", &["hr"]),
            ("_ _ _ _ a\n\na------\n\n---a---", &["p _ _ _ _ a", "p a------", "p ---a---"]),
            ("*-*", &["p -"]),
            ("- foo\n***\n- bar", &["- p foo", "hr", "- p bar"]),
            ("Foo\n***\nbar", &["p Foo", "hr", "p bar"]),
            ("* Foo\n* * *\n* Bar", &["- p Foo", "hr", "- p Bar"]),
            ("- Foo\n- * * *", &["- p Foo", "- hr"]),
        ]);
    }

    #[test]
    fn fenced_code_forms() {
        cases(&[
            ("```\n<\n >\n```", &["code[] <\n >"]),
            ("~~~\naaa\n```\n~~~", &["code[] aaa\n```"]),
            ("````\naaa\n```\n``````", &["code[] aaa\n```"]),
            ("```\n\n  \n```", &["code[] \n  "]),
            ("```\n```", &["code[] "]),
            ("   ```\n   aaa\n    aaa\n  aaa\n   ```", &["code[] aaa\n aaa\naaa"]),
            ("    ```\n    aaa\n    ```", &["code[] ```\naaa\n```"]),
            ("```\naaa\n  ```", &["code[] aaa"]),
            ("```\naaa\n    ```", &["code[] aaa\n    ```"]),
            ("``` ```\naaa", &["p  aaa"]),
            ("~~~~~~\naaa\n~~~ ~~", &["code[] aaa\n~~~ ~~"]),
            ("foo\n```\nbar\n```\nbaz", &["p foo", "code[] bar", "p baz"]),
            ("```ruby startline=3 $%@#$\ndef foo(x)\n```", &["code[ruby] def foo(x)"]),
            ("~~~ aa ``` ~~~\nfoo\n~~~", &["code[aa] foo"]),
            ("```\n``` aaa\n```", &["code[] ``` aaa"]),
            ("- ```\n  code\n  ```\n- b", &["- code[] code", "- p b"]),
            ("> ```\n> a\nb", &["> code[] a", "p b"]),
            ("```{.python .numberLines}\nx\n```\n```{r, echo=FALSE}\ny\n```\n```rust,ignore\nz\n```", &[
                "code[python] x",
                "code[r] y",
                "code[rust] z",
            ]),
            ("``` c\\+\\+ &amp;\nx\n```", &["code[c++] x"]),
        ]);
    }

    #[test]
    fn indented_code_forms() {
        cases(&[
            ("    a simple\n      indented code block", &["code[] a simple\n  indented code block"]),
            ("  - foo\n\n    bar", &["- p foo", "  p bar"]),
            ("1.  foo\n\n    - bar", &["1. p foo", "  - p bar"]),
            ("    <a/>\n    *hi*\n\n    - one", &["code[] <a/>\n*hi*\n\n- one"]),
            ("    chunk1\n\n    chunk2\n  \n \n \n    chunk3", &["code[] chunk1\n\nchunk2\n\n\n\nchunk3"]),
            ("    chunk1\n      \n      chunk2", &["code[] chunk1\n  \n  chunk2"]),
            ("    foo\nbar", &["code[] foo", "p bar"]),
            ("# Heading\n    foo\nHeading\n------\n    foo\n----", &["h1 Heading", "code[] foo", "h2 Heading", "code[] foo", "hr"]),
            ("        foo\n    bar", &["code[]     foo\nbar"]),
            ("\n    \n    foo\n    ", &["code[] foo"]),
            ("    foo  ", &["code[] foo  "]),
        ]);
    }

    #[test]
    fn block_quote_forms() {
        cases(&[
            ("># Foo\n>bar\n> baz", &["> h1 Foo", "> p bar baz"]),
            ("   > # Foo\n   > bar\n > baz", &["> h1 Foo", "> p bar baz"]),
            ("    > # Foo", &["code[] > # Foo"]),
            ("> # Foo\n> bar\nbaz", &["> h1 Foo", "> p bar baz"]),
            ("> bar\nbaz\n> foo", &["> p bar baz foo"]),
            ("> - foo\n- bar", &["> - p foo", "- p bar"]),
            (">     foo\n    bar", &["> code[] foo", "code[] bar"]),
            ("> ```\nfoo\n```", &["> code[] ", "p foo", "code[] "]),
            ("> foo\n    - bar", &["> p foo - bar"]),
            (">", &[]),
            (">\n>  \n> ", &[]),
            (">\n> foo\n>  ", &["> p foo"]),
            ("> foo\n>\n> bar", &["> p foo", "> p bar"]),
            ("foo\n> bar", &["p foo", "> p bar"]),
            ("> aaa\n***\n> bbb", &["> p aaa", "hr", "> p bbb"]),
            ("> bar\n\nbaz", &["> p bar", "p baz"]),
            ("> bar\n>\nbaz", &["> p bar", "p baz"]),
            ("> > > foo\nbar", &["> > > p foo bar"]),
            (">>> foo\n> bar\n>>baz", &["> > > p foo bar baz"]),
            (">     code\n\n>    not code", &["> code[] code", "> p not code"]),
        ]);
    }

    #[test]
    fn list_item_forms() {
        cases(&[
            (
                "1.  A paragraph\n    with two lines.\n\n        indented code\n\n    > A block quote.",
                &["1. p A paragraph with two lines.", "  code[] indented code", "  > p A block quote."],
            ),
            ("- one\n\n two", &["- p one", "p two"]),
            ("- one\n\n  two", &["- p one", "  p two"]),
            (" -    one\n\n     two", &["- p one", "code[]  two"]),
            (" -    one\n\n      two", &["- p one", "  p two"]),
            ("   > > 1.  one\n>>\n>>     two", &["> > 1. p one", "> >   p two"]),
            (">>- one\n>>\n  >  > two", &["> > - p one", "> > p two"]),
            ("- foo\n\n\n  bar", &["- p foo", "  p bar"]),
            (
                "1.  foo\n\n    ```\n    bar\n    ```\n\n    baz\n\n    > bam",
                &["1. p foo", "  code[] bar", "  p baz", "  > p bam"],
            ),
            ("- Foo\n\n      bar\n\n\n      baz", &["- p Foo", "  code[] bar\n\n\nbaz"]),
            ("123456789. ok\n\nx\n\n0. zero\n\ny\n\n003. three\n\n-1. not", &["123456789. p ok", "p x", "0. p zero", "p y", "3. p three", "p -1. not"]),
            ("  10.  foo\n\n           bar", &["10. p foo", "  code[] bar"]),
            ("1.      indented code\n\n   paragraph\n\n       more code", &["1. code[]  indented code", "  p paragraph", "  code[] more code"]),
            ("   foo\n\nbar", &["p foo", "p bar"]),
            ("-    foo\n\n  bar", &["- p foo", "p bar"]),
            ("-\n  foo\n-\n  ```\n  bar\n  ```\n-\n      baz", &["- p foo", "- code[] bar", "- code[] baz"]),
            ("-   \n  foo", &["- p foo"]),
            ("-\n\n  foo", &["-", "p foo"]),
            ("- foo\n-\n- bar", &["- p foo", "-", "- p bar"]),
            ("- foo\n-   \n- bar", &["- p foo", "-", "- p bar"]),
            ("1. foo\n2.\n3. bar", &["1. p foo", "2.", "3. p bar"]),
            ("*", &["-"]),
            ("foo\n*\n\nfoo\n1.", &["p foo *", "p foo 1."]),
            ("    1.  A paragraph", &["code[] 1.  A paragraph"]),
            ("  1.  A paragraph\nwith two lines.", &["1. p A paragraph with two lines."]),
            ("> 1. > Blockquote\ncontinued here.", &["> 1. > p Blockquote continued here."]),
            ("- foo\n  - bar\n    - baz\n      - boo", &["- p foo", "  - p bar", "    - p baz", "      - p boo"]),
            ("- foo\n - bar\n  - baz\n   - boo", &["- p foo", "- p bar", "- p baz", "- p boo"]),
            ("10) foo\n    - bar", &["10. p foo", "  - p bar"]),
            ("10) foo\n   - bar", &["10. p foo", "- p bar"]),
            ("- - foo", &["- - p foo"]),
            ("1. - 2. foo", &["1. - 2. p foo"]),
            ("- # Foo\n- Bar\n  ---\n  baz", &["- h1 Foo", "- h2 Bar", "  p baz"]),
        ]);
    }

    #[test]
    fn list_forms() {
        cases(&[
            ("- foo\n- bar\n+ baz", &["- p foo", "- p bar", "- p baz"]),
            ("1. foo\n2. bar\n3) baz", &["1. p foo", "2. p bar", "3. p baz"]),
            ("Foo\n- bar\n- baz", &["p Foo", "- p bar", "- p baz"]),
            ("The number of windows in my house is\n14.  The number of doors is 6.", &["p The number of windows in my house is 14. The number of doors is 6."]),
            ("The number of windows in my house is\n1.  The number of doors is 6.", &["p The number of windows in my house is", "1. p The number of doors is 6."]),
            ("- foo\n\n- bar\n\n\n- baz", &["- p foo", "- p bar", "- p baz"]),
            ("- foo\n  - bar\n    - baz\n\n\n      bim", &["- p foo", "  - p bar", "    - p baz", "      p bim"]),
            ("- foo\n- bar\n\n<!-- -->\n\n- baz\n- bim", &["- p foo", "- p bar", "- p baz", "- p bim"]),
            ("- a\n - b\n  - c\n   - d\n  - e\n - f\n- g", &["- p a", "- p b", "- p c", "- p d", "- p e", "- p f", "- p g"]),
            ("1. a\n\n  2. b\n\n   3. c", &["1. p a", "2. p b", "3. p c"]),
            ("1. a\n\n  2. b\n\n    3. c", &["1. p a", "2. p b", "code[] 3. c"]),
            ("* a\n*\n\n* c", &["- p a", "-", "- p c"]),
            ("- a\n- b\n\n  c\n- d", &["- p a", "- p b", "  p c", "- p d"]),
            ("- a\n- b\n\n  [ref]: /url\n- d", &["- p a", "- p b", "- p d"]),
            ("- a\n- ```\n  b\n\n\n  ```\n- c", &["- p a", "- code[] b\n\n", "- p c"]),
            ("- a\n  - b\n\n    c\n- d", &["- p a", "  - p b", "    p c", "- p d"]),
            ("* a\n  > b\n  >\n* c", &["- p a", "  > p b", "- p c"]),
            ("- a\n  > b\n  ```\n  c\n  ```\n- d", &["- p a", "  > p b", "  code[] c", "- p d"]),
            ("1. ```\n   foo\n   ```\n\n   bar", &["1. code[] foo", "  p bar"]),
            ("* foo\n  * bar\n\n  baz", &["- p foo", "  - p bar", "  p baz"]),
            ("- a\n  - b\n  - c\n\n- d\n  - e\n  - f", &["- p a", "  - p b", "  - p c", "- p d", "  - p e", "  - p f"]),
        ]);
        // The kinds and numbers of the lists themselves.
        let doc = blocks("- a\n+ b\n\n7) c\n8) d\n\n1. e");
        let kinds: Vec<(bool, i64, usize)> = doc
            .iter()
            .map(|block| match block {
                Block::List { ordered, start, items } => (*ordered, *start, items.len()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(kinds, [(false, 1, 1), (false, 1, 1), (true, 7, 2), (true, 1, 1)]);
    }

    #[test]
    fn task_item_forms() {
        cases(&[
            ("1. [x] done\n2. [ ] todo", &["1. [x] p done", "2. [ ] p todo"]),
            ("- [ ] a\n  - [x] b", &["- [ ] p a", "  - [x] p b"]),
            ("> - [x] quoted", &["> - [x] p quoted"]),
            ("- [x]", &["- [x]"]),
            ("- [x]\ttab", &["- [x] p tab"]),
            ("-  [x] two spaces", &["- [x] p two spaces"]),
            ("- [x] **bold** [link](/u)", &["- [x] p bold link"]),
            ("- [x]not\n- [ x] no\n- x [ ] no", &["- p [x]not", "- p [ x] no", "- p x [ ] no"]),
            ("- [x] first\n\n  second paragraph", &["- [x] p first", "  p second paragraph"]),
            // Only a paragraph can start with a task marker.
            ("- ```\n  [x] code\n  ```", &["- code[] [x] code"]),
        ]);
    }

    #[test]
    fn table_forms() {
        cases(&[
            ("| foo | bar |\n| --- | --- |\n| baz | bim |", &["table foo|bar / baz|bim"]),
            ("| abc | defghi |\n:-: | -----------:\nbar | baz", &["table abc|defghi / bar|baz"]),
            ("| f\\|oo  |\n| ------ |\n| b `\\|` az |\n| b **\\|** im |", &["table f|oo / b | az / b | im"]),
            ("| abc | def |\n| --- | --- |\n| bar | baz |\n> bar", &["table abc|def / bar|baz", "> p bar"]),
            ("| abc | def |\n| --- | --- |\n| bar | baz |\nbar\n\nbar", &["table abc|def / bar|baz / bar|", "p bar"]),
            ("| abc | def |\n| --- |\n| bar |", &["p | abc | def | | --- | | bar |"]),
            ("| abc | def |\n| --- | --- |\n| bar |\n| bar | baz | boo |", &["table abc|def / bar| / bar|baz"]),
            ("| abc | def |\n| --- | --- |", &["table abc|def / "]),
            ("| a |\n|---|\n| b |", &["table a / b"]),
            ("a|b\n-|-\n1|2", &["table a|b / 1|2"]),
            ("- | a |\n  |---|\n  | b |", &["- table a / b"]),
            ("|a|\n|-|\n# H", &["table a / ", "h1 H"]),
            ("|a|b|\n|-|-|\n|  | x |", &["table a|b / |x"]),
            ("| a |\n| - |\n\n| b |", &["table a / ", "p | b |"]),
            // Four spaces make the delimiter row paragraph text.
            ("| a |\n    | - |", &["p | a | | - |"]),
        ]);
        let Block::Table { align, .. } = &blocks("|a|b|c|d|\n|-|:-|:-:|-:|")[0] else { panic!() };
        assert_eq!(align, &[Align::None, Align::Left, Align::Center, Align::Right]);
        let Block::Table { header, rows, .. } = &blocks("| *a* | [b](/u) |\n|-|-|\n| ![i](p.png) | `c` |")[0] else {
            panic!()
        };
        assert_eq!(show(&header[0]), "<i>a</i>");
        assert_eq!(show(&header[1]), "<a /u>b</a>");
        assert_eq!(show(&rows[0][0]), "![i](p.png)");
        assert_eq!(show(&rows[0][1]), "<c>c</c>");
    }

    #[test]
    fn link_reference_definition_forms() {
        let links = |text: &str| -> Vec<String> {
            let mut out = Vec::new();
            for block in blocks(text) {
                if let Block::Paragraph { inlines } | Block::Heading { inlines, .. } = block {
                    out.push(show(&inlines));
                }
            }
            out
        };
        assert_eq!(links("[foo]: /url 'title'\n\n[foo]"), ["<a /url>foo</a>"]);
        assert_eq!(links("[foo]:\n/url\n\n[foo]"), ["<a /url>foo</a>"]);
        assert_eq!(links("[foo]: /url \"title\" ok"), ["[foo]: /url \"title\" ok"]);
        assert_eq!(links("[foo]: /url\n\"title\" ok\n\n[foo]"), ["\"title\" ok", "<a /url>foo</a>"]);
        assert_eq!(outline(&blocks("    [foo]: /url \"title\"\n\n[foo]")), ["code[] [foo]: /url \"title\"", "p [foo]"]);
        assert_eq!(outline(&blocks("```\n[foo]: /url\n```\n\n[foo]")), ["code[] [foo]: /url", "p [foo]"]);
        assert_eq!(links("Foo\n[bar]: /baz\n\n[bar]"), ["Foo [bar]: /baz", "[bar]"]);
        assert_eq!(links("# [Foo]\n[foo]: /url\n> bar"), ["<a /url>Foo</a>"]);
        assert_eq!(
            links("[foo]: /foo-url \"foo\"\n[bar]: /bar-url\n  \"bar\"\n[baz]: /baz-url\n\n[foo],\n[bar],\n[baz]"),
            ["<a /foo-url>foo</a>, <a /bar-url>bar</a>, <a /baz-url>baz</a>"]
        );
        assert_eq!(links("[foo]\n\n> [foo]: /url"), ["<a /url>foo</a>"]);
        assert_eq!(blocks("[foo]: /url"), []);
        assert_eq!(links("[\nfoo\n]: /url\nbar"), ["bar"]);
        assert_eq!(links("[foo]: /url1\n\n[foo]: /url2\n\n[foo]"), ["<a /url1>foo</a>"]);
        // Definitions inside lists and quotes count everywhere.
        assert_eq!(links("- [a]: /in-list\n\n> [b]: /in-quote\n\n[a] [b]"), ["<a /in-list>a</a> <a /in-quote>b</a>"]);
    }

    #[test]
    fn html_block_kinds() {
        cases(&[
            // 1: raw text, to its closing tag, blank lines and all.
            ("<script>\nalert(1)\n\n</script>\n*after*", &["p after"]),
            ("<style>p { color: red }</style>\n*x*", &["p x"]),
            ("<pre>\n**not md**\n\n</pre>", &["code[] **not md**"]),
            ("<textarea>\n\nhidden\n</textarea>\nshown", &["p shown"]),
            // 2 to 5: comments, instructions, declarations, CDATA.
            ("<!-- a\n\nb -->\nc", &["p c"]),
            ("<?php\n\necho 1;\n\n?>\nok", &["p ok"]),
            ("<!DOCTYPE html>\nok", &["p ok"]),
            ("<![CDATA[\nx < y\n]]>\nok", &["p x < y", "p ok"]),
            // 6: block tags, to the next blank line; no Markdown inside.
            ("<div>\n*not emphasized*\n</div>", &["p *not emphasized*"]),
            ("<div>\n\n*emphasized*\n\n</div>", &["p emphasized"]),
            ("Foo\n<div>\nbar\n</div>", &["p Foo", "p bar"]),
            ("<DIV CLASS=\"foo\">\n\n*Markdown*\n\n</DIV>", &["p Markdown"]),
            ("<h1 align=\"center\">Name</h1>\n\nText", &["h1 Name", "p Text"]),
            // 7: any other complete tag alone on its line.
            ("<span class=\"x\">\n\n*md*\n\n</span>", &["p md"]),
            ("<a href=\"foo\">\n*bar*\n</a>", &["p *bar*"]),
            ("Foo\n<a href=\"bar\">\nbaz", &["p Foo baz"]),
            ("<img src=\"logo.png\" alt=\"Logo\">", &["p Logo"]),
            ("<br>\n\n<br/>", &[]),
            // A table across blocks stays one; cells written in Markdown
            // follow a table that would otherwise be empty.
            ("<table>\n\n<tr>\n\n<td>\nHi\n</td>\n\n</tr>\n\n</table>", &["table  / Hi"]),
            ("<table>\n<tr>\n<td>\n\n**Markdown** here\n\n</td>\n</tr>\n</table>", &["p Markdown here"]),
        ]);
        let doc = parse("<h1 align=\"center\">\n  <img src=\"logo.svg\" width=\"80\"><br>\n  My App\n</h1>\n\n# Second");
        assert_eq!(doc.title.as_deref(), Some("My App"));
        let Block::Paragraph { inlines } = &blocks("<a href=\"foo\">\n*bar*\n</a>")[0] else { panic!() };
        assert_eq!(show(inlines), "<a foo>*bar*</a>");
    }

    #[test]
    fn titles() {
        assert_eq!(parse("Title\n=====\n\n# Later").title.as_deref(), Some("Title"));
        assert_eq!(parse("> # Quoted\n\n## Two").title, None);
        assert_eq!(parse("# *Styled* [title](/u) `code`").title.as_deref(), Some("Styled title code"));
        assert_eq!(parse("#\n\n# Real").title.as_deref(), Some("Real"));
        assert_eq!(parse("").title, None);
    }
}
