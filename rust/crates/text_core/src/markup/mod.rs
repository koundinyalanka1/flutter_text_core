//! Markdown and HTML as one reading model, for drawing natively instead of
//! in a web view.
//!
//! A [`Document`] is a list of [`Block`]s (headings, paragraphs, quotes,
//! lists, code, rules and tables) whose text is flattened into styled runs
//! ([`Inline`]). [`markdown`] reads CommonMark with GitHub's extensions and
//! [`html`] reads real-world HTML however broken it is. Neither fails: what
//! they cannot place is kept as text.

mod entities;
mod html;
mod inline;
mod markdown;

/// Read Markdown: CommonMark with GitHub's tables, strikethrough, task
/// lists and bare links. The title is the first level-1 heading.
pub fn markdown(text: &str) -> Document {
    limited(markdown::parse(text))
}

/// Read an HTML page. The title is the `<title>`, or else the first level-1
/// heading.
pub fn html(text: &str) -> Document {
    limited(html::parse(text))
}

/// Most parts a document is read into: blocks, list items, table cells and
/// runs of text. A longer document is cut short there and marked
/// [`Document::truncated`]. Whoever shows it needs memory for each part: a
/// few megabytes of one-word list items would otherwise come to millions.
pub const MAX_PARTS: usize = 100_000;

/// Most nodes either reader builds its tree of before reading the rest of
/// the text stops: past this the parts would be cut anyway, and the tree of
/// a few megabytes of tags would take a gigabyte.
const MAX_NODES: usize = 3 * MAX_PARTS;

/// Quotes and lists nested deeper than this are flattened into their
/// parent, so that whoever walks the result recursively stays safe.
const MAX_NESTING: usize = 32;

/// Tables with more cells than this end early; the remaining rows follow as
/// paragraphs.
const MAX_TABLE_CELLS: usize = 1 << 20;

/// More columns than this are dropped.
const MAX_COLUMNS: usize = 256;

/// A document to read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
    /// The blocks are only the beginning: the text went on past
    /// [`MAX_PARTS`] parts, or past what the reader builds a tree of.
    pub truncated: bool,
}

/// What is left of [`MAX_PARTS`] while a document is cut to it.
struct Budget {
    left: usize,
    cut: bool,
}

impl Budget {
    /// Take `n` parts; false, and the document cut, when fewer are left.
    fn take(&mut self, n: usize) -> bool {
        if self.cut || n > self.left {
            self.cut = true;
            return false;
        }
        self.left -= n;
        true
    }

    /// Keep what fits of `blocks`, in reading order.
    fn blocks(&mut self, blocks: &mut Vec<Block>) {
        for i in 0..blocks.len() {
            if !self.take(1) {
                blocks.truncate(i);
                return;
            }
            match &mut blocks[i] {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines } => self.inlines(inlines),
                Block::Quote { blocks } => self.blocks(blocks),
                Block::List { items, .. } => {
                    for j in 0..items.len() {
                        if !self.take(1) {
                            items.truncate(j);
                            break;
                        }
                        self.blocks(&mut items[j].blocks);
                        if self.cut {
                            // An item cut to nothing goes too.
                            items.truncate(if items[j].blocks.is_empty() { j } else { j + 1 });
                            break;
                        }
                    }
                }
                Block::Table { header, rows, .. } => {
                    if !self.take(header.len()) {
                        header.clear();
                    }
                    for (r, row) in rows.iter().enumerate() {
                        if !self.take(row.len()) {
                            rows.truncate(r);
                            break;
                        }
                    }
                }
                Block::Code { .. } | Block::Rule => {}
            }
            if self.cut {
                // A block cut to nothing goes too: the model has no empty
                // headings, paragraphs, quotes, lists or tables.
                let empty = match &blocks[i] {
                    Block::Heading { inlines, .. } | Block::Paragraph { inlines } => inlines.is_empty(),
                    Block::Quote { blocks } => blocks.is_empty(),
                    Block::List { items, .. } => items.is_empty(),
                    Block::Table { header, rows, .. } => header.is_empty() && rows.is_empty(),
                    Block::Code { .. } | Block::Rule => false,
                };
                blocks.truncate(if empty { i } else { i + 1 });
                return;
            }
        }
    }

    fn inlines(&mut self, inlines: &mut Vec<Inline>) {
        if !self.take(inlines.len()) {
            inlines.truncate(self.left);
            self.left = 0;
        }
    }
}

