//! HTML, read forgivingly into the reading model.
//!
//! A tokenizer after HTML5's (tags with quoted or bare attributes,
//! comments, doctypes, CDATA, raw text in `<script>`, `<style>`,
//! `<textarea>` and `<title>`, named and numeric character references)
//! feeds a tree builder that closes what HTML lets authors leave open
//! (`<p>`, `<li>`, `<td>`, `<tr>`...), reopens bold or italic cut short by
//! the end of a paragraph, and ignores end tags that close nothing. The
//! tree is then mapped to blocks: what a reader would see, without the
//! head, scripts, styles, forms or hidden elements.

use std::collections::HashMap;

use super::{entities, first_title, link_target, table, Align, Block, Cell, Document, Inline, Inlines, Item, Style};
use super::{MAX_COLUMNS, MAX_NESTING, MAX_NODES};

/// Elements nest at most this deep; deeper ones are added as siblings, so
/// that walking the tree recursively stays safe.
const MAX_DEPTH: usize = 128;

/// At most this many formatting elements are remembered for reopening.
const MAX_FORMATTING: usize = 64;

const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "image", "img", "input", "keygen", "link",
    "meta", "param", "source", "track", "wbr",
];

/// Elements whose content is raw text, without tags or (except for the
/// last two) character references.
const RAW_TEXT: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes", "noscript", "plaintext"];
const ESCAPABLE_RAW_TEXT: &[&str] = &["title", "textarea"];

/// What may stay in `<head>`; anything else ends it.
const HEAD_CONTENT: &[&str] =
    &["base", "basefont", "bgsound", "link", "meta", "noscript", "script", "style", "template", "title"];

const HEADINGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];

/// Start tags that end an open `<p>`.
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "details", "dialog", "dir", "div", "dl", "fieldset",
    "figcaption", "figure", "footer", "header", "hgroup", "main", "menu", "nav", "ol", "p", "search", "section",
    "summary", "ul", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "listing", "form", "table", "hr", "xmp", "plaintext",
    "li", "dd", "dt",
];

/// Elements a browser reopens when a block cuts them short.
const FORMATTING: &[&str] =
    &["a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u"];

/// HTML5's "special" elements, which stray end tags do not reach past.
const SPECIAL: &[&str] = &[
    "address", "applet", "area", "article", "aside", "base", "basefont", "bgsound", "blockquote", "body", "br", "button",
    "caption", "center", "col", "colgroup", "dd", "details", "dir", "div", "dl", "dt", "embed", "fieldset", "figcaption",
    "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup",
    "hr", "html", "iframe", "img", "input", "keygen", "li", "link", "listing", "main", "marquee", "menu", "meta", "nav",
    "noembed", "noframes", "noscript", "object", "ol", "p", "param", "plaintext", "pre", "script", "search", "section",
    "select", "source", "style", "summary", "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead",
    "title", "tr", "track", "ul", "wbr", "xmp",
];

/// Scopes: where the search for an open element to close stops.
const DEFAULT_SCOPE: &[&str] = &["applet", "caption", "html", "table", "td", "th", "marquee", "object", "template"];
const BUTTON_SCOPE: &[&str] =
    &["applet", "caption", "html", "table", "td", "th", "marquee", "object", "template", "button"];
const LIST_SCOPE: &[&str] =
    &["applet", "caption", "html", "table", "td", "th", "marquee", "object", "template", "ol", "ul"];
const TABLE_SCOPE: &[&str] = &["html", "table", "template"];

/// Elements that fence off formatting: bold opened outside a cell is not
/// reopened inside it.
const MARKERS: &[&str] = &["applet", "caption", "marquee", "object", "td", "template", "th"];

const TABLE_PARTS: &[&str] = &["caption", "col", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr"];

/// Elements that show nothing a reader needs.
const SKIP: &[&str] = &[
    "head", "title", "script", "style", "noscript", "template", "svg", "iframe", "object", "embed", "canvas", "audio",
    "video", "input", "select", "textarea", "button", "datalist", "option", "optgroup", "output", "progress", "meter",
    "map", "area", "frameset", "frame", "noframes", "noembed", "annotation", "annotation-xml", "base", "link", "meta",
    "param", "source", "track",
];

/// Elements that stand on lines of their own.
const BOUNDARY: &[&str] = &[
    "p", "div", "address", "article", "aside", "blockquote", "center", "dd", "details", "dialog", "dir", "dl",
    "fieldset", "figcaption", "figure", "footer", "form", "header", "hgroup", "legend", "li", "main", "menu", "nav",
    "ol", "ul", "section", "summary", "search", "table", "caption", "thead", "tbody", "tfoot", "tr", "td", "th",
    "h1", "h2", "h3", "h4", "h5", "h6", "hr", "pre", "listing", "xmp", "plaintext",
];

fn is_html_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0C')
}

fn is_blank(text: &str) -> bool {
    text.bytes().all(is_html_space)
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// A CSS property's value in a `style` attribute, lower-cased.
fn style_property(attrs: &[(String, String)], property: &str) -> Option<String> {
    let style = attr(attrs, "style")?.to_ascii_lowercase();
    style.split(';').find_map(|declaration| {
        let (name, value) = declaration.split_once(':')?;
        (name.trim() == property).then(|| value.trim().trim_end_matches("!important").trim().to_owned())
    })
}

fn is_hidden(attrs: &[(String, String)]) -> bool {
    attr(attrs, "hidden").is_some() || style_property(attrs, "display").as_deref() == Some("none")
}

/// Parse a page.
pub(super) fn parse(text: &str) -> Document {
    let (nodes, truncated) = build(text);
    let mapper = Mapper { nodes: &nodes };
    let blocks = mapper.blocks();
    let title = mapper.title(0).or_else(|| first_title(&blocks));
    Document { title, blocks, truncated }
}

/// Parse a piece of HTML inside Markdown; true with it when it was cut
/// short.
pub(super) fn fragment(text: &str) -> (Vec<Block>, bool) {
    let (nodes, truncated) = build(text);
    (Mapper { nodes: &nodes }.blocks(), truncated)
}

/// A start tag: lower-case name and attribute names, attribute values with
/// their character references decoded.
#[derive(Debug, Default)]
struct StartTag {
    name: String,
    attrs: Vec<(String, String)>,
    self_closing: bool,
}

#[derive(Debug)]
enum Token {
    Text(String),
    Start(StartTag),
    End(String),
}

struct Tokenizer<'a> {
    src: &'a str,
    pos: usize,
    /// After a start tag whose content is raw text: its name, and whether
    /// character references count in it.
    raw: Option<(String, bool)>,
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let rest = &self.src[self.pos..];
            if rest.is_empty() {
                return None;
            }
            if let Some((name, escapable)) = self.raw.take() {
                let end = if name == "plaintext" { rest.len() } else { raw_text_end(rest, &name) };
                self.pos += end;
                if end > 0 {
                    let text = &rest[..end];
                    return Some(Token::Text(if escapable { entities::decode_all(text, false) } else { text.to_owned() }));
                }
                continue;
            }
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                self.pos += end;
                return Some(Token::Text(entities::decode_all(&rest[..end], false)));
            }
            if let Some(token) = self.markup() {
                return Some(token);
            }
        }
    }
}

/// Where raw text ends: at `</name` followed by white space, `/` or `>`.
fn raw_text_end(rest: &str, name: &str) -> usize {
    let b = rest.as_bytes();
    let mut from = 0;
    while let Some(i) = rest[from..].find("</") {
        let at = from + i;
        let after = at + 2 + name.len();
        if after <= b.len()
            && b[at + 2..after].eq_ignore_ascii_case(name.as_bytes())
            && b.get(after).is_none_or(|&c| is_html_space(c) || c == b'/' || c == b'>')
        {
            return at;
        }
        from = at + 2;
    }
    rest.len()
}

impl<'a> Tokenizer<'a> {
    fn new(src: &'a str) -> Self {
        Tokenizer { src, pos: 0, raw: None }
    }

    /// Skip to just after the next `>`, or to the end.
    fn skip_past_gt(&mut self) {
        let rest = &self.src[self.pos..];
        self.pos += rest.find('>').map_or(rest.len(), |i| i + 1);
    }

