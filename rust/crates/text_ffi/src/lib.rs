//! The FFI layer of the text engine, in Rust, for `dart:ffi`.
//!
//! Conventions are those of `doc_ffi`: UTF-8 strings, caller-owned results
//! freed with [`text_free_string`], negative or NULL results on failure with
//! the reason in [`text_last_error`]. Offsets given to and returned from
//! Dart are in UTF-16 code units, which is how Dart strings count.

use std::ffi::c_char;

use doc_ffi::{
    call_i64, call_str, codes, last_error_ptr, opt_str_arg, read_file, save_verified, str_arg,
    FfiError, Registry,
};
use serde::Serialize;
use text_core::search::{self, SearchOptions};
use text_core::{json, Encoding, LineEnding, TextDocument};

pub use text_core;

mod markup;

/// Files larger than this are refused rather than read into memory. A
/// 32-bit phone's process has far less room, and the text is held twice
/// (bytes and characters), so it gets half.
#[cfg(target_pointer_width = "64")]
const MAX_FILE_BYTES: u64 = 256 << 20;
#[cfg(not(target_pointer_width = "64"))]
const MAX_FILE_BYTES: u64 = 128 << 20;

/// Markdown or HTML larger than this is refused by the reading view.
const MAX_MARKUP_BYTES: usize = 32 << 20;
const MAX_QUERY_BYTES: usize = 1 << 20;
const MAX_PAGE_BYTES: usize = 4 << 20;
const MAX_GET_BYTES: usize = 32 << 20;

fn check_text_size(text: &str, max: usize) -> Result<(), FfiError> {
    if text.len() > max {
        return Err(FfiError::new(codes::TOO_LARGE, "this text operation exceeds its size limit"));
    }
    Ok(())
}

unsafe fn json_text_arg(text: *const c_char, name: &str, max: usize) -> Result<String, FfiError> {
    let json = str_arg(text, name)?;
    check_text_size(json, max.saturating_mul(6).saturating_add(2))?;
    let text: String = serde_json::from_str(json).map_err(|e| FfiError::invalid(e.to_string()))?;
    check_text_size(&text, max)?;
    Ok(text)
}

static DOCS: Registry<TextDocument> = Registry::new();

fn encoding(label: &str) -> Result<Encoding, FfiError> {
    Encoding::from_label(label).ok_or_else(|| FfiError::unsupported(format!("unknown encoding '{label}'")))
}

/// Library version (static; do not free).
#[no_mangle]
pub extern "C" fn text_core_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// This thread's last error, `"CODE: message"` (borrowed; do not free).
#[no_mangle]
pub extern "C" fn text_last_error() -> *const c_char {
    last_error_ptr()
}

/// Free a string returned by this library.
///
/// # Safety
/// `ptr` must be NULL or a string from this library, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn text_free_string(ptr: *mut c_char) {
    doc_ffi::free_c_string(ptr)
}

/// Open a text file. `fallback_encoding` (a label such as `windows-1252`)
/// is used when the bytes could be any single-byte code page; NULL means
/// Windows-1252. Returns a handle, or -1.
///
/// # Safety
/// String arguments must be NULL or valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_open(path: *const c_char, fallback_encoding: *const c_char) -> i64 {
    call_i64(|| {
        let path = str_arg(path, "path")?;
        let fallback = match opt_str_arg(fallback_encoding, "fallback encoding")? {
            Some(label) => encoding(label)?,
            None => Encoding::WINDOWS_1252,
        };
        let bytes = read_file(path, MAX_FILE_BYTES)?;
        let doc = TextDocument::from_bytes_limited(bytes, fallback, MAX_FILE_BYTES as usize)
            .map_err(|message| FfiError::new(codes::TOO_LARGE, message))?;
        Ok(DOCS.insert(doc))
    })
}

/// A new, empty UTF-8 document. Returns a handle.
#[no_mangle]
pub extern "C" fn text_new() -> i64 {
    call_i64(|| Ok(DOCS.insert(TextDocument::new())))
}

