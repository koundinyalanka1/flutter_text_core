# flutter_text_core

DocHelper's plain-text engine, written from scratch in Rust and called from
Dart through `dart:ffi`. One of DocHelper's per-format libraries (see
`PLAN.md` in the app repository).

- Detects and decodes UTF-8, UTF-16, UTF-32 and the common single-byte code
  pages; saves back in the same encoding, byte-order mark and line endings,
  or converts them on request.
- Warns before saving characters an encoding cannot hold.
- Re-reads an unedited file in another encoding when the guess was wrong.
- Pages through very large files by line.
- Find and replace (match case, whole words), with UTF-16 offsets for Dart.
- Validates and re-indents JSON without reordering keys or rewriting numbers.
- Reads Markdown and HTML into blocks for a native reading view
  (`TextCore.markdown`, `TextCore.html`, and their `Async` forms): see
  [Reading views](#reading-views).

```dart
final doc = await TextCore.open('/path/notes.txt');
final info = doc.info();            // encoding, line endings, lines, ...
doc.setText(doc.text.replaceAll('teh', 'the'));
doc.setFormat(lineEnding: LineEnding.crlf);
await doc.save('/path/notes.txt');  // atomic
doc.close();
```

## Reading views

`TextCore.markdown(text)` and `TextCore.html(text)` return a
`MarkupDocument`: a `title` and a list of `MarkupBlock`s to draw with
Flutter widgets instead of a web view.

| Block | Holds |
| --- | --- |
| `MarkupHeading` | `level` 1 to 6, `inlines` |
| `MarkupParagraph` | `inlines` |
| `MarkupQuote` | `blocks` |
| `MarkupList` | `ordered`, `start`, `items` (each a `MarkupListItem`: `checked` for task items, `blocks`) |
| `MarkupCode` | `language` (may be empty), `text` |
| `MarkupRule` | nothing |
| `MarkupTable` | `align` per column (`MarkupAlign.none/left/center/right`), `header` cells, `rows` of cells; a cell is a list of inlines |

Inlines are `MarkupText` runs (`text`, `bold`, `italic`, `code`, `strike`,
`link`), `MarkupImage`s (`alt`, `src`, `link`) and `MarkupBreak`s.
Neighbouring runs always differ in style; white space is already collapsed
the way a browser would, except in code.

```dart
final doc = await TextCore.markdownAsync(readme);   // or htmlAsync(page)
for (final block in doc.blocks) {
  switch (block) {
    case MarkupHeading(:final level, :final inlines): ...
    case MarkupList(:final ordered, :final start, :final items): ...
    // ...
  }
}
```

- **Markdown** is CommonMark (headings, emphasis by the flanking rules,
  code spans and blocks, links and images inline and by reference,
  autolinks, quotes and lists nested by indentation, escapes and entities)
  with GitHub's tables, strikethrough, task lists and bare `http(s)://`,
  `www.` and email addresses. YAML front matter shows as a code block. The
  title is the first level-1 heading.
- **HTML** goes through a forgiving tokenizer (quoted or bare attributes,
  comments, doctypes, CDATA, raw text in `script`, `style`, `textarea` and
  `title`, every named character reference that stands for a single
  character, numeric references) and a tree builder that closes what HTML lets authors leave
  open (`p`, `li`, `td`, `tr`, ...) and reopens bold or italic cut short by
  a block. The head (except the `<title>`, which is the title, or else the
  first `<h1>`), scripts, styles, `noscript`, `template`, SVG, frames,
  objects, form controls and hidden elements are left out.
- **HTML inside Markdown** (a README's centred logo, `<details>`,
  `<kbd>`) is read as HTML, so it shows as it would on GitHub rather than as
  tags.
- Links to nowhere (`[a]()`, `href=""`) and `javascript:` links read as
  plain text; `<u>`, `<sup>` and `<sub>` as plain text too (the model has
  no underline or raised text).
- Nothing fails: odd input still gives blocks, quotes and lists nested
  past a few dozen levels are cut short, and time stays linear on input
  built to be slow. Text over 32 MB throws `TextCoreException` with code
  `TOO_LARGE`.
- A document is read into at most 100,000 parts (blocks, list items, table
  cells and runs of text), and each reader's tree stops at 300,000 nodes:
  past that the blocks are its beginning and `MarkupDocument.truncated` is
  set. Without the limit, five megabytes of one-word list items came to
  two gigabytes in the app.

`cargo run --release --example markupcli -- outline FILE` (from `rust/`)
prints a file's blocks; `sweep` reads many files and reports panics and the
slowest.

## Layout

| Path | What |
| --- | --- |
| `rust/crates/text_core` | the engine: documents, encodings, search, JSON, Markdown and HTML (`markup`) |
| `rust/crates/text_ffi` | the Rust FFI layer (`text_*` functions, exported `extern "C"` for `dart:ffi`) |
| `lib/` | the Dart API |

Shared building blocks (encodings, FFI plumbing) come from
`packages/doc_common`.

## Building and testing

```bash
bash scripts/build_libs.sh macos --only text     # host library for tests
bash scripts/build_libs.sh android --only text   # jniLibs for the app
(cd packages/flutter_text_core/rust && cargo test)
TEXT_CORE_LIB_PATH=$PWD/packages/flutter_text_core/macos/Frameworks/libtext_ffi.dylib \
  flutter test packages/flutter_text_core
```

On Linux, `(cd packages/flutter_text_core/rust && cargo build --release)`
(or `bash scripts/build_libs.sh host`) leaves the library in
`rust/target/release/libtext_ffi.so`; point `TEXT_CORE_LIB_PATH` there, or
`eval "$(bash scripts/build_libs.sh env)"`. Without it the tests that
need the engine skip themselves.