    /// At `<`: a tag, or something skipped (comment, doctype), or a
    /// literal `<`.
    fn markup(&mut self) -> Option<Token> {
        let rest = &self.src[self.pos..];
        let b = rest.as_bytes();
        match b.get(1) {
            Some(b'!') if rest.starts_with("<!--") => {
                let body = &rest[4..];
                self.pos += if body.starts_with('>') {
                    5
                } else if body.starts_with("->") {
                    6
                } else {
                    body.find("-->").map_or(rest.len(), |i| 4 + i + 3)
                };
                None
            }
            Some(b'!') if rest.get(..9).is_some_and(|s| s.eq_ignore_ascii_case("<![CDATA[")) => {
                let end = rest[9..].find("]]>").map_or(rest.len(), |i| 9 + i);
                self.pos += (end + 3).min(rest.len());
                Some(Token::Text(rest[9..end].to_owned()))
            }
            Some(b'!' | b'?') => {
                self.skip_past_gt();
                None
            }
            Some(b'/') => match b.get(2) {
                Some(c) if c.is_ascii_alphabetic() => {
                    let len = rest[2..]
                        .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
                        .unwrap_or(rest.len() - 2);
                    let name = rest[2..2 + len].to_ascii_lowercase();
                    match rest[2 + len..].find('>') {
                        Some(i) => {
                            self.pos += 2 + len + i + 1;
                            Some(Token::End(name))
                        }
                        None => {
                            // A tag cut off by the end of the file is dropped.
                            self.pos = self.src.len();
                            None
                        }
                    }
                }
                _ => {
                    self.skip_past_gt();
                    None
                }
            },
            Some(c) if c.is_ascii_alphabetic() => self.start_tag(),
            _ => {
                self.pos += 1;
                Some(Token::Text("<".into()))
            }
        }
    }

    fn start_tag(&mut self) -> Option<Token> {
        let src = self.src;
        let b = src.as_bytes();
        let mut i = self.pos + 1;
        let name_start = i;
        while i < b.len() && !is_html_space(b[i]) && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        let mut tag = StartTag {
            name: src[name_start..i].to_ascii_lowercase(),
            ..StartTag::default()
        };
        loop {
            while i < b.len() && (is_html_space(b[i]) || (b[i] == b'/' && b.get(i + 1) != Some(&b'>'))) {
                i += 1;
            }
            match b.get(i) {
                None => {
                    self.pos = b.len();
                    return None;
                }
                Some(b'>') => {
                    i += 1;
                    break;
                }
                Some(b'/') => {
                    tag.self_closing = true;
                    i += 2;
                    break;
                }
                Some(_) => {}
            }
            // An attribute: its name may start with `=`.
            let name_start = i;
            if b[i] == b'=' {
                i += 1;
            }
            while i < b.len() && !is_html_space(b[i]) && !matches!(b[i], b'/' | b'>' | b'=') {
                i += 1;
            }
            let name = src[name_start..i].to_ascii_lowercase();
            let mut j = i;
            while j < b.len() && is_html_space(b[j]) {
                j += 1;
            }
            let mut value = String::new();
            if b.get(j) == Some(&b'=') {
                j += 1;
                while j < b.len() && is_html_space(b[j]) {
                    j += 1;
                }
                match b.get(j) {
                    Some(&quote @ (b'"' | b'\'')) => {
                        let Some(len) = src[j + 1..].find(quote as char) else {
                            self.pos = b.len();
                            return None;
                        };
                        value = entities::decode_all(&src[j + 1..j + 1 + len], true);
                        i = j + 1 + len + 1;
                    }
                    _ => {
                        let start = j;
                        while j < b.len() && !is_html_space(b[j]) && b[j] != b'>' {
                            j += 1;
                        }
                        value = entities::decode_all(&src[start..j], true);
                        i = j;
                    }
                }
            }
            if !tag.attrs.iter().any(|(n, _)| *n == name) {
                tag.attrs.push((name, value));
            }
        }
        self.pos = i;
        if RAW_TEXT.contains(&tag.name.as_str()) {
            self.raw = Some((tag.name.clone(), false));
        } else if ESCAPABLE_RAW_TEXT.contains(&tag.name.as_str()) {
            self.raw = Some((tag.name.clone(), true));
        }
        Some(Token::Start(tag))
    }
}

#[derive(Debug)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<usize>,
}

#[derive(Debug)]
enum Node {
    Element(Element),
    Text(String),
}

/// Builds the tree the way a browser would, in simplified form.
struct TreeBuilder {
    nodes: Vec<Node>,
    /// Which nodes are on the stack of open elements.
    open: Vec<bool>,
    /// The open elements; the first is the root.
    stack: Vec<usize>,
    /// Formatting elements to reopen after a block cuts them short; `None`
    /// is a marker that fences off those opened outside a cell.
    formatting: Vec<Option<usize>>,
    head_open: bool,
    /// How many elements of each name are on the stack, so that an end tag
    /// closing nothing (most stray ones) is dropped without walking it.
    open_names: HashMap<String, usize>,
}

/// The tree of `text`, and whether it stopped at [`MAX_NODES`] nodes.
fn build(text: &str) -> (Vec<Node>, bool) {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let normalized;
    let text = if text.contains('\r') {
        normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        text
    };
    let mut builder = TreeBuilder::new();
    for token in Tokenizer::new(text) {
        if builder.nodes.len() >= MAX_NODES {
            return (builder.nodes, true);
        }
        match token {
            Token::Text(text) => builder.text(text),
            Token::Start(tag) => builder.start(tag),
            Token::End(name) => builder.end(&name),
        }
    }
    (builder.nodes, false)
}

impl TreeBuilder {
    fn new() -> Self {
        TreeBuilder {
            nodes: vec![Node::Element(Element {
                name: "#root".into(),
                attrs: Vec::new(),
                children: Vec::new(),
            })],
            open: vec![true],
            stack: vec![0],
            formatting: Vec::new(),
            head_open: false,
            open_names: HashMap::new(),
        }
    }

    fn name(&self, id: usize) -> &str {
        match &self.nodes[id] {
            Node::Element(e) => &e.name,
            Node::Text(_) => "",
        }
    }

    fn current(&self) -> usize {
        self.stack.last().copied().unwrap_or(0)
    }

    fn append(&mut self, node: Node) -> usize {
        let id = self.nodes.len();
        self.nodes.push(node);
        self.open.push(false);
        let parent = self.current();
        if let Node::Element(e) = &mut self.nodes[parent] {
            e.children.push(id);
        }
        id
    }

    /// Add an element and, room permitting, open it.
    fn open_element(&mut self, name: String, attrs: Vec<(String, String)>) -> Option<usize> {
        let id = self.append(Node::Element(Element {
            name,
            attrs,
            children: Vec::new(),
        }));
        if self.stack.len() >= MAX_DEPTH {
            return None;
        }
        self.stack.push(id);
        self.open[id] = true;
        if let Node::Element(e) = &self.nodes[id] {
            match self.open_names.get_mut(e.name.as_str()) {
                Some(count) => *count += 1,
                None => {
                    self.open_names.insert(e.name.clone(), 1);
                }
            }
        }
        Some(id)
    }

    fn pop(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(id) = self.stack.pop() else {
            return;
        };
        self.open[id] = false;
        if let Node::Element(e) = &self.nodes[id] {
            if let Some(count) = self.open_names.get_mut(e.name.as_str()) {
                *count -= 1;
                if *count == 0 {
                    self.open_names.remove(e.name.as_str());
                }
            }
        }
        let name = self.name(id);
        if name == "head" {
            self.head_open = false;
        } else if MARKERS.contains(&name) {
            while let Some(Some(_)) = self.formatting.pop() {}
        }
    }

    /// Close `stack[index]` and everything opened inside it.
    fn pop_until(&mut self, index: usize) {
        while self.stack.len() > index.max(1) {
            self.pop();
        }
    }

    /// The stack index of the nearest open element named in `names`,
    /// unless an element named in `barriers` comes first.
    fn in_scope(&self, names: &[&str], barriers: &[&str]) -> Option<usize> {
        for i in (1..self.stack.len()).rev() {
            let name = self.name(self.stack[i]);
            if names.contains(&name) {
                return Some(i);
            }
            if barriers.contains(&name) {
                return None;
            }
        }
        None
    }

    fn close_head(&mut self) {
        if let Some(i) = self.in_scope(&["head"], &[]) {
            self.pop_until(i);
        }
        self.head_open = false;
    }

    fn close_p(&mut self) {
        if let Some(i) = self.in_scope(&["p"], BUTTON_SCOPE) {
            self.pop_until(i);
        }
    }

    /// A new `<li>` (or `<dd>`, `<dt>`) closes the open one of its list.
    fn close_item(&mut self, names: &[&str]) {
        for i in (1..self.stack.len()).rev() {
            let name = self.name(self.stack[i]);
            if names.contains(&name) {
                self.pop_until(i);
                return;
            }
            if SPECIAL.contains(&name) && !matches!(name, "address" | "div" | "p") {
                return;
            }
        }
    }