/// Close a document. Returns 0, or -1 for an unknown handle.
#[no_mangle]
pub extern "C" fn text_close(handle: i64) -> i64 {
    call_i64(|| {
        if DOCS.remove(handle) {
            Ok(0)
        } else {
            Err(FfiError::new(codes::BAD_HANDLE, format!("no open document has handle {handle}")))
        }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    encoding: &'static str,
    bom: bool,
    line_ending: &'static str,
    mixed_line_endings: bool,
    decode_errors: usize,
    confident: bool,
    binary: bool,
    modified: bool,
    can_reinterpret: bool,
    lines: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    words: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    characters: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unmappable: Option<usize>,
}

/// The document's format and, when `with_stats` is non-zero, its word,
/// character and UTF-16 length counts and unmappable characters (these read
/// the whole text).
#[no_mangle]
pub extern "C" fn text_info_json(handle: i64, with_stats: i32) -> *mut c_char {
    call_str(|| {
        DOCS.with(handle, |doc| {
            let stats = (with_stats != 0).then(|| doc.stats());
            let info = Info {
                encoding: doc.encoding.name(),
                bom: doc.bom,
                line_ending: doc.line_ending.name(),
                mixed_line_endings: doc.mixed_line_endings,
                decode_errors: doc.decode_errors,
                confident: doc.confident,
                binary: doc.binary,
                modified: doc.is_modified(),
                can_reinterpret: doc.can_reinterpret(),
                lines: doc.line_count(),
                words: stats.map(|s| s.words),
                characters: stats.map(|s| s.characters),
                length: stats.map(|s| s.utf16_len),
                unmappable: (with_stats != 0).then(|| doc.unmappable()),
            };
            serde_json::to_string(&info).map_err(|e| FfiError::new(codes::ERROR, e.to_string()))
        })
    })
}

/// The whole text, with `\n` line breaks.
#[no_mangle]
pub extern "C" fn text_get(handle: i64) -> *mut c_char {
    call_str(|| DOCS.with(handle, |doc| {
        check_text_size(doc.text(), MAX_GET_BYTES)?;
        if doc.text().contains('\0') { return Err(FfiError::invalid("use text_get_json for text containing NUL")); }
        Ok(doc.text().to_owned())
    }))
}

#[no_mangle]
pub extern "C" fn text_get_json(handle: i64) -> *mut c_char {
    call_str(|| DOCS.with(handle, |doc| {
        check_text_size(doc.text(), MAX_GET_BYTES)?;
        serde_json::to_string(doc.text()).map_err(|e| FfiError::new(codes::ERROR, e.to_string()))
    }))
}

/// Replace the whole text. Returns 1 if it changed, 0 if not.
///
/// # Safety
/// `text` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_set(handle: i64, text: *const c_char) -> i64 {
    call_i64(|| {
        let text = str_arg(text, "text")?;
        check_text_size(text, MAX_FILE_BYTES as usize)?;
        DOCS.with(handle, |doc| Ok(doc.set_text(text.to_owned()) as i64))
    })
}

#[no_mangle]
pub unsafe extern "C" fn text_set_json(handle: i64, text: *const c_char) -> i64 {
    call_i64(|| {
        let text = json_text_arg(text, "text", MAX_FILE_BYTES as usize)?;
        DOCS.with(handle, |doc| Ok(doc.set_text(text) as i64))
    })
}

/// Lines `first..first + count` as a JSON array of strings.
#[no_mangle]
pub extern "C" fn text_lines_json(handle: i64, first: i64, count: i64) -> *mut c_char {
    call_str(|| {
        let first = usize::try_from(first).map_err(|_| FfiError::invalid("first line is negative"))?;
        let count = usize::try_from(count).map_err(|_| FfiError::invalid("line count is negative"))?;
        DOCS.with(handle, |doc| {
            let lines = doc.lines_limited(first, count.min(100_000), MAX_PAGE_BYTES)
                .map_err(|message| FfiError::new(codes::TOO_LARGE, message))?;
            serde_json::to_string(&lines).map_err(|e| FfiError::new(codes::ERROR, e.to_string()))
        })
    })
}

fn options(flags: i32) -> SearchOptions {
    SearchOptions {
        case_sensitive: flags & 1 != 0,
        whole_word: flags & 2 != 0,
    }
}

/// Find `query`: a JSON array of `[start, end, line]`, with UTF-16 offsets
/// and 0-based line numbers, at most `limit` of them. `flags`: 1 = match
/// case, 2 = whole words.
///
/// # Safety
/// `query` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_find_json(handle: i64, query: *const c_char, flags: i32, limit: i64) -> *mut c_char {
    call_str(|| find(handle, str_arg(query, "query")?, flags, limit))
}

