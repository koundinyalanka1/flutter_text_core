//! The reading model of Markdown and HTML as JSON, for Dart:
//!
//! ```text
//! {"title": "..." or null, "blocks": [block, ...], "truncated": true}
//!
//! block   {"type": "heading", "level": 1, "inlines": [inline, ...]}
//!         {"type": "paragraph", "inlines": [...]}
//!         {"type": "quote", "blocks": [...]}
//!         {"type": "list", "ordered": true, "start": 1,
//!          "items": [{"checked": true, "blocks": [...]}, ...]}
//!         {"type": "code", "language": "rust", "text": "..."}
//!         {"type": "rule"}
//!         {"type": "table", "align": ["none", "left", "center", "right"],
//!          "header": [cell, ...], "rows": [[cell, ...], ...]}
//! cell    [inline, ...]
//! inline  {"type": "text", "text": "...", "bold": true, "italic": true,
//!          "code": true, "strike": true, "link": "..."}
//!         {"type": "image", "alt": "...", "src": "...", "link": "..."}
//!         {"type": "break"}
//! ```
//!
//! Flags that are false, links that are absent and the `checked` of items
//! that are not tasks are left out. `truncated` says the blocks are only
//! the beginning of a document too long to read whole.

use serde::ser::{Serialize, SerializeMap, Serializer};
use text_core::markup::{Block, Document, Inline, Item};

/// A borrowed part of the model, serialized in the shape above.
pub(crate) struct Json<'a, T: ?Sized>(pub &'a T);

impl<T> Serialize for Json<'_, [T]>
where
    for<'b> Json<'b, T>: Serialize,
{
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(Json))
    }
}

impl<T> Serialize for Json<'_, Vec<T>>
where
    for<'b> Json<'b, [T]>: Serialize,
{
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Json(self.0.as_slice()).serialize(s)
    }
}

impl Serialize for Json<'_, Document> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("title", &self.0.title)?;
        map.serialize_entry("blocks", &Json(&self.0.blocks))?;
        if self.0.truncated {
            map.serialize_entry("truncated", &true)?;
        }
        map.end()
    }
}

impl Serialize for Json<'_, Block> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match self.0 {
            Block::Heading { level, inlines } => {
                map.serialize_entry("type", "heading")?;
                map.serialize_entry("level", level)?;
                map.serialize_entry("inlines", &Json(inlines))?;
            }
            Block::Paragraph { inlines } => {
                map.serialize_entry("type", "paragraph")?;
                map.serialize_entry("inlines", &Json(inlines))?;
            }
            Block::Quote { blocks } => {
                map.serialize_entry("type", "quote")?;
                map.serialize_entry("blocks", &Json(blocks))?;
            }
            Block::List { ordered, start, items } => {
                map.serialize_entry("type", "list")?;
                map.serialize_entry("ordered", ordered)?;
                map.serialize_entry("start", start)?;
                map.serialize_entry("items", &Json(items))?;
            }
            Block::Code { language, text } => {
                map.serialize_entry("type", "code")?;
                map.serialize_entry("language", language)?;
                map.serialize_entry("text", text)?;
            }
            Block::Rule => map.serialize_entry("type", "rule")?,
            Block::Table { align, header, rows } => {
                map.serialize_entry("type", "table")?;
                let names: Vec<&str> = align.iter().map(|a| a.name()).collect();
                map.serialize_entry("align", &names)?;
                map.serialize_entry("header", &Json(header))?;
                map.serialize_entry("rows", &Json(rows))?;
            }
        }
        map.end()
    }
}

impl Serialize for Json<'_, Item> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        if let Some(checked) = self.0.checked {
            map.serialize_entry("checked", &checked)?;
        }
        map.serialize_entry("blocks", &Json(&self.0.blocks))?;
        map.end()
    }
}

impl Serialize for Json<'_, Inline> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match self.0 {
            Inline::Text { text, style } => {
                map.serialize_entry("type", "text")?;
                map.serialize_entry("text", text)?;
                for (name, on) in [
                    ("bold", style.bold),
                    ("italic", style.italic),
                    ("code", style.code),
                    ("strike", style.strike),
                ] {
                    if on {
                        map.serialize_entry(name, &true)?;
                    }
                }
                if let Some(link) = &style.link {
                    map.serialize_entry("link", link)?;
                }
            }
            Inline::Image { alt, src, link } => {
                map.serialize_entry("type", "image")?;
                map.serialize_entry("alt", alt)?;
                map.serialize_entry("src", src)?;
                if let Some(link) = link {
                    map.serialize_entry("link", link)?;
                }
            }
            Inline::Break => map.serialize_entry("type", "break")?,
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_documented_shape() {
        let doc = text_core::markup::markdown(
            "# Title\n\n**b** [l](/u) ![a](i.png)  \nx\n\n> - [x] done\n\n```sh\nls\n```\n\n---\n\n| h | k |\n|:-:|--:|\n| 1 |",
        );
        let json = serde_json::to_string(&Json(&doc)).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"title":"Title","blocks":["#,
                r#"{"type":"heading","level":1,"inlines":[{"type":"text","text":"Title"}]},"#,
                r#"{"type":"paragraph","inlines":[{"type":"text","text":"b","bold":true},{"type":"text","text":" "},"#,
                r#"{"type":"text","text":"l","link":"/u"},{"type":"text","text":" "},"#,
                r#"{"type":"image","alt":"a","src":"i.png"},{"type":"break"},{"type":"text","text":"x"}]},"#,
                r#"{"type":"quote","blocks":[{"type":"list","ordered":false,"start":1,"items":[{"checked":true,"#,
                r#""blocks":[{"type":"paragraph","inlines":[{"type":"text","text":"done"}]}]}]}]},"#,
                r#"{"type":"code","language":"sh","text":"ls"},{"type":"rule"},"#,
                r#"{"type":"table","align":["center","right"],"header":[[{"type":"text","text":"h"}],"#,
                r#"[{"type":"text","text":"k"}]],"rows":[[[{"type":"text","text":"1"}],[]]]}]}"#,
            )
        );
        let empty = serde_json::to_string(&Json(&text_core::markup::html(""))).unwrap();
        assert_eq!(empty, r#"{"title":null,"blocks":[]}"#);
        let cut = text_core::markup::Document { truncated: true, ..Default::default() };
        assert_eq!(serde_json::to_string(&Json(&cut)).unwrap(), r#"{"title":null,"blocks":[],"truncated":true}"#);
    }
}