    fn close_in_table(&mut self, names: &[&str]) {
        if let Some(i) = self.in_scope(names, TABLE_SCOPE) {
            self.pop_until(i);
        }
    }

    fn text(&mut self, mut text: String) {
        if text.contains('\0') {
            text = text.replace('\0', "");
        }
        if !is_blank(&text) {
            // Text straight inside `<head>` (not in its `<title>`) ends it.
            if self.head_open && self.name(self.current()) == "head" {
                self.close_head();
            }
            self.reconstruct();
        }
        let parent = self.current();
        let last = match &self.nodes[parent] {
            Node::Element(e) => e.children.last().copied(),
            Node::Text(_) => None,
        };
        if let Some(Node::Text(previous)) = last.map(|id| &mut self.nodes[id]) {
            previous.push_str(&text);
        } else {
            self.append(Node::Text(text));
        }
    }

    fn start(&mut self, tag: StartTag) {
        let name = tag.name.as_str();
        match name {
            "html" | "body" => {
                if name == "body" {
                    self.close_head();
                }
                return;
            }
            "head" => {
                if !self.head_open && self.open_element(tag.name, tag.attrs).is_some() {
                    self.head_open = true;
                }
                return;
            }
            _ => {}
        }
        if self.head_open && !HEAD_CONTENT.contains(&name) {
            self.close_head();
        }
        if CLOSES_P.contains(&name) {
            self.close_p();
        }
        match name {
            "li" => self.close_item(&["li"]),
            "dd" | "dt" => self.close_item(&["dd", "dt"]),
            "tr" => {
                self.close_in_table(&["td", "th"]);
                self.close_in_table(&["tr"]);
            }
            "td" | "th" => self.close_in_table(&["td", "th"]),
            "thead" | "tbody" | "tfoot" | "caption" | "colgroup" => {
                self.close_in_table(&["td", "th"]);
                self.close_in_table(&["tr"]);
                self.close_in_table(&["thead", "tbody", "tfoot", "caption", "colgroup"]);
            }
            "a" => {
                // An `<a>` inside an open `<a>` ends the first.
                if let Some(i) = self.active_formatting("a") {
                    let id = self.formatting[i];
                    self.end_formatting("a");
                    self.formatting.retain(|e| *e != id);
                    if let Some(at) = id.and_then(|id| self.stack.iter().rposition(|&s| s == id)) {
                        self.pop_until(at);
                    }
                }
            }
            "option" | "optgroup" => {
                if self.name(self.current()) == "option" {
                    self.pop();
                }
            }
            "button" => {
                if let Some(i) = self.in_scope(&["button"], DEFAULT_SCOPE) {
                    self.pop_until(i);
                }
            }
            // A heading directly inside another ends it.
            _ if HEADINGS.contains(&name) && HEADINGS.contains(&self.name(self.current())) => self.pop(),
            _ => {}
        }
        if !CLOSES_P.contains(&name) && !TABLE_PARTS.contains(&name) {
            self.reconstruct();
        }
        // XHTML's `<br/>` and `<div/>` are empty; raw text elements never
        // are, as their content has already been read as theirs.
        let raw = RAW_TEXT.contains(&name) || ESCAPABLE_RAW_TEXT.contains(&name);
        let empty = VOID.contains(&name) || (tag.self_closing && !raw);
        let formatting = FORMATTING.contains(&name);
        let marker = MARKERS.contains(&name);
        if empty {
            self.append(Node::Element(Element {
                name: tag.name,
                attrs: tag.attrs,
                children: Vec::new(),
            }));
            return;
        }
        let Some(id) = self.open_element(tag.name, tag.attrs) else {
            return;
        };
        if formatting {
            self.push_formatting(id);
        }
        if marker {
            self.formatting.push(None);
        }
    }

    fn end(&mut self, name: &str) {
        match name {
            "html" | "body" => return,
            "head" => {
                self.close_head();
                return;
            }
            "br" => {
                self.start(StartTag {
                    name: "br".into(),
                    ..StartTag::default()
                });
                return;
            }
            "p" => {
                match self.in_scope(&["p"], BUTTON_SCOPE) {
                    Some(i) => self.pop_until(i),
                    // A stray `</p>` still breaks the text, as an empty paragraph.
                    None => {
                        self.append(Node::Element(Element {
                            name: "p".into(),
                            attrs: Vec::new(),
                            children: Vec::new(),
                        }));
                    }
                }
                return;
            }
            _ => {}
        }
        if FORMATTING.contains(&name) {
            self.end_formatting(name);
            return;
        }
        let barriers = if HEADINGS.contains(&name) {
            if let Some(i) = self.in_scope(HEADINGS, DEFAULT_SCOPE) {
                self.pop_until(i);
            }
            return;
        } else if name == "li" {
            LIST_SCOPE
        } else if TABLE_PARTS.contains(&name) || name == "table" {
            TABLE_SCOPE
        } else if SPECIAL.contains(&name) {
            DEFAULT_SCOPE
        } else {
            self.end_other(name);
            return;
        };
        if let Some(i) = self.in_scope(&[name], barriers) {
            self.pop_until(i);
        }
    }

    /// Any other end tag closes the nearest open element of its name,
    /// unless a special element stands in between.
    fn end_other(&mut self, name: &str) {
        if !self.open_names.contains_key(name) {
            return;
        }
        for i in (1..self.stack.len()).rev() {
            let open = self.name(self.stack[i]);
            if open == name {
                self.pop_until(i);
                return;
            }
            if SPECIAL.contains(&open) {
                return;
            }
        }
    }

    /// The index in `formatting` of an element called `name` opened since
    /// the last marker.
    fn active_formatting(&self, name: &str) -> Option<usize> {
        for (i, entry) in self.formatting.iter().enumerate().rev() {
            match entry {
                None => return None,
                Some(id) if self.name(*id) == name => return Some(i),
                Some(_) => {}
            }
        }
        None
    }

    /// The end tag of a formatting element: a simplified "adoption agency".
    /// It closes the element and whatever was opened inside it; those
    /// formatting elements reopen with the next text.
    fn end_formatting(&mut self, name: &str) {
        let Some(i) = self.active_formatting(name) else {
            self.end_other(name);
            return;
        };
        let Some(id) = self.formatting[i] else {
            return;
        };
        match self.stack.iter().rposition(|&s| s == id) {
            None => {
                self.formatting.remove(i);
            }
            Some(at) => {
                let blocked = self.stack[at + 1..].iter().any(|&s| DEFAULT_SCOPE.contains(&self.name(s)));
                if !blocked {
                    self.formatting.remove(i);
                    self.pop_until(at);
                }
            }
        }
    }

    fn push_formatting(&mut self, id: usize) {
        let after_marker = self.formatting.iter().rposition(Option::is_none).map_or(0, |i| i + 1);
        let same = |builder: &Self, other: usize| match (&builder.nodes[id], &builder.nodes[other]) {
            (Node::Element(a), Node::Element(b)) => a.name == b.name && a.attrs == b.attrs,
            _ => false,
        };
        // At most three alike, as browsers keep ("Noah's ark").
        let alike: Vec<usize> = (after_marker..self.formatting.len())
            .filter(|&i| self.formatting[i].is_some_and(|other| same(self, other)))
            .collect();
        if alike.len() >= 3 {
            self.formatting.remove(alike[0]);
        }
        if self.formatting.len() < MAX_FORMATTING {
            self.formatting.push(Some(id));
        }
    }

    /// Reopen formatting elements closed by a block before more text.
    fn reconstruct(&mut self) {
        let mut first = self.formatting.len();
        while first > 0 {
            match self.formatting[first - 1] {
                Some(id) if !self.open[id] => first -= 1,
                _ => break,
            }
        }
        for i in first..self.formatting.len() {
            let Some(old) = self.formatting[i] else {
                continue;
            };
            let (name, attrs) = match &self.nodes[old] {
                Node::Element(e) => (e.name.clone(), e.attrs.clone()),
                Node::Text(_) => continue,
            };
            match self.open_element(name, attrs) {
                Some(id) => self.formatting[i] = Some(id),
                None => break,
            }
        }
    }
}

/// Walks the tree into blocks.
struct Mapper<'t> {
    nodes: &'t [Node],
}

#[derive(Clone, Default)]
struct Context {
    style: Style,
    /// Inside `<pre>` in a heading or cell, where white space is kept.
    pre: bool,
    /// How many quotes and lists enclose this point.
    nesting: usize,
}

