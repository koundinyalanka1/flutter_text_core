//! Markdown's inline syntax, flattened into styled runs: emphasis and
//! strikethrough, code spans, links and images (inline and by reference),
//! autolinks and bare links, raw HTML, escapes and entities.
//!
//! This is the CommonMark reference algorithm. Delimiter runs (`*`, `_`,
//! `~`) and brackets go on stacks while the text is scanned; each `]`
//! resolves a link or image, and emphasis is matched afterwards ("process
//! emphasis"). Instead of building a tree, matches insert start and end
//! marks into a linked list of pieces, and a last pass turns the list into
//! runs of text with their styles.

use std::collections::HashMap;

use super::{entities, link_target, Inline, Inlines, Style};

/// Link reference definitions: normalized label to destination.
pub(super) type Refs = HashMap<String, String>;

const NONE: usize = usize::MAX;

/// Link destinations may nest parentheses this deep.
const MAX_PARENS: usize = 32;

/// Link labels are at most this many characters long.
const MAX_LABEL: usize = 999;

/// What a start or end mark opens or closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Span {
    Strong,
    Emphasis,
    Strike,
    Link,
    Image,
}

#[derive(Debug)]
enum Piece {
    Text(String),
    Code(String),
    SoftBreak,
    HardBreak,
    Html(Tag),
    /// A span's start, with the destination of a link or image.
    Start(Span, String),
    End(Span),
}

/// A raw HTML tag.
#[derive(Debug, Default)]
pub(super) struct Tag {
    /// Lower case.
    pub name: String,
    pub closing: bool,
    pub self_closing: bool,
    /// Names in lower case; values with their character references decoded.
    pub attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// Searches for closing sequences that already failed: none occurs at or
/// after the recorded offset, so a later search from further on fails at
/// once. This keeps text full of unclosed `<!--` or `"` linear.
pub(super) struct Unclosed([usize; 7]);

const COMMENT: usize = 0;
const INSTRUCTION: usize = 1;
const CDATA: usize = 2;
const DECLARATION: usize = 3;
const DOUBLE_QUOTE: usize = 4;
const SINGLE_QUOTE: usize = 5;
const TITLE: usize = 6;

impl Unclosed {
    pub fn new() -> Self {
        Unclosed([usize::MAX; 7])
    }

