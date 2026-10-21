//! Properties both readers keep whatever they are given: no panics, sane
//! shapes, bounded nesting, and linear time on input built to be slow.

use super::*;

/// Each block as one line of plain text, to compare structure quickly:
/// `h1 text`, `p text`, `> ` before a quote's lines, `- ` or `3. ` (and
/// `[x] `) before a list item's first line and two spaces before the rest,
/// `code[language] text`, `hr`, and `table header|cells / row / row`.
pub(super) fn outline(blocks: &[Block]) -> Vec<String> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => out.push(format!("h{level} {}", plain_text(inlines))),
            Block::Paragraph { inlines } => out.push(format!("p {}", plain_text(inlines))),
            Block::Quote { blocks } => out.extend(outline(blocks).into_iter().map(|l| format!("> {l}"))),
            Block::List { ordered, start, items } => {
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered { format!("{}.", start + i as i64) } else { "-".into() };
                    let check = match item.checked {
                        Some(true) => "[x] ",
                        Some(false) => "[ ] ",
                        None => "",
                    };
                    let lines = outline(&item.blocks);
                    if lines.is_empty() {
                        out.push(format!("{marker} {check}").trim_end().to_owned());
                    }
                    for (j, line) in lines.into_iter().enumerate() {
                        out.push(if j == 0 { format!("{marker} {check}{line}") } else { format!("  {line}") });
                    }
                }
            }
            Block::Code { language, text } => out.push(format!("code[{language}] {text}")),
            Block::Rule => out.push("hr".into()),
            Block::Table { header, rows, .. } => {
                let row = |cells: &[Cell]| cells.iter().map(|c| plain_text(c)).collect::<Vec<_>>().join("|");
                out.push(format!("table {} / {}", row(header), rows.iter().map(|r| row(r)).collect::<Vec<_>>().join(" / ")));
            }
        }
    }
    out
}

/// Runs written out compactly to compare styles: `<b>`, `<i>`, `<s>` and
/// `<c>` (code) around styled text inside `<a target>` for links,
/// `![alt](src)` for images (in `[...](link)` when linked) and `<br>`.
pub(super) fn show(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text, style } => {
                let tags: Vec<&str> = [(style.bold, "b"), (style.italic, "i"), (style.strike, "s"), (style.code, "c")]
                    .into_iter()
                    .filter_map(|(on, tag)| on.then_some(tag))
                    .collect();
                if let Some(link) = &style.link {
                    out.push_str(&format!("<a {link}>"));
                }
                for tag in &tags {
                    out.push_str(&format!("<{tag}>"));
                }
                out.push_str(text);
                for tag in tags.iter().rev() {
                    out.push_str(&format!("</{tag}>"));
                }
                if style.link.is_some() {
                    out.push_str("</a>");
                }
            }
            Inline::Image { alt, src, link: None } => out.push_str(&format!("![{alt}]({src})")),
            Inline::Image { alt, src, link: Some(link) } => out.push_str(&format!("[![{alt}]({src})]({link})")),
            Inline::Break => out.push_str("<br>"),
        }
    }
    out
}

/// Check the promises the model makes to the app.
fn check(blocks: &[Block], depth: usize) {
    assert!(depth <= 2 * MAX_NESTING + 2, "nested too deep");
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => {
                assert!((1..=6).contains(level));
                assert!(!inlines.is_empty(), "empty heading");
                check_inlines(inlines);
            }
            Block::Paragraph { inlines } => {
                assert!(!inlines.is_empty(), "empty paragraph");
                check_inlines(inlines);
            }
            Block::Quote { blocks } => {
                assert!(!blocks.is_empty(), "empty quote");
                check(blocks, depth + 1);
            }
            Block::List { ordered, start, items } => {
                assert!(!items.is_empty());
                assert!(*ordered || *start == 1);
                for item in items {
                    check(&item.blocks, depth + 1);
                }
            }
            Block::Code { .. } | Block::Rule => {}
            Block::Table { align, header, rows } => {
                assert!(!align.is_empty() && align.len() <= MAX_COLUMNS);
                assert!(header.is_empty() || header.len() == align.len());
                for cell in header {
                    check_inlines(cell);
                }
                for row in rows {
                    assert_eq!(row.len(), align.len());
                    for cell in row {
                        check_inlines(cell);
                    }
                }
            }
        }
    }
}

