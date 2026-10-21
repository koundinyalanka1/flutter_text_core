//! Find and replace over plain text.

/// How to match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    /// Only match where the query is not part of a longer word.
    pub whole_word: bool,
}

/// A match, as byte offsets into the searched text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn at_word_boundaries(text: &str, m: Match) -> bool {
    let before = text[..m.start].chars().next_back();
    let after = text[m.end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

/// Every non-overlapping match, up to `limit`.
pub fn find_all(text: &str, query: &str, options: SearchOptions, limit: usize) -> Vec<Match> {
    let mut out = Vec::new();
    let limit = limit.min(10_000);
    if limit != 0 {
        visit_matches(text, query, options, |m| {
            out.push(m);
            out.len() < limit
        });
    }
    out
}

fn visit_matches(text: &str, query: &str, options: SearchOptions, mut visit: impl FnMut(Match) -> bool) {
    if query.is_empty() { return; }
    if options.case_sensitive {
        for (start, found) in text.match_indices(query) {
            let m = Match { start, end: start + found.len() };
            if options.whole_word && !at_word_boundaries(text, m) { continue; }
            if !visit(m) { break; }
        }
        return;
    }
    let needle: Vec<char> = query.chars().map(fold).collect();
    let mut prefix = vec![0; needle.len()];
    let mut matched = 0;
    for i in 1..needle.len() {
        while matched > 0 && needle[i] != needle[matched] { matched = prefix[matched - 1]; }
        if needle[i] == needle[matched] { matched += 1; }
        prefix[i] = matched;
    }
    let mut starts = vec![0; needle.len()];
    matched = 0;
    for (i, (byte, c)) in text.char_indices().enumerate() {
        starts[i % needle.len()] = byte;
        let folded = fold(c);
        while matched > 0 && folded != needle[matched] { matched = prefix[matched - 1]; }
        if folded == needle[matched] { matched += 1; }
        if matched != needle.len() { continue; }
        let m = Match { start: starts[(i + 1) % needle.len()], end: byte + c.len_utf8() };
        if options.whole_word && !at_word_boundaries(text, m) {
            matched = prefix[matched - 1];
            continue;
        }
        if !visit(m) { break; }
        // Skip the characters inside the match.
        matched = 0;
    }
}

/// Replace every match; returns the new text and the number of replacements.
pub fn replace_all(text: &str, query: &str, replacement: &str, options: SearchOptions) -> (String, usize) {
    replace_all_limited(text, query, replacement, options, usize::MAX).expect("replacement fits in memory")
}

pub fn replace_all_limited(
    text: &str, query: &str, replacement: &str, options: SearchOptions, max_bytes: usize,
) -> Result<(String, usize), &'static str> {
    let mut length = text.len();
    let mut count = 0;
    let mut overflow = false;
    visit_matches(text, query, options, |m| {
        count += 1;
        match length.checked_sub(m.end - m.start).and_then(|n| n.checked_add(replacement.len())) {
            Some(next) if next <= max_bytes => length = next,
            _ => overflow = true,
        }
        !overflow
    });
    if overflow || length > max_bytes { return Err("the replacement would make the text too large"); }
    let mut out = String::new();
    out.try_reserve_exact(length).map_err(|_| "not enough memory for the replacement")?;
    let mut last = 0;
    visit_matches(text, query, options, |m| {
        out.push_str(&text[last..m.start]);
        out.push_str(replacement);
        last = m.end;
        true
    });
    out.push_str(&text[last..]);
    Ok((out, count))
}