    fn find(&mut self, src: &str, from: usize, needle: &str, slot: usize) -> Option<usize> {
        if from >= self.0[slot] || from > src.len() {
            return None;
        }
        match src[from..].find(needle) {
            Some(i) => Some(from + i),
            None => {
                self.0[slot] = from;
                None
            }
        }
    }
}

struct Node {
    piece: Piece,
    prev: usize,
    next: usize,
}

/// A run of `*`, `_` or `~` that may open or close emphasis.
struct Delimiter {
    node: usize,
    ch: u8,
    /// Delimiters not yet used.
    count: usize,
    /// The run's length as written, for the "multiple of 3" rule.
    original: usize,
    can_open: bool,
    can_close: bool,
    prev: usize,
    next: usize,
}

/// A `[` or `![` that may start a link or image.
struct Bracket {
    node: usize,
    /// Offset of the `[`.
    at: usize,
    image: bool,
    /// False inside a link, where no other link may start.
    active: bool,
    /// Another bracket opened after this one, so its text cannot be a
    /// reference label.
    bracket_after: bool,
    /// The top of the delimiter stack when this bracket was pushed.
    delimiter: usize,
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    refs: &'a Refs,
    nodes: Vec<Node>,
    head: usize,
    tail: usize,
    delimiters: Vec<Delimiter>,
    /// The last delimiter on the stack.
    top: usize,
    brackets: Vec<Bracket>,
    /// Brackets below this index are all inactive or images.
    inactive: usize,
    /// Offsets of every run of backticks, by length; read on first need.
    backticks: Option<HashMap<usize, Vec<usize>>>,
    unclosed: Unclosed,
}

/// Parse a paragraph's, heading's or cell's text (already stripped of its
/// block markers and trimmed) into runs.
pub(super) fn parse(text: &str, refs: &Refs) -> Vec<Inline> {
    let mut parser = Parser::new(text, refs);
    parser.run();
    parser.process_emphasis(NONE);
    bare_links(parser.flatten())
}

/// Parse a link reference definition (`[label]: destination "title"`) at
/// the start of `s` into `refs`, unless the label is already defined.
/// Returns the bytes it took, through the end of its line, or 0 when `s`
/// does not start with one.
pub(super) fn parse_reference(s: &str, refs: &mut Refs) -> usize {
    let label_len = link_label(s);
    if label_len == 0 || s.as_bytes().get(label_len) != Some(&b':') {
        return 0;
    }
    let label = normalize_label(&s[..label_len]);
    if label.is_empty() {
        return 0;
    }
    let none = Refs::new();
    let mut p = Parser::new(s, &none);
    p.pos = label_len + 1;
    p.skip_spnl();
    let Some(destination) = p.link_destination() else {
        return 0;
    };
    let before_title = p.pos;
    p.skip_spnl();
    let title = if p.pos != before_title { p.link_title() } else { None };
    if title.is_none() {
        p.pos = before_title;
    }
    let mut at_end = p.at_line_end();
    if !at_end && title.is_some() {
        // A title followed by more text: still a definition without it, if
        // the destination ends its line.
        p.pos = before_title;
        at_end = p.at_line_end();
    }
    if !at_end {
        return 0;
    }
    refs.entry(label).or_insert(destination);
    p.pos
}

/// A link label as CommonMark compares them: without its brackets, white
/// space collapsed and case folded.
fn normalize_label(label: &str) -> String {
    let inner = label.strip_prefix('[').and_then(|l| l.strip_suffix(']')).unwrap_or(label);
    let mut out = String::with_capacity(inner.len());
    for word in inner.split([' ', '\t', '\n', '\r']).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    // Lower, upper and lower again folds ẞ, ß and SS alike.
    out.to_lowercase().to_uppercase().to_lowercase()
}

/// The length of a link label (`[...]`, no unescaped brackets inside, at
/// most 999 characters) at the start of `s`, or 0.
fn link_label(s: &str) -> usize {
    if !s.starts_with('[') {
        return 0;
    }
    let mut escaped = false;
    for (count, (i, c)) in s.char_indices().skip(1).enumerate() {
        if count > MAX_LABEL {
            return 0;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '[' => return 0,
            ']' => return i + 1,
            _ => {}
        }
    }
    0
}

/// Text with backslash escapes and character references resolved, as in
/// link destinations, titles and code block info strings.
pub(super) fn unescape(s: &str) -> String {
    if !s.contains(['\\', '&']) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find(['\\', '&']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('\\') {
            match rest[1..].chars().next() {
                Some(c) if c.is_ascii_punctuation() => {
                    out.push(c);
                    rest = &rest[2..];
                }
                _ => {
                    out.push('\\');
                    rest = &rest[1..];
                }
            }
        } else if let Some((c, used)) = entities::decode_strict(rest) {
            out.push(c);
            rest = &rest[used..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn is_special(c: u8) -> bool {
    matches!(c, b'\n' | b'\\' | b'`' | b'*' | b'_' | b'~' | b'[' | b']' | b'!' | b'<' | b'&')
}

/// Unicode punctuation and symbols, which CommonMark's flanking rules treat
/// alike. Outside ASCII this covers the common blocks rather than every
/// character of categories P and S.
fn is_punctuation(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    matches!(c as u32,
        0xA1..=0xA9 | 0xAB | 0xAC | 0xAE..=0xB1 | 0xB4 | 0xB6..=0xB8 | 0xBB | 0xBF | 0xD7 | 0xF7
        | 0x2010..=0x2027 | 0x2030..=0x205E | 0x20A0..=0x20C0 | 0x2100..=0x2101 | 0x2103..=0x2106
        | 0x2108..=0x2109 | 0x2116..=0x2118 | 0x211E..=0x2123 | 0x2125 | 0x2127 | 0x2129 | 0x212E
        | 0x2190..=0x2BFF | 0x2E00..=0x2E7F | 0x3001..=0x3003 | 0x3008..=0x3020 | 0x3030 | 0x303D
        | 0x30FB | 0xFE10..=0xFE19 | 0xFE30..=0xFE6B | 0xFF01..=0xFF0F | 0xFF1A..=0xFF20
        | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 | 0x1F000..=0x1FAFF)
}

fn is_ascii_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0C')
}

/// An autolink (`<scheme:...>` or `<user@host>`) at the start of `s`: its
/// length and destination.
fn autolink(s: &str) -> Option<(usize, String)> {
    // Neither kind holds spaces or brackets, so the search stops at them.
    let end = 1 + s.as_bytes()[1..].iter().position(|&c| c == b'>' || c == b'<' || c <= b' ')?;
    if s.as_bytes()[end] != b'>' {
        return None;
    }
    let inner = &s[1..end];
    // URI: a scheme of 2 to 32 characters and a colon.
    let scheme_len = inner.find(':').unwrap_or(0);
    let scheme = &inner[..scheme_len];
    if (2..=32).contains(&scheme.len())
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
    {
        return Some((end + 1, inner.to_owned()));
    }
    // Email address.
    let (user, host) = inner.split_once('@')?;
    let user_ok = !user.is_empty()
        && user.bytes().all(|c| c.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&c));
    let host_ok = host.split('.').all(|label| {
        let l = label.as_bytes();
        !l.is_empty()
            && l.len() <= 63
            && l[0].is_ascii_alphanumeric()
            && l[l.len() - 1].is_ascii_alphanumeric()
            && l.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
    });
    if user_ok && host_ok {
        return Some((end + 1, format!("mailto:{inner}")));
    }
    None
}

/// Raw HTML at `pos` (comment, processing instruction, declaration, CDATA
/// or tag): its length, and the tag when it is one.
pub(super) fn raw_html(src: &str, pos: usize, unclosed: &mut Unclosed) -> Option<(usize, Option<Tag>)> {
    let rest = &src[pos..];
    if let Some(after) = rest.strip_prefix("<!--") {
        if after.starts_with('>') {
            return Some((5, None));
        }
        if after.starts_with("->") {
            return Some((6, None));
        }
        let end = unclosed.find(src, pos + 4, "-->", COMMENT)?;
        return Some((end + 3 - pos, None));
    }
    if rest.starts_with("<?") {
        let end = unclosed.find(src, pos + 2, "?>", INSTRUCTION)?;
        return Some((end + 2 - pos, None));
    }
    if rest.starts_with("<![CDATA[") {
        let end = unclosed.find(src, pos + 9, "]]>", CDATA)?;
        return Some((end + 3 - pos, None));
    }
    if rest.starts_with("<!") && rest.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) {
        let end = unclosed.find(src, pos + 2, ">", DECLARATION)?;
        return Some((end + 1 - pos, None));
    }
    let (len, tag) = html_tag(src, pos, unclosed)?;
    Some((len, Some(tag)))
}

/// An HTML open or closing tag at `pos`, by CommonMark's grammar: its
/// length and what it says.
pub(super) fn html_tag(src: &str, pos: usize, unclosed: &mut Unclosed) -> Option<(usize, Tag)> {
    let b = src.as_bytes();
    let mut i = pos + 1;
    let closing = b.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    if !b.get(i)?.is_ascii_alphabetic() {
        return None;
    }
    let name_start = i;
    while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-') {
        i += 1;
    }
    let mut tag = Tag {
        name: src[name_start..i].to_ascii_lowercase(),
        closing,
        ..Tag::default()
    };
    let skip_space = |mut i: usize| {
        while b.get(i).copied().is_some_and(is_ascii_space) {
            i += 1;
        }
        i
    };
    if closing {
        i = skip_space(i);
        return (b.get(i) == Some(&b'>')).then_some((i + 1 - pos, tag));
    }
    loop {
        let spaced = skip_space(i);
        match b.get(spaced) {
            Some(b'>') => return Some((spaced + 1 - pos, tag)),
            Some(b'/') if b.get(spaced + 1) == Some(&b'>') => {
                tag.self_closing = true;
                return Some((spaced + 2 - pos, tag));
            }
            Some(c) if spaced > i && (c.is_ascii_alphabetic() || *c == b'_' || *c == b':') => {
                i = spaced;
                let name_start = i;
                while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(c)) {
                    i += 1;
                }
                let name = src[name_start..i].to_ascii_lowercase();
                let mut value = String::new();
                let before_value = skip_space(i);
                if b.get(before_value) == Some(&b'=') {
                    i = skip_space(before_value + 1);
                    let raw = match b.get(i)? {
                        &quote @ (b'"' | b'\'') => {
                            let slot = if quote == b'"' { DOUBLE_QUOTE } else { SINGLE_QUOTE };
                            let close = unclosed.find(src, i + 1, if quote == b'"' { "\"" } else { "'" }, slot)?;
                            let raw = &src[i + 1..close];
                            i = close + 1;
                            raw
                        }
                        _ => {
                            let start = i;
                            while b.get(i).is_some_and(|c| *c > b' ' && !b"\"'=<>`".contains(c)) {
                                i += 1;
                            }
                            if i == start {
                                return None;
                            }
                            &src[start..i]
                        }
                    };
                    value = entities::decode_all(raw, true);
                }
                tag.attrs.push((name, value));
            }
            _ => return None,
        }
    }
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, refs: &'a Refs) -> Self {
        Parser {
            src,
            pos: 0,
            refs,
            nodes: Vec::new(),
            head: NONE,
            tail: NONE,
            delimiters: Vec::new(),
            top: NONE,
            brackets: Vec::new(),
            inactive: 0,
            backticks: None,
            unclosed: Unclosed::new(),
        }
    }

    fn append(&mut self, piece: Piece) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            piece,
            prev: self.tail,
            next: NONE,
        });
        if self.tail == NONE {
            self.head = id;
        } else {
            self.nodes[self.tail].next = id;
        }
        self.tail = id;
        id
    }

    fn text(&mut self, text: &str) -> usize {
        self.append(Piece::Text(text.to_owned()))
    }

    fn insert_after(&mut self, at: usize, piece: Piece) {
        let next = self.nodes[at].next;
        let id = self.nodes.len();
        self.nodes.push(Node { piece, prev: at, next });
        self.nodes[at].next = id;
        if next == NONE {
            self.tail = id;
        } else {
            self.nodes[next].prev = id;
        }
    }

    fn insert_before(&mut self, at: usize, piece: Piece) {
        let prev = self.nodes[at].prev;
        let id = self.nodes.len();
        self.nodes.push(Node { piece, prev, next: at });
        self.nodes[at].prev = id;
        if prev == NONE {
            self.head = id;
        } else {
            self.nodes[prev].next = id;
        }
    }

    fn unlink(&mut self, id: usize) {
        let (prev, next) = (self.nodes[id].prev, self.nodes[id].next);
        if prev == NONE {
            self.head = next;
        } else {
            self.nodes[prev].next = next;
        }
        if next == NONE {
            self.tail = prev;
        } else {
            self.nodes[next].prev = prev;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn run(&mut self) {
        let src = self.src;
        let b = src.as_bytes();
        while self.pos < b.len() {
            match b[self.pos] {
                b'\n' => self.newline(),
                b'\\' => self.backslash(),
                b'`' => self.code_span(),
                c @ (b'*' | b'_' | b'~') => self.delimiter_run(c),
                b'[' => {
                    let node = self.text("[");
                    self.push_bracket(node, self.pos, false);
                    self.pos += 1;
                }
                b'!' if b.get(self.pos + 1) == Some(&b'[') => {
                    let node = self.text("![");
                    self.push_bracket(node, self.pos + 1, true);
                    self.pos += 2;
                }
                b'!' => {
                    self.text("!");
                    self.pos += 1;
                }
                b']' => self.close_bracket(),
                b'<' => self.angle(),
                b'&' => self.entity(),
                _ => {
                    let end = b[self.pos..].iter().position(|&c| is_special(c)).map_or(b.len(), |i| self.pos + i);
                    self.text(&src[self.pos..end]);
                    self.pos = end;
                }
            }
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
    }

    /// Spaces or tabs, with at most one line ending among them.
    fn skip_spnl(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'\n') {
            self.pos += 1;
            while matches!(self.peek(), Some(b' ' | b'\t')) {
                self.pos += 1;
            }
        }
    }

    /// Only spaces or tabs to the end of the line; if so, moves past it.
    fn at_line_end(&mut self) -> bool {
        let mut i = self.pos;
        let b = self.src.as_bytes();
        while matches!(b.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        match b.get(i) {
            None => self.pos = i,
            Some(b'\n') => self.pos = i + 1,
            Some(_) => return false,
        }
        true
    }

    /// A line ending: hard after two or more spaces, otherwise soft.
    fn newline(&mut self) {
        self.pos += 1;
        let mut hard = false;
        if let Some(Node {
            piece: Piece::Text(text),
            ..
        }) = self.nodes.get_mut(self.tail)
        {
            if text.ends_with(' ') {
                hard = text.ends_with("  ");
                let kept = text.trim_end_matches(' ').len();
                text.truncate(kept);
            }
        }
        self.append(if hard { Piece::HardBreak } else { Piece::SoftBreak });
        self.skip_spaces();
    }

    fn backslash(&mut self) {
        match self.src.as_bytes().get(self.pos + 1) {
            Some(b'\n') => {
                self.pos += 2;
                self.append(Piece::HardBreak);
                self.skip_spaces();
            }
            Some(&c) if c.is_ascii_punctuation() => {
                self.append(Piece::Text((c as char).to_string()));
                self.pos += 2;
            }
            _ => {
                self.text("\\");
                self.pos += 1;
            }
        }
    }

    fn code_span(&mut self) {
        let b = self.src.as_bytes();
        let start = self.pos;
        let n = b[start..].iter().take_while(|&&c| c == b'`').count();
        let open_end = start + n;
        match self.closing_backticks(n, open_end) {
            Some(close) => {
                let mut code = self.src[open_end..close].replace('\n', " ");
                if code.len() >= 2 && code.starts_with(' ') && code.ends_with(' ') && code.bytes().any(|c| c != b' ') {
                    code = code[1..code.len() - 1].to_owned();
                }
                self.append(Piece::Code(code));
                self.pos = close + n;
            }
            None => {
                self.append(Piece::Text("`".repeat(n)));
                self.pos = open_end;
            }
        }
    }

    /// The first run of exactly `n` backticks at or after `from`.
    fn closing_backticks(&mut self, n: usize, from: usize) -> Option<usize> {
        let src = self.src;
        let runs = self.backticks.get_or_insert_with(|| {
            let b = src.as_bytes();
            let mut runs: HashMap<usize, Vec<usize>> = HashMap::new();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'`' {
                    let len = b[i..].iter().take_while(|&&c| c == b'`').count();
                    runs.entry(len).or_default().push(i);
                    i += len;
                } else {
                    i += 1;
                }
            }
            runs
        });
        let starts = runs.get(&n)?;
        starts.get(starts.partition_point(|&p| p < from)).copied()
    }

    fn delimiter_run(&mut self, c: u8) {
        let src = self.src;
        let start = self.pos;
        let n = src.as_bytes()[start..].iter().take_while(|&&x| x == c).count();
        let end = start + n;
        self.pos = end;
        let node = self.text(&src[start..end]);
        if c == b'~' && n > 2 {
            return;
        }
        let before = src[..start].chars().next_back().unwrap_or('\n');
        let after = src[end..].chars().next().unwrap_or('\n');
        let (before_space, after_space) = (before.is_whitespace(), after.is_whitespace());
        let (before_punct, after_punct) = (is_punctuation(before), is_punctuation(after));
        let left = !after_space && (!after_punct || before_space || before_punct);
        let right = !before_space && (!before_punct || after_space || after_punct);
        let (can_open, can_close) = if c == b'_' {
            (left && (!right || before_punct), right && (!left || after_punct))
        } else {
            (left, right)
        };
        if !can_open && !can_close {
            return;
        }
        let id = self.delimiters.len();
        self.delimiters.push(Delimiter {
            node,
            ch: c,
            count: n,
            original: n,
            can_open,
            can_close,
            prev: self.top,
            next: NONE,
        });
        if self.top != NONE {
            self.delimiters[self.top].next = id;
        }
        self.top = id;
    }

    fn remove_delimiter(&mut self, id: usize) {
        let (prev, next) = (self.delimiters[id].prev, self.delimiters[id].next);
        if prev != NONE {
            self.delimiters[prev].next = next;
        }
        if next == NONE {
            self.top = prev;
        } else {
            self.delimiters[next].prev = prev;
        }
    }

    fn push_bracket(&mut self, node: usize, at: usize, image: bool) {
        if let Some(last) = self.brackets.last_mut() {
            last.bracket_after = true;
        }
        self.brackets.push(Bracket {
            node,
            at,
            image,
            active: true,
            bracket_after: false,
            delimiter: self.top,
        });
    }

    fn pop_bracket(&mut self) -> Option<Bracket> {
        let bracket = self.brackets.pop();
        self.inactive = self.inactive.min(self.brackets.len());
        bracket
    }

    fn close_bracket(&mut self) {
        self.pos += 1;
        let after = self.pos;
        let Some(opener) = self.brackets.last() else {
            self.text("]");
            return;
        };
        if !opener.active {
            self.pop_bracket();
            self.text("]");
            return;
        }
        let (image, at, bracket_after) = (opener.image, opener.at, opener.bracket_after);
        let src = self.src;
        let mut destination = None;
        if self.peek() == Some(b'(') {
            self.pos += 1;
            destination = self.inline_destination();
            if destination.is_none() {
                self.pos = after;
            }
        }
        if destination.is_none() {
            // [text][label], [text][] or [text].
            let label_len = link_label(&src[self.pos..]);
            let label = if label_len > 2 {
                Some(&src[self.pos..self.pos + label_len])
            } else if !bracket_after {
                Some(&src[at..after])
            } else {
                None
            };
            self.pos += label_len;
            destination = label.and_then(|label| self.refs.get(&normalize_label(label))).cloned();
        }
        let Some(destination) = destination else {
            self.pop_bracket();
            self.pos = after;
            self.text("]");
            return;
        };
        let Some(opener) = self.pop_bracket() else {
            return;
        };
        let span = if image { Span::Image } else { Span::Link };
        self.nodes[opener.node].piece = Piece::Start(span, destination);
        self.append(Piece::End(span));
        self.process_emphasis(opener.delimiter);
        if !image {
            // No links inside links.
            for bracket in &mut self.brackets[self.inactive..] {
                if !bracket.image {
                    bracket.active = false;
                }
            }
            self.inactive = self.brackets.len();
        }
    }

    /// After `](`: destination, optional title and `)`.
    fn inline_destination(&mut self) -> Option<String> {
        self.skip_spnl();
        let destination = self.link_destination()?;
        let before_title = self.pos;
        self.skip_spnl();
        if self.pos > before_title {
            self.link_title();
        }
        self.skip_spnl();
        if self.peek() == Some(b')') {
            self.pos += 1;
            Some(destination)
        } else {
            None
        }
    }

    /// `<...>`, or text without spaces and with balanced parentheses.
    fn link_destination(&mut self) -> Option<String> {
        let b = self.src.as_bytes();
        if self.peek() == Some(b'<') {
            let mut i = self.pos + 1;
            loop {
                match b.get(i) {
                    Some(b'>') => {
                        let destination = unescape(&self.src[self.pos + 1..i]);
                        self.pos = i + 1;
                        return Some(destination);
                    }
                    Some(b'\\') if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => i += 2,
                    Some(b'<' | b'\n') | None => return None,
                    Some(_) => i += 1,
                }
            }
        }
        let start = self.pos;
        let mut depth = 0;
        let mut i = start;
        while let Some(&c) = b.get(i) {
            match c {
                b'\\' if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) => i += 2,
                b'(' => {
                    depth += 1;
                    if depth > MAX_PARENS {
                        return None;
                    }
                    i += 1;
                }
                b')' if depth == 0 => break,
                b')' => {
                    depth -= 1;
                    i += 1;
                }
                c if c <= b' ' || c == 0x7F => break,
                _ => i += 1,
            }
        }
        if (i == start && b.get(i) != Some(&b')')) || depth != 0 {
            return None;
        }
        self.pos = i;
        Some(unescape(&self.src[start..i]))
    }

    /// `"..."`, `'...'` or `(...)`.
    fn link_title(&mut self) -> Option<String> {
        let b = self.src.as_bytes();
        let open = self.peek()?;
        let (close, slot) = match open {
            b'"' => (b'"', DOUBLE_QUOTE),
            b'\'' => (b'\'', SINGLE_QUOTE),
            b'(' => (b')', TITLE),
            _ => return None,
        };
        if self.pos >= self.unclosed.0[slot] {
            return None;
        }
        let mut i = self.pos + 1;
        while let Some(&c) = b.get(i) {
            if c == b'\\' && b.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
                i += 2;
            } else if c == close {
                let title = unescape(&self.src[self.pos + 1..i]);
                self.pos = i + 1;
                return Some(title);
            } else if open == b'(' && c == b'(' {
                return None;
            } else {
                i += 1;
            }
        }
        self.unclosed.0[slot] = self.pos;
        None
    }

    fn angle(&mut self) {
        let src = self.src;
        if let Some((len, destination)) = autolink(&src[self.pos..]) {
            self.append(Piece::Start(Span::Link, destination));
            self.text(&src[self.pos + 1..self.pos + len - 1]);
            self.append(Piece::End(Span::Link));
            self.pos += len;
        } else if let Some((len, tag)) = raw_html(src, self.pos, &mut self.unclosed) {
            if let Some(tag) = tag {
                self.append(Piece::Html(tag));
            }
            self.pos += len;
        } else {
            self.text("<");
            self.pos += 1;
        }
    }

    fn entity(&mut self) {
        match entities::decode_strict(&self.src[self.pos..]) {
            Some((c, used)) => {
                self.append(Piece::Text(c.to_string()));
                self.pos += used;
            }
            None => {
                self.text("&");
                self.pos += 1;
            }
        }
    }

    /// Match the delimiters above `bottom` into emphasis, strong emphasis
    /// and strikethrough, then drop them from the stack.
    fn process_emphasis(&mut self, bottom: usize) {
        if self.top == bottom {
            return;
        }
        // Where the search for an opener stopped last time, by delimiter
        // kind, so that runs of unmatched closers stay linear.
        let mut openers_bottom = [bottom; 16];
        let mut closer = self.top;
        while closer != NONE && self.delimiters[closer].prev != bottom {
            closer = self.delimiters[closer].prev;
        }
        while closer != NONE {
            let c = &self.delimiters[closer];
            if !c.can_close {
                closer = c.next;
                continue;
            }
            let slot = match c.ch {
                b'_' => usize::from(c.can_open) * 3 + c.original % 3,
                b'*' => 6 + usize::from(c.can_open) * 3 + c.original % 3,
                _ => 12 + c.original.min(3),
            };
            let mut opener = c.prev;
            let mut found = false;
            while opener != NONE && opener != bottom && opener != openers_bottom[slot] {
                let o = &self.delimiters[opener];
                found = if c.ch == b'~' {
                    o.ch == b'~' && o.can_open && o.count == c.count
                } else {
                    let odd = (c.can_open || o.can_close)
                        && !c.original.is_multiple_of(3)
                        && (o.original + c.original).is_multiple_of(3);
                    o.ch == c.ch && o.can_open && !odd
                };
                if found {
                    break;
                }
                opener = o.prev;
            }
            if !found {
                openers_bottom[slot] = c.prev;
                let next = c.next;
                if !c.can_open {
                    self.remove_delimiter(closer);
                }
                closer = next;
                continue;
            }
            let (opener_count, closer_count) = (self.delimiters[opener].count, self.delimiters[closer].count);
            let ch = self.delimiters[closer].ch;
            let (used, span) = match ch {
                b'~' => (closer_count, Span::Strike),
                _ if opener_count >= 2 && closer_count >= 2 => (2, Span::Strong),
                _ => (1, Span::Emphasis),
            };
            let (opener_node, closer_node) = (self.delimiters[opener].node, self.delimiters[closer].node);
            for node in [opener_node, closer_node] {
                if let Piece::Text(text) = &mut self.nodes[node].piece {
                    text.truncate(text.len().saturating_sub(used));
                }
            }
            self.delimiters[opener].count -= used;
            self.delimiters[closer].count -= used;
            self.insert_after(opener_node, Piece::Start(span, String::new()));
            self.insert_before(closer_node, Piece::End(span));
            // Delimiters between the two can no longer match anything.
            self.delimiters[opener].next = closer;
            self.delimiters[closer].prev = opener;
            if self.delimiters[opener].count == 0 {
                self.unlink(opener_node);
                self.remove_delimiter(opener);
            }
            if self.delimiters[closer].count == 0 {
                self.unlink(closer_node);
                let next = self.delimiters[closer].next;
                self.remove_delimiter(closer);
                closer = next;
            }
        }
        while self.top != NONE && self.top != bottom {
            self.remove_delimiter(self.top);
        }
    }

    /// Walk the pieces into runs.
    fn flatten(&self) -> Vec<Inline> {
        let mut out = Inlines::default();
        let mut state = State::default();
        let mut i = self.head;
        while i != NONE {
            let node = &self.nodes[i];
            match &node.piece {
                Piece::Text(text) => state.text(&mut out, text, false),
                Piece::Code(code) => state.text(&mut out, code, true),
                Piece::SoftBreak => state.text(&mut out, " ", false),
                Piece::HardBreak => state.line_break(&mut out),
                Piece::Html(tag) => state.html(&mut out, tag),
                Piece::Start(span, destination) => state.start(*span, destination),
                Piece::End(span) => state.end(&mut out, *span),
            }
            i = node.next;
        }
        out.finish()
    }
}