fn find(handle: i64, query: &str, flags: i32, limit: i64) -> Result<String, FfiError> {
    check_text_size(query, MAX_QUERY_BYTES)?;
    let limit = usize::try_from(limit).unwrap_or(0).min(10_000);
    DOCS.with(handle, |doc| {
        let matches = search::find_all(doc.text(), query, options(flags), limit);
        let mut at = 0;
        let mut line = 0;
        let lines: Vec<usize> = matches.iter().map(|m| {
            line += doc.text().as_bytes()[at..m.start].iter().filter(|&&b| b == b'\n').count();
            at = m.start;
            line
        }).collect();
        let bytes: Vec<usize> = matches.iter().flat_map(|m| [m.start, m.end]).collect();
        let offsets = search::utf16_offsets(doc.text(), &bytes);
        let found: Vec<[usize; 3]> = offsets.chunks(2).zip(lines).map(|(p, line)| [p[0], p[1], line]).collect();
        serde_json::to_string(&found).map_err(|e| FfiError::new(codes::ERROR, e.to_string()))
    })
}

#[no_mangle]
pub unsafe extern "C" fn text_find_query_json(handle: i64, query: *const c_char, flags: i32, limit: i64) -> *mut c_char {
    call_str(|| find(handle, &json_text_arg(query, "query", MAX_QUERY_BYTES)?, flags, limit))
}

/// Replace every match of `query`. Returns the number replaced.
///
/// # Safety
/// String arguments must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_replace_all(
    handle: i64,
    query: *const c_char,
    replacement: *const c_char,
    flags: i32,
) -> i64 {
    call_i64(|| replace_all(handle, str_arg(query, "query")?, str_arg(replacement, "replacement")?, flags, MAX_FILE_BYTES as usize))
}

fn replace_all(handle: i64, query: &str, replacement: &str, flags: i32, max_bytes: usize) -> Result<i64, FfiError> {
    check_text_size(query, MAX_QUERY_BYTES)?;
    check_text_size(replacement, MAX_QUERY_BYTES)?;
    DOCS.with(handle, |doc| {
        let (text, count) = search::replace_all_limited(doc.text(), query, replacement, options(flags), max_bytes.min(MAX_FILE_BYTES as usize))
            .map_err(|message| FfiError::new(codes::TOO_LARGE, message))?;
        if count > 0 { doc.set_text(text); }
        Ok(count as i64)
    })
}

#[no_mangle]
pub unsafe extern "C" fn text_replace_all_json(
    handle: i64, query: *const c_char, replacement: *const c_char, flags: i32, max_bytes: i64,
) -> i64 {
    call_i64(|| {
        let max_bytes = usize::try_from(max_bytes).map_err(|_| FfiError::invalid("replacement limit is invalid"))?;
        replace_all(handle, &json_text_arg(query, "query", MAX_QUERY_BYTES)?,
            &json_text_arg(replacement, "replacement", MAX_QUERY_BYTES)?, flags, max_bytes)
    })
}

