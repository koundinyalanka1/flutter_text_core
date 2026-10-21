# flutter_text_core: notes for coding agents

> **Since this guide was written (2026-10-19):** saving, file limits and
> several counts read from files changed in phase 10's wave 2. The app's
> `docs/HANDOFF.md`, "Changed since the engine guides were written", lists
> them; trust it, and the code, over the numbers below.

This is the guide for every coding agent (Codex, Cursor, Claude and others)
working in this repository, on its own or as part of the DocHelper app.
`CLAUDE.md` imports it, so Claude Code reads the same rules. The app's own
guide is `AGENTS.md` at the root of the app repository; read it too when a
change reaches the app.

## What this is

DocHelper is a Flutter app, Android first, whose document engines are
written from scratch in Rust. It is growing from a PDF toolkit into an
all-in-one office app (word processor, spreadsheets, presentations, PDFs)
so the owner's father can do office work on a phone instead of a laptop.

This repository is DocHelper's **plain-text engine**.

- **Repository:** `github.com/koundinyalanka1/flutter_text_core`, branch
  `main`. It is a git submodule of the app (`koundinyalanka1/pdfhelper`)
  at `packages/flutter_text_core`.
- **Native library:** `libtext_ffi` (Rust crate `text_ffi`). Every
  exported function starts with `text_`.
- **Dart package:** `flutter_text_core`, an FFI plugin. Its API is
  `TextCore` (entry points), `TextDocument` (an open file) and
  `MarkupDocument` (Markdown or HTML read into blocks).
- **File kinds:** any text file. The app sends it the extensions listed as
  `DocKind.text` in `lib/models/doc_kind.dart`: `.txt .log .md .json .xml
  .html .htm .ini .yaml .toml`, source code and config files, subtitles
  (`.srt .vtt`), `.vcf .ics` and more. CSV and TSV belong to the sheet
  engine; RTF to the word engine.
- **What it does:**
  - detects the encoding (UTF-8, UTF-16 and UTF-32 in both byte orders,
    32 single-byte code pages) and line endings, and saves back in the same
    encoding, byte-order mark and line endings, or converts on request;
  - counts characters the chosen encoding cannot hold, so the app can warn
    before saving;
  - re-reads an unedited file in another encoding when the guess was wrong;
  - pages through large files by line;
  - finds and replaces (match case, whole words) with UTF-16 offsets;
  - validates and re-indents JSON without reordering keys or respelling
    numbers;
  - reads Markdown and HTML into one reading model (`text_core::markup`)
    that the app draws natively instead of in a web view.
- **In the app:** `lib/screens/text_editor_screen.dart` (the editor) and
  `lib/widgets/markup_view.dart` (the Markdown and HTML page view). The
  JSON tree (`lib/widgets/json_tree_view.dart`) is drawn in Dart.

## The owner's rules

These apply here in full.

- **Everything is Rust, written from scratch**: the engine and its FFI
  layer. No C or C++ is written or vendored. `extern "C"` on exported Rust
  functions is only the calling convention `dart:ffi` needs.
- **Allowed dependencies** are only these small primitives:
  - `flate2` (raw deflate);
  - RustCrypto (`aes`, `cbc`, `sha2`, `md-5`) and `getrandom`;
  - `serde`/`serde_json`, only at the FFI boundary;
  - `thiserror`.

  The PDF engine also vendors standalone JPEG, JPEG 2000 and JBIG2
  codecs; nothing else does. Do not add document, ZIP, XML, encoding,
  Markdown, HTML, regex, spreadsheet, font or Office crates. Data such as
  code-page tables and entity tables is fine. Today `text_core` depends
  only on `doc_encoding`; `text_ffi` adds `doc_ffi`, `serde` and
  `serde_json`.
- **One library per kind of file.** This engine is a self-contained
  Flutter FFI plugin with its own Rust workspace and native library. Code
  every engine needs lives in the Rust-only `packages/doc_common` of the
  app.