/// The styles in force while flattening.
#[derive(Default)]
struct State {
    strong: usize,
    emphasis: usize,
    strike: usize,
    /// Open links, innermost last; `None` for one going nowhere.
    links: Vec<Option<String>>,
    /// Images being read, innermost last: source and alt text so far.
    images: Vec<(String, String)>,
    /// Counts of open raw HTML tags: `<b>`, `<i>`, `<s>`, `<code>`.
    html_bold: usize,
    html_italic: usize,
    html_strike: usize,
    html_code: usize,
    /// Open `<a>` tags and their `href`s.
    html_links: Vec<Option<String>>,
}

impl State {
    fn style(&self, code: bool) -> Style {
        Style {
            bold: self.strong + self.html_bold > 0,
            italic: self.emphasis + self.html_italic > 0,
            code: code || self.html_code > 0,
            strike: self.strike + self.html_strike > 0,
            link: self.links.last().cloned().flatten().or_else(|| self.html_links.iter().rev().find_map(Clone::clone)),
        }
    }

    /// Markdown code spans keep their spaces; everything else collapses.
    fn text(&mut self, out: &mut Inlines, text: &str, code_span: bool) {
        if let Some((_, alt)) = self.images.last_mut() {
            alt.push_str(text);
        } else {
            out.text(text, &self.style(code_span), !code_span);
        }
    }

