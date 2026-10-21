//! A text file in memory.

use doc_encoding::{detect, looks_binary, Encoding};

/// How a file breaks its lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    /// `\n`: Linux, Android, macOS.
    Lf,
    /// `\r\n`: Windows.
    CrLf,
    /// `\r`: classic Mac OS.
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    /// `LF`, `CRLF` or `CR`.
    pub fn name(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "LF" => Some(LineEnding::Lf),
            "CRLF" => Some(LineEnding::CrLf),
            "CR" => Some(LineEnding::Cr),
            _ => None,
        }
    }
}

/// Counts for a status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub lines: usize,
    pub words: usize,
    /// Unicode scalar values.
    pub characters: usize,
    /// UTF-16 code units, the length a Dart string has.
    pub utf16_len: usize,
}

/// A text file: its content as Unicode with `\n` line breaks, and what it
/// takes to write it back the way it was.
#[derive(Clone, Debug)]
pub struct TextDocument {
    text: String,
    /// The file's bytes, kept until the first edit so that the user can
    /// reopen it in another encoding without losing anything.
    original: Option<Vec<u8>>,
    pub encoding: Encoding,
    /// Whether to write a byte-order mark.
    pub bom: bool,
    pub line_ending: LineEnding,
    /// The file mixed line-ending styles; saving makes them all
    /// [`TextDocument::line_ending`].
    pub mixed_line_endings: bool,
    /// Characters that could not be decoded and became U+FFFD.
    pub decode_errors: usize,
    /// False when the encoding is a guess between code pages.
    pub confident: bool,
    /// The bytes looked like a binary file, not text.
    pub binary: bool,
    modified: bool,
    line_starts: Option<Vec<usize>>,
    line_count: usize,
    stats_cache: Option<Stats>,
    source_encoding: Encoding,
    source_lossless: bool,
    original_format: Option<(Encoding, bool, LineEnding, bool)>,
    original_endings: Option<Vec<LineEnding>>,
    can_reinterpret: bool,
    max_text_bytes: usize,
}

const INDEX_STRIDE: usize = 128;

/// Normalize line breaks to `\n`, reporting the dominant style and whether
/// styles were mixed.
fn normalize_line_endings(text: String) -> (String, LineEnding, bool, Option<Vec<LineEnding>>) {
    let bytes = text.as_bytes();
    let (mut crlf, mut cr, mut lf) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 2;
                continue;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    let kinds = (crlf > 0) as u8 + (cr > 0) as u8 + (lf > 0) as u8;
    let ending = if crlf > lf && crlf >= cr {
        LineEnding::CrLf
    } else if cr > lf && cr > crlf {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    };
    if crlf == 0 && cr == 0 {
        return (text, ending, false, None);
    }
    let mut endings = (kinds > 1).then(Vec::new);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            let style = if chars.peek() == Some(&'\n') {
                chars.next();
                LineEnding::CrLf
            } else {
                LineEnding::Cr
            };
            if let Some(endings) = &mut endings { endings.push(style); }
            out.push('\n');
        } else {
            if c == '\n' {
                if let Some(endings) = &mut endings { endings.push(LineEnding::Lf); }
            }
            out.push(c);
        }
    }
    (out, ending, kinds > 1, endings)
}

