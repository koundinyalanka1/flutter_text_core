// The reading model of Markdown and HTML: blocks of styled text for the app
// to draw natively. Built by TextCore.markdown and TextCore.html.
//
// Every class has value equality, so a test or a widget can compare whole
// documents.

/// A Markdown or HTML document as blocks to draw.
class MarkupDocument {
  const MarkupDocument({
    this.title,
    this.blocks = const [],
    this.truncated = false,
  });

  factory MarkupDocument.fromJson(Map<String, dynamic> json) => MarkupDocument(
    title: json['title'] as String?,
    blocks: MarkupBlock.listFromJson(json['blocks']),
    truncated: json['truncated'] == true,
  );

  /// The first level-1 heading (Markdown), or the `<title>` and else the
  /// first `<h1>` (HTML).
  final String? title;
  final List<MarkupBlock> blocks;

  /// The blocks are only the beginning of a document too long to read
  /// whole: about a hundred thousand blocks, list items, table cells and
  /// runs of text are read, and the rest is left out.
  final bool truncated;

  @override
  bool operator ==(Object other) =>
      other is MarkupDocument &&
      other.title == title &&
      other.truncated == truncated &&
      _same(other.blocks, blocks);

  @override
  int get hashCode => Object.hash(title, truncated, _hash(blocks));

  @override
  String toString() =>
      'MarkupDocument($title, $blocks${truncated ? ', truncated' : ''})';
}

/// How a table column is aligned.
enum MarkupAlign {
  none,
  left,
  center,
  right;

  static MarkupAlign fromName(String? name) =>
      values.firstWhere((a) => a.name == name, orElse: () => MarkupAlign.none);
}

/// A block of a document.
sealed class MarkupBlock {
  const MarkupBlock();

  /// Kinds of block this version does not know become empty paragraphs.
  factory MarkupBlock.fromJson(Map<String, dynamic> json) =>
      switch (json['type']) {
        'heading' => MarkupHeading(
          (json['level'] as int? ?? 1).clamp(1, 6),
          MarkupInline.listFromJson(json['inlines']),
        ),
        'paragraph' => MarkupParagraph(
          MarkupInline.listFromJson(json['inlines']),
        ),
        'quote' => MarkupQuote(MarkupBlock.listFromJson(json['blocks'])),
        'list' => MarkupList(
          ordered: json['ordered'] == true,
          start: json['start'] as int? ?? 1,
          items: [
            for (final item in (json['items'] as List? ?? const []))
              MarkupListItem.fromJson(item as Map<String, dynamic>),
          ],
        ),
        'code' => MarkupCode(
          language: json['language'] as String? ?? '',
          text: json['text'] as String? ?? '',
        ),
        'rule' => const MarkupRule(),
        'table' => MarkupTable(
          align: [
            for (final name in (json['align'] as List? ?? const []))
              MarkupAlign.fromName(name as String?),
          ],
          header: [
            for (final cell in (json['header'] as List? ?? const []))
              MarkupInline.listFromJson(cell),
          ],
          rows: [
            for (final row in (json['rows'] as List? ?? const []))
              [for (final cell in row as List) MarkupInline.listFromJson(cell)],
          ],
        ),
        _ => const MarkupParagraph([]),
      };

  static List<MarkupBlock> listFromJson(Object? json) => [
    for (final block in (json as List? ?? const []))
      MarkupBlock.fromJson(block as Map<String, dynamic>),
  ];
}

final class MarkupHeading extends MarkupBlock {
  const MarkupHeading(this.level, this.inlines);

  /// 1 to 6.
  final int level;
  final List<MarkupInline> inlines;

  @override
  bool operator ==(Object other) =>
      other is MarkupHeading &&
      other.level == level &&
      _same(other.inlines, inlines);

  @override
  int get hashCode => Object.hash(level, _hash(inlines));

  @override
  String toString() => 'MarkupHeading($level, $inlines)';
}

final class MarkupParagraph extends MarkupBlock {
  const MarkupParagraph(this.inlines);

  final List<MarkupInline> inlines;

  @override
  bool operator ==(Object other) =>
      other is MarkupParagraph && _same(other.inlines, inlines);

  @override
  int get hashCode => _hash(inlines);

  @override
  String toString() => 'MarkupParagraph($inlines)';
}

final class MarkupQuote extends MarkupBlock {
  const MarkupQuote(this.blocks);

  final List<MarkupBlock> blocks;

  @override
  bool operator ==(Object other) =>
      other is MarkupQuote && _same(other.blocks, blocks);

  @override
  int get hashCode => _hash(blocks);

  @override
  String toString() => 'MarkupQuote($blocks)';
}

final class MarkupList extends MarkupBlock {
  const MarkupList({
    required this.ordered,
    this.start = 1,
    required this.items,
  });

  final bool ordered;

  /// The first item's number (1 for bullet lists); HTML allows 0 and
  /// negative numbers.
  final int start;
  final List<MarkupListItem> items;

  @override
  bool operator ==(Object other) =>
      other is MarkupList &&
      other.ordered == ordered &&
      other.start == start &&
      _same(other.items, items);

  @override
  int get hashCode => Object.hash(ordered, start, _hash(items));

  @override
  String toString() =>
      'MarkupList(${ordered ? 'ordered from $start' : 'bullets'}, $items)';
}

/// A list item. [checked] is set for task-list items (`- [x] done`).
class MarkupListItem {
  const MarkupListItem({this.checked, this.blocks = const []});

  factory MarkupListItem.fromJson(Map<String, dynamic> json) => MarkupListItem(
    checked: json['checked'] as bool?,
    blocks: MarkupBlock.listFromJson(json['blocks']),
  );

  final bool? checked;
  final List<MarkupBlock> blocks;

