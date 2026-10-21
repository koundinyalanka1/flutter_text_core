//! A developer tool for trying real Markdown and HTML files in the reading
//! model. Files ending in `.html`, `.htm` or `.xhtml` are read as HTML,
//! everything else as Markdown.
//!
//! ```text
//! markupcli outline FILE...   print each file's title and blocks
//! markupcli sweep   FILE...   read every file, report failures, slow files
//!                             and totals (file names may also come on stdin)
//! ```

use std::io::BufRead;
use std::time::{Duration, Instant};

use text_core::markup::{self, plain_text, Block, Document, Inline};
use text_core::{Encoding, TextDocument};

fn is_html(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm") || lower.ends_with(".xhtml")
}

fn read(path: &str) -> Option<(Document, Duration)> {
    let bytes = std::fs::read(path).map_err(|e| eprintln!("{path}: {e}")).ok()?;
    let text = TextDocument::from_bytes(bytes, Encoding::WINDOWS_1252).text().to_owned();
    let started = Instant::now();
    let doc = if is_html(path) { markup::html(&text) } else { markup::markdown(&text) };
    Some((doc, started.elapsed()))
}

fn inline_text(inlines: &[Inline]) -> String {
    let text = plain_text(inlines);
    let short: String = text.chars().take(100).collect();
    if short.len() < text.len() {
        format!("{short}…")
    } else {
        short
    }
}

fn outline(blocks: &[Block], indent: &str) {
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => println!("{indent}h{level} {}", inline_text(inlines)),
            Block::Paragraph { inlines } => println!("{indent}p  {}", inline_text(inlines)),
            Block::Quote { blocks } => {
                println!("{indent}quote");
                outline(blocks, &format!("{indent}  "));
            }
            Block::List { ordered, start, items } => {
                println!("{indent}{} list from {start}, {} items", if *ordered { "ordered" } else { "bullet" }, items.len());
                for item in items {
                    let check = match item.checked {
                        Some(true) => " [x]",
                        Some(false) => " [ ]",
                        None => "",
                    };
                    println!("{indent}  item{check}");
                    outline(&item.blocks, &format!("{indent}    "));
                }
            }
            Block::Code { language, text } => println!("{indent}code [{language}] {} lines", text.lines().count()),
            Block::Rule => println!("{indent}rule"),
            Block::Table { align, header, rows } => {
                println!("{indent}table {} columns, {} rows", align.len(), rows.len());
                if !header.is_empty() {
                    let names: Vec<String> = header.iter().map(|cell| inline_text(cell)).collect();
                    println!("{indent}  header {}", names.join(" | "));
                }
            }
        }
    }
}

fn count(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Quote { blocks } => 1 + count(blocks),
            Block::List { items, .. } => 1 + items.iter().map(|item| count(&item.blocks)).sum::<usize>(),
            _ => 1,
        })
        .sum()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("outline") => {
            for path in &args[1..] {
                let Some((doc, took)) = read(path) else { continue };
                println!("== {path} ({took:?})");
                println!("title: {}", doc.title.as_deref().unwrap_or("(none)"));
                outline(&doc.blocks, "");
            }
        }
        Some("sweep") => {
            let mut paths: Vec<String> = args[1..].to_vec();
            if paths.is_empty() {
                paths = std::io::stdin().lock().lines().map_while(Result::ok).collect();
            }
            let (mut files, mut blocks, mut failed) = (0, 0, 0);
            let mut total = Duration::ZERO;
            let mut slowest = (Duration::ZERO, String::new());
            for path in &paths {
                match std::panic::catch_unwind(|| read(path)) {
                    Ok(Some((doc, took))) => {
                        files += 1;
                        blocks += count(&doc.blocks);
                        total += took;
                        if took > slowest.0 {
                            slowest = (took, path.clone());
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        failed += 1;
                        println!("PANIC {path}");
                    }
                }
            }
            println!("{files} files, {blocks} blocks, {failed} panics, {total:?} in all");
            println!("slowest: {:?} {}", slowest.0, slowest.1);
        }
        _ => eprintln!("usage: markupcli outline|sweep FILE..."),
    }
}