- **Never damage a user's file.**
  - Edit documents in place. In zipped formats, copy unchanged ZIP
    entries raw and keep XML you do not understand. Here that means: an
    unedited file saves back byte for byte (see "Round trip" below).
  - Save atomically (`doc_ffi::write_file_atomically`: a temporary file,
    fsync, rename).
  - Formats that cannot be written back are saved as their modern
    equivalent, and the app says so. (Every text format can be written
    back.)
- **Never change package identifiers.** Keep the Dart package name
  `flutter_text_core`, the Android plugin namespace
  `com.yourmateapps.flutter_text_core`, the podspec name, the library name
  `libtext_ffi` and the `text_` symbol names. In the app, never change
  `com.yourmateapps.pdfhelper` (the Android `applicationId`, namespace and
  Kotlin package, the iOS bundle ID and its `.debug` suffix scheme) or the
  Dart package name `pdfhelper`: AdMob and Google Play are linked to them.
- **Commit or push only when the owner asks.**
  - Work on `main`. No feature branches.
  - A commit message is one plain imperative sentence saying what the
    change does for the user, without a type prefix (see `git log`:
    "Read Markdown and HTML files as formatted documents").
  - Commit and push in this repository first, then commit the updated
    gitlink in the app.

## Layout

| Path | What |
| --- | --- |
| `rust/Cargo.toml` | The workspace: `text_core` and `text_ffi`; release profile with LTO and one codegen unit |
| `rust/crates/text_core/src/document.rs` | `TextDocument`: decode through `doc_encoding::detect`, normalize line breaks to `\n` (recording the dominant style and whether styles were mixed), keep the original bytes until the first edit, `encode` back, `unmappable`, the line index, `Stats` |
| `rust/crates/text_core/src/search.rs` | `find_all`, `replace_all` (case folding by simple lowercase; a word character is alphanumeric or `_`), `utf16_offsets` |
| `rust/crates/text_core/src/json.rs` | `format` and `validate`, token by token with an explicit stack (no recursion), so keys, numbers and escapes stay as written; errors carry a 1-based line and column |
| `rust/crates/text_core/src/markup/mod.rs` | The reading model (`Document`, `Block`, `Inline`, `Style`, `Item`, `Align`), the limits, cutting to `MAX_PARTS`, table fitting, white-space collapsing |
| `rust/crates/text_core/src/markup/markdown.rs` | Markdown blocks: CommonMark with GitHub's tables and task lists; raw HTML blocks go through the HTML reader |
| `rust/crates/text_core/src/markup/inline.rs` | Markdown inlines by the CommonMark delimiter algorithm, flattened into styled runs |
| `rust/crates/text_core/src/markup/html.rs` | An HTML5-style tokenizer and a forgiving tree builder, mapped to blocks without the head, scripts, styles, forms or hidden elements |
| `rust/crates/text_core/src/markup/entities.rs` | Every HTML named character reference that stands for one character, and numeric references |
| `rust/crates/text_core/src/markup/tests.rs` | Properties both readers keep on generated, garbled and slow-to-parse input |
| `rust/crates/text_core/examples/markupcli.rs` | Developer tool: `outline` prints a file's blocks, `sweep` reads many files and reports panics and the slowest |
| `rust/crates/text_ffi/src/lib.rs` | Every exported `text_*` function and the FFI tests |
| `rust/crates/text_ffi/src/markup.rs` | The JSON shape of the reading model, documented at the top of the file |
| `rust/crates/text_ffi/build.rs` | Links Android builds with 16 KB pages; other engines copy it |
| `lib/flutter_text_core.dart` | The library: exports `markup.dart` and the public names of `text_core_api.dart` |
| `lib/src/bindings.dart` | `TextBindings`: raw lookups, library loading, `TEXT_CORE_LIB_PATH` |
| `lib/src/text_core_api.dart` | `TextCore`, `TextDocument`, `TextFileInfo`, `TextEncodingInfo`, `TextMatch`, `LineEnding`, `TextCoreException` |
| `lib/src/markup.dart` | `MarkupDocument`, sealed `MarkupBlock` (`MarkupHeading`, `MarkupParagraph`, `MarkupQuote`, `MarkupList` with `MarkupListItem`, `MarkupCode`, `MarkupRule`, `MarkupTable`), `MarkupAlign`, sealed `MarkupInline` (`MarkupText`, `MarkupImage`, `MarkupBreak`); value equality throughout |
| `test/text_core_test.dart` | 5 tests of documents, encodings, find and replace, JSON |
| `test/markup_test.dart` | 21 tests of the reading model, including random input |
| `android/` | The Android library project (no Java or Kotlin); bundles `src/main/jniLibs/<abi>/libtext_ffi.so` |
| `macos/flutter_text_core.podspec` | Vendors `macos/Frameworks/libtext_ffi.dylib` |