fn check_inlines(inlines: &[Inline]) {
    for pair in inlines.windows(2) {
        if let [Inline::Text { style: a, .. }, Inline::Text { style: b, .. }] = pair {
            assert_ne!(a, b, "neighbouring runs share a style: {inlines:?}");
        }
    }
    for inline in inlines {
        match inline {
            Inline::Text { text, style } => {
                assert!(!text.is_empty(), "empty run in {inlines:?}");
                assert!(style.link.as_ref().is_none_or(|l| !l.is_empty()), "empty link in {inlines:?}");
            }
            Inline::Image { alt, src, link } => {
                assert!(!alt.is_empty() || !src.is_empty(), "empty image in {inlines:?}");
                assert!(link.as_ref().is_none_or(|l| !l.is_empty()), "empty link in {inlines:?}");
            }
            Inline::Break => {}
        }
    }
    assert!(!matches!(inlines.last(), Some(Inline::Break)), "trailing break in {inlines:?}");
}

fn check_document(doc: &Document) {
    check(&doc.blocks, 0);
    if let Some(title) = &doc.title {
        assert!(!title.is_empty() && title.trim() == title, "title {title:?}");
    }
}

fn both(text: &str) {
    check_document(&markdown(text));
    check_document(&html(text));
}

/// A small xorshift generator, so the "fuzzing" is repeatable.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "*", "**", "_", "__", "~", "~~", "`", "``", "```", "~~~", "[", "]", "(", ")", "![", "](", "]: ", "<", ">", "&", "#",
    "# ", "## ", "-", "- ", "+ ", "* ", "1. ", "2) ", "|", "| --- |", ":-:", "\\", "\n", "\n\n", " ", "  ", "\t", "    ",
    "> ", "a", "word", "é", "😀", "\u{0}", "\r", "http://x.y/z", "www.a.b", "<me@x.y>", "&amp;", "&#", "&#x", "&nbsp;",
    "<!--", "-->", "<b>", "</b>", "<i>", "</i>", "<p>", "</p>", "<div>", "</div>", "<li>", "<ul>", "</ul>", "<ol start=",
    "<table>", "<tr>", "<td>", "<th>", "</td>", "</table>", "<pre>", "</pre>", "<script>", "</script>", "<a href=",
    "\"", "'", "=", "/>", "<br>", "<img src=x alt=", "<![CDATA[", "]]>", "<!DOCTYPE", "<title>", "<h1>", "</h1>",
    "<blockquote>", "<input type=checkbox checked>", "[ ] ", "[x] ", "===", "---", "***", "<span>", "<?", "?>",
    "me@x.y", "@", ".", "ftp://", "https://", "www.", "_(", ")", ";", "&a;", "\u{A0}", "\u{301}", "ẞ", "\u{FEFF}",
    "<details>", "<summary>", "</details>", "<dl><dt>", "<dd>", "<caption>", "<thead>", "<tfoot>", "colspan=3",
    "<textarea>", "<style>", "<svg>", "<plaintext>", "<xmp>", "hidden", "style=display:none", "javascript:", "<q>",
    "[foo]", "[foo]: /u", "{.py}", "\r\n", ">>", "  - ", "    - ", "10. ", "\\|",
];

/// Realistic documents to cut up and garble.
const SEEDS: &[&str] = &[
    "# T\n\n> q *e* **s** ~~x~~ `c` [l](/u \"t\") ![i](/p) <b>h</b> &amp; https://a.b\n\
     - [x] task\n  1. one\n\n| a | b |\n|:-|-:|\n| 1 | 2 |\n\n```rust\ncode\n```\n\n[r]: /ref\n\
     <table><tr><td>c</td></tr></table><ul><li>i<li>j</ul><pre>p</pre><script>s</script>",
    "<p align=\"center\">\n  <a href=\"https://ci\"><img src=\"badge.svg\" alt=\"CI\"></a>\n</p>\n\n\
     Project\n=======\n\nInstall with `cargo add x` or see [the docs][docs].\n\n\
     1. First\n2. Second\n   * nested *em* and __strong__\n\n     ```sh\n     run\n     ```\n\n\
     <details>\n<summary>More</summary>\n\nHidden text, me@example.com, www.example.com.\n\n</details>\n\n\
     [docs]: https://docs.example \"Docs\"\n\n---\n\nFooter &copy; 2024\\\nline two  \nthree",
    "<!DOCTYPE html><html lang=en><head><meta charset=\"utf-8\"><title>Shop &amp; Caf&eacute;</title>\
     <style>body{}</style><script>var a = '<p>';</script></head><body><header><nav><a href=/>Home</a></nav></header>\
     <main><h1 id=t>Menu</h1><p>Fresh <b>coffee <i>daily</b> &mdash; more<p>Open <time>9</time>-5\
     <ul><li>Tea<li><input type=checkbox checked> Cake<ul><li>Lemon</ul></ul><ol start=4><li>four</ol>\
     <table><caption>Prices</caption><thead><tr><th align=left>Item<th>Cost<tbody><tr><td>Tea<td>2\
     <tr><td colspan=2>Total</table><blockquote><p>Quote<blockquote>Deeper</blockquote></blockquote>\
     <pre class=\"language-js\">\nlet x = 1 &lt; 2;\n</pre><dl><dt>Term<dd>Def</dl><hr><figure><img src=a.png alt=A>\
     <figcaption>Fig</figcaption></figure><form><label>Q <input></label><button>Go</button></form></main>\
     <footer><p hidden>no<p>&copy; 2024</footer></body></html>",
];

