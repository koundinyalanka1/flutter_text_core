# flutter_text_core: notes for Claude Code sessions

The guide for this repository lives in `AGENTS.md`, so every coding agent
reads the same rules.

@AGENTS.md

## Claude Code only

- **The file-writing tool decodes `\uXXXX` escapes** in file content. To
  put a literal backslash-u sequence in Rust or Dart source (JSON escapes in
  `json.rs` tests, Dart escapes in `markup_test.dart`), build it with
  `concat!` or an escaped string, as `json.rs` does.
