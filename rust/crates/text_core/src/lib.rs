//! The plain-text engine: open a text file in whatever encoding and line
//! endings it uses, edit it as Unicode with `\n` line breaks, and save it
//! back exactly as it came (or converted, when asked).
//!
//! Also: paging through very large files by line, search and replace, a
//! JSON formatter that keeps key order and number spelling, and Markdown
//! and HTML read into blocks for a reading view ([`markup`]).

mod document;
pub mod json;
pub mod markup;
pub mod search;

pub use document::{LineEnding, Stats, TextDocument};
pub use doc_encoding::Encoding;