/// `seed` changed a few times at random: pieces inserted, spans deleted
/// or repeated.
fn garble(random: &mut Random, seed: &str) -> String {
    let mut text = seed.to_owned();
    let boundary = |text: &str, at: usize| (0..=at).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    for _ in 0..1 + random.below(6) {
        let at = boundary(&text, random.below(text.len() + 1));
        match random.below(3) {
            0 => text.insert_str(at, PIECES[random.below(PIECES.len())]),
            1 => {
                let end = boundary(&text, (at + random.below(40)).min(text.len()));
                text.replace_range(at.min(end)..end, "");
            }
            _ => {
                let end = boundary(&text, (at + random.below(40)).min(text.len()));
                let span = text[at.min(end)..end].to_owned();
                text.insert_str(end, &span.repeat(1 + random.below(3)));
            }
        }
    }
    text
}

#[test]
fn generated_input_never_breaks_the_model() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for _ in 0..5000 {
        let len = random.below(120);
        let text: String = (0..len).map(|_| PIECES[random.below(PIECES.len())]).collect();
        both(&text);
    }
    // Random bytes, as a mis-detected binary file might arrive.
    for _ in 0..300 {
        let len = random.below(400);
        let bytes: Vec<u8> = (0..len).map(|_| random.next() as u8).collect();
        both(&String::from_utf8_lossy(&bytes));
    }
    // Real-looking documents, garbled.
    for _ in 0..3000 {
        let seed = SEEDS[random.below(SEEDS.len())];
        both(&garble(&mut random, seed));
    }
    // Every prefix of each, so that each construct is also seen cut off
    // half way.
    for seed in SEEDS {
        for end in (0..=seed.len()).filter(|&i| seed.is_char_boundary(i)) {
            both(&seed[..end]);
        }
    }
}

#[test]
fn input_built_to_be_slow_stays_fast() {
    let n = 20_000;
    let cases = [
        "*a ".repeat(n),
        "_a ".repeat(n),
        "a** ".repeat(n),
        "[".repeat(n),
        "]".repeat(n),
        "[a](".repeat(n),
        "[a](b \"".repeat(n),
        "[a](<b".repeat(n),
        "![[](".repeat(n),
        "<!--".repeat(n),
        "<a b='".repeat(n),
        "<?".repeat(n),
        "&#".repeat(n),
        "~~a ".repeat(n),
        "**_*".repeat(n),
        (1..300).map(|i| "`".repeat(i) + "x").collect::<String>(),
        "> ".repeat(n) + "deep",
        "- ".repeat(n) + "deep",
        "a\n".repeat(n),
        "> a\n".repeat(n),
        "- a\n  ".repeat(n),
        "| a ".repeat(500) + "|\n" + &"|-".repeat(500) + "|\n" + &"x\n".repeat(n),
        "[x]: /u\n".repeat(n) + "[x]",
        "http://a.b ".repeat(n),
        "www._(".repeat(n),
        "www.a.b_".repeat(n),
        "http://a.b".to_owned() + &")".repeat(n),
        "http://a.b/".to_owned() + &"&a;".repeat(n),
        "hééé ".repeat(n),
        "a@".repeat(n),
        "x@y.".repeat(n),
        "a.".repeat(n) + "@",
        "<div>".repeat(n),
        "<b>".repeat(n) + "</p>text",
        "<p><b>".repeat(n),
        "<li>".repeat(n),
        "<table><tr><td>".repeat(n),
        "<a href=x>".repeat(n),
        "<b><i><u>".repeat(n) + &"</b>x".repeat(n),
        "<b></i>".repeat(n),
        "<foo><bar></baz>".repeat(n),
        "&".repeat(n) + "amp;",
        "&abcdefghijklmnopqrstuvwxyzabcdefghijklmnopq ".repeat(n),
        "&frac34".repeat(n),
        "<blockquote><ul><li>".repeat(n),
        "<table>\n\n".repeat(n),
        "<span>\n\n*a*\n\n".repeat(n),
    ];
    for case in &cases {
        let started = std::time::Instant::now();
        both(case);
        // Generous, for a debug build on a busy machine; quadratic time
        // would take minutes.
        assert!(started.elapsed().as_secs() < 20, "slow on {:?}", &case[..40.min(case.len())]);
    }
}

