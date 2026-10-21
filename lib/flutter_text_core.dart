/// flutter_text_core: DocHelper's plain-text engine, built from scratch in
/// Rust.
///
/// Opens text files in any common encoding (UTF-8/16/32, Windows, ISO-8859,
/// DOS and Mac code pages) and saves them back with the same encoding,
/// byte-order mark and line endings unless told otherwise. Also pages
/// through very large files by line, finds and replaces, formats JSON, and
/// reads Markdown and HTML into blocks to draw natively
/// ([TextCore.markdown], [TextCore.html]).
library;

export 'src/markup.dart';
export 'src/text_core_api.dart'
    show
        LineEnding,
        TextCore,
        TextCoreException,
        TextDocument,
        TextEncodingInfo,
        TextFileInfo,
        TextMatch;