    fn line_break(&mut self, out: &mut Inlines) {
        if let Some((_, alt)) = self.images.last_mut() {
            alt.push(' ');
        } else {
            out.line_break();
        }
    }

    fn image(&mut self, out: &mut Inlines, alt: String, src: String) {
        if let Some((_, outer)) = self.images.last_mut() {
            outer.push_str(&alt);
        } else if !src.is_empty() || !alt.trim_ascii().is_empty() {
            // White space collapsed, as in HTML; `![]()` shows nothing.
            let alt = alt.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
            let link = self.style(false).link;
            out.image(alt, src, link);
        }
    }

    fn start(&mut self, span: Span, destination: &str) {
        match span {
            Span::Strong => self.strong += 1,
            Span::Emphasis => self.emphasis += 1,
            Span::Strike => self.strike += 1,
            Span::Link => self.links.push(link_target(destination)),
            Span::Image => self.images.push((destination.to_owned(), String::new())),
        }
    }

    fn end(&mut self, out: &mut Inlines, span: Span) {
        match span {
            Span::Strong => self.strong = self.strong.saturating_sub(1),
            Span::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            Span::Strike => self.strike = self.strike.saturating_sub(1),
            Span::Link => {
                self.links.pop();
            }
            Span::Image => {
                if let Some((src, alt)) = self.images.pop() {
                    self.image(out, alt, src);
                }
            }
        }
    }

    /// Raw HTML: the tags with a meaning here change the style; the rest
    /// are dropped, keeping the text between them.
    fn html(&mut self, out: &mut Inlines, tag: &Tag) {
        let count = |n: &mut usize| {
            if tag.closing {
                *n = n.saturating_sub(1);
            } else if !tag.self_closing {
                *n += 1;
            }
        };
        match tag.name.as_str() {
            "br" => self.line_break(out),
            "img" if !tag.closing => {
                let src = tag.attr("src").unwrap_or_default().to_owned();
                let alt = tag.attr("alt").unwrap_or_default().to_owned();
                if !src.is_empty() || !alt.is_empty() {
                    self.image(out, alt, src);
                }
            }
            "b" | "strong" => count(&mut self.html_bold),
            "i" | "em" | "cite" | "var" | "dfn" => count(&mut self.html_italic),
            "s" | "strike" | "del" => count(&mut self.html_strike),
            "code" | "kbd" | "tt" | "samp" => count(&mut self.html_code),
            "a" if tag.closing => {
                self.html_links.pop();
            }
            "a" if !tag.self_closing => self.html_links.push(tag.attr("href").and_then(link_target)),
            _ => {}
        }
    }
}

/// GitHub's extended autolinks: `http://`, `https://`, `ftp://` and `www.`
/// addresses and email addresses in plain text become links.
fn bare_links(runs: Vec<Inline>) -> Vec<Inline> {
    let plain = |style: &Style| style.link.is_none() && !style.code;
    let any = runs.iter().any(|run| {
        matches!(run, Inline::Text { text, style }
            if plain(style) && (text.contains("://") || text.contains("www.") || text.contains('@')))
    });
    if !any {
        return runs;
    }
    let mut out = Vec::with_capacity(runs.len() + 2);
    for run in runs {
        match run {
            Inline::Text { text, style } if plain(&style) => split_links(&text, &style, &mut out),
            // A link found may run into a link written next to it.
            Inline::Text { text, style } => push_run(&mut out, &text, style),
            other => out.push(other),
        }
    }
    out
}

fn push_run(out: &mut Vec<Inline>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text { text: last, style: last_style }) = out.last_mut() {
        if *last_style == style {
            last.push_str(text);
            return;
        }
    }
    out.push(Inline::Text {
        text: text.to_owned(),
        style,
    });
}

fn split_links(text: &str, style: &Style, out: &mut Vec<Inline>) {
    let mut emitted = 0;
    let mut i = 0;
    while i < text.len() {
        match link_at(text, emitted, i) {
            Ok((start, end, destination)) => {
                push_run(out, &text[emitted..start], style.clone());
                let linked = Style {
                    link: Some(destination),
                    ..style.clone()
                };
                push_run(out, &text[start..end], linked);
                i = end;
                emitted = end;
            }
            Err(skip) => i += skip,
        }
    }
    push_run(out, &text[emitted..], style.clone());
}

/// A bare address starting at byte `at`, or an email address around the
/// `@` there, reaching back no further than `from`: where it starts and
/// ends, and its destination. Otherwise how far to move on.
fn link_at(text: &str, from: usize, at: usize) -> Result<(usize, usize, String), usize> {
    match text.as_bytes()[at] {
        b'h' | b'H' | b'w' | b'f' | b'F' => {
            let before = text[..at].chars().next_back();
            if !before.is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '(')) {
                return Err(1);
            }
            url_at(&text[at..]).map(|(len, destination)| (at, at + len, destination))
        }
        b'@' => email_at(text, from, at).ok_or(1),
        _ => Err(1),
    }
}