/// `document` cut to [`MAX_PARTS`] parts.
fn limited(mut document: Document) -> Document {
    let mut budget = Budget { left: MAX_PARTS, cut: false };
    budget.blocks(&mut document.blocks);
    document.truncated |= budget.cut;
    document
}

/// A block of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// `level` is 1 to 6.
    Heading { level: u8, inlines: Vec<Inline> },
    Paragraph { inlines: Vec<Inline> },
    Quote { blocks: Vec<Block> },
    /// `start` is the first item's number; 1 for bullet lists.
    List { ordered: bool, start: i64, items: Vec<Item> },
    /// `language` is the first word of a fence's info string or a
    /// `language-*` class, or empty.
    Code { language: String, text: String },
    Rule,
    /// Every row has one cell per column (`align.len()`), and so has the
    /// header unless it is empty, which only HTML tables without a header
    /// row have.
    Table { align: Vec<Align>, header: Vec<Cell>, rows: Vec<Vec<Cell>> },
}

/// A table cell's content.
pub type Cell = Vec<Inline>;

/// A list item. `checked` is set for task-list items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// How a table column is aligned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

impl Align {
    /// `none`, `left`, `center` or `right`.
    pub fn name(self) -> &'static str {
        match self {
            Align::None => "none",
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// How a run of text looks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    /// Monospaced: inline code, keyboard keys and the like.
    pub code: bool,
    pub strike: bool,
    /// Where the run links to, as written.
    pub link: Option<String>,
}

/// A piece of a paragraph, heading or cell. Neighbouring text runs always
/// differ in style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text { text: String, style: Style },
    Image { alt: String, src: String, link: Option<String> },
    /// A hard line break.
    Break,
}

/// The text of some inlines, images as their alt text and breaks as spaces.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Break => out.push(' '),
        }
    }
    out
}

/// What a link points to, if anywhere worth following: an empty target
/// (`[a]()`, `<a href="">`) only reloads the page, and script URLs do
/// nothing outside a browser, so their text reads as plain text.
fn link_target(href: &str) -> Option<String> {
    let href = href.trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_control());
    let scheme = href.split_once(':').map_or("", |(scheme, _)| scheme);
    if href.is_empty() || scheme.eq_ignore_ascii_case("javascript") || scheme.eq_ignore_ascii_case("vbscript") {
        return None;
    }
    Some(href.to_owned())
}

/// The first level-1 heading's text, white space collapsed. Badges in the
/// heading (images) are left out, unless the heading is only a picture.
fn first_title(blocks: &[Block]) -> Option<String> {
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    blocks.iter().find_map(|block| match block {
        Block::Heading { level: 1, inlines } => {
            let text: Vec<Inline> = inlines.iter().filter(|i| !matches!(i, Inline::Image { .. })).cloned().collect();
            let title = Some(words(&plain_text(&text))).filter(|t| !t.is_empty());
            title.or_else(|| Some(words(&plain_text(inlines))).filter(|t| !t.is_empty()))
        }
        _ => None,
    })
}