/// Choose how the file is written. NULL leaves a setting alone; `bom` is
/// -1 (leave), 0 or 1. Returns 0.
///
/// # Safety
/// String arguments must be NULL or valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_set_format(
    handle: i64,
    encoding_label: *const c_char,
    bom: i32,
    line_ending: *const c_char,
) -> i64 {
    call_i64(|| {
        let new_encoding = opt_str_arg(encoding_label, "encoding")?.map(encoding).transpose()?;
        let new_ending = match opt_str_arg(line_ending, "line ending")? {
            Some(name) => Some(
                LineEnding::from_name(name).ok_or_else(|| FfiError::invalid(format!("unknown line ending '{name}'")))?,
            ),
            None => None,
        };
        DOCS.with(handle, |doc| {
            doc.set_format(new_encoding, (bom >= 0).then_some(bom != 0), new_ending);
            Ok(0)
        })
    })
}

/// Re-read the unedited file in another encoding. Returns 0.
///
/// # Safety
/// `encoding_label` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_reinterpret(handle: i64, encoding_label: *const c_char) -> i64 {
    call_i64(|| {
        let target = encoding(str_arg(encoding_label, "encoding")?)?;
        DOCS.with(handle, |doc| {
            doc.reinterpret(target).map_err(FfiError::unsupported)?;
            Ok(0)
        })
    })
}

/// Write the current text to `path` (atomically) without marking the
/// document saved, for sharing a snapshot. Returns the number of
/// characters the encoding could not represent.
///
/// # Safety
/// `path` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_write_copy(handle: i64, path: *const c_char) -> i64 {
    call_i64(|| {
        let path = str_arg(path, "path")?;
        DOCS.with(handle, |doc| {
            let (bytes, unmappable) = doc.encode();
            save_verified(path, &bytes, |_| Ok(()))?;
            Ok(unmappable as i64)
        })
    })
}

/// Save to `path` (atomically). Returns the number of characters the
/// encoding could not represent, which were written as `?`; the caller
/// should have warned about them first.
///
/// # Safety
/// `path` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_save(handle: i64, path: *const c_char) -> i64 {
    call_i64(|| {
        let path = str_arg(path, "path")?;
        DOCS.with(handle, |doc| {
            let (bytes, unmappable) = doc.encode();
            // Read back and compared before it replaces the file, with the
            // previous version kept as a backup when a folder is set.
            save_verified(path, &bytes, |_| Ok(()))?;
            doc.mark_saved();
            Ok(unmappable as i64)
        })
    })
}

#[derive(Serialize)]
struct EncodingInfo {
    name: &'static str,
    unicode: bool,
}

/// Every supported encoding: `[{"name": "UTF-8", "unicode": true}, ...]`.
#[no_mangle]
pub extern "C" fn text_encodings_json() -> *mut c_char {
    call_str(|| {
        let all: Vec<EncodingInfo> = Encoding::all()
            .into_iter()
            .map(|e| EncodingInfo {
                name: e.name(),
                unicode: e.is_unicode(),
            })
            .collect();
        serde_json::to_string(&all).map_err(|e| FfiError::new(codes::ERROR, e.to_string()))
    })
}

/// Re-indent JSON (`indent` spaces; 0 compacts it). On invalid JSON returns
/// NULL with an `INVALID_ARGUMENT` error naming the line and column.
///
/// # Safety
/// `text` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_format_json(text: *const c_char, indent: i32) -> *mut c_char {
    call_str(|| format_json(str_arg(text, "text")?, indent))
}

fn format_json(text: &str, indent: i32) -> Result<String, FfiError> {
    check_text_size(text, json::MAX_OUTPUT)?;
    json::format(text, indent.max(0) as usize).map_err(|e| {
        if e.too_large { FfiError::new(codes::TOO_LARGE, e.message) } else { FfiError::invalid(e.to_string()) }
    })
}

#[no_mangle]
pub unsafe extern "C" fn text_format_json_input(text: *const c_char, indent: i32) -> *mut c_char {
    call_str(|| format_json(&json_text_arg(text, "text", json::MAX_OUTPUT)?, indent))
}