- There is no `ios/` folder: the plugin declares Android and macOS only.
- **Binaries are git-ignored** (`android/src/main/jniLibs/*/`,
  `macos/Frameworks/*.dylib`, `rust/target/`). Build them; never commit
  them. Whether to commit them or use Git LFS is an open owner decision.

## Building and testing

Every command below was run on the Linux cloud machine (4 CPUs, Rust
1.97, Flutter 3.47.7) and succeeded as shown. Start with:

```bash
export PATH=/opt/flutter/bin:$HOME/.cargo/bin:$PATH
```

- **It builds only inside the app checkout.** Both crates depend on
  `../../../../doc_common/crates/doc_encoding` (and `text_ffi` on
  `doc_ffi`), which resolves to the app's `packages/doc_common`. A clone of
  this repository alone does not build: clone the app and run
  `git submodule update --init --recursive`. Rust 1.88 or newer is needed
  app-wide.

**Rust, from `packages/flutter_text_core/rust`:**

```bash
cargo build --release                       # target/release/libtext_ffi.so
cargo test --release                        # text_core 87 passed, text_ffi 6 passed
cargo clippy --release --all-targets -- -D warnings   # clean
cargo run --release --example markupcli -- outline ../README.md
find ../../.. \( -name target -o -name build -o -name .dart_tool -o -name test_corpus -o -name .git \) -prune \
  -o \( -name '*.md' -o -name '*.html' \) -type f -print | ./target/release/examples/markupcli sweep
```

- `cargo test --release` took about 40 s when its test binaries had to be
  built, and 3 s to run. In debug the `text_core` tests take 18 s (the
  markup property tests).
- The sweep read the app checkout's 45 Markdown and HTML files: about
  2,200 blocks, 0 panics, 11 ms in all. `sweep` also takes file names as
  arguments. HTML is chosen by the `.html`, `.htm` or `.xhtml` extension;
  files are decoded as `TextDocument` would.

**Dart, from `packages/flutter_text_core`:**

```bash
TEXT_CORE_LIB_PATH=$PWD/rust/target/release/libtext_ffi.so flutter test   # 26 passed
flutter analyze                                                          # No issues found
```

Without `TEXT_CORE_LIB_PATH` the same run reports 1 passed and 25 skipped
and still says "All tests passed!": the tests skip themselves when
`TextCore.isAvailable` is false. Always set it.

**Through the app's scripts, from the app root:**

```bash
bash scripts/build_libs.sh host --only text     # release build for this machine
bash scripts/build_libs.sh test --only text     # cargo test (debug): all of doc_common, then text
eval "$(bash scripts/build_libs.sh env --only text)"   # exports TEXT_CORE_LIB_PATH (and PDF_CORE_LIB_PATH)
flutter test test/screens/text_editor_screen_test.dart test/widgets/markup_view_test.dart   # 48 passed
ANDROID_NDK_HOME=<ndk> bash scripts/build_libs.sh android --only text
```

- On Linux, `host` also builds the PDF engine's host library, whatever
  `--only` says. `test` always runs every `doc_common` test first; the
  whole run took 42 s here with warm caches. On a Mac, `host` puts a dylib in
  `macos/Frameworks/`, and `macos` builds a universal one.