/// Convert sorted byte offsets into UTF-16 offsets (what Dart strings use),
/// in one pass over the text.
pub fn utf16_offsets(text: &str, sorted_bytes: &[usize]) -> Vec<usize> {
    let mut out = Vec::with_capacity(sorted_bytes.len());
    let mut wanted = sorted_bytes.iter().peekable();
    let mut utf16 = 0;
    for (byte, c) in text.char_indices() {
        if wanted.peek().is_none() { break; }
        while let Some(&&target) = wanted.peek() {
            if target > byte {
                break;
            }
            out.push(utf16);
            wanted.next();
        }
        utf16 += c.len_utf16();
    }
    out.extend(wanted.map(|_| utf16));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str, query: &str, options: SearchOptions) -> Vec<&'static str> {
        find_all(text, query, options, 100)
            .into_iter()
            .map(|m| &*Box::leak(text[m.start..m.end].to_owned().into_boxed_str()))
            .collect()
    }

    #[test]
    fn case_insensitive_and_whole_word() {
        let text = "Total, total; TOTALS and subtotal. Ünïcode ÜNÏCODE";
        let any = SearchOptions::default();
        assert_eq!(spans(text, "total", any), ["Total", "total", "TOTAL", "total"]);
        let words = SearchOptions { whole_word: true, ..any };
        assert_eq!(spans(text, "total", words), ["Total", "total"]);
        let exact = SearchOptions { case_sensitive: true, ..any };
        assert_eq!(spans(text, "total", exact), ["total", "total"]);
        assert_eq!(spans(text, "ünïcode", any), ["Ünïcode", "ÜNÏCODE"]);
        assert!(find_all(text, "", any, 10).is_empty());
    }

    #[test]
    fn matches_do_not_overlap() {
        assert_eq!(find_all("aaaa", "aa", SearchOptions::default(), 10).len(), 2);
        let exact = SearchOptions { case_sensitive: true, ..Default::default() };
        assert_eq!(find_all("aaaa", "aa", exact, 10).len(), 2);
    }

    #[test]
    fn replaces_everything() {
        let (out, n) = replace_all("Rs. 10, rs. 20", "rs.", "₹", SearchOptions::default());
        assert_eq!((out.as_str(), n), ("₹ 10, ₹ 20", 2));
        let (out, n) = replace_all("cat catalog", "cat", "dog", SearchOptions { whole_word: true, ..Default::default() });
        assert_eq!((out.as_str(), n), ("dog catalog", 1));
    }

    #[test]
    fn hostile_prefix_queries_are_linear_and_results_and_replacement_are_bounded() {
        let text = "a".repeat(8 << 20);
        let query = "A".repeat(32_768) + "B";
        let started = std::time::Instant::now();
        assert!(find_all(&text, &query, SearchOptions::default(), 10_000).is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(find_all(&text, "A", SearchOptions::default(), usize::MAX).len(), 10_000);
        assert!(replace_all_limited(&text, "a", &query, SearchOptions::default(), 32 << 20).is_err());
        let (out, count) = replace_all_limited(&text, "A", "", SearchOptions::default(), text.len()).unwrap();
        assert!(out.is_empty());
        assert_eq!(count, text.len());
    }

    #[test]
    fn folded_unicode_offsets_and_rejected_word_boundaries_remain_correct() {
        let options = SearchOptions { whole_word: true, ..Default::default() };
        assert_eq!(spans("ababa aba ABA", "aba", options), ["aba", "ABA"]);
        assert_eq!(spans("Kk Kk", "kk", SearchOptions::default()), ["Kk", "Kk"]);
        let found = find_all("😀Kk Kk", "kk", SearchOptions::default(), 10);
        let bytes: Vec<usize> = found.iter().flat_map(|m| [m.start, m.end]).collect();
        assert_eq!(utf16_offsets("😀Kk Kk", &bytes), [2, 4, 5, 7]);
    }

    #[test]
    fn converts_offsets_to_utf16() {
        let text = "a😀bఅc";
        // Byte offsets of 'a', 'b', 'అ', 'c' and the end.
        let bytes = [0, 5, 6, 9, 10];
        assert_eq!(utf16_offsets(text, &bytes), [0, 3, 4, 5, 6]);
    }
}