/// Markup text for [`text_markdown_json`] and [`text_html_json`].
///
/// # Safety
/// `text` must be NULL or valid NUL-terminated UTF-8.
unsafe fn markup_arg<'a>(text: *const c_char) -> Result<&'a str, FfiError> {
    let text = str_arg(text, "text")?;
    if text.len() > MAX_MARKUP_BYTES {
        return Err(FfiError::new(
            codes::TOO_LARGE,
            format!("{} MB is more than the reading view takes ({} MB)", text.len() >> 20, MAX_MARKUP_BYTES >> 20),
        ));
    }
    Ok(text)
}

fn markup_json(doc: &text_core::markup::Document) -> Result<String, FfiError> {
    serde_json::to_string(&markup::Json(doc)).map_err(|e| FfiError::new(codes::ERROR, e.to_string()))
}

/// Markdown (CommonMark with GitHub's tables, strikethrough, task lists and
/// bare links) as blocks to draw: `{"title": ..., "blocks": [...]}`, in the
/// shape `markup.rs` describes. Odd input never fails; NULL only for text
/// that is not UTF-8 or is over 32 MB (`TOO_LARGE`).
///
/// # Safety
/// `text` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_markdown_json(text: *const c_char) -> *mut c_char {
    call_str(|| markup_json(&text_core::markup::markdown(markup_arg(text)?)))
}

/// An HTML page as blocks to draw, in the shape of [`text_markdown_json`];
/// the title is the page's `<title>` or else its first `<h1>`.
///
/// # Safety
/// `text` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_html_json(text: *const c_char) -> *mut c_char {
    call_str(|| markup_json(&text_core::markup::html(markup_arg(text)?)))
}


/// Keep the version a save replaces in `dir`, a folder in the app's private
/// storage (at most 500 MB of them in all, oldest dropped first); NULL or
/// `""` keeps none. This library keeps its own setting, so the app sets it
/// in every engine. Returns 0, or -1 with the last error set.
///
/// # Safety
/// `dir` must be NULL or valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_set_backup_dir(dir: *const c_char) -> i64 {
    doc_ffi::set_backup_dir_c(dir)
}