- **Android.** `android` runs `cargo ndk -t arm64-v8a -t armeabi-v7a -t
  x86_64 -o android/src/main/jniLibs build -p text_ffi --release`. It
  needs `cargo-ndk`, the targets `aarch64-linux-android`,
  `armv7-linux-androideabi` and `x86_64-linux-android`, and an NDK (the
  script finds `$ANDROID_HOME/ndk/<latest>` when `ANDROID_NDK_HOME` is
  unset). On the cloud machine the NDK was
  `/home/user/android-ndk/android-ndk-r27c`; elsewhere fetch
  `dl.google.com/android/repository/android-ndk-r27c-linux.zip`.
- **16 KB pages.** `text_ffi/build.rs` passes `-z max-page-size=16384` and
  `-z common-page-size=16384` to Android links. Checked with
  `readelf -lW android/src/main/jniLibs/<abi>/libtext_ffi.so`: every
  `LOAD` segment is aligned `0x4000` on all three ABIs. In an APK, the
  app's `python3 scripts/check_android_native.py <apk>` checks it.
- **The app's Android preflight** (`verifyEngines` in
  `android/app/build.gradle.kts`) refuses a build when a `jniLibs` binary
  is missing or older than the newest `.rs` or `Cargo.toml` under this
  package's `rust/crates` or anywhere under `doc_common/crates`. When cargo
  finds the build up to date, `cargo ndk` leaves the copied files' times
  alone (seen here), so after a change to a `doc_common` crate this engine
  does not use, the preflight can still refuse. Rebuild; if cargo reports
  nothing to do, `touch` the three libraries.

## The FFI layer

Shared conventions come from `doc_ffi` in `packages/doc_common`.

- Rust functions exported with `#[no_mangle] pub extern "C"`, all named
  `text_*`. There is no C code.
- Strings are UTF-8 and NUL-terminated. Returned strings belong to the
  caller and are freed with `text_free_string`. `text_last_error` and
  `text_core_version` return borrowed static or thread-local strings:
  never free them.
- Failures return -1 or NULL; `text_last_error` then gives
  `"CODE: message"` for this thread. Codes this engine raises: `IO`,
  `INVALID_ARGUMENT` (missing or non-UTF-8 argument, invalid JSON, bad
  line count), `UNSUPPORTED` (unknown encoding, reinterpreting an edited
  file), `TOO_LARGE`, `BAD_HANDLE`, `PANIC`, `ERROR`. The shared set also
  has `ENCRYPTED`, `WRONG_PASSWORD`, `CORRUPT` and `NOT_FOUND`.
- Every call runs inside `call_i64` or `call_str`, which catch panics and
  turn them into `PANIC`. Allocation aborts and stack overflows cannot be
  caught; bound inputs instead.
- Open documents live in `static DOCS: Registry<TextDocument>`. Handles
  start at 1, are process-wide and usable from any isolate; each session
  has its own lock.
- Structured results are JSON (serde only in `text_ffi`). Offsets given to
  Dart are UTF-16 code units; lines are 0-based.

**Exported functions:**

| Purpose | Functions |
| --- | --- |
| Library | `text_core_version`, `text_last_error`, `text_free_string`, `text_encodings_json` (`[{"name","unicode"}]`, Unicode first) |
| Sessions | `text_open(path, fallback_encoding)` (NULL fallback means windows-1252), `text_new`, `text_close` |
| Reading | `text_info_json(handle, with_stats)` (encoding, bom, lineEnding, mixedLineEndings, decodeErrors, confident, binary, modified, canReinterpret, lines; with stats also words, characters, length, unmappable), `text_get`, `text_lines_json(handle, first, count)` (at most 100,000 lines) |
| Editing | `text_set` (whole text; 1 if changed), `text_find_json(handle, query, flags, limit)` (`[[start, end, line], ...]`), `text_replace_all(handle, query, replacement, flags)`; flags: 1 match case, 2 whole words |
| Format | `text_set_format(handle, encoding, bom, line_ending)` (NULL or "" leaves a setting; `bom` -1/0/1, only for Unicode encodings; a single-byte encoding drops the BOM), `text_reinterpret(handle, encoding)` |
| Saving | `text_save(handle, path)` (atomic, marks saved), `text_write_copy(handle, path)` (atomic, does not mark saved); both return how many characters were written as `?` |
| Stateless | `text_format_json(text, indent)` (0 compacts), `text_markdown_json(text)`, `text_html_json(text)` |