/// Blocks being built, and the paragraph in progress.
struct Flow {
    blocks: Vec<Block>,
    line: Inlines,
    /// Headings and cells hold only inlines: what would be separate blocks
    /// inside them is separated by line breaks instead.
    inline: bool,
}

impl Flow {
    fn new(inline: bool) -> Self {
        Flow {
            blocks: Vec::new(),
            line: Inlines::default(),
            inline,
        }
    }

    /// End the paragraph in progress.
    fn flush(&mut self) {
        if self.inline {
            self.line.boundary();
            return;
        }
        let inlines = std::mem::take(&mut self.line).finish();
        if !inlines.is_empty() {
            self.blocks.push(Block::Paragraph { inlines });
        }
    }

    fn push(&mut self, block: Block) {
        self.flush();
        self.blocks.push(block);
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush();
        self.blocks
    }
}

/// A table row's cells: node, whether `<th>`, alignment and column span.
#[derive(Default)]
struct Row {
    cells: Vec<(usize, bool, Align, usize)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Head,
    Body,
    Foot,
}

#[derive(Default)]
struct Rows {
    caption: Vec<Inline>,
    head: Vec<Row>,
    body: Vec<Row>,
    foot: Vec<Row>,
    /// Cells outside any `<tr>`, gathered into a row of their own.
    loose: Option<(Section, Row)>,
}

impl Rows {
    fn add(&mut self, section: Section, row: Row) {
        match section {
            Section::Head => self.head.push(row),
            Section::Body => self.body.push(row),
            Section::Foot => self.foot.push(row),
        }
    }

    fn end_loose(&mut self) {
        if let Some((section, row)) = self.loose.take() {
            self.add(section, row);
        }
    }
}

fn cell_align(attrs: &[(String, String)]) -> Align {
    let value = style_property(attrs, "text-align").or_else(|| attr(attrs, "align").map(str::to_ascii_lowercase));
    match value.as_deref() {
        Some("left" | "start") => Align::Left,
        Some("center" | "middle") => Align::Center,
        Some("right" | "end") => Align::Right,
        _ => Align::None,
    }
}

/// A leading integer, as browsers read `start` and `colspan`.
fn leading_int(value: &str) -> Option<i64> {
    let value = value.trim();
    let (sign, digits) = match value.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, value.strip_prefix('+').unwrap_or(value)),
    };
    let len = digits.bytes().take_while(u8::is_ascii_digit).count().min(15);
    digits[..len].parse::<i64>().ok().map(|n| sign * n)
}