/// Whitespace that HTML collapses (and Markdown, rendered as HTML, too).
fn is_collapsible(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

/// Builds the inlines of one paragraph, heading or cell: merges runs of the
/// same style, collapses white space the way a browser would, and drops
/// what would not show (leading and trailing spaces, trailing breaks).
#[derive(Default)]
struct Inlines {
    runs: Vec<Inline>,
}

impl Inlines {
    /// Whether a collapsible space here would not show: at the start of a
    /// line or after another space.
    fn at_space(&self) -> bool {
        match self.runs.last() {
            None | Some(Inline::Break) => true,
            Some(Inline::Text { text, .. }) => text.ends_with(' '),
            Some(Inline::Image { .. }) => false,
        }
    }

    /// Add text. With `collapse`, each run of white space becomes a single
    /// space, or nothing where [`Inlines::at_space`].
    fn text(&mut self, text: &str, style: &Style, collapse: bool) {
        if !collapse {
            self.push(text, style);
            return;
        }
        if !text.contains(is_collapsible) {
            self.push(text, style);
            return;
        }
        let mut space = self.at_space();
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if is_collapsible(c) {
                if !space {
                    out.push(' ');
                    space = true;
                }
            } else {
                out.push(c);
                space = false;
            }
        }
        self.push(&out, style);
    }

    fn push(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        if let Some(Inline::Text { text: last, style: last_style }) = self.runs.last_mut() {
            if last_style == style {
                last.push_str(text);
                return;
            }
        }
        self.runs.push(Inline::Text {
            text: text.to_owned(),
            style: style.clone(),
        });
    }

    fn image(&mut self, alt: String, src: String, link: Option<String>) {
        self.runs.push(Inline::Image { alt, src, link });
    }

    fn line_break(&mut self) {
        self.trim_end();
        self.runs.push(Inline::Break);
    }

    /// Between two blocks squeezed into one line of inlines: a break,
    /// unless at the start or right after one.
    fn boundary(&mut self) {
        if !matches!(self.runs.last(), None | Some(Inline::Break)) {
            self.line_break();
        }
    }

    /// Drop trailing spaces, except from code, whose spaces are its own.
    fn trim_end(&mut self) {
        while let Some(Inline::Text { text, style }) = self.runs.last_mut() {
            if style.code {
                return;
            }
            let kept = text.trim_end_matches(' ').len();
            if kept > 0 {
                text.truncate(kept);
                return;
            }
            self.runs.pop();
        }
    }

    fn finish(mut self) -> Vec<Inline> {
        loop {
            self.trim_end();
            if !matches!(self.runs.last(), Some(Inline::Break)) {
                return self.runs;
            }
            self.runs.pop();
        }
    }
}

/// A table whose rows are fitted to `align.len()` columns. Past
/// [`MAX_TABLE_CELLS`] the table ends and each further row becomes a
/// paragraph of its cells, separated by vertical bars.
fn table(align: Vec<Align>, header: Vec<Cell>, rows: Vec<Vec<Cell>>) -> Vec<Block> {
    let width = align.len();
    if width == 0 {
        return Vec::new();
    }
    let fit = |mut cells: Vec<Cell>| {
        cells.truncate(width);
        cells.resize_with(width, Vec::new);
        cells
    };
    let header = if header.is_empty() { header } else { fit(header) };
    let room = MAX_TABLE_CELLS / width;
    let mut fitted = Vec::with_capacity(rows.len().min(room));
    let mut after = Vec::new();
    for (i, row) in rows.into_iter().enumerate() {
        if i < room {
            fitted.push(fit(row));
            continue;
        }
        let mut line = Inlines::default();
        for (j, cell) in row.into_iter().enumerate() {
            if j > 0 {
                line.text(" | ", &Style::default(), false);
            }
            for inline in cell {
                match inline {
                    Inline::Text { text, style } => line.text(&text, &style, false),
                    Inline::Image { alt, src, link } => line.image(alt, src, link),
                    Inline::Break => line.text(" ", &Style::default(), true),
                }
            }
        }
        let inlines = line.finish();
        if !inlines.is_empty() {
            after.push(Block::Paragraph { inlines });
        }
    }
    let mut blocks = vec![Block::Table {
        align,
        header,
        rows: fitted,
    }];
    blocks.extend(after);
    blocks
}

#[cfg(test)]
mod tests;