## The Dart API

- `TextBindings` loads `TEXT_CORE_LIB_PATH` when set; otherwise
  `libtext_ffi.so` on Android and Linux, the process and then
  `libtext_ffi.dylib` on macOS, `text_ffi.dll` on Windows. One instance
  per isolate.
- `text_markdown_json` and `text_html_json` are looked up optionally, so
  an older library still loads; `TextCore.canReadMarkup` says whether they
  are there. Other functions are required.
- Heavy calls run in `Isolate.run`: `TextCore.open`, `TextDocument.save`,
  `TextDocument.writeCopy`, `TextCore.markdownAsync` and `htmlAsync`.
  Everything else runs on the calling isolate, including `info(withStats:
  true)`, `find`, `replaceAll`, `lines` and `formatJson` (phase 10 moves
  the slow ones).
- Errors throw `TextCoreException(code, message)`, parsed from
  `"CODE: message"`. A closed `TextDocument` throws `StateError`; `close`
  is safe twice.
- `TextMatch` offsets are UTF-16, ready for a `TextEditingController`.

## Engine conventions and pitfalls

**Round trip.**

- Text is held as Unicode with `\n` breaks. `encode` writes the chosen
  line ending, encoding and BOM. For a cleanly decoded file with one
  line-ending style, an unedited save is byte-identical; Rust tests cover
  CRLF, UTF-16LE with a BOM, and windows-1251 read as windows-1252 and
  then reinterpreted.
- Where it is not yet lossless (phase 10, B8):
  - mixed line endings are saved in the dominant style
    (`mixed_line_endings` is reported, but the app offers no choice);
  - undecodable input becomes U+FFFD and is counted in `decodeErrors`; it
    does not save back as the original bytes (a lone surrogate in UTF-16,
    for example);
  - nine code pages fail a 256-byte round trip: windows-1253, 1255, 1257
    and 874, and ISO-8859-3, -6, -7, -8 and -11;
  - characters a single-byte encoding cannot hold are written as `?`;
    callers check `unmappable` and warn first (the editor offers UTF-8).
- The original bytes are kept until the first edit, so `reinterpret`
  works only before it. That doubles memory for big files.

**Detection.** In order: a byte-order mark; BOM-less UTF-16 by its zero
bytes; valid UTF-8 (ASCII counts); otherwise a single-byte code page,
windows-1251 when the high bytes look Cyrillic, else the fallback, with
`confident` false. `binary` means zero bytes outside UTF-16/32 or many
control characters in the first 64 KB. Multi-byte East Asian code pages
(Shift_JIS, GBK, Big5, EUC) are not supported.

**Limits.**

| Limit | Value | Where |
| --- | --- | --- |
| File opened | 256 MB, else `TOO_LARGE` | `MAX_FILE_BYTES`, `text_ffi` |
| Markdown or HTML text | 32 MB, else `TOO_LARGE` | `MAX_MARKUP_BYTES`, `text_ffi` |
| Lines per `text_lines_json` | 100,000 | `text_ffi` |
| JSON nesting | 10,000 | `json::MAX_DEPTH` |
| Parts of a reading document (blocks, items, cells, runs) | 100,000, then `truncated` | `markup::MAX_PARTS` |
| Nodes either markup reader builds | 300,000 | `MAX_NODES` |
| Quote and list nesting | 32, deeper is flattened | `MAX_NESTING` |
| HTML element depth; open formatting elements | 128; 64 | `html.rs` |
| Table cells; columns | 1,048,576 (then rows become paragraphs); 256 | `MAX_TABLE_CELLS`, `MAX_COLUMNS` |

The app adds its own: files over 300,000 UTF-16 units open read-only and
paged, Markdown over about 1 MB and anything over about 5 MB get no
preview (`TextEditorScreen`).

**Markup reader.**

- Neither reader fails: odd input still gives blocks, and time stays
  linear on input built to be slow (`markup/tests.rs` checks both).