  @override
  bool operator ==(Object other) =>
      other is MarkupListItem &&
      other.checked == checked &&
      _same(other.blocks, blocks);

  @override
  int get hashCode => Object.hash(checked, _hash(blocks));

  @override
  String toString() =>
      'MarkupListItem(${checked == null ? '' : '[${checked! ? 'x' : ' '}] '}$blocks)';
}

final class MarkupCode extends MarkupBlock {
  const MarkupCode({this.language = '', required this.text});

  /// A fence's info string's first word or an HTML `language-*` class, or
  /// empty.
  final String language;

  /// Lines separated by `\n`, without a final line break.
  final String text;

  @override
  bool operator ==(Object other) =>
      other is MarkupCode && other.language == language && other.text == text;

  @override
  int get hashCode => Object.hash(language, text);

  @override
  String toString() => 'MarkupCode($language, ${text.length} chars)';
}

final class MarkupRule extends MarkupBlock {
  const MarkupRule();

  @override
  bool operator ==(Object other) => other is MarkupRule;

  @override
  int get hashCode => (MarkupRule).hashCode;

  @override
  String toString() => 'MarkupRule()';
}

/// A table: every row has one cell per entry of [align], and so has
/// [header] unless it is empty, which only HTML tables without a header
/// row have. A cell is a list of inlines.
final class MarkupTable extends MarkupBlock {
  const MarkupTable({
    required this.align,
    this.header = const [],
    this.rows = const [],
  });

  final List<MarkupAlign> align;
  final List<List<MarkupInline>> header;
  final List<List<List<MarkupInline>>> rows;

  @override
  bool operator ==(Object other) =>
      other is MarkupTable &&
      _same(other.align, align) &&
      _same(other.header, header) &&
      _same(other.rows, rows);

  @override
  int get hashCode => Object.hash(_hash(align), _hash(header), _hash(rows));

  @override
  String toString() =>
      'MarkupTable(${align.length} columns, ${rows.length} rows)';
}

/// A piece of a paragraph, heading or cell. Neighbouring [MarkupText]s
/// always differ in style.
sealed class MarkupInline {
  const MarkupInline();

  /// Kinds of inline this version does not know become empty text.
  factory MarkupInline.fromJson(Map<String, dynamic> json) =>
      switch (json['type']) {
        'text' => MarkupText(
          json['text'] as String? ?? '',
          bold: json['bold'] == true,
          italic: json['italic'] == true,
          code: json['code'] == true,
          strike: json['strike'] == true,
          link: json['link'] as String?,
        ),
        'image' => MarkupImage(
          alt: json['alt'] as String? ?? '',
          src: json['src'] as String? ?? '',
          link: json['link'] as String?,
        ),
        'break' => const MarkupBreak(),
        _ => const MarkupText(''),
      };

  static List<MarkupInline> listFromJson(Object? json) => [
    for (final inline in (json as List? ?? const []))
      MarkupInline.fromJson(inline as Map<String, dynamic>),
  ];

  /// The text of [inlines], images as their alt text and breaks as spaces:
  /// for a table of contents, search or accessibility labels.
  static String plainText(Iterable<MarkupInline> inlines) {
    final out = StringBuffer();
    for (final inline in inlines) {
      out.write(switch (inline) {
        MarkupText(:final text) => text,
        MarkupImage(:final alt) => alt,
        MarkupBreak() => ' ',
      });
    }
    return out.toString();
  }
}

/// A run of text in one style.
final class MarkupText extends MarkupInline {
  const MarkupText(
    this.text, {
    this.bold = false,
    this.italic = false,
    this.code = false,
    this.strike = false,
    this.link,
  });

  final String text;
  final bool bold;
  final bool italic;

  /// Monospaced: inline code, keyboard keys and the like.
  final bool code;
  final bool strike;

  /// Where the run links to, as written in the document (it may be
  /// relative, a `mailto:` or a `#fragment`).
  final String? link;

  @override
  bool operator ==(Object other) =>
      other is MarkupText &&
      other.text == text &&
      other.bold == bold &&
      other.italic == italic &&
      other.code == code &&
      other.strike == strike &&
      other.link == link;

  @override
  int get hashCode => Object.hash(text, bold, italic, code, strike, link);

  @override
  String toString() {
    final flags = [
      if (bold) 'bold',
      if (italic) 'italic',
      if (code) 'code',
      if (strike) 'strike',
      if (link != null) 'link: $link',
    ];
    return 'MarkupText("$text"${flags.isEmpty ? '' : ', ${flags.join(', ')}'})';
  }
}

/// A picture, by its [src] as written (a URL or a path relative to the
/// document).
final class MarkupImage extends MarkupInline {
  const MarkupImage({required this.alt, required this.src, this.link});

  final String alt;
  final String src;
  final String? link;

  @override
  bool operator ==(Object other) =>
      other is MarkupImage &&
      other.alt == alt &&
      other.src == src &&
      other.link == link;

  @override
  int get hashCode => Object.hash(alt, src, link);

  @override
  String toString() =>
      'MarkupImage("$alt", $src${link == null ? '' : ', link: $link'})';
}

/// A hard line break.
final class MarkupBreak extends MarkupInline {
  const MarkupBreak();

  @override
  bool operator ==(Object other) => other is MarkupBreak;

  @override
  int get hashCode => (MarkupBreak).hashCode;

  @override
  String toString() => 'MarkupBreak()';
}

/// Equality that looks inside lists.
bool _same(Object? a, Object? b) {
  if (a is List && b is List) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (!_same(a[i], b[i])) return false;
    }
    return true;
  }
  return a == b;
}

int _hash(Object? value) =>
    value is List ? Object.hashAll(value.map(_hash)) : value.hashCode;