impl Mapper<'_> {
    fn blocks(&self) -> Vec<Block> {
        let mut flow = Flow::new(false);
        self.children(0, &mut flow, &Context::default());
        flow.finish()
    }

    /// The first `<title>`'s text, outside SVG and MathML (which have
    /// titles of their own).
    fn title(&self, id: usize) -> Option<String> {
        let Node::Element(e) = &self.nodes[id] else {
            return None;
        };
        if e.name == "title" {
            let mut text = String::new();
            for &child in &e.children {
                self.collect_text(child, &mut text);
            }
            let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
            return (!title.is_empty()).then_some(title);
        }
        if matches!(e.name.as_str(), "svg" | "math") {
            return None;
        }
        e.children.iter().find_map(|&child| self.title(child))
    }

    fn children(&self, id: usize, flow: &mut Flow, cx: &Context) {
        if let Node::Element(e) = &self.nodes[id] {
            for &child in &e.children {
                self.node(child, flow, cx);
            }
        }
    }

    fn node(&self, id: usize, flow: &mut Flow, cx: &Context) {
        match &self.nodes[id] {
            Node::Text(text) if cx.pre => {
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        flow.line.line_break();
                    }
                    flow.line.text(line, &cx.style, false);
                }
            }
            Node::Text(text) => flow.line.text(text, &cx.style, true),
            Node::Element(e) => self.element(id, e, flow, cx),
        }
    }

    fn element(&self, id: usize, e: &Element, flow: &mut Flow, cx: &Context) {
        let name = e.name.as_str();
        if SKIP.contains(&name) || is_hidden(&e.attrs) || (name == "dialog" && attr(&e.attrs, "open").is_none()) {
            return;
        }
        let room = !flow.inline && cx.nesting < MAX_NESTING;
        match name {
            "br" => flow.line.line_break(),
            "img" | "image" => self.image(e, flow, cx),
            "hr" if !flow.inline => flow.push(Block::Rule),
            "pre" | "listing" | "xmp" | "plaintext" => self.code(id, e, flow, cx),
            "blockquote" if room => {
                let inner = self.nested(id, cx);
                flow.flush();
                if !inner.is_empty() {
                    flow.blocks.push(Block::Quote { blocks: inner });
                }
            }
            "ul" | "ol" | "menu" | "dir" if room => self.list(e, flow, cx),
            "table" if !flow.inline => self.table(id, flow, cx),
            _ if HEADINGS.contains(&name) && !flow.inline => {
                flow.flush();
                let mut inner = Flow::new(true);
                self.children(id, &mut inner, cx);
                let inlines = inner.line.finish();
                if !inlines.is_empty() {
                    let level = name.as_bytes()[1] - b'0';
                    flow.blocks.push(Block::Heading { level, inlines });
                }
            }
            "dt" => {
                flow.flush();
                let cx = Context {
                    style: Style {
                        bold: true,
                        ..cx.style.clone()
                    },
                    ..cx.clone()
                };
                self.children(id, flow, &cx);
                flow.flush();
            }
            "q" => {
                flow.line.text("\u{201C}", &cx.style, true);
                self.children(id, flow, cx);
                flow.line.text("\u{201D}", &cx.style, true);
            }
            _ if BOUNDARY.contains(&name) => {
                flow.flush();
                self.children(id, flow, cx);
                flow.flush();
            }
            _ => {
                let mut style = cx.style.clone();
                match name {
                    "b" | "strong" => style.bold = true,
                    "i" | "em" | "cite" | "var" | "dfn" => style.italic = true,
                    "s" | "strike" | "del" => style.strike = true,
                    "code" | "kbd" | "tt" | "samp" => style.code = true,
                    "a" => {
                        if let Some(href) = attr(&e.attrs, "href").and_then(link_target) {
                            style.link = Some(href);
                        }
                    }
                    _ => {}
                }
                if style == cx.style {
                    self.children(id, flow, cx);
                } else {
                    let cx = Context { style, ..cx.clone() };
                    self.children(id, flow, &cx);
                }
            }
        }
    }

    /// The blocks inside a quote or list item, one level deeper.
    fn nested(&self, id: usize, cx: &Context) -> Vec<Block> {
        let cx = Context {
            nesting: cx.nesting + 1,
            ..cx.clone()
        };
        let mut inner = Flow::new(false);
        self.children(id, &mut inner, &cx);
        inner.finish()
    }

    fn image(&self, e: &Element, flow: &mut Flow, cx: &Context) {
        let src = [attr(&e.attrs, "src"), attr(&e.attrs, "data-src")]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|s| !s.is_empty())
            .unwrap_or("");
        let alt = attr(&e.attrs, "alt").unwrap_or("").split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        if !src.is_empty() || !alt.is_empty() {
            flow.line.image(alt, src.to_owned(), cx.style.link.clone());
        }
    }

    /// All the text inside, line breaks for `<br>`, as in `<pre>`.
    fn collect_text(&self, id: usize, out: &mut String) {
        match &self.nodes[id] {
            Node::Text(text) => out.push_str(text),
            Node::Element(e) if e.name == "br" => out.push('\n'),
            Node::Element(e) if SKIP.contains(&e.name.as_str()) || is_hidden(&e.attrs) => {}
            Node::Element(e) => {
                for &child in &e.children {
                    self.collect_text(child, out);
                }
            }
        }
    }

    /// A `language-*` or `lang-*` class (or a `lang` attribute) on a
    /// `<pre>` or its `<code>`.
    fn language(&self, e: &Element) -> String {
        let code = e.children.iter().find_map(|&child| match &self.nodes[child] {
            Node::Element(c) if c.name == "code" => Some(c),
            _ => None,
        });
        for attrs in std::iter::once(&e.attrs).chain(code.map(|c| &c.attrs)) {
            let classes = attr(attrs, "class").unwrap_or("");
            for class in classes.split_ascii_whitespace() {
                if let Some(language) = class.strip_prefix("language-").or_else(|| class.strip_prefix("lang-")) {
                    return language.to_owned();
                }
            }
            if let Some(language) = attr(attrs, "lang").or_else(|| attr(attrs, "data-lang")) {
                return language.trim().to_owned();
            }
        }
        String::new()
    }

    fn code(&self, id: usize, e: &Element, flow: &mut Flow, cx: &Context) {
        if flow.inline {
            let cx = Context {
                style: Style {
                    code: true,
                    ..cx.style.clone()
                },
                pre: true,
                ..cx.clone()
            };
            flow.flush();
            self.children(id, flow, &cx);
            flow.flush();
            return;
        }
        let mut text = String::new();
        self.collect_text(id, &mut text);
        // HTML drops the line break right after `<pre>`.
        let text = text.strip_prefix('\n').unwrap_or(&text).trim_end();
        if text.is_empty() {
            return;
        }
        flow.push(Block::Code {
            language: self.language(e),
            text: text.to_owned(),
        });
    }

    fn list(&self, e: &Element, flow: &mut Flow, cx: &Context) {
        flow.flush();
        let ordered = e.name == "ol";
        let start = if ordered { attr(&e.attrs, "start").and_then(leading_int).unwrap_or(1) } else { 1 };
        let cx = Context {
            nesting: cx.nesting + 1,
            ..cx.clone()
        };
        let mut items = Vec::new();
        let mut current: Option<(Option<bool>, Flow)> = None;
        for &child in &e.children {
            match &self.nodes[child] {
                Node::Element(li) if li.name == "li" => {
                    if is_hidden(&li.attrs) {
                        continue;
                    }
                    if let Some((checked, item)) = current.take() {
                        items.push(Item {
                            checked,
                            blocks: item.finish(),
                        });
                    }
                    let mut item = Flow::new(false);
                    self.children(child, &mut item, &cx);
                    // Whatever strays in after the item starts a new line.
                    item.flush();
                    current = Some((self.checkbox(child, 0).flatten(), item));
                }
                Node::Text(text) if is_blank(text) => {}
                // Anything else between items belongs to the one before.
                _ => {
                    let (_, item) = current.get_or_insert_with(|| (None, Flow::new(false)));
                    self.node(child, item, &cx);
                }
            }
        }
        if let Some((checked, item)) = current {
            items.push(Item {
                checked,
                blocks: item.finish(),
            });
        }
        if !items.is_empty() {
            flow.blocks.push(Block::List { ordered, start, items });
        }
    }

    /// A task-list checkbox leading a list item, as GitHub renders them:
    /// `Some(Some(checked))` once found, `Some(None)` when text comes first.
    fn checkbox(&self, id: usize, depth: usize) -> Option<Option<bool>> {
        let Node::Element(e) = &self.nodes[id] else {
            return None;
        };
        for &child in &e.children {
            match &self.nodes[child] {
                Node::Text(text) if is_blank(text) => {}
                Node::Text(_) => return Some(None),
                Node::Element(c) if c.name == "input" => {
                    let checkbox = attr(&c.attrs, "type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox"));
                    return Some(checkbox.then(|| attr(&c.attrs, "checked").is_some()));
                }
                Node::Element(c) if depth < 3 && matches!(c.name.as_str(), "p" | "label" | "span" | "div") => {
                    if let Some(found) = self.checkbox(child, depth + 1) {
                        return Some(found);
                    }
                }
                Node::Element(_) => return Some(None),
            }
        }
        None
    }

    fn table(&self, id: usize, flow: &mut Flow, cx: &Context) {
        flow.flush();
        let mut rows = Rows::default();
        // Content misplaced among the rows shows before the table, as
        // browsers "foster-parent" it.
        let mut before = Flow::new(false);
        self.table_children(id, Section::Body, &mut rows, &mut before, cx);
        rows.end_loose();
        flow.blocks.extend(before.finish());
        if !rows.caption.is_empty() {
            flow.blocks.push(Block::Paragraph {
                inlines: std::mem::take(&mut rows.caption),
            });
        }

        let mut body: Vec<Row> = Vec::new();
        let mut head = rows.head.into_iter();
        let header = head.next();
        body.extend(head);
        body.extend(rows.body);
        body.extend(rows.foot);
        let header = header.or_else(|| {
            let all_th = body.first().is_some_and(|row| !row.cells.is_empty() && row.cells.iter().all(|c| c.1));
            all_th.then(|| body.remove(0))
        });
        let width = header
            .iter()
            .chain(&body)
            .map(|row| row.cells.iter().map(|c| c.3).sum::<usize>())
            .max()
            .unwrap_or(0)
            .min(MAX_COLUMNS);
        if width == 0 {
            return;
        }
        let mut align = vec![Align::None; width];
        if let Some(first) = header.as_ref().or(body.first()) {
            let mut column = 0;
            for &(_, _, a, span) in &first.cells {
                if column < width {
                    align[column] = a;
                }
                column += span;
            }
        }
        let cells = |row: &Row, in_header: bool| -> Vec<Cell> {
            let mut out = Vec::new();
            for &(cell, th, _, span) in &row.cells {
                let mut inner = Flow::new(true);
                let cx = Context {
                    style: Style {
                        bold: cx.style.bold || (th && !in_header),
                        ..cx.style.clone()
                    },
                    ..cx.clone()
                };
                self.children(cell, &mut inner, &cx);
                out.push(inner.line.finish());
                for _ in 1..span {
                    out.push(Vec::new());
                }
                if out.len() >= width {
                    break;
                }
            }
            out
        };
        let header = header.map(|row| cells(&row, true)).unwrap_or_default();
        let body: Vec<Vec<Cell>> = body.iter().map(|row| cells(row, false)).collect();
        // A table of empty cells shows nothing; in Markdown it is often the
        // HTML around cells written in Markdown, which follow it instead.
        if header.iter().chain(body.iter().flatten()).all(Vec::is_empty) {
            return;
        }
        flow.blocks.extend(table(align, header, body));
    }

    fn table_children(&self, id: usize, section: Section, rows: &mut Rows, before: &mut Flow, cx: &Context) {
        let Node::Element(e) = &self.nodes[id] else {
            return;
        };
        for &child in &e.children {
            match &self.nodes[child] {
                Node::Text(text) if is_blank(text) => {}
                Node::Element(c) if is_hidden(&c.attrs) => {}
                Node::Element(c) => match c.name.as_str() {
                    "caption" => {
                        let mut inner = Flow::new(true);
                        self.children(child, &mut inner, cx);
                        rows.caption = inner.line.finish();
                    }
                    "colgroup" | "col" => {}
                    "thead" | "tbody" | "tfoot" => {
                        rows.end_loose();
                        let section = match c.name.as_str() {
                            "thead" => Section::Head,
                            "tfoot" => Section::Foot,
                            _ => Section::Body,
                        };
                        self.table_children(child, section, rows, before, cx);
                        rows.end_loose();
                    }
                    "form" => self.table_children(child, section, rows, before, cx),
                    "tr" => {
                        rows.end_loose();
                        let mut row = Row::default();
                        for &cell in &c.children {
                            match &self.nodes[cell] {
                                Node::Element(td) if matches!(td.name.as_str(), "td" | "th") => {
                                    if !is_hidden(&td.attrs) {
                                        row.cells.push(self.cell(cell, td));
                                    }
                                }
                                Node::Text(text) if is_blank(text) => {}
                                _ => self.node(cell, before, cx),
                            }
                        }
                        rows.add(section, row);
                    }
                    "td" | "th" => {
                        let cell = self.cell(child, c);
                        rows.loose.get_or_insert_with(|| (section, Row::default())).1.cells.push(cell);
                    }
                    _ => self.node(child, before, cx),
                },
                Node::Text(_) => self.node(child, before, cx),
            }
        }
    }

    fn cell(&self, id: usize, e: &Element) -> (usize, bool, Align, usize) {
        let span = attr(&e.attrs, "colspan").and_then(leading_int).unwrap_or(1).clamp(1, MAX_COLUMNS as i64);
        (id, e.name == "th", cell_align(&e.attrs), span as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::plain_text;
    use crate::markup::tests::{outline, show};

    fn t(text: &str) -> Inline {
        Inline::Text {
            text: text.into(),
            style: Style::default(),
        }
    }

    fn styled(text: &str, f: impl FnOnce(&mut Style)) -> Inline {
        let mut style = Style::default();
        f(&mut style);
        Inline::Text {
            text: text.into(),
            style,
        }
    }

    fn para(text: &str) -> Block {
        Block::Paragraph { inlines: vec![t(text)] }
    }

    fn blocks(text: &str) -> Vec<Block> {
        parse(text).blocks
    }

    fn lines(text: &str) -> Vec<String> {
        outline(&blocks(text))
    }

    #[test]
    fn tokenizes_tags_attributes_and_text() {
        let tokens: Vec<_> = Tokenizer::new("a<B Href=x id='1' data-v=\"&lt;&amp\" checked/>b</B ><!-- c --><!doctype html><?php x ?>\
            <![CDATA[<raw>]]>1 < 2</ x><")
            .map(|t| format!("{t:?}"))
            .collect();
        assert_eq!(tokens, [
            "Text(\"a\")",
            "Start(StartTag { name: \"b\", attrs: [(\"href\", \"x\"), (\"id\", \"1\"), (\"data-v\", \"<&\"), (\"checked\", \"\")], self_closing: true })",
            "Text(\"b\")",
            "End(\"b\")",
            "Text(\"<raw>\")",
            "Text(\"1 \")",
            "Text(\"<\")",
            "Text(\" 2\")",
            "Text(\"<\")",
        ]);
        let raw: Vec<_> = Tokenizer::new("<script>if (a<b && c>d) x='</p>'</SCRIPT >after<title>A &amp; B</title>")
            .map(|t| format!("{t:?}"))
            .collect();
        assert_eq!(raw[1], "Text(\"if (a<b && c>d) x='</p>'\")");
        assert_eq!(raw[2], "End(\"script\")");
        assert_eq!(raw[5], "Text(\"A & B\")");
        // A tag cut off by the end of the file is dropped.
        assert_eq!(Tokenizer::new("text<a href=\"x").count(), 1);
        assert_eq!(Tokenizer::new("<p class=a").count(), 0);
    }

    #[test]
    fn maps_a_page() {
        let doc = parse(
            "<!DOCTYPE html><html><head><title> My  Page </title><meta charset=utf-8>\
             <style>p { color: red }</style><script>alert('x')</script></head>\
             <body><h1>Welcome</h1><p>Some <b>bold</b>, <i>italic</i> and <a href=\"/x\">a link</a>.</p>\
             <hr><h2 id=more>More&nbsp;info</h2><p>Line<br>break</p></body></html>",
        );
        assert_eq!(doc.title.as_deref(), Some("My Page"));
        assert_eq!(doc.blocks[0], Block::Heading { level: 1, inlines: vec![t("Welcome")] });
        assert_eq!(doc.blocks[1], Block::Paragraph {
            inlines: vec![
                t("Some "),
                styled("bold", |s| s.bold = true),
                t(", "),
                styled("italic", |s| s.italic = true),
                t(" and "),
                styled("a link", |s| s.link = Some("/x".into())),
                t("."),
            ]
        });
        assert_eq!(doc.blocks[2], Block::Rule);
        assert_eq!(doc.blocks[3], Block::Heading { level: 2, inlines: vec![t("More\u{A0}info")] });
        assert_eq!(doc.blocks[4], Block::Paragraph { inlines: vec![t("Line"), Inline::Break, t("break")] });
        assert_eq!(doc.blocks.len(), 5);
        assert_eq!(parse("<h1>Fallback</h1>").title.as_deref(), Some("Fallback"));
        assert_eq!(parse("<svg><title>icon</title></svg><p>x</p>").title, None);
    }

    #[test]
    fn collapses_white_space_except_in_pre() {
        assert_eq!(blocks("<p>\n  a\n  <b> b </b>  c\t\n</p>"), [Block::Paragraph {
            inlines: vec![t("a "), styled("b ", |s| s.bold = true), t("c")]
        }]);
        assert_eq!(blocks("<pre class=\"language-rust\">\nfn main() {\n    <b>x</b>();\n}\n</pre>"), [Block::Code {
            language: "rust".into(),
            text: "fn main() {\n    x();\n}".into()
        }]);
        assert_eq!(blocks("<pre><code class=\"lang-py\">a &lt; b</code></pre>"), [Block::Code {
            language: "py".into(),
            text: "a < b".into()
        }]);
        assert_eq!(blocks("<p>a <code>x  y</code></p>"), [Block::Paragraph {
            inlines: vec![t("a "), styled("x y", |s| s.code = true)]
        }]);
    }

    #[test]
    fn closes_implicitly() {
        assert_eq!(lines("<p>one<p>two<div>three</div>four"), ["p one", "p two", "p three", "p four"]);
        assert_eq!(lines("<ul><li>a<li>b<ul><li>b1<li>b2</ul><li>c</ul>"), ["- p a", "- p b", "  - p b1", "  - p b2", "- p c"]);
        assert_eq!(lines("<dl><dt>Term<dd>Meaning<dt>Other<dd>Too</dl>"), ["p Term", "p Meaning", "p Other", "p Too"]);
        assert_eq!(lines("<table><tr><td>1<td>2<tr><td>3<td>4</table>"), ["table  / 1|2 / 3|4"]);
        assert_eq!(lines("<h1>a<h2>b</h1>c"), ["h1 a", "h2 b", "p c"]);
        assert_eq!(lines("<ol start=\"3\"><li>three<li>four</ol>"), ["3. p three", "4. p four"]);
    }

    #[test]
    fn survives_broken_markup() {
        assert_eq!(lines("<b>bold <i>both</b> italic</i> plain"), ["p bold both italic plain"]);
        let doc = blocks("<b>bold <i>both</b> italic</i> plain");
        let Block::Paragraph { inlines } = &doc[0] else { panic!() };
        assert_eq!(inlines[1], styled("both", |s| (s.bold, s.italic) = (true, true)));
        assert_eq!(inlines[2], styled(" italic", |s| s.italic = true));
        // Bold left open across paragraphs carries on, as in a browser.
        let doc = blocks("<p><b>one</p><p>two</b></p><p>three</p>");
        assert_eq!(doc[1], Block::Paragraph { inlines: vec![styled("two", |s| s.bold = true)] });
        assert_eq!(doc[2], para("three"));
        assert_eq!(lines("</div></p>text</span></li><p>after"), ["p text", "p after"]);
        assert_eq!(lines("<div><p>unclosed <b>everything"), ["p unclosed everything"]);
        assert_eq!(lines("<a href=1>one<a href=2>two</a>"), ["p onetwo"]);
        assert_eq!(lines("<p>a</p><p>b"), ["p a", "p b"]);
        assert_eq!(lines("x</p>y"), ["p x", "p y"]);
        assert_eq!(lines("<<>><a <p>&;&#;</a>"), ["p <<>>&;&#;"]);
        assert_eq!(lines("<p>missing head</p><title>T</title>"), ["p missing head"]);
        assert_eq!(lines("<head><title>T</title><p>no body tag"), ["p no body tag"]);
    }

    #[test]
    fn skips_what_a_reader_does_not_see() {
        let text = "<p>keep</p><script>no()</script><style>.x{}</style><noscript>no</noscript>\
                    <template><p>no</p></template><svg><text>no</text></svg><iframe src=x>no</iframe>\
                    <form><label>Name</label><input value=no><select><option>no</select><button>no</button></form>\
                    <p hidden>no</p><div style=\"color: red; display : none\">no</div><p style=\"display:block\">yes</p>";
        assert_eq!(lines(text), ["p keep", "p Name", "p yes"]);
    }

    #[test]
    fn lists_and_quotes() {
        assert_eq!(
            lines("<blockquote><p>quoted</p><blockquote>deeper</blockquote></blockquote>"),
            ["> p quoted", "> > p deeper"]
        );
        assert_eq!(
            lines("<ul class=contains-task-list><li><input type=checkbox checked disabled> done</li>\
                   <li><p><input type=\"checkbox\"> todo</p></li><li>plain <input type=checkbox></li></ul>"),
            ["- [x] p done", "- [ ] p todo", "- p plain"]
        );
        assert_eq!(lines("<ul><li>a</li><ul><li>nested</li></ul><li>b</li></ul>"), ["- p a", "  - p nested", "- p b"]);
        assert_eq!(lines("<ol start=-2><li>x</ol><ol start=junk><li>y</ol>"), ["-2. p x", "1. p y"]);
        let deep = "<blockquote>".repeat(100) + "deep";
        let doc = blocks(&deep);
        let mut depth = 0;
        let mut level = &doc;
        while let Some(Block::Quote { blocks }) = level.first() {
            depth += 1;
            level = blocks;
        }
        assert_eq!(depth, MAX_NESTING);
        assert_eq!(outline(level), ["p deep"]);
    }

    #[test]
    fn tables() {
        let doc = blocks(
            "<table><caption>Prices</caption><thead><tr><th align=left>Item<th style='text-align:right'>Cost</tr></thead>\
             <tbody><tr><td>Tea<td>₹ 20<tr><td colspan=2>Total <b>later</b></tbody>\
             <tfoot><tr><th>Sum<td>20</tfoot></table>",
        );
        assert_eq!(doc[0], para("Prices"));
        let Block::Table { align, header, rows } = &doc[1] else { panic!("{doc:?}") };
        assert_eq!(align, &[Align::Left, Align::Right]);
        assert_eq!(header, &[vec![t("Item")], vec![t("Cost")]]);
        assert_eq!(rows[0], [vec![t("Tea")], vec![t("₹ 20")]]);
        assert_eq!(rows[1], [vec![t("Total "), styled("later", |s| s.bold = true)], vec![]]);
        assert_eq!(rows[2], [vec![styled("Sum", |s| s.bold = true)], vec![t("20")]]);
        // No header row: the header is empty; ragged rows are padded.
        assert_eq!(lines("<table><tr><td>a<tr><td>b<td>c</table>"), ["table  / a| / b|c"]);
        // Misplaced text goes before the table; cells may hold blocks.
        assert_eq!(lines("<table>stray<tr><td><p>one</p><p>two</p></td></tr></table>"), ["p stray", "table  / one two"]);
        assert_eq!(lines("<table><tr><td><table><tr><td>inner</table></td><td>x</table>"), ["table  / inner|x"]);
        assert_eq!(lines("<table></table><table><tr></tr></table>"), Vec::<String>::new());
    }

    #[test]
    fn images_and_entities() {
        assert_eq!(
            blocks("<p><a href=\"https://ci\"><img src=\"badge.svg\" alt=\" build  status \"></a><img data-src=lazy.png></p>"),
            [Block::Paragraph {
                inlines: vec![
                    Inline::Image { alt: "build status".into(), src: "badge.svg".into(), link: Some("https://ci".into()) },
                    Inline::Image { alt: "".into(), src: "lazy.png".into(), link: None },
                ]
            }]
        );
        assert_eq!(lines("<p>&copy; 2024 &mdash; caf&eacute; &#x1F600; &#8364;5 &euro &amp;c</p>"), ["p © 2024 — café 😀 €5 &euro &c"]);
        assert_eq!(lines("<a href=\"?a=1&copy=2&amp;b\">x</a>"), ["p x"]);
        let Block::Paragraph { inlines } = &blocks("<a href=\"?a=1&copy=2&amp;b\">x</a>")[0] else { panic!() };
        assert_eq!(inlines[0], styled("x", |s| s.link = Some("?a=1&copy=2&b".into())));
        assert_eq!(lines("<p>She said <q>hi</q> <sup>2</sup>H<sub>2</sub>O</p>"), ["p She said \u{201C}hi\u{201D} 2H2O"]);
    }

    #[test]
    fn depth_is_bounded() {
        let deep = "<div><span>".repeat(5000) + "x";
        assert_eq!(lines(&deep), ["p x"]);
        let (nodes, _) = build(&("<b>".repeat(10_000) + "x"));
        assert!(nodes.len() > MAX_DEPTH);
        let rows = "<table>".to_owned() + &"<tr><td>c".repeat(20_000) + "</table>";
        assert!(matches!(&blocks(&rows)[0], Block::Table { rows, .. } if rows.len() == 20_000));
    }

    /// Each input with its blocks written out by [`outline`].
    fn cases(cases: &[(&str, &[&str])]) {
        for (input, expected) in cases {
            assert_eq!(lines(input), *expected, "{input:?}");
        }
    }

    /// The runs of the only paragraph or heading of `text`, by [`show`].
    fn runs(text: &str) -> String {
        match &blocks(text)[..] {
            [Block::Paragraph { inlines } | Block::Heading { inlines, .. }] => show(inlines),
            other => panic!("{text:?} gave {other:?}"),
        }
    }

    #[test]
    fn tokenizer_edge_cases() {
        cases(&[
            ("<P>Upper</P><BR><HR>", &["p Upper", "hr"]),
            ("<Ul><LI>a<Li>b</uL>", &["- p a", "- p b"]),
            ("<p title='a>b'>quoted gt</p><p class=a/b>slash in value</p>", &["p quoted gt", "p slash in value"]),
            ("</>text<!>more", &["p textmore"]),
            ("<?xml version=\"1.0\"?><p>x", &["p x"]),
            ("x<!-- a -- b -->y<!--->z<!-->w", &["p xyzw"]),
            ("a<!-- unclosed <p>b", &["p a"]),
            ("<p>1 < 2 and 3 > 2</p><p>a <3 b</p>", &["p 1 < 2 and 3 > 2", "p a <3 b"]),
            ("<scriptx>shown</scriptx>", &["p shown"]),
            ("<script>a</scriptx>still script</script>after", &["p after"]),
            ("<SCRIPT type=text/javascript>if (a < b) document.write('<p>x</p>')</SCRIPT>after", &["p after"]),
            ("<plaintext><b>all</b> raw</plaintext>", &["code[] <b>all</b> raw</plaintext>"]),
            ("<xmp><b>raw</b></xmp>", &["code[] <b>raw</b>"]),
            ("<p>a</p\n>b", &["p a", "p b"]),
            ("<p>x<a href=\"oops", &["p x"]),
            ("<p>x<", &["p x<"]),
            ("<div <p>attr garbage>x", &["p attr garbage>x"]),
            ("<p =x>y</p><div/>z", &["p y", "p z"]),
            ("a\0b\r\nc", &["p ab c"]),
            ("\u{FEFF}<p>bom", &["p bom"]),
        ]);
        let Block::Paragraph { inlines } = &blocks("<img src=x alt=one alt=two><a href = \"spaced\" >s</a>")[0] else {
            panic!()
        };
        assert_eq!(show(inlines), "![one](x)<a spaced>s</a>");
        assert_eq!(parse("<title>A <b>tag</b> &amp; text</title>").title.as_deref(), Some("A <b>tag</b> & text"));
    }

    #[test]
    fn character_references() {
        cases(&[
            ("&amp;&lt;&gt;&quot;&apos;&#39;&#x27;", &["p &<>\"'''"]),
            ("&AMP &LT &copy2024", &["p & < ©2024"]),
            ("&notin; &notit; &notin", &["p ∉ ¬it; ¬in"]),
            ("&#128512; &#x1F600 &#150; &#x80; &#xD800; &#1114112;", &["p 😀 😀 – € \u{FFFD} \u{FFFD}"]),
            ("&unknown; & ; &# &#x;", &["p &unknown; & ; &# &#x;"]),
            ("&hellip;&trade;&larr;&rarr;&le;&ge;&ne;&infin;&sum;&radic;&deg;&micro;", &["p …™←→≤≥≠∞∑√°µ"]),
            ("&frac13; &check; &bigstar; &rightarrow; &NotEqualTilde;", &["p ⅓ ✓ ★ → &NotEqualTilde;"]),
        ]);
        assert_eq!(runs("<a href=\"?a=1&copy=2&lt=3&amp;b&lt;\">x</a>"), "<a ?a=1&copy=2&lt=3&b<>x</a>");
        assert_eq!(runs("<img alt=\"caf&eacute;\" src=x>"), "![café](x)");
    }

    #[test]
    fn maps_each_element() {
        cases(&[
            ("<h1>1</h1><h2>2</h2><h3>3</h3><h4>4</h4><h5>5</h5><h6>6</h6>", &["h1 1", "h2 2", "h3 3", "h4 4", "h5 5", "h6 6"]),
            (
                "<header>H</header><nav>N</nav><main><article><section>S</section></article><aside>A</aside></main>\
                 <footer>F</footer>",
                &["p H", "p N", "p S", "p A", "p F"],
            ),
            ("<div>a<div>b</div>c</div><span>d</span><span>e</span>", &["p a", "p b", "p c", "p de"]),
            ("<figure><img src=f.png alt=F><figcaption>Caption</figcaption></figure>", &["p F", "p Caption"]),
            ("<center>c</center><address>addr</address>", &["p c", "p addr"]),
            ("<details><summary>S</summary>D</details>", &["p S", "p D"]),
            ("<dialog>closed</dialog><dialog open>open</dialog>", &["p open"]),
            ("<p>a<hr>b", &["p a", "hr", "p b"]),
            ("<ul><li><pre>code\n  x</pre></li></ul>", &["- code[] code\n  x"]),
            ("<ol start=\"0\"><li>z</ol><ol reversed start=3><li>a<li>b</ol>", &["0. p z", "3. p a", "4. p b"]),
            ("<ul><li>a</li>text between<li>b</ul>", &["- p a", "  p text between", "- p b"]),
            ("<ul><li>a<ul><li>b<ol><li>c</ol></ul></ul>", &["- p a", "  - p b", "    1. p c"]),
            ("<li>orphan</li>", &["p orphan"]),
            ("<menu><li>m</menu><dir><li>d</dir>", &["- p m", "- p d"]),
            ("<a href=x><div>block</div></a>", &["p block"]),
            ("<p><u>u</u> <ins>i</ins> <mark>m</mark> <small>s</small> <big>b</big> <abbr>a</abbr> <font>f</font></p>", &[
                "p u i m s b a f",
            ]),
        ]);
        assert_eq!(runs("<h3>Multi<br>line</h3>"), "Multi<br>line");
        assert_eq!(runs("<p><strong>s</strong><b>b</b><em>e</em><i>i</i><cite>c</cite><var>v</var><dfn>d</dfn></p>"), "<b>sb</b><i>eicvd</i>");
        assert_eq!(runs("<p><s>s</s><strike>k</strike><del>d</del> <code>c</code><kbd>k</kbd><tt>t</tt><samp>s</samp></p>"), "<s>skd</s> <c>ckts</c>");
        assert_eq!(runs("<p><a href=\"/u\">l <b>b</b></a></p>"), "<a /u>l </a><a /u><b>b</b></a>");
        assert_eq!(
            runs("<a href=\"javascript:void(0)\">js</a> <a href=\"\">e</a> <a>n</a> <a href=\"  /t  \">t</a>"),
            "js e n <a /t>t</a>"
        );
        assert_eq!(runs("<p><img src=\"a.png\" alt=\"A\"> <img alt=\"only alt\"> <img src=\"\"></p>"), "![A](a.png) ![only alt]()");
        assert_eq!(runs("<picture><source srcset=x><img src=\" y.png \" alt=P></picture>"), "![P](y.png)");
        assert_eq!(runs("<p><b>a</b> <i>b</i></p>"), "<b>a</b> <i>b</i>");
        assert_eq!(runs("<p><b>a </b> <i> b</i></p>"), "<b>a </b><i>b</i>");
    }

    #[test]
    fn skips_head_and_hidden_content() {
        cases(&[
            (
                "<head><meta charset=utf-8><link rel=stylesheet href=x><style>s</style><script>j</script>\
                 <base href=/></head><body>b",
                &["p b"],
            ),
            ("<noscript>ns</noscript><template>t</template><svg><text>s</text></svg><math><mi>x</mi></math>", &["p x"]),
            ("<iframe>i</iframe><object>o</object><embed src=x><canvas>c</canvas><audio>a</audio><video>v</video>", &[]),
            (
                "<form>Name: <input name=n> <select><option>o</select> <textarea>t</textarea> <button>b</button>\
                 <datalist><option>d</datalist> <output>o</output> <progress>p</progress> <meter>m</meter></form>",
                &["p Name:"],
            ),
            (
                "<p hidden>h</p><div style=\"display:none !important\">d</div><span style='DISPLAY: NONE'>s</span>\
                 <ul><li hidden>gone<li>kept</ul><table><tr hidden><td>no<tr><td>yes</table>",
                &["- p kept", "table  / yes"],
            ),
            ("<html><body>a</body></html>after", &["p aafter"]),
        ]);
        let doc = parse("<p>body</p><title>Late</title>");
        assert_eq!(doc.title.as_deref(), Some("Late"));
        assert_eq!(outline(&doc.blocks), ["p body"]);
    }

    #[test]
    fn white_space() {
        cases(&[
            ("<p>  a \n\t b  </p>", &["p a b"]),
            ("<p>a&nbsp;&nbsp;b</p>", &["p a\u{A0}\u{A0}b"]),
            ("<pre>  lead\n\ttab  </pre>", &["code[]   lead\n\ttab"]),
            ("<pre>\nfirst\n</pre><pre>\n\nsecond</pre>", &["code[] first", "code[] \nsecond"]),
            ("<pre>a<br>b</pre><pre>&lt;tag&gt;</pre>", &["code[] a\nb", "code[] <tag>"]),
            ("<pre>a\r\nb\rc</pre>", &["code[] a\nb\nc"]),
            ("<pre>   </pre><pre></pre>", &[]),
        ]);
        let Block::Table { rows, .. } = &blocks("<table><tr><td><pre>a\n  b</pre></table>")[0] else { panic!() };
        assert_eq!(show(&rows[0][0]), "<c>a</c><br><c>  b</c>");
    }

    #[test]
    fn closes_what_html_lets_authors_leave_open() {
        cases(&[
            ("<p>a<table><tr><td>t</table>b", &["p a", "table  / t", "p b"]),
            ("<p>a<h2>b</h2>c", &["p a", "h2 b", "p c"]),
            ("<p>a<pre>b</pre><p>c<blockquote>d</blockquote>", &["p a", "code[] b", "p c", "> p d"]),
            ("<li>a<li>b", &["p a", "p b"]),
            ("<ul><li>a<p>p1<li>b</ul>", &["- p a", "  p p1", "- p b"]),
            ("<table><tr><th>h<td>d<tr><td>x</table>", &["table  / h|d / x|"]),
            ("<table><thead><tr><th>a<tbody><tr><td>1<tfoot><tr><td>f</table>", &["table a / 1 / f"]),
            ("<table><tfoot><tr><td>foot</tfoot><tbody><tr><td>body</tbody></table>", &["table  / body / foot"]),
            ("<h1>a<h1>b", &["h1 a", "h1 b"]),
            ("<ul><li>unclosed list", &["- p unclosed list"]),
            ("<ol><li>a<ol><li>b", &["1. p a", "  1. p b"]),
            ("<blockquote>unclosed", &["> p unclosed"]),
            ("<pre>unclosed\n  pre", &["code[] unclosed\n  pre"]),
            ("<h2>unclosed heading", &["h2 unclosed heading"]),
            ("<td>loose cell</td><tr><td>loose row", &["p loose cell", "p loose row"]),
            ("<table><tr><td>a</td></tr></table></td></tr></table>more", &["table  / a", "p more"]),
            ("</b></i></a></span></div></table></tr></td></li></ul></h1></pre>", &[]),
        ]);
        assert_eq!(runs("<a href=1>x<a href=2>y</a>z"), "<a 1>x</a><a 2>y</a>z");
        assert_eq!(runs("<b><b><b><b>x</b></b></b></b>y"), "<b>x</b>y");
        let doc = blocks("<p><b>bold<i>both</p>after");
        assert_eq!(outline(&doc), ["p boldboth", "p after"]);
        let Block::Paragraph { inlines } = &doc[1] else { panic!() };
        assert_eq!(show(inlines), "<b><i>after</i></b>");
    }

    #[test]
    fn table_shapes() {
        let Block::Table { align, rows, .. } = &blocks("<table><tr><td colspan=1000>x<td colspan=0>y</table>")[0] else {
            panic!()
        };
        assert_eq!(align.len(), MAX_COLUMNS);
        assert_eq!(rows[0].len(), MAX_COLUMNS);
        assert_eq!(plain_text(&rows[0][0]), "x");
        cases(&[
            ("<table><caption>C</caption><tr><td> </td></tr></table>", &["p C"]),
            ("<table><tr><td>a<td>b</tr><tr><td colspan=2>wide</table>", &["table  / a|b / wide|"]),
            ("<table><tr><td rowspan=2>r<td>1<tr><td>2</table>", &["table  / r|1 / 2|"]),
            ("<table><tr><td><img src=i alt=I></table>", &["table  / I"]),
            ("<ul><li><table><tr><td>x</table></ul>", &["- table  / x"]),
            ("<h1>a<table><tr><td>b</table></h1>", &["h1 a b"]),
            ("<table><thead><tr><td>h</td></tr></thead><tr><td>b</table>", &["table h / b"]),
        ]);
        let Block::Table { rows, .. } = &blocks("<table><tr><td>a<br>b</td></tr></table>")[0] else { panic!() };
        assert_eq!(show(&rows[0][0]), "a<br>b");
        let Block::Table { align, .. } = &blocks("<table><tr><th align=CENTER>a<th style=\"text-align: end\">b<th>c</table>")[0]
        else {
            panic!()
        };
        assert_eq!(align, &[Align::Center, Align::Right, Align::None]);
    }

    #[test]
    fn titles() {
        assert_eq!(parse("<html><head><title>  Multi\n line  </title></head>").title.as_deref(), Some("Multi line"));
        assert_eq!(parse("<title> </title><h2>Two</h2><h1>Fallback</h1>").title.as_deref(), Some("Fallback"));
        assert_eq!(parse("<p>none</p>").title, None);
    }
}