- The JSON shape is documented at the top of `text_ffi/src/markup.rs`;
  `false` flags, absent links and non-task `checked` are left out. Change
  that file, `lib/src/markup.dart` and the shape checks in `text_ffi`'s
  tests together.
- Links to nowhere and `javascript:`/`vbscript:` links read as plain text.
  `<u>`, `<sup>` and `<sub>` read as plain text (no underline or raised
  text in the model).

**Strings and NUL.** Strings cross as NUL-terminated C strings. A NUL in
an argument cuts it short there, and `into_c_string` turns a NUL in a
returned string into a space. So `TextDocument.text` shows a file's NUL
characters as spaces, and a caller that edits that text and sets it back
saves spaces. The Dart
markup calls replace NUL with U+FFFD before calling; `setText`, `find`
and `replaceAll` do not.

**Style.** There is no `rustfmt.toml`, and the code is hand-formatted to
about 120 columns: `cargo fmt --check` reports some 270 differences. Do
not run `cargo fmt` over the workspace; match the surrounding style.
`dart format` the Dart files you change.

**Adding a function.** Export it in `text_ffi/src/lib.rs` through
`call_i64` or `call_str`, with a Rust test; add its typedef and lookup in
`lib/src/bindings.dart` and a method in `lib/src/text_core_api.dart`
(through `Isolate.run` when it can be slow); add a Dart test; then
rebuild the host library and the Android libraries.

**Tooling on the owner's Mac.** Cargo lives in `~/.cargo/bin`, which is
not on `PATH`; zsh does not word-split unquoted variables (use `xargs`);
BSD `sed` has no `\b`. Device checks go on the emulator
`pdfhelper_api36_16kb`, not his phones.

## Where work stands and what is next

- Phase 2 built the engine and the text editor; phase 9 added the Markdown
  and HTML reader. The engine's code is committed and pushed; its last
  commit is "Keep Markdown and HTML previews quick and small on very large
  or hostile pages". The Markdown and HTML views have not run on a device
  or emulator yet; the editor was last checked on one in phase 2.
- Phase 10 (hardening and release) is planned in the app's
  `docs/PHASE10_PLAN.md`. This engine is in **B8, Text and archive**
  (wave 2, not started), which also covers `doc_encoding`,
  `text_editor_screen.dart` and the archive engine:
  1. Lossless round trip: map undecodable bytes to private-use code
     points that encode back to the same bytes; fix the nine code pages;
     until then warn with a count and offer Save a copy; offer a
     line-ending choice for mixed files.
  2. Off the UI isolate: `info(withStats)` in `Isolate.run` with lazy
     stats, `lines()` borrowing the line index instead of copying it, find
     and replace in an isolate for large files (a 64 MB file takes 1.3 s;
     each page of a large file 79 ms).
  3. Format JSON: cap the output (8 times the input or 32 MB), stop
     indenting past 64 levels, run it in `Isolate.run` (50 KB formats to
     250 MB today).
  4. A verify check for B1's `doc_ffi::save_verified`, and `logError` in
     the editor.
  5. A lower text-file limit on 32-bit phones; no `original` copy for
     valid UTF-8.

  Verify: `cargo test`; every code page round-trips all 256 bytes; an
  unedited UTF-16 file with a lone surrogate saves byte-identical;
  depth-10,000 JSON gives a clear error; `flutter test` for the package and
  the text editor.
- Other batches reach this engine: B1 changes `doc_ffi` under it
  (`save_verified` with a backup, poisoned sessions refuse edits, a
  16 MB-stack backstop around every call); B9 adds a text sweep and a
  mutation fuzzer, run again with `RUST_MIN_STACK=524288`; B10 adds draft
  autosave to the text editor.
- `doc_encoding` is shared with the sheet, word and slide engines (CSV,
  RTF, legacy Office). After changing it, run
  `bash scripts/build_libs.sh test` and `host` for all engines, then
  the app's `flutter test` with the env exports.
- **2026-10-19 (uncommitted):** `tests/mutate.rs` (encodings round-trip
  byte for byte; markup, JSON and search fuzz); the text editor keeps
  drafts (`EditorDraft`).
- Later (PLAN §4.5): our own regex engine and syntax highlighting.