impl Default for TextDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl TextDocument {
    /// A new, empty UTF-8 document with `\n` line breaks.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            original: None,
            encoding: Encoding::Utf8,
            bom: false,
            line_ending: LineEnding::Lf,
            mixed_line_endings: false,
            decode_errors: 0,
            confident: true,
            binary: false,
            modified: false,
            line_starts: None,
            line_count: 1,
            stats_cache: None,
            source_encoding: Encoding::Utf8,
            source_lossless: false,
            original_format: None,
            original_endings: None,
            can_reinterpret: false,
            max_text_bytes: usize::MAX,
        }
    }

    /// Read a file's bytes, detecting its encoding (using `fallback` when
    /// the bytes could be any single-byte code page).
    pub fn from_bytes(bytes: Vec<u8>, fallback: Encoding) -> Self {
        Self::from_bytes_limited(bytes, fallback, usize::MAX).expect("text fits in memory")
    }

    pub fn from_bytes_limited(bytes: Vec<u8>, fallback: Encoding, max_text_bytes: usize) -> Result<Self, &'static str> {
        let detection = detect(&bytes, fallback);
        let mut doc = Self::new();
        doc.max_text_bytes = max_text_bytes;
        doc.binary = looks_binary(&bytes);
        doc.confident = detection.confident;
        doc.load(&bytes, detection.encoding, detection.bom_len)?;
        if detection.encoding != Encoding::Utf8 || doc.decode_errors != 0 {
            doc.original = Some(bytes);
        }
        Ok(doc)
    }

    fn load(&mut self, bytes: &[u8], encoding: Encoding, bom_len: usize) -> Result<(), &'static str> {
        let decoded = encoding.decode_lossless_limited(&bytes[bom_len..], self.max_text_bytes)?;
        let (text, ending, mixed, endings) = normalize_line_endings(decoded.text);
        self.text = text;
        self.encoding = encoding;
        self.source_encoding = encoding;
        self.source_lossless = true;
        self.bom = bom_len > 0;
        self.line_ending = ending;
        self.mixed_line_endings = mixed;
        self.original_format = Some((encoding, self.bom, ending, mixed));
        self.original_endings = endings;
        self.decode_errors = decoded.errors;
        self.modified = false;
        self.line_starts = None;
        self.line_count = self.text.bytes().filter(|&b| b == b'\n').count() + 1;
        self.stats_cache = None;
        self.can_reinterpret = true;
        Ok(())
    }

    /// Re-read the original bytes as `encoding` ("the text looks wrong, try
    /// Cyrillic"). Only possible before the first edit.
    pub fn reinterpret(&mut self, encoding: Encoding) -> Result<(), &'static str> {
        if !self.can_reinterpret {
            return Err("the text has been edited, so it can no longer be read in another encoding");
        }
        let bytes = self.original.take().unwrap_or_else(|| self.encode().0);
        let bom_len = doc_encoding::sniff_bom(&bytes)
            .filter(|(found, _)| *found == encoding)
            .map_or(0, |(_, len)| len);
        if let Err(error) = self.load(&bytes, encoding, bom_len) {
            self.original = Some(bytes);
            return Err(error);
        }
        self.confident = true;
        if encoding != Encoding::Utf8 || self.decode_errors != 0 {
            self.original = Some(bytes);
        }
        Ok(())
    }

    pub fn can_reinterpret(&self) -> bool {
        self.can_reinterpret
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Replace the whole text. Any `\r\n` or `\r` becomes `\n`.
    pub fn set_text(&mut self, text: String) -> bool {
        let (text, _, _, _) = normalize_line_endings(text);
        if text != self.text {
            self.text = text;
            self.modified = true;
            self.original = None;
            self.original_format = None;
            self.original_endings = None;
            self.can_reinterpret = false;
            self.line_starts = None;
            self.line_count = self.text.bytes().filter(|&b| b == b'\n').count() + 1;
            self.stats_cache = None;
            return true;
        }
        false
    }

    pub fn set_format(&mut self, encoding: Option<Encoding>, bom: Option<bool>, ending: Option<LineEnding>) {
        let before = (self.encoding, self.bom, self.line_ending, self.mixed_line_endings);
        if let Some(encoding) = encoding { self.encoding = encoding; }
        if let Some(bom) = bom { self.bom = bom; }
        if !self.encoding.is_unicode() { self.bom = false; }
        if let Some(ending) = ending {
            self.line_ending = ending;
            self.mixed_line_endings = false;
            self.original_endings = None;
        }
        if before != (self.encoding, self.bom, self.line_ending, self.mixed_line_endings) {
            self.modified = true;
            self.can_reinterpret = false;
        }
    }

    /// Mark the current state as saved.
    pub fn mark_saved(&mut self) {
        self.modified = false;
        if self.original_endings.is_none() { self.mixed_line_endings = false; }
    }

    /// The bytes to write: the text with the chosen line breaks, encoding
    /// and byte-order mark. Also returns how many characters the encoding
    /// could not represent (each written as `?`).
    pub fn encode(&self) -> (Vec<u8>, usize) {
        if self.original_format == Some((self.encoding, self.bom, self.line_ending, self.mixed_line_endings)) {
            if let Some(bytes) = &self.original { return (bytes.clone(), 0); }
        }
        let text = if self.mixed_line_endings && self.original_endings.is_some() {
            let mut text = String::with_capacity(self.text.len());
            let mut endings = self.original_endings.iter().flatten();
            for part in self.text.split_inclusive('\n') {
                if let Some(line) = part.strip_suffix('\n') {
                    text.push_str(line);
                    text.push_str(endings.next().copied().unwrap_or(self.line_ending).as_str());
                } else {
                    text.push_str(part);
                }
            }
            std::borrow::Cow::Owned(text)
        } else if self.line_ending == LineEnding::Lf {
            std::borrow::Cow::Borrowed(self.text.as_str())
        } else {
            std::borrow::Cow::Owned(self.text.replace('\n', self.line_ending.as_str()))
        };
        let encoded = if self.source_lossless {
            self.encoding.encode_lossless(&text, self.source_encoding)
        } else {
            self.encoding.encode(&text)
        };
        let mut bytes = encoded.bytes;
        if self.bom { bytes.splice(..0, self.encoding.bom().iter().copied()); }
        (bytes, encoded.unmappable)
    }

    /// Characters the current encoding cannot represent.
    pub fn unmappable(&self) -> usize {
        if self.source_lossless {
            self.encoding.unmappable_lossless(&self.text, self.source_encoding)
        } else if self.encoding.is_unicode() {
            0
        } else {
            self.encoding.encode(&self.text).unmappable
        }
    }

    fn starts(&mut self) -> &[usize] {
        if self.line_starts.is_none() {
            let mut starts = vec![0];
            starts.extend(self.text.match_indices('\n').enumerate()
                .filter(|(line, _)| (line + 1) % INDEX_STRIDE == 0).map(|(_, (i, _))| i + 1));
            self.line_starts = Some(starts);
        }
        self.line_starts.as_deref().unwrap_or(&[])
    }

    /// Number of lines (a final line break starts an empty last line).
    pub fn line_count(&mut self) -> usize {
        self.line_count
    }

    /// Lines `first..first + count`, without their line breaks.
    pub fn lines(&mut self, first: usize, count: usize) -> Vec<String> {
        self.lines_limited(first, count, usize::MAX).unwrap_or_default()
    }

    pub fn lines_limited(&mut self, first: usize, count: usize, max_bytes: usize) -> Result<Vec<String>, &'static str> {
        if first >= self.line_count { return Ok(Vec::new()); }
        self.starts();
        let starts = self.line_starts.as_deref().unwrap_or(&[]);
        let start = starts[first / INDEX_STRIDE];
        let mut lines = Vec::new();
        let mut remaining = max_bytes;
        for line in self.text[start..].split('\n').skip(first % INDEX_STRIDE).take(count) {
            remaining = remaining.checked_sub(line.len()).ok_or("these lines are too large to display together")?;
            lines.push(line.to_owned());
        }
        Ok(lines)
    }

    /// The line containing byte offset `at` (0-based).
    pub fn line_of(&mut self, at: usize) -> usize {
        let at = at.min(self.text.len());
        let starts = self.starts();
        let block = match starts.binary_search(&at) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let start = starts[block];
        block * INDEX_STRIDE + self.text.as_bytes()[start..at].iter().filter(|&&b| b == b'\n').count()
    }

    pub fn stats(&mut self) -> Stats {
        if let Some(stats) = self.stats_cache { return stats; }
        let mut stats = Stats { lines: self.line_count, words: 0, characters: 0, utf16_len: 0 };
        let mut in_word = false;
        for c in self.text.chars() {
            stats.characters += 1;
            stats.utf16_len += c.len_utf16();
            let word = !c.is_whitespace();
            stats.words += (word && !in_word) as usize;
            in_word = word;
        }
        self.stats_cache = Some(stats);
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(bytes: &[u8]) -> TextDocument {
        TextDocument::from_bytes(bytes.to_vec(), Encoding::WINDOWS_1252)
    }

    #[test]
    fn keeps_windows_files_as_they_were() {
        let bytes = b"first line\r\nsecond line\r\n";
        let doc = open(bytes);
        assert_eq!(doc.text(), "first line\nsecond line\n");
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        assert!(!doc.mixed_line_endings);
        assert_eq!(doc.encode(), (bytes.to_vec(), 0));
    }

    #[test]
    fn keeps_bom_and_utf16() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("తెలుగు\r\nline".encode_utf16().flat_map(u16::to_le_bytes));
        let doc = open(&bytes);
        assert_eq!(doc.encoding, Encoding::Utf16Le);
        assert!(doc.bom);
        assert_eq!(doc.text(), "తెలుగు\nline");
        assert_eq!(doc.encode().0, bytes);
    }

    #[test]
    fn mixed_line_endings_are_kept_until_an_edit_or_explicit_choice() {
        let mut doc = open(b"a\r\nb\r\nc\nd\re");
        assert!(doc.mixed_line_endings);
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        assert_eq!(doc.line_count(), 5);
        assert_eq!(doc.encode().0, b"a\r\nb\r\nc\nd\re");
        assert!(doc.original.is_none());
        doc.set_text(doc.text().to_owned());
        assert!(!doc.is_modified());
        assert_eq!(doc.encode().0, b"a\r\nb\r\nc\nd\re");
        doc.set_format(None, None, Some(LineEnding::CrLf));
        assert!(doc.is_modified());
        assert!(!doc.mixed_line_endings);
        assert_eq!(doc.encode().0, b"a\r\nb\r\nc\r\nd\r\ne");
    }

    #[test]
    fn single_byte_text_survives_and_reinterprets() {
        let bytes = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, b' ', b'm', b'i', b'r'];
        let mut doc = TextDocument::from_bytes(bytes.to_vec(), Encoding::WINDOWS_1252);
        assert!(!doc.confident);
        assert_eq!(doc.encode().0, bytes);
        doc.reinterpret(Encoding::from_label("windows-1251").unwrap()).unwrap();
        assert_eq!(doc.text(), "Привет mir");
        assert_eq!(doc.encode().0, bytes);
        doc.set_text("edited".into());
        assert!(doc.reinterpret(Encoding::Utf8).is_err());
    }

    #[test]
    fn editing_and_unmappable_characters() {
        let mut doc = open(b"caf\xE9");
        assert_eq!(doc.text(), "café");
        assert!(!doc.is_modified());
        doc.set_text("café ₹ 100\r\nnext".into());
        assert!(doc.is_modified());
        assert_eq!(doc.text(), "café ₹ 100\nnext");
        assert_eq!(doc.unmappable(), 1);
        let (bytes, unmappable) = doc.encode();
        assert_eq!(unmappable, 1);
        assert_eq!(bytes, b"caf\xE9 ? 100\nnext");
        doc.encoding = Encoding::Utf8;
        assert_eq!(doc.unmappable(), 0);
    }

    #[test]
    fn paging_by_line() {
        let mut doc = open(b"zero\none\ntwo\n");
        assert_eq!(doc.line_count(), 4);
        assert_eq!(doc.lines(1, 2), ["one", "two"]);
        assert_eq!(doc.lines(3, 10), [""]);
        assert!(doc.lines(9, 1).is_empty());
        assert_eq!(doc.line_of(0), 0);
        assert_eq!(doc.line_of(5), 1);
        assert_eq!(doc.line_of(7), 1);
        let stats = doc.stats();
        assert_eq!((stats.lines, stats.words, stats.characters), (4, 3, 13));
    }

    #[test]
    fn every_code_page_saves_every_byte_without_edits() {
        let bytes: Vec<u8> = (0..=255).collect();
        for encoding in Encoding::all().into_iter().filter(|e| !e.is_unicode()) {
            let mut doc = open(&bytes);
            doc.reinterpret(encoding).unwrap();
            assert_eq!(doc.encode(), (bytes.clone(), 0), "{}", encoding.name());
            doc.set_text(doc.text().to_owned());
            doc.mark_saved();
            assert_eq!(doc.encode(), (bytes.clone(), 0), "{}", encoding.name());
        }
    }

    #[test]
    fn malformed_unicode_keeps_exact_bytes_and_survives_unrelated_edits() {
        for encoding in Encoding::all().into_iter().filter(|e| e.is_unicode()) {
            let bad = match encoding {
                Encoding::Utf8 => vec![0xED, 0xA0, 0x80, 0xFF, 0xC0],
                Encoding::Utf16Le => vec![0, 0xD8, 0x41, 0, 0, 0xDC],
                Encoding::Utf16Be => vec![0xD8, 0, 0, 0x41, 0xDC, 0],
                Encoding::Utf32Le => vec![0, 0xD8, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF],
                Encoding::Utf32Be => vec![0, 0, 0xD8, 0, 0xFF, 0xFF, 0xFF, 0xFF],
                _ => unreachable!(),
            };
            let mut bytes = encoding.bom().to_vec();
            bytes.extend_from_slice(&bad);
            let mut doc = open(&bytes);
            assert_eq!(doc.encoding, encoding);
            assert!(doc.decode_errors > 0);
            assert_eq!(doc.encode(), (bytes.clone(), 0));
            doc.set_text(format!("{}!", doc.text()));
            bytes.extend_from_slice(&encoding.encode("!").bytes);
            assert_eq!(doc.encode(), (bytes, 0), "{}", encoding.name());
            let odd: Vec<u8> = [encoding.bom(), bad.as_slice(), &[0x19]].concat();
            assert_eq!(open(&odd).encode(), (odd, 0));
        }
    }

    #[test]
    fn valid_utf8_needs_no_original_buffer_and_can_still_reinterpret() {
        for text in ["plain", "a\r\nb\nc\rd\r\n", "\u{FEFF}a\r\nb\n", "\u{F0000}\u{F0100} literal"] {
            let mut doc = open(text.as_bytes());
            assert!(doc.original.is_none());
            assert!(doc.can_reinterpret());
            assert_eq!(doc.encode(), (text.as_bytes().to_vec(), 0));
            doc.reinterpret(Encoding::WINDOWS_1252).unwrap();
            assert_eq!(doc.encode(), (text.as_bytes().to_vec(), 0));
            doc.reinterpret(Encoding::Utf8).unwrap();
            assert!(doc.original.is_none());
            assert_eq!(doc.encode(), (text.as_bytes().to_vec(), 0));
        }
        let mut doc = TextDocument::new();
        doc.set_text("\u{F0000}".into());
        assert_eq!(doc.encode().0, "\u{F0000}".as_bytes());
    }

    #[test]
    fn large_line_queries_use_a_sparse_index_and_cached_statistics() {
        let mut doc = open("word value\n".repeat(1_000_000).as_bytes());
        assert!(doc.stats_cache.is_none());
        let stats = doc.stats();
        assert_eq!((stats.lines, stats.words), (1_000_001, 2_000_000));
        assert_eq!(doc.stats(), stats);
        let start = std::time::Instant::now();
        for page in 0..1_000 {
            assert_eq!(doc.lines(page * 777, 2), ["word value", "word value"]);
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert!(doc.line_starts.as_ref().unwrap().len() < 8_000);
        assert_eq!(doc.line_of(999_999 * 11 + 1), 999_999);
        assert_eq!(doc.lines(1_000_000, 10), [""]);
        assert!(doc.lines_limited(0, 100, 8).is_err());
        doc.set_text("new text".into());
        assert!(doc.stats_cache.is_none());
        assert_eq!(doc.stats().words, 2);
    }

    #[test]
    fn empty_and_binary_files() {
        let mut doc = open(b"");
        assert_eq!(doc.line_count(), 1);
        assert_eq!(doc.encoding, Encoding::Utf8);
        assert!(!doc.binary);
        let doc = open(&[0x89, b'P', b'N', b'G', 0, 0, 0, 13, 0x49, 0x48]);
        assert!(doc.binary);
    }
}
