//! Fixed-seed mutation of text files built here, in every Unicode form and
//! a code page: an unedited file always writes back byte for byte, an
//! edited one reads back as written, and the Markdown, HTML, JSON and
//! search readers survive damaged input without a panic or hang.

use std::time::Duration;

use doc_testkit::{mutate, run_limited, Limits, Rng};
use text_core::search::{find_all, replace_all, utf16_offsets, SearchOptions};
use text_core::{json, markup, Encoding, LineEnding, TextDocument};

const SEED: u64 = 0x5445_5854_4239_0001;
const SAMPLE: &str = "Rs. 100 paid \u{20b9}\r\nदाल 50\r\n\u{1F600} caf\u{e9}\n\ttabbed\rold mac\r\n";
const MARKDOWN: &[u8] = b"# Accounts\n\n- Rice **100**\n- Dal _50_\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quoted [link](http://x) ![p](img.png)\n\n```\ncode\n```\n";
const HTML: &[u8] = b"<html><body><h1>Accounts</h1><p>Rice <b>100</b><br><a href='x'>link</a></p><table><tr><td>1</td></tr></table><ul><li>one<li>two</ul><img src='p.png'></body></html>";
const JSON: &[u8] = br#"{"items":[{"name":"Rice","amount":100},{"name":"Dal","amount":50.5e0,"paid":true,"note":null}],"text":"\u20b9 \"q\""}"#;

/// The sample in different encodings and with and without marks.
fn seeds() -> Vec<Vec<u8>> {
    let mut out = vec![SAMPLE.as_bytes().to_vec()];
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(SAMPLE.as_bytes());
    out.push(bom);
    for encoding in [Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Utf32Le] {
        let mut bytes = encoding.bom().to_vec();
        bytes.extend(encoding.encode(SAMPLE).bytes);
        out.push(bytes);
    }
    out.push(Encoding::WINDOWS_1252.encode("Caf\u{e9} \u{20ac}5\r\nna\u{ef}ve\r\n").bytes);
    out
}

/// Open without editing: the bytes written are the bytes read. Then edit:
/// what is written reads back as the edited text.
fn round_trip(bytes: Vec<u8>) {
    let doc = TextDocument::from_bytes(bytes.clone(), Encoding::WINDOWS_1252);
    let (written, _) = doc.encode();
    assert_eq!(written, bytes, "an unedited {} file wrote back different bytes", doc.encoding.name());

    let mut doc = doc;
    let _ = doc.stats();
    let n = doc.line_count();
    let _ = doc.lines(n / 2, 10);
    let edited = format!("{}\nadded line \u{e9}", doc.text());
    doc.set_text(edited.clone());
    let (written, unmappable) = doc.encode();
    if unmappable == 0 {
        let mut again = TextDocument::from_bytes(written, Encoding::WINDOWS_1252);
        if again.encoding == doc.encoding {
            // A stray byte kept from the original may now pair up with
            // padding as a character, but the typed text reads back whole.
            assert!(again.text().ends_with("\nadded line \u{e9}"), "edited {} text: {:?}", doc.encoding.name(), again.text());
            if doc.decode_errors == 0 {
                assert_eq!(again.text(), doc.text(), "edited {} text", doc.encoding.name());
            }
        }
        let _ = again.stats();
    }
}

#[test]
fn every_seed_writes_back_unchanged_and_edits_read_back() {
    run_limited("every_seed_writes_back_unchanged_and_edits_read_back", Limits::mutation(Duration::from_secs(20)), || {
        for seed in seeds() {
            round_trip(seed);
        }
        let mut doc = TextDocument::from_bytes(SAMPLE.as_bytes().to_vec(), Encoding::WINDOWS_1252);
        assert!(doc.mixed_line_endings);
        doc.set_format(Some(Encoding::Utf16Le), Some(true), Some(LineEnding::CrLf));
        let (bytes, _) = doc.encode();
        let again = TextDocument::from_bytes(bytes, Encoding::WINDOWS_1252);
        assert_eq!((again.encoding, again.bom, again.line_ending), (Encoding::Utf16Le, true, LineEnding::CrLf));
        assert_eq!(again.text(), doc.text());
    }).unwrap();
}

#[test]
fn fuzz_encodings_fixed_seed() {
    run_limited("fuzz_encodings_fixed_seed", Limits::mutation(Duration::from_secs(120)), || {
        let seeds = seeds();
        let mut rng = Rng::new(SEED);
        for iteration in 0..8192 {
            let bytes = mutate(&mut rng, &seeds[iteration % seeds.len()], &seeds[(iteration + 1) % seeds.len()]);
            round_trip(bytes.clone());
            // Reading the same bytes as another encoding is allowed until the first edit.
            let mut doc = TextDocument::from_bytes(bytes, Encoding::WINDOWS_1252);
            let all = Encoding::all();
            let _ = doc.reinterpret(all[iteration % all.len()]);
            let _ = doc.encode();
        }
        eprintln!("text encodings seed={SEED:x} iterations=8192");
    }).unwrap();
}

#[test]
fn fuzz_markup_json_and_search_fixed_seed() {
    run_limited("fuzz_markup_json_and_search_fixed_seed", Limits::mutation(Duration::from_secs(120)), || {
        let seeds = [MARKDOWN, HTML, JSON];
        let mut rng = Rng::new(SEED ^ 1);
        let mut formatted = 0;
        for iteration in 0..6144 {
            let bytes = mutate(&mut rng, seeds[iteration % 3], seeds[(iteration + 1) % 3]);
            let text = String::from_utf8_lossy(&bytes);
            let _ = markup::markdown(&text);
            let _ = markup::html(&text);
            if let Ok(pretty) = json::format(&text, 2) {
                formatted += 1;
                json::validate(&pretty).expect("formatted JSON is valid");
                assert_eq!(json::format(&pretty, 2).unwrap(), pretty, "formatting is stable");
            }
            let query: String = text.chars().skip(iteration % 17).take(1 + iteration % 5).collect();
            for options in [SearchOptions::default(), SearchOptions { case_sensitive: true, whole_word: true }] {
                let found = find_all(&text, &query, options, 1000);
                let offsets: Vec<usize> = found.iter().map(|m| m.start).collect();
                let _ = utf16_offsets(&text, &offsets);
                let (_, count) = replace_all(&text, &query, "\u{20b9}", options);
                assert!(count >= found.len().min(1000) || found.len() == 1000);
            }
        }
        assert!(formatted > 0);
        eprintln!("markup/JSON/search seed={:x} iterations=6144 json_formatted={formatted}", SEED ^ 1);
    }).unwrap();
}