#[test]
fn huge_tables_end_early() {
    let header = "| a ".repeat(100) + "|\n" + &"|-".repeat(100) + "|\n";
    let rows = "| x |\n".repeat(MAX_TABLE_CELLS / 100 + 10);
    // Read without the part limit, which would cut this table much sooner.
    let doc = super::markdown::parse(&(header + &rows));
    check(&doc.blocks, 0);
    let Block::Table { rows, .. } = &doc.blocks[0] else { panic!() };
    assert_eq!(rows.len(), MAX_TABLE_CELLS / 100);
    assert_eq!(doc.blocks.len(), 11);
    assert_eq!(doc.blocks[1], Block::Paragraph {
        inlines: vec![Inline::Text { text: "x".into(), style: Style::default() }]
    });
}

/// The parts [`MAX_PARTS`] counts in `blocks`.
fn parts(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| {
            1 + match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines } => inlines.len(),
                Block::Quote { blocks } => parts(blocks),
                Block::List { items, .. } => items.iter().map(|item| 1 + parts(&item.blocks)).sum(),
                Block::Table { header, rows, .. } => header.len() + rows.iter().map(Vec::len).sum::<usize>(),
                Block::Code { .. } | Block::Rule => 0,
            }
        })
        .sum()
}

#[test]
fn long_documents_are_cut_short() {
    let cases = [
        // Each item is three parts: the item, its paragraph and its text.
        ("- a\n".repeat(200_000), Some("- p a")),
        ("> - [ ] buy milk\n".repeat(100_000), Some("> - [ ] p buy milk")),
        // One paragraph of hundreds of thousands of runs.
        ("*a* b ".repeat(200_000), None),
        ("| a | b |\n|---|---|\n".to_owned() + &"| x | y |\n".repeat(100_000), None),
        ("<p>x</p>".repeat(200_000), Some("p x")),
        ("<b>x<p>".repeat(1_000_000), None),
        ("<ul><li>a<li>b</ul>".repeat(100_000), Some("- p a")),
        ("<div>".repeat(500_000), None),
    ];
    for (text, first) in &cases {
        let started = std::time::Instant::now();
        let doc = if text.starts_with('<') { html(text) } else { markdown(text) };
        let label = &text[..20.min(text.len())];
        assert!(doc.truncated, "{label}");
        check(&doc.blocks, 0);
        let count = parts(&doc.blocks);
        assert!(count <= MAX_PARTS, "{label}: {count}");
        if let Some(first) = first {
            assert_eq!(outline(&doc.blocks)[0], *first, "{label}");
            assert!(count > MAX_PARTS / 2, "{label}: {count}");
        }
        assert!(started.elapsed().as_secs() < 20, "slow on {label}");
    }
    // The beginning of a long paragraph is kept.
    let doc = markdown(&"*a* b ".repeat(200_000));
    let Block::Paragraph { inlines } = &doc.blocks[0] else { panic!() };
    assert_eq!(inlines.len(), MAX_PARTS - 1);
    // Markdown with a huge HTML block in it says it was cut too.
    let doc = markdown(&("# Notes\n\n<div>\n".to_owned() + &"<b>x<p>".repeat(1_000_000)));
    assert!(doc.truncated);
    assert_eq!(outline(&doc.blocks)[0], "h1 Notes");
    // Ordinary documents are whole.
    assert!(!markdown(&"- a\n".repeat(1000)).truncated);
    assert!(!html(&"<p>x</p>".repeat(1000)).truncated);
    assert!(!markdown("").truncated);
}

#[test]
fn plain_text_reads_runs_images_and_breaks() {
    let doc = markdown("**Hello** ![big](w.png) world  \nagain");
    let Block::Paragraph { inlines } = &doc.blocks[0] else { panic!() };
    assert_eq!(plain_text(inlines), "Hello big world again");
}

#[test]
fn titles_leave_out_badges() {
    assert_eq!(markdown("# Serde &emsp; [![Build](b.svg)](ci) ![v](v.svg)\n").title.as_deref(), Some("Serde"));
    assert_eq!(markdown("# ![Logo](logo.png)").title.as_deref(), Some("Logo"));
    assert_eq!(markdown("# ![](logo.png)").title, None);
    assert_eq!(html("<h1>  Two\n words </h1>").title.as_deref(), Some("Two words"));
}