/// The email address around the `@` at `at`, starting no earlier than
/// `from`, by GitHub's rules: letters, digits and `.-_+` before the `@`,
/// then a domain with at least one dot, not ending in `-` or `_`. Returns
/// where it starts and ends and its `mailto:` destination.
fn email_at(text: &str, from: usize, at: usize) -> Option<(usize, usize, String)> {
    let b = text.as_bytes();
    let mut start = at;
    while start > from && (b[start - 1].is_ascii_alphanumeric() || b".-_+".contains(&b[start - 1])) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < b.len() && (b[end].is_ascii_alphanumeric() || b".-_".contains(&b[end])) {
        end += 1;
    }
    // A full stop after the address ends the sentence instead.
    while end > at + 1 && b[end - 1] == b'.' {
        end -= 1;
    }
    let domain = &text[at + 1..end];
    let valid = start < at
        && domain.contains('.')
        && !domain.split('.').any(str::is_empty)
        && !domain.ends_with(['-', '_']);
    valid.then(|| (start, end, format!("mailto:{}", &text[start..end])))
}

/// A bare address at the start of `s`: its length, after GitHub's trimming
/// of trailing punctuation, and its destination. When `s` starts like an
/// address but is not a valid one, the error is the length of the run of
/// text it starts, in which no other address is then looked for; this
/// keeps text full of almost-addresses linear.
fn url_at(s: &str) -> Result<(usize, String), usize> {
    let b = s.as_bytes();
    let starts = |prefix: &str| b.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix.as_bytes()));
    let (scheme, www) = if starts("https://") {
        (8, false)
    } else if starts("http://") {
        (7, false)
    } else if starts("ftp://") {
        (6, false)
    } else if s.starts_with("www.") {
        (0, true)
    } else {
        return Err(1);
    };
    let run = s[scheme..].find(|c: char| c.is_whitespace() || c == '<').map_or(s.len(), |i| scheme + i);
    let opens = s[..run].bytes().filter(|&c| c == b'(').count();
    let mut closes = s[..run].bytes().filter(|&c| c == b')').count();
    let mut end = run;
    loop {
        let url = &s[..end];
        match url.as_bytes().last() {
            Some(b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' | b'\'' | b'"') => end -= 1,
            Some(b')') if closes > opens => {
                closes -= 1;
                end -= 1;
            }
            Some(b';') => {
                // An entity-like ending such as `&amp;` is not part of it.
                let entity = |&a: &usize| {
                    a + 2 < url.len() && url[a + 1..url.len() - 1].bytes().all(|c| c.is_ascii_alphanumeric())
                };
                match url.rfind('&').filter(entity) {
                    Some(a) => end = a,
                    None => break,
                }
            }
            _ => break,
        }
    }
    let url = &s[..end];
    let host_end = url
        .get(scheme..)
        .and_then(|rest| rest.find(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))))
        .map_or(url.len(), |i| scheme + i);
    let host = url.get(scheme..host_end).unwrap_or("");
    let segments: Vec<&str> = host.split('.').collect();
    let valid = !host.is_empty()
        && (!www || (segments.len() >= 2 && segments.iter().all(|s| !s.is_empty())))
        && segments.iter().rev().take(2).all(|s| !s.contains('_'));
    if !valid {
        return Err(run.max(1));
    }
    let destination = if www { format!("http://{url}") } else { url.to_owned() };
    Ok((end, destination))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::tests::show;

    fn runs(text: &str) -> Vec<Inline> {
        parse(text, &Refs::new())
    }

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

    fn b(text: &str) -> Inline {
        styled(text, |s| s.bold = true)
    }

    fn i(text: &str) -> Inline {
        styled(text, |s| s.italic = true)
    }

    fn code(text: &str) -> Inline {
        styled(text, |s| s.code = true)
    }

    fn link(text: &str, to: &str) -> Inline {
        styled(text, |s| s.link = Some(to.into()))
    }

    #[test]
    fn emphasis_and_strong() {
        assert_eq!(runs("*a* _b_"), [i("a"), t(" "), i("b")]);
        assert_eq!(runs("**a** __b__"), [b("a"), t(" "), b("b")]);
        assert_eq!(runs("***a***"), [styled("a", |s| (s.bold, s.italic) = (true, true))]);
        assert_eq!(runs("*a **b** c*"), [i("a "), styled("b", |s| (s.bold, s.italic) = (true, true)), i(" c")]);
        assert_eq!(runs("**a *b* c**"), [b("a "), styled("b", |s| (s.bold, s.italic) = (true, true)), b(" c")]);
        // Not flanking, or intraword underscores.
        assert_eq!(runs("a * b *"), [t("a * b *")]);
        assert_eq!(runs("snake_case_name"), [t("snake_case_name")]);
        assert_eq!(runs("a*b*c"), [t("a"), i("b"), t("c")]);
        assert_eq!(runs("*(*a*)*"), [i("(a)")]);
        // The rule of three, and unmatched runs.
        assert_eq!(runs("*foo**bar**baz*"), [i("foo"), styled("bar", |s| (s.bold, s.italic) = (true, true)), i("baz")]);
        assert_eq!(runs("**foo*"), [t("*"), i("foo")]);
        assert_eq!(runs("*foo**"), [i("foo"), t("*")]);
        assert_eq!(runs("__foo, __bar__, baz__"), [b("foo, bar, baz")]);
        assert_eq!(runs("foo*"), [t("foo*")]);
        assert_eq!(runs("*"), [t("*")]);
    }

    #[test]
    fn strikethrough() {
        assert_eq!(runs("~~gone~~ ~also~"), [styled("gone", |s| s.strike = true), t(" "), styled("also", |s| s.strike = true)]);
        assert_eq!(runs("~~~not~~~"), [t("~~~not~~~")]);
        assert_eq!(runs("~~a~"), [t("~~a~")]);
        assert_eq!(runs("**~~both~~**"), [styled("both", |s| (s.bold, s.strike) = (true, true))]);
    }

    #[test]
    fn code_spans() {
        assert_eq!(runs("`a`"), [code("a")]);
        assert_eq!(runs("`` a ` b ``"), [code("a ` b")]);
        assert_eq!(runs("` `` `"), [code("``")]);
        assert_eq!(runs("`  `"), [code("  ")]);
        assert_eq!(runs("`a\nb`"), [code("a b")]);
        assert_eq!(runs("`*not*` *yes*"), [code("*not*"), t(" "), i("yes")]);
        assert_eq!(runs("```a``"), [t("```a``")]);
        assert_eq!(runs("`foo\\`bar`"), [code("foo\\"), t("bar`")]);
        assert_eq!(runs("**a `b** c`"), [t("**a "), code("b** c")]);
    }

    #[test]
    fn escapes_entities_and_breaks() {
        assert_eq!(runs("\\*not\\* \\a"), [t("*not* \\a")]);
        assert_eq!(runs("&copy; &amp;&#35;&#x41; &nope; &copy"), [t("© &#A &nope; &copy")]);
        assert_eq!(runs("&ast;a&ast;"), [t("*a*")]);
        assert_eq!(runs("a  \nb\\\nc\nd"), [t("a"), Inline::Break, t("b"), Inline::Break, t("c d")]);
        assert_eq!(runs("a    b"), [t("a b")]);
        assert_eq!(runs("a\n   b"), [t("a b")]);
    }

    #[test]
    fn inline_links_and_images() {
        assert_eq!(runs("[a](/u)"), [link("a", "/u")]);
        assert_eq!(runs("[a](</my u> \"t\")"), [link("a", "/my u")]);
        assert_eq!(runs("[a](/u 'title' )x"), [link("a", "/u"), t("x")]);
        assert_eq!(runs("[a](foo(and(bar)))"), [link("a", "foo(and(bar))")]);
        // Links to nowhere read as plain text.
        assert_eq!(runs("[a]() [b](<>) [c](javascript:alert(1)) <JavaScript:x>"), [t("a b c JavaScript:x")]);
        assert_eq!(runs("[a](/u\\)x)"), [link("a", "/u)x")]);
        assert_eq!(runs("[a] (/u)"), [t("[a] (/u)")]);
        assert_eq!(runs("[**b** c](/u)"), [styled("b", |s| (s.bold, s.link) = (true, Some("/u".into()))), link(" c", "/u")]);
        assert_eq!(runs("[a [b](/1)](/2)"), [t("[a "), link("b", "/1"), t("](/2)")]);
        assert_eq!(runs("*[a*](/u)"), [t("*"), link("a*", "/u")]);
        assert_eq!(
            runs("![alt *text*](/i.png) and [![badge](/b.svg)](/ci)"),
            [
                Inline::Image { alt: "alt text".into(), src: "/i.png".into(), link: None },
                t(" and "),
                Inline::Image { alt: "badge".into(), src: "/b.svg".into(), link: Some("/ci".into()) },
            ]
        );
        assert_eq!(runs("![a ![b](/x)](/y)"), [Inline::Image { alt: "a b".into(), src: "/y".into(), link: None }]);
    }

    #[test]
    fn reference_links() {
        let mut refs = Refs::new();
        for def in ["[foo]: /url \"title\"", "[Bar Baz]:\n  <my url>", "[FOO]: /ignored", "[ẞ]: /ss"] {
            assert!(parse_reference(def, &mut refs) > 0, "{def}");
        }
        assert_eq!(parse_reference("[x]: /u \"t\" junk", &mut refs), 0);
        assert_eq!(parse_reference("[x]:", &mut refs), 0);
        assert_eq!(parse_reference("[]: /u", &mut refs), 0);
        assert_eq!(parse_reference("[y]: /u\n\"t\"\nmore", &mut refs), "[y]: /u\n\"t\"\n".len());
        assert_eq!(refs["foo"], "/url");
        let p = |s: &str| parse(s, &refs);
        assert_eq!(p("[foo] [Foo][] [x][foo] [bar   baz] [SS]"), [
            link("foo", "/url"),
            t(" "),
            link("Foo", "/url"),
            t(" "),
            link("x", "/url"),
            t(" "),
            link("bar baz", "my url"),
            t(" "),
            link("SS", "/ss"),
        ]);
        assert_eq!(p("[foo][nope] [nope]"), [t("[foo][nope] [nope]")]);
        assert_eq!(p("![foo]"), [Inline::Image { alt: "foo".into(), src: "/url".into(), link: None }]);
    }

    #[test]
    fn autolinks_and_bare_links() {
        assert_eq!(runs("<https://a.b/c?d>"), [link("https://a.b/c?d", "https://a.b/c?d")]);
        assert_eq!(runs("<me@example.com>"), [link("me@example.com", "mailto:me@example.com")]);
        assert_eq!(runs("<m:abc> <a b>"), [t("<m:abc>")]);
        assert_eq!(
            runs("See https://example.com/a_b. Or (www.example.org/x)!"),
            [
                t("See "),
                link("https://example.com/a_b", "https://example.com/a_b"),
                t(". Or ("),
                link("www.example.org/x", "http://www.example.org/x"),
                t(")!"),
            ]
        );
        assert_eq!(runs("http://x.org/a&amp;b; www.a_b.c_d"), [link("http://x.org/a", "http://x.org/a"), t("&b; www.a_b.c_d")]);
        assert_eq!(runs("`http://code.org` xhttp://no.pe www."), [code("http://code.org"), t(" xhttp://no.pe www.")]);
        assert_eq!(runs("[t](/u) http://localhost:8080/"), [link("t", "/u"), t(" "), link("http://localhost:8080/", "http://localhost:8080/")]);
    }

    #[test]
    fn raw_html() {
        assert_eq!(runs("a <b>bold</b> <!-- gone --> <kbd>Ctrl</kbd>"), [t("a "), b("bold"), t(" "), code("Ctrl")]);
        assert_eq!(runs("x<br>y<br/>z"), [t("x"), Inline::Break, t("y"), Inline::Break, t("z")]);
        assert_eq!(
            runs("<a href=\"/u?a=1&amp;b=2\">go</a> <img src=x.png alt='X'>"),
            [link("go", "/u?a=1&b=2"), t(" "), Inline::Image { alt: "X".into(), src: "x.png".into(), link: None }]
        );
        assert_eq!(runs("a < b > c <3 <!-- open"), [t("a < b > c <3 <!-- open")]);
        assert_eq!(runs("Vec<T> <unknown attr=1>"), [t("Vec")]);
        assert_eq!(runs("<a b='unclosed"), [t("<a b='unclosed")]);
    }

    #[test]
    fn labels_and_unescaping() {
        assert_eq!(normalize_label("[ Foo \n BAR ]"), "foo bar");
        assert_eq!(link_label("[a\\]b] c"), 6);
        assert_eq!(link_label("[a[b]"), 0);
        assert_eq!(link_label(&format!("[{}]", "x".repeat(1000))), 0);
        assert_eq!(unescape("a\\*b &amp; \\q"), "a*b & \\q");
    }

    /// Each input with its runs written out by [`show`].
    fn cases(refs: &Refs, cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(show(&parse(input, refs)), *expected, "{input:?}");
        }
    }

    #[test]
    fn emphasis_by_the_flanking_rules() {
        cases(&Refs::new(), &[
            ("*foo bar*", "<i>foo bar</i>"),
            ("a * foo bar*", "a * foo bar*"),
            ("a*\"foo\"*", "a*\"foo\"*"),
            ("* a *", "* a *"),
            ("foo*bar*", "foo<i>bar</i>"),
            ("5*6*78", "5<i>6</i>78"),
            ("_foo bar_", "<i>foo bar</i>"),
            ("_ foo bar_", "_ foo bar_"),
            ("a_\"foo\"_", "a_\"foo\"_"),
            ("foo_bar_", "foo_bar_"),
            ("5_6_78", "5_6_78"),
            ("пристаням_стремятся_", "пристаням_стремятся_"),
            ("aa_\"bb\"_cc", "aa_\"bb\"_cc"),
            ("foo-_(bar)_", "foo-<i>(bar)</i>"),
            ("_foo*", "_foo*"),
            ("*foo bar *", "*foo bar *"),
            ("*(*foo)", "*(*foo)"),
            ("*(*foo*)*", "<i>(foo)</i>"),
            ("*foo*bar", "<i>foo</i>bar"),
            ("_foo bar _", "_foo bar _"),
            ("_(_foo)", "_(_foo)"),
            ("_(_foo_)_", "<i>(foo)</i>"),
            ("_foo_bar", "_foo_bar"),
            ("_пристаням_стремятся", "_пристаням_стремятся"),
            ("_foo_bar_baz_", "<i>foo_bar_baz</i>"),
            ("_(bar)_.", "<i>(bar)</i>."),
            ("**foo bar**", "<b>foo bar</b>"),
            ("** foo bar**", "** foo bar**"),
            ("a**\"foo\"**", "a**\"foo\"**"),
            ("foo**bar**", "foo<b>bar</b>"),
            ("__foo bar__", "<b>foo bar</b>"),
            ("__ foo bar__", "__ foo bar__"),
            ("__\nfoo bar__", "__ foo bar__"),
            ("foo__bar__", "foo__bar__"),
            ("foo-__(bar)__", "foo-<b>(bar)</b>"),
            ("**foo bar **", "**foo bar **"),
            ("**(**foo)", "**(**foo)"),
            ("*(**foo**)*", "<i>(</i><b><i>foo</i></b><i>)</i>"),
            (
                "**Gomphocarpus (*Gomphocarpus physocarpus*, syn.\n*Asclepias physocarpa*)**",
                "<b>Gomphocarpus (</b><b><i>Gomphocarpus physocarpus</i></b><b>, syn. </b>\
                 <b><i>Asclepias physocarpa</i></b><b>)</b>",
            ),
            ("**foo \"*bar*\" foo**", "<b>foo \"</b><b><i>bar</i></b><b>\" foo</b>"),
            ("**foo**bar", "<b>foo</b>bar"),
            ("__foo__bar", "__foo__bar"),
            ("__foo__bar__baz__", "<b>foo__bar__baz</b>"),
            ("*foo\nbar*", "<i>foo bar</i>"),
            ("_foo __bar__ baz_", "<i>foo </i><b><i>bar</i></b><i> baz</i>"),
            ("*foo *bar**", "<i>foo bar</i>"),
            ("*foo**bar*", "<i>foo**bar</i>"),
            ("***foo** bar*", "<b><i>foo</i></b><i> bar</i>"),
            ("*foo **bar***", "<i>foo </i><b><i>bar</i></b>"),
            ("foo***bar***baz", "foo<b><i>bar</i></b>baz"),
            ("foo******bar*********baz", "foo<b>bar</b>***baz"),
            ("*foo **bar *baz* bim** bop*", "<i>foo </i><b><i>bar baz bim</i></b><i> bop</i>"),
            ("** is not an empty emphasis", "** is not an empty emphasis"),
            ("**** is not an empty strong emphasis", "**** is not an empty strong emphasis"),
            ("**foo\nbar**", "<b>foo bar</b>"),
            ("foo *\\**", "foo <i>*</i>"),
            ("***foo**", "*<b>foo</b>"),
            ("****foo*", "***<i>foo</i>"),
            ("**foo***", "<b>foo</b>*"),
            ("*foo****", "<i>foo</i>***"),
            ("_____foo_____", "<b><i>foo</i></b>"),
            ("*foo _bar* baz_", "<i>foo _bar</i> baz_"),
            ("*foo __bar *baz bim__ bam*", "<i>foo </i><b><i>bar *baz bim</i></b><i> bam</i>"),
            ("**foo **bar baz**", "**foo <b>bar baz</b>"),
            ("*foo *bar baz*", "*foo <i>bar baz</i>"),
            ("*a `*`*", "<i>a </i><i><c>*</c></i>"),
            ("_a `_`_", "<i>a </i><i><c>_</c></i>"),
            ("**a<https://foo.bar/?q=**>", "**a<a https://foo.bar/?q=**>https://foo.bar/?q=**</a>"),
            // Punctuation and symbols outside ASCII flank like ASCII's.
            ("«*quoted*»", "«<i>quoted</i>»"),
            ("€*5*", "€<i>5</i>"),
        ]);
    }

    #[test]
    fn strikethrough_with_one_or_two_tildes() {
        cases(&Refs::new(), &[
            ("~~Hi~~ Hello, ~there~ world!", "<s>Hi</s> Hello, <s>there</s> world!"),
            ("a~~b~~c", "a<s>b</s>c"),
            ("~~ not ~~", "~~ not ~~"),
            ("~~one~ two~~", "<s>one~ two</s>"),
            ("~~unclosed", "~~unclosed"),
            ("~~**strong**~~ *~~em~~*", "<b><s>strong</s></b> <i><s>em</s></i>"),
            ("~~a\nb~~", "<s>a b</s>"),
        ]);
    }

    #[test]
    fn code_spans_keep_their_text() {
        cases(&Refs::new(), &[
            ("`foo`", "<c>foo</c>"),
            ("`` foo ` bar ``", "<c>foo ` bar</c>"),
            ("`  ``  `", "<c> `` </c>"),
            ("` a`", "<c> a</c>"),
            ("` b `", "<c>b</c>"),
            ("`\u{A0}b\u{A0}`", "<c>\u{A0}b\u{A0}</c>"),
            ("`foo   bar \nbaz`", "<c>foo   bar  baz</c>"),
            ("*foo`*`", "*foo<c>*</c>"),
            ("[not a `link](/foo`)", "[not a <c>link](/foo</c>)"),
            ("`<a href=\"`\">`", "<c><a href=\"</c>\">`"),
            ("`<https://foo.bar.`baz>`", "<c><https://foo.bar.</c>baz>`"),
            ("`foo", "`foo"),
            ("`foo``bar``", "`foo<c>bar</c>"),
            ("`&amp; \\* <b>`", "<c>&amp; \\* <b></c>"),
        ]);
    }

    #[test]
    fn inline_link_forms() {
        cases(&Refs::new(), &[
            ("[link](/uri \"title\")", "<a /uri>link</a>"),
            ("[link](/uri 'title')", "<a /uri>link</a>"),
            ("[link](/uri (title))", "<a /uri>link</a>"),
            ("[link](/my uri)", "[link](/my uri)"),
            ("[link](foo\nbar)", "[link](foo bar)"),
            ("[a](<b)c>)", "<a b)c>a</a>"),
            ("[link](<foo\\>)", "[link](<foo>)"),
            ("[link](\\(foo\\))", "<a (foo)>link</a>"),
            ("[link](foo(and(bar))", "[link](foo(and(bar))"),
            ("[link](#fragment) [m](mailto:a@b.c)", "<a #fragment>link</a> <a mailto:a@b.c>m</a>"),
            ("[link](\"title\")", "<a \"title\">link</a>"),
            ("[link](/url \"title \\\"&quot;\")", "<a /url>link</a>"),
            ("[link](   /uri\n  \"title\"  )", "<a /uri>link</a>"),
            ("[link [foo [bar]]](/uri)", "<a /uri>link [foo [bar]]</a>"),
            ("[link] bar](/uri)", "[link] bar](/uri)"),
            ("[link [bar](/uri)", "[link <a /uri>bar</a>"),
            ("[link \\[bar](/uri)", "<a /uri>link [bar</a>"),
            (
                "[link *foo **bar** `#`*](/uri)",
                "<a /uri>link </a><a /uri><i>foo </i></a><a /uri><b><i>bar</i></b></a><a /uri><i> </i></a>\
                 <a /uri><i><c>#</c></i></a>",
            ),
            ("[![moon](moon.jpg)](/uri)", "[![moon](moon.jpg)](/uri)"),
            ("[foo *[bar [baz](/uri)](/uri)*](/uri)", "[foo <i>[bar </i><a /uri><i>baz</i></a><i>](/uri)</i>](/uri)"),
            ("[foo *bar](baz*)", "<a baz*>foo *bar</a>"),
            ("*foo [bar* baz]", "<i>foo [bar</i> baz]"),
            ("[foo`](/uri)`", "[foo<c>](/uri)</c>"),
            ("[foo<https://example.com/?search=](uri)>", "[foo<a https://example.com/?search=](uri)>https://example.com/?search=](uri)</a>"),
            ("[link](/f&ouml;&ouml; \"f&ouml;&ouml;\")", "<a /föö>link</a>"),
            // Unicode and percent signs are kept as written.
            ("[link](foo%20b&auml;)", "<a foo%20bä>link</a>"),
        ]);
    }

    #[test]
    fn reference_link_forms() {
        let mut refs = Refs::new();
        for def in [
            "[foo]: /url \"title\"",
            "[ref]: /uri",
            "[bar]: /bar",
            "[foo!]: /bang",
            "[foo*]: /star",
            "[baz]: /baz",
            "[ΑΓΩ]: /φου",
            "[moon]: moon.jpg",
        ] {
            assert!(parse_reference(def, &mut refs) > 0, "{def}");
        }
        cases(&refs, &[
            ("[link [foo [qux]]][ref]", "<a /uri>link [foo [qux]]</a>"),
            // A link inside the text wins; the outer brackets stay text.
            ("[link [foo [bar]]][ref]", "[link [foo <a /bar>bar</a>]]<a /uri>ref</a>"),
            ("[link \\[bar][ref]", "<a /uri>link [bar</a>"),
            ("[Foo][BaR]", "<a /bar>Foo</a>"),
            ("[αγω]", "<a /φου>αγω</a>"),
            ("[foo] [bar]", "<a /url>foo</a> <a /bar>bar</a>"),
            ("[nope] [bar]", "[nope] <a /bar>bar</a>"),
            ("[foo][]", "<a /url>foo</a>"),
            ("[*foo* bar][]", "[<i>foo</i> bar][]"),
            ("[bar][foo\\!]", "[bar][foo!]"),
            ("[nope][ref[]", "[nope][ref[]"),
            ("[foo][ref[]", "<a /url>foo</a>[ref[]"),
            ("[[[foo]]]", "[[<a /url>foo</a>]]"),
            ("\\[foo]", "[foo]"),
            ("*[foo*]", "*<a /star>foo*</a>"),
            ("[foo][bar][baz]", "<a /bar>foo</a><a /baz>baz</a>"),
            ("[x][bar][baz]", "<a /bar>x</a><a /baz>baz</a>"),
            ("[foo](not a link)", "<a /url>foo</a>(not a link)"),
            ("[foo][ref] and [ref][foo]", "<a /uri>foo</a> and <a /url>ref</a>"),
            ("![moon]", "![moon](moon.jpg)"),
            ("![*moon* light][moon]", "![moon light](moon.jpg)"),
            ("\\![foo]", "!<a /url>foo</a>"),
            ("!\\[foo]", "![foo]"),
        ]);
        // The first definition of a label wins; labels match whatever their
        // case and white space.
        assert!(parse_reference("[FOO]: /second", &mut refs) > 0);
        assert_eq!(show(&parse("[ Foo\n]", &refs)), "<a /url>Foo</a>");
        // Definitions need a destination ending its line.
        assert_eq!(parse_reference("[x]: <bar>(baz)", &mut refs), 0);
        assert_eq!(parse_reference("[x]: /url \"title\" ok", &mut refs), 0);
        assert_eq!(parse_reference("[x]: /url\n\"title\" ok", &mut refs), "[x]: /url\n".len());
        assert_eq!(parse_reference("[\nlabel\n]: /url", &mut refs), "[\nlabel\n]: /url".len());
        assert_eq!(refs["label"], "/url");
    }

    #[test]
    fn image_forms() {
        cases(&Refs::new(), &[
            ("![foo](/url \"title\")", "![foo](/url)"),
            ("![foo *bar* **baz**](train.jpg)", "![foo bar baz](train.jpg)"),
            ("![foo ![bar](/url)](/url2)", "![foo bar](/url2)"),
            ("![foo [bar](/url)](/url2)", "![foo bar](/url2)"),
            ("![[[foo](uri1)](uri2)](uri3)", "![[foo](uri2)](uri3)"),
            ("![foo](<url>)", "![foo](url)"),
            ("![](/url)", "![](/url)"),
            ("My ![foo bar](/path/to/train.jpg  \"title\"   )", "My ![foo bar](/path/to/train.jpg)"),
            ("![a\nb](/i)", "![a b](/i)"),
            ("![  spaced   out ](/i) ![]() ![ ]()", "![spaced out](/i)"),
            ("[![a](/i) and text](/l)", "[![a](/i)](/l)<a /l> and text</a>"),
        ]);
    }

    #[test]
    fn autolink_forms() {
        cases(&Refs::new(), &[
            ("<http://foo.bar.baz>", "<a http://foo.bar.baz>http://foo.bar.baz</a>"),
            (
                "<https://foo.bar.baz/test?q=hello&id=22&boolean>",
                "<a https://foo.bar.baz/test?q=hello&id=22&boolean>https://foo.bar.baz/test?q=hello&id=22&boolean</a>",
            ),
            ("<irc://foo.bar:2233/baz>", "<a irc://foo.bar:2233/baz>irc://foo.bar:2233/baz</a>"),
            ("<MAILTO:FOO@BAR.BAZ>", "<a MAILTO:FOO@BAR.BAZ>MAILTO:FOO@BAR.BAZ</a>"),
            ("<a+b+c:d>", "<a a+b+c:d>a+b+c:d</a>"),
            ("<made-up-scheme://foo,bar>", "<a made-up-scheme://foo,bar>made-up-scheme://foo,bar</a>"),
            ("<localhost:5001/foo>", "<a localhost:5001/foo>localhost:5001/foo</a>"),
            ("<https://foo.bar/baz bim>", "<https://foo.bar/baz bim>"),
            ("<https://example.com/\\[\\>", "<a https://example.com/\\[\\>https://example.com/\\[\\</a>"),
            ("<foo@bar.example.com>", "<a mailto:foo@bar.example.com>foo@bar.example.com</a>"),
            ("<foo+special@Bar.baz-bar0.com>", "<a mailto:foo+special@Bar.baz-bar0.com>foo+special@Bar.baz-bar0.com</a>"),
            ("<>", "<>"),
            ("<m:abc>", "<m:abc>"),
            ("<foo.bar.baz>", "<foo.bar.baz>"),
            ("<javascript:alert(1)>", "javascript:alert(1)"),
        ]);
    }

    #[test]
    fn bare_links_by_githubs_rules() {
        cases(&Refs::new(), &[
            ("www.commonmark.org", "<a http://www.commonmark.org>www.commonmark.org</a>"),
            (
                "Visit www.commonmark.org/help for more information.",
                "Visit <a http://www.commonmark.org/help>www.commonmark.org/help</a> for more information.",
            ),
            ("Visit www.commonmark.org.", "Visit <a http://www.commonmark.org>www.commonmark.org</a>."),
            ("Visit www.commonmark.org/a.b.", "Visit <a http://www.commonmark.org/a.b>www.commonmark.org/a.b</a>."),
            (
                "www.google.com/search?q=Markup+(business)",
                "<a http://www.google.com/search?q=Markup+(business)>www.google.com/search?q=Markup+(business)</a>",
            ),
            (
                "www.google.com/search?q=Markup+(business)))",
                "<a http://www.google.com/search?q=Markup+(business)>www.google.com/search?q=Markup+(business)</a>))",
            ),
            (
                "(www.google.com/search?q=Markup+(business))",
                "(<a http://www.google.com/search?q=Markup+(business)>www.google.com/search?q=Markup+(business)</a>)",
            ),
            (
                "www.google.com/search?q=(business))+ok",
                "<a http://www.google.com/search?q=(business))+ok>www.google.com/search?q=(business))+ok</a>",
            ),
            (
                "www.google.com/search?q=commonmark&hl;",
                "<a http://www.google.com/search?q=commonmark>www.google.com/search?q=commonmark</a>&hl;",
            ),
            ("www.commonmark.org/he<lp", "<a http://www.commonmark.org/he>www.commonmark.org/he</a><lp"),
            ("HTTPS://UP.ORG/x", "<a HTTPS://UP.ORG/x>HTTPS://UP.ORG/x</a>"),
            ("ftp://foo.bar.baz.", "<a ftp://foo.bar.baz>ftp://foo.bar.baz</a>."),
            ("*http://em.org*", "<a http://em.org><i>http://em.org</i></a>"),
            ("foo@bar.baz", "<a mailto:foo@bar.baz>foo@bar.baz</a>"),
            (
                "hello@mail+xyz.example isn't valid, but hello+xyz@mail.example is.",
                "hello@mail+xyz.example isn't valid, but <a mailto:hello+xyz@mail.example>hello+xyz@mail.example</a> is.",
            ),
            ("a.b-c_d@a.b", "<a mailto:a.b-c_d@a.b>a.b-c_d@a.b</a>"),
            ("a.b-c_d@a.b.", "<a mailto:a.b-c_d@a.b>a.b-c_d@a.b</a>."),
            ("a.b-c_d@a.b- a.b-c_d@a.b_ me@host @at.org x@y..z", "a.b-c_d@a.b- a.b-c_d@a.b_ me@host @at.org x@y..z"),
            // Not at the start of a word, in a host with underscores where
            // they are not allowed, nor in code or existing links.
            ("xhttp://no.pe www.a_b.c_d www. http://", "xhttp://no.pe www.a_b.c_d www. http://"),
            ("www.x_y.example.com", "<a http://www.x_y.example.com>www.x_y.example.com</a>"),
            ("`www.code.org` [l](/u)", "<c>www.code.org</c> <a /u>l</a>"),
            // Found next to the same link written out: one run.
            ("<me@x.y>me@x.y [x.org](http://x.org)http://x.org", "<a mailto:me@x.y>me@x.yme@x.y</a> <a http://x.org>x.orghttp://x.org</a>"),
        ]);
    }

    #[test]
    fn raw_html_forms() {
        cases(&Refs::new(), &[
            ("<a><bab><c2c>", ""),
            ("<a/><b2/>", ""),
            ("<a  /><b2\ndata=\"foo\" >", ""),
            ("<33> <__>", "<33> <__>"),
            ("<a h*#ref=\"hi\">", "<a h*#ref=\"hi\">"),
            ("<a href=\"hi'> <a href=hi'>", "<a href=\"hi'> <a href=hi'>"),
            ("< a><\nfoo><bar/ >\n<foo bar=baz\nbim!bop />", "< a>< foo><bar/ > <foo bar=baz bim!bop />"),
            ("<a href='bar'title=title>", "<a href='bar'title=title>"),
            ("</a></foo >", ""),
            ("</a href=\"foo\">", "</a href=\"foo\">"),
            ("foo <!-- this is a --\ncomment - with hyphens -->", "foo"),
            ("foo <?php echo $a; ?> <!ELEMENT br EMPTY> <![CDATA[>&<]]> bar", "foo bar"),
            ("foo <a href=\"\\*\">x</a>", "foo <a \\*>x</a>"),
            ("<a href=\"&ouml;\">x", "<a ö>x</a>"),
            ("<sup>2</sup> <sub>x</sub> <u>u</u> <ins>i</ins> <mark>m</mark> <span>s</span>", "2 x u i m s"),
            (
                "<strong>b</strong><em>i</em><del>d</del><s>s</s><code>c</code><tt>t</tt><samp>s</samp><var>v</var>",
                "<b>b</b><i>i</i><s>ds</s><c>cts</c><i>v</i>",
            ),
            ("<b>open *em*", "<b>open </b><b><i>em</i></b>"),
            ("</b>stray<br/>x", "stray<br>x"),
            ("<a href=x><img src=i.png alt=I></a>", "[![I](i.png)](x)"),
            ("<a name=anchor>no link</a> <a href=\"\">empty</a>", "no link empty"),
            ("<img alt=\"\" src=\"\"> <img>", ""),
        ]);
    }

    #[test]
    fn escapes_and_entities() {
        cases(&Refs::new(), &[
            (
                "\\!\\\"\\#\\$\\%\\&\\'\\(\\)\\*\\+\\,\\-\\.\\/\\:\\;\\<\\=\\>\\?\\@\\[\\\\\\]\\^\\_\\`\\{\\|\\}\\~",
                "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~",
            ),
            ("\\A\\a\\3\\φ\\«", "\\A\\a\\3\\φ\\«"),
            ("\\\\*emphasis*", "\\<i>emphasis</i>"),
            ("`` \\[\\` ``", "<c>\\[\\`</c>"),
            ("[foo](/bar\\* \"ti\\*tle\")", "<a /bar*>foo</a>"),
            ("&nbsp; &amp; &copy; &AElig; &frac34; &hearts;", "\u{A0} & © Æ ¾ ♥"),
            // HTML5's names too, except the few for two characters.
            ("&Dcaron; &HilbertSpace; &DifferentialD; &ClockwiseContourIntegral; &ngE;", "Ď ℋ ⅆ ∲ &ngE;"),
            ("&#35; &#1234; &#992; &#0;", "# Ӓ Ϡ \u{FFFD}"),
            ("&#X22; &#XD06; &#xcab;", "\" ആ ಫ"),
            (
                "&nbsp &x; &#; &#x; &#87654321; &#abcdef0; &ThisIsNotDefined; &hi?;",
                "&nbsp &x; &#; &#x; &#87654321; &#abcdef0; &ThisIsNotDefined; &hi?;",
            ),
            ("&copy &MadeUpEntity;", "&copy &MadeUpEntity;"),
            ("`f&ouml;&ouml;`", "<c>f&ouml;&ouml;</c>"),
            ("&#42;foo&#42; &ast;bar&ast;", "*foo* *bar*"),
            ("foo&#10;&#10;bar &#9;tab", "foo bar tab"),
        ]);
    }

    #[test]
    fn hard_and_soft_breaks() {
        cases(&Refs::new(), &[
            ("foo  \nbaz", "foo<br>baz"),
            ("foo\\\nbaz", "foo<br>baz"),
            ("foo       \nbaz", "foo<br>baz"),
            ("foo  \n     bar", "foo<br>bar"),
            ("foo\\\n     bar", "foo<br>bar"),
            ("*foo  \nbar*", "<i>foo</i><br><i>bar</i>"),
            ("*foo\\\nbar*", "<i>foo</i><br><i>bar</i>"),
            ("`code  \nspan`", "<c>code   span</c>"),
            ("`code\\\nspan`", "<c>code\\ span</c>"),
            ("<a href=\"foo  \nbar\">x</a>", "<a foo  \nbar>x</a>"),
            ("foo\\", "foo\\"),
            ("foo  ", "foo"),
            ("foo \n baz", "foo baz"),
            ("a<br>b<br/><br>c", "a<br>b<br><br>c"),
        ]);
    }
}