/// Remove the temporary files that saves left in `dir` when the app was
/// killed mid-save, if at least `min_age_seconds` old (so a save in
/// progress is left alone). Every engine names them the same way, so this
/// one call cleans up after all of them. Returns how many were removed, or
/// -1 with the last error set.
///
/// # Safety
/// `dir` must be valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn text_remove_stale_temp_files(dir: *const c_char, min_age_seconds: i64) -> i64 {
    doc_ffi::remove_stale_temp_files_c(dir, min_age_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    fn take(ptr: *mut c_char) -> String {
        assert!(!ptr.is_null(), "{}", doc_ffi::last_error());
        let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { text_free_string(ptr) };
        text
    }

    fn c(text: &str) -> CString {
        CString::new(text).unwrap()
    }

    #[test]
    fn open_edit_save_round_trip() {
        let dir = std::env::temp_dir().join(format!("text_ffi_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, b"Total: 100\r\ntotal again\r\n").unwrap();
        let cpath = c(path.to_str().unwrap());
        let handle = unsafe { text_open(cpath.as_ptr(), std::ptr::null()) };
        assert!(handle > 0, "{}", doc_ffi::last_error());

        let info: serde_json::Value = serde_json::from_str(&take(text_info_json(handle, 1))).unwrap();
        assert_eq!(info["encoding"], "UTF-8");
        assert_eq!(info["lineEnding"], "CRLF");
        assert_eq!(info["lines"], 3);
        assert_eq!(info["words"], 4);
        assert_eq!(take(text_get(handle)), "Total: 100\ntotal again\n");

        let found = take(unsafe { text_find_json(handle, c("total").as_ptr(), 0, 10) });
        assert_eq!(found, "[[0,5,0],[11,16,1]]");
        assert_eq!(take(unsafe { text_find_json(handle, c("total").as_ptr(), 1, 10) }), "[[11,16,1]]");
        assert_eq!(unsafe { text_replace_all(handle, c("total").as_ptr(), c("Sum").as_ptr(), 0) }, 2);
        assert_eq!(take(text_lines_json(handle, 0, 2)), r#"["Sum: 100","Sum again"]"#);

        assert_eq!(unsafe { text_set(handle, c("₹ 500\nend").as_ptr()) }, 1);
        assert_eq!(unsafe { text_set_format(handle, c("windows-1252").as_ptr(), -1, std::ptr::null()) }, 0);
        assert_eq!(unsafe { text_save(handle, cpath.as_ptr()) }, 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"? 500\r\nend");
        assert_eq!(unsafe { text_set_format(handle, c("UTF-8").as_ptr(), 1, c("LF").as_ptr()) }, 0);
        assert_eq!(unsafe { text_save(handle, cpath.as_ptr()) }, 0);
        assert_eq!(std::fs::read(&path).unwrap(), "\u{FEFF}₹ 500\nend".as_bytes());

        assert_eq!(text_close(handle), 0);
        assert_eq!(text_close(handle), -1);
        assert!(doc_ffi::last_error().starts_with("BAD_HANDLE"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn json_text_boundary_keeps_nul_and_same_length_edits() {
        let handle = text_new();
        let input = c(&serde_json::to_string("a\0b\0c").unwrap());
        assert_eq!(unsafe { text_set_json(handle, input.as_ptr()) }, 1);
        assert_eq!(unsafe { text_set_json(handle, input.as_ptr()) }, 0);
        assert!(text_get(handle).is_null());
        assert!(doc_ffi::last_error().starts_with("INVALID_ARGUMENT"));
        let text: String = serde_json::from_str(&take(text_get_json(handle))).unwrap();
        assert_eq!(text, "a\0b\0c");
        let query = c(&serde_json::to_string("\0b").unwrap());
        assert_eq!(take(unsafe { text_find_query_json(handle, query.as_ptr(), 0, 10) }), "[[1,3,0]]");
        let replacement = c(&serde_json::to_string("X\0").unwrap());
        assert_eq!(unsafe { text_replace_all_json(handle, query.as_ptr(), replacement.as_ptr(), 0, 1024) }, 1);
        let text: String = serde_json::from_str(&take(text_get_json(handle))).unwrap();
        assert_eq!(text, "aX\0\0c");
        let changed = c(&serde_json::to_string("zX\0\0c").unwrap());
        assert_eq!(unsafe { text_set_json(handle, changed.as_ptr()) }, 1);
        let path = std::env::temp_dir().join(format!("text_nul_{}.txt", std::process::id()));
        assert_eq!(unsafe { text_save(handle, c(path.to_str().unwrap()).as_ptr()) }, 0);
        assert_eq!(std::fs::read(&path).unwrap(), b"zX\0\0c");
        std::fs::remove_file(path).unwrap();
        let json = c(&serde_json::to_string("{}\0ignored").unwrap());
        assert!(unsafe { text_format_json_input(json.as_ptr(), 2) }.is_null());
        assert!(doc_ffi::last_error().starts_with("INVALID_ARGUMENT"));
        text_close(handle);
    }

    #[test]
    fn queries_and_replacement_refuse_excess_without_changing_the_document() {
        let handle = text_new();
        let input = c(&"a".repeat(20_000));
        assert_eq!(unsafe { text_set(handle, input.as_ptr()) }, 1);
        let found: serde_json::Value = serde_json::from_str(&take(unsafe {
            text_find_json(handle, c("a").as_ptr(), 0, i64::MAX)
        })).unwrap();
        assert_eq!(found.as_array().unwrap().len(), 10_000);
        assert_eq!(unsafe { text_replace_all(handle, c("a").as_ptr(), c(&"b".repeat(32_768)).as_ptr(), 0) }, -1);
        assert!(doc_ffi::last_error().starts_with("TOO_LARGE"));
        assert_eq!(take(text_get(handle)), "a".repeat(20_000));
        let query = c(&"a".repeat(MAX_QUERY_BYTES + 1));
        assert!(unsafe { text_find_json(handle, query.as_ptr(), 0, 1) }.is_null());
        assert!(doc_ffi::last_error().starts_with("TOO_LARGE"));
        let huge_line = c(&"x".repeat(MAX_PAGE_BYTES + 1));
        assert_eq!(unsafe { text_set(handle, huge_line.as_ptr()) }, 1);
        assert!(text_lines_json(handle, 0, 1).is_null());
        assert!(doc_ffi::last_error().starts_with("TOO_LARGE"));
        text_close(handle);
    }

    #[test]
    fn errors_are_reported() {
        let missing = c("/definitely/not/here.txt");
        assert_eq!(unsafe { text_open(missing.as_ptr(), std::ptr::null()) }, -1);
        assert!(doc_ffi::last_error().starts_with("IO: here.txt"));
        assert_eq!(unsafe { text_open(missing.as_ptr(), c("klingon").as_ptr()) }, -1);
        assert!(doc_ffi::last_error().starts_with("UNSUPPORTED"));
        assert!(unsafe { text_format_json(c("{oops}").as_ptr(), 2) }.is_null());
        assert_eq!(doc_ffi::last_error(), "INVALID_ARGUMENT: line 1, column 2: expected a key in double quotes");
        assert_eq!(take(unsafe { text_format_json(c("[1,2]").as_ptr(), 0) }), "[1,2]");
        let encodings = take(text_encodings_json());
        assert!(encodings.starts_with(r#"[{"name":"UTF-8","unicode":true}"#));
    }

    #[test]
    fn reads_markdown_and_html() {
        let doc: serde_json::Value =
            serde_json::from_str(&take(unsafe { text_markdown_json(c("# Notes\n\n- [ ] milk").as_ptr()) })).unwrap();
        assert_eq!(doc["title"], "Notes");
        assert_eq!(doc["blocks"][1]["items"][0]["checked"], false);
        let page: serde_json::Value = serde_json::from_str(&take(unsafe {
            text_html_json(c("<title>Page</title><p>caf&eacute; <b>bold").as_ptr())
        }))
        .unwrap();
        assert_eq!(page["title"], "Page");
        assert_eq!(page["blocks"][0]["inlines"][1], serde_json::json!({"type": "text", "text": "bold", "bold": true}));
        assert!(unsafe { text_html_json(std::ptr::null()) }.is_null());
        assert_eq!(doc_ffi::last_error(), "INVALID_ARGUMENT: text is missing");
        let huge = c(&"a".repeat(MAX_MARKUP_BYTES + 1));
        assert!(unsafe { text_markdown_json(huge.as_ptr()) }.is_null());
        assert!(doc_ffi::last_error().starts_with("TOO_LARGE"));
    }

    /// Check JSON blocks against the shape `markup.rs` documents.
    fn check_json_blocks(blocks: &serde_json::Value) {
        for block in blocks.as_array().unwrap() {
            match block["type"].as_str().unwrap() {
                "heading" => {
                    assert!((1..=6).contains(&block["level"].as_u64().unwrap()));
                    check_json_inlines(&block["inlines"]);
                }
                "paragraph" => check_json_inlines(&block["inlines"]),
                "quote" => check_json_blocks(&block["blocks"]),
                "list" => {
                    assert!(block["ordered"].is_boolean() && block["start"].is_i64());
                    for item in block["items"].as_array().unwrap() {
                        assert!(item.get("checked").is_none_or(serde_json::Value::is_boolean));
                        check_json_blocks(&item["blocks"]);
                    }
                }
                "code" => assert!(block["language"].is_string() && block["text"].is_string()),
                "rule" => assert_eq!(block.as_object().unwrap().len(), 1),
                "table" => {
                    let align = block["align"].as_array().unwrap();
                    assert!(align.iter().all(|a| matches!(a.as_str(), Some("none" | "left" | "center" | "right"))));
                    for cell in block["header"].as_array().unwrap() {
                        check_json_inlines(cell);
                    }
                    for row in block["rows"].as_array().unwrap() {
                        assert_eq!(row.as_array().unwrap().len(), align.len());
                        for cell in row.as_array().unwrap() {
                            check_json_inlines(cell);
                        }
                    }
                }
                other => panic!("block type {other}"),
            }
        }
    }

    fn check_json_inlines(inlines: &serde_json::Value) {
        for inline in inlines.as_array().unwrap() {
            assert!(inline.get("link").is_none_or(serde_json::Value::is_string));
            match inline["type"].as_str().unwrap() {
                "text" => {
                    assert!(!inline["text"].as_str().unwrap().is_empty());
                    for flag in ["bold", "italic", "code", "strike"] {
                        assert!(inline.get(flag).is_none_or(|v| v.as_bool() == Some(true)));
                    }
                }
                "image" => assert!(inline["alt"].is_string() && inline["src"].is_string()),
                "break" => {}
                other => panic!("inline type {other}"),
            }
        }
    }

    #[test]
    fn markup_json_holds_any_text() {
        // What JSON must escape comes through as it was.
        let tricky = "q\"uote\\back\tslash \u{1}\u{7F} 😀 \u{2028}é";
        let json = take(unsafe { text_markdown_json(c(&format!("`{tricky}`")).as_ptr()) });
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(doc["blocks"][0]["inlines"][0]["text"], tricky);
        let json = take(unsafe { text_html_json(c(&format!("<pre>{tricky}</pre>")).as_ptr()) });
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(doc["blocks"][0]["text"], tricky);

        // Bytes that are not UTF-8 are refused.
        let bad = [b'a', 0xFF, 0xFE, 0];
        assert!(unsafe { text_html_json(bad.as_ptr().cast()) }.is_null());
        assert!(doc_ffi::last_error().starts_with("INVALID_ARGUMENT"), "{}", doc_ffi::last_error());
        assert!(unsafe { text_markdown_json(std::ptr::null()) }.is_null());

        // Generated input always gives JSON of the documented shape.
        let pieces = [
            "*", "_", "~~", "`", "[", "](", ")", "![", "<b>", "</p>", "<table><tr><td>", "<th>", "\n", "\n\n", "- ", "> ",
            "| a | b |\n|:-|-:|\n", "&amp;", "é", "\"", "\\", "<img src=x alt=y>", "# ", "1. ", "[ ] ", "[x] ", "x",
            "<ol start=-3><li>", "```\n", "http://a.b ", "<br>", "<a href=u>", "\t", "<pre>",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize
        };
        for _ in 0..2000 {
            let len = next() % 60;
            let text: String = (0..len).map(|_| pieces[next() % pieces.len()]).collect();
            for read in [text_markdown_json, text_html_json] {
                let doc: serde_json::Value = serde_json::from_str(&take(unsafe { read(c(&text).as_ptr()) })).unwrap();
                assert!(doc["title"].is_null() || doc["title"].is_string());
                check_json_blocks(&doc["blocks"]);
            }
        }
    }

    #[test]
    fn reinterpret_before_editing() {
        let dir = std::env::temp_dir().join(format!("text_ffi_re_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ru.txt");
        std::fs::write(&path, [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, b'!']).unwrap();
        let handle = unsafe { text_open(c(path.to_str().unwrap()).as_ptr(), std::ptr::null()) };
        let info: serde_json::Value = serde_json::from_str(&take(text_info_json(handle, 0))).unwrap();
        assert_eq!(info["confident"], false);
        assert_eq!(unsafe { text_reinterpret(handle, c("windows-1251").as_ptr()) }, 0);
        assert_eq!(take(text_get(handle)), "Привет!");
        text_close(handle);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
