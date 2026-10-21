// The Markdown and HTML reading views. The model's JSON parsing is tested
// on its own; the rest runs against the real native library (point
// TEXT_CORE_LIB_PATH at libtext_ffi, see scripts/build_libs.sh env).

import 'dart:convert';
import 'dart:math';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_text_core/flutter_text_core.dart';

const _plain = MarkupText('');

MarkupParagraph _para(String text) => MarkupParagraph([MarkupText(text)]);

/// Each block as a line of plain text, to compare structure quickly: `h1
/// text`, `p text`, `> ` before a quote's lines, `- ` or `3. ` (and `[x] `)
/// before a list item's first line and two spaces before the rest,
/// `code[language] text`, `hr`, and `table header|cells / row / row`.
List<String> _outline(List<MarkupBlock> blocks) {
  final out = <String>[];
  String text(List<MarkupInline> inlines) => MarkupInline.plainText(inlines);
  String row(List<List<MarkupInline>> cells) => cells.map(text).join('|');
  for (final block in blocks) {
    switch (block) {
      case MarkupHeading(:final level, :final inlines):
        out.add('h$level ${text(inlines)}');
      case MarkupParagraph(:final inlines):
        out.add('p ${text(inlines)}');
      case MarkupQuote(:final blocks):
        out.addAll(_outline(blocks).map((line) => '> $line'));
      case MarkupList(:final ordered, :final start, :final items):
        for (var i = 0; i < items.length; i++) {
          final marker = ordered ? '${start + i}.' : '-';
          final check = switch (items[i].checked) {
            true => '[x] ',
            false => '[ ] ',
            null => '',
          };
          final lines = _outline(items[i].blocks);
          if (lines.isEmpty) out.add('$marker $check'.trimRight());
          for (var j = 0; j < lines.length; j++) {
            out.add(j == 0 ? '$marker $check${lines[j]}' : '  ${lines[j]}');
          }
        }
      case MarkupCode(:final language, :final text):
        out.add('code[$language] $text');
      case MarkupRule():
        out.add('hr');
      case MarkupTable(:final header, :final rows):
        out.add('table ${row(header)} / ${rows.map(row).join(' / ')}');
    }
  }
  return out;
}

/// The inlines of a document that is a single paragraph or heading.
List<MarkupInline> _only(MarkupDocument doc) => switch (doc.blocks) {
  [MarkupParagraph(:final inlines)] => inlines,
  [MarkupHeading(:final inlines)] => inlines,
  final other => throw StateError('not one paragraph: $other'),
};

/// The promises the model makes to whoever draws it.
void _check(List<MarkupBlock> blocks, [int depth = 0]) {
  expect(depth, lessThan(100));
  for (final block in blocks) {
    switch (block) {
      case MarkupHeading(:final level, :final inlines):
        expect(level, inInclusiveRange(1, 6));
        expect(inlines, isNotEmpty);
        _checkInlines(inlines);
      case MarkupParagraph(:final inlines):
        expect(inlines, isNotEmpty);
        _checkInlines(inlines);
      case MarkupQuote(:final blocks):
        expect(blocks, isNotEmpty);
        _check(blocks, depth + 1);
      case MarkupList(:final items):
        expect(items, isNotEmpty);
        for (final item in items) {
          _check(item.blocks, depth + 1);
        }
      case MarkupCode():
      case MarkupRule():
        break;
      case MarkupTable(:final align, :final header, :final rows):
        expect(align, isNotEmpty);
        expect(header.isEmpty || header.length == align.length, isTrue);
        for (final row in rows) {
          expect(row, hasLength(align.length));
          row.forEach(_checkInlines);
        }
    }
  }
}

void _checkInlines(List<MarkupInline> inlines) {
  String style(MarkupText t) =>
      '${t.bold} ${t.italic} ${t.code} ${t.strike} ${t.link}';
  for (var i = 0; i < inlines.length; i++) {
    final inline = inlines[i];
    if (inline is MarkupText) {
      expect(inline.text, isNotEmpty);
      final next = i + 1 < inlines.length ? inlines[i + 1] : null;
      // Neighbouring runs differ in style.
      if (next is MarkupText) expect(style(next), isNot(style(inline)));
    }
  }
  if (inlines.isNotEmpty) expect(inlines.last, isNot(isA<MarkupBreak>()));
}

void main() {
  final available = TextCore.isAvailable && TextCore.canReadMarkup;

  test('reads the JSON shape the engine writes', () {
    const json = '''
{"title": "Doc", "blocks": [
  {"type": "heading", "level": 2, "inlines": [{"type": "text", "text": "H", "bold": true}]},
  {"type": "paragraph", "inlines": [
    {"type": "text", "text": "a", "italic": true, "code": true, "strike": true, "link": "/x"},
    {"type": "image", "alt": "pic", "src": "p.png", "link": "/y"},
    {"type": "break"},
    {"type": "future-inline"}]},
  {"type": "quote", "blocks": [{"type": "rule"}]},
  {"type": "list", "ordered": true, "start": -2, "items": [
    {"checked": true, "blocks": []}, {"blocks": [{"type": "code", "language": "sh", "text": "ls"}]}]},
  {"type": "table", "align": ["none", "left", "center", "right", "odd"],
   "header": [[], [], [], [], []], "rows": [[[{"type": "text", "text": "1"}], [], [], [], []]]},
  {"type": "future-block"}
]}''';
    final doc = MarkupDocument.fromJson(
      jsonDecode(json) as Map<String, dynamic>,
    );
    expect(doc.title, 'Doc');
    expect(doc.blocks, [
      const MarkupHeading(2, [MarkupText('H', bold: true)]),
      const MarkupParagraph([
        MarkupText('a', italic: true, code: true, strike: true, link: '/x'),
        MarkupImage(alt: 'pic', src: 'p.png', link: '/y'),
        MarkupBreak(),
        _plain,
      ]),
      const MarkupQuote([MarkupRule()]),
      const MarkupList(
        ordered: true,
        start: -2,
        items: [
          MarkupListItem(checked: true),
          MarkupListItem(
            blocks: [MarkupCode(language: 'sh', text: 'ls')],
          ),
        ],
      ),
      const MarkupTable(
        align: [
          MarkupAlign.none,
          MarkupAlign.left,
          MarkupAlign.center,
          MarkupAlign.right,
          MarkupAlign.none,
        ],
        header: [[], [], [], [], []],
        rows: [
          [
            [MarkupText('1')],
            [],
            [],
            [],
            [],
          ],
        ],
      ),
      const MarkupParagraph([]),
    ]);
    expect(
      doc,
      MarkupDocument.fromJson(jsonDecode(json) as Map<String, dynamic>),
    );
    expect(
      doc.hashCode,
      MarkupDocument.fromJson(
        jsonDecode(json) as Map<String, dynamic>,
      ).hashCode,
    );
    expect(doc, isNot(const MarkupDocument(title: 'Doc')));
    expect(
      MarkupInline.plainText((doc.blocks[1] as MarkupParagraph).inlines),
      'apic ',
    );
    expect(MarkupDocument.fromJson(const {}).blocks, isEmpty);
    expect(MarkupDocument.fromJson(const {}).title, isNull);
    expect(MarkupDocument.fromJson(const {}).truncated, isFalse);
    final cut = MarkupDocument.fromJson(const {
      'title': null,
      'blocks': [],
      'truncated': true,
    });
    expect(cut.truncated, isTrue);
    expect(cut, isNot(const MarkupDocument()));
    expect(cut, const MarkupDocument(truncated: true));
  });

  test('reads Markdown', () {
    final doc = TextCore.markdown('''
# Shopping *list*

Buy **milk** and `eggs`, see [the shop](https://shop.example "Shop")
or https://example.com.\\
Then ~~nothing~~.

- [x] bread
- [ ] butter
  1. salted

> quoted
> > nested

```dart
void main() {}
```

| Item | Price |
|:-----|------:|
| Tea  | ₹ 20  |

***
''');
    expect(doc.title, 'Shopping list');
    expect(doc.blocks, [
      const MarkupHeading(1, [
        MarkupText('Shopping '),
        MarkupText('list', italic: true),
      ]),
      const MarkupParagraph([
        MarkupText('Buy '),
        MarkupText('milk', bold: true),
        MarkupText(' and '),
        MarkupText('eggs', code: true),
        MarkupText(', see '),
        MarkupText('the shop', link: 'https://shop.example'),
        MarkupText(' or '),
        MarkupText('https://example.com', link: 'https://example.com'),
        MarkupText('.'),
        MarkupBreak(),
        MarkupText('Then '),
        MarkupText('nothing', strike: true),
        MarkupText('.'),
      ]),
      MarkupList(
        ordered: false,
        items: [
          MarkupListItem(checked: true, blocks: [_para('bread')]),
          MarkupListItem(
            checked: false,
            blocks: [
              _para('butter'),
              MarkupList(
                ordered: true,
                items: [
                  MarkupListItem(blocks: [_para('salted')]),
                ],
              ),
            ],
          ),
        ],
      ),
      MarkupQuote([
        _para('quoted'),
        MarkupQuote([_para('nested')]),
      ]),
      const MarkupCode(language: 'dart', text: 'void main() {}'),
      const MarkupTable(
        align: [MarkupAlign.left, MarkupAlign.right],
        header: [
          [MarkupText('Item')],
          [MarkupText('Price')],
        ],
        rows: [
          [
            [MarkupText('Tea')],
            [MarkupText('₹ 20')],
          ],
        ],
      ),
      const MarkupRule(),
    ]);
  }, skip: !available);

  group('Markdown', () {
    List<String> outline(String text) =>
        _outline(TextCore.markdown(text).blocks);
    List<MarkupInline> runs(String text) => _only(TextCore.markdown(text));

    test('headings, paragraphs, breaks and rules', () {
      expect(
        outline(
          'Title\n=====\n\nSub *title*\n---\n\n### Three ###\n\n'
          '#hashtag\n\n    # code\n\n***\n- - -\n___',
        ),
        [
          'h1 Title',
          'h2 Sub title',
          'h3 Three',
          'p #hashtag',
          'code[] # code',
          'hr',
          'hr',
          'hr',
        ],
      );
      expect(runs('one  \ntwo\\\nthree\nfour'), const [
        MarkupText('one'),
        MarkupBreak(),
        MarkupText('two'),
        MarkupBreak(),
        MarkupText('three four'),
      ]);
      expect(TextCore.markdown('Intro\n\n# First\n\n# Second').title, 'First');
      expect(TextCore.markdown('## Only two').title, isNull);
    });

    test('emphasis, strikethrough and code by CommonMark rules', () {
      expect(runs('*a* **b** ***c*** _d_ __e__ ~~f~~ ~g~ `h`'), const [
        MarkupText('a', italic: true),
        MarkupText(' '),
        MarkupText('b', bold: true),
        MarkupText(' '),
        MarkupText('c', bold: true, italic: true),
        MarkupText(' '),
        MarkupText('d', italic: true),
        MarkupText(' '),
        MarkupText('e', bold: true),
        MarkupText(' '),
        MarkupText('f', strike: true),
        MarkupText(' '),
        MarkupText('g', strike: true),
        MarkupText(' '),
        MarkupText('h', code: true),
      ]);
      expect(runs('snake_case_name 2*3*4 a * b *'), const [
        MarkupText('snake_case_name 2'),
        MarkupText('3', italic: true),
        MarkupText('4 a * b *'),
      ]);
      expect(runs('*foo**bar**baz*'), const [
        MarkupText('foo', italic: true),
        MarkupText('bar', bold: true, italic: true),
        MarkupText('baz', italic: true),
      ]);
      expect(runs('`` a ` b `` `*not*` \\*escaped\\*'), const [
        MarkupText('a ` b', code: true),
        MarkupText(' '),
        MarkupText('*not*', code: true),
        MarkupText(' *escaped*'),
      ]);
    });

    test('links, images, references and bare addresses', () {
      expect(
        runs(
          '[a](/u "t") [b][ref] [Ref] <https://x.org> www.y.org '
          'me@z.org ![pic](p.png) [![badge](b.svg)](/ci) [none]() '
          '[js](javascript:alert(1))\n\n[ref]: /r',
        ),
        const [
          MarkupText('a', link: '/u'),
          MarkupText(' '),
          MarkupText('b', link: '/r'),
          MarkupText(' '),
          MarkupText('Ref', link: '/r'),
          MarkupText(' '),
          MarkupText('https://x.org', link: 'https://x.org'),
          MarkupText(' '),
          MarkupText('www.y.org', link: 'http://www.y.org'),
          MarkupText(' '),
          MarkupText('me@z.org', link: 'mailto:me@z.org'),
          MarkupText(' '),
          MarkupImage(alt: 'pic', src: 'p.png'),
          MarkupText(' '),
          MarkupImage(alt: 'badge', src: 'b.svg', link: '/ci'),
          MarkupText(' none js'),
        ],
      );
    });

    test('lists, tasks and quotes nest', () {
      expect(
        outline(
          '3. three\n4. four\n   - nested\n\n     loose\n\n'
          '- [x] done\n- [ ] todo\n  - [X] sub\n\n'
          '> quote\nlazy\n> > deeper\n>\n> - in quote',
        ),
        [
          '3. p three',
          '4. p four',
          '  - p nested',
          '    p loose',
          '- [x] p done',
          '- [ ] p todo',
          '  - [x] p sub',
          '> p quote lazy',
          '> > p deeper',
          '> - p in quote',
        ],
      );
      final list = TextCore.markdown('7) a\n8) b').blocks.single as MarkupList;
      expect(list.ordered, isTrue);
      expect(list.start, 7);
      expect(list.items, hasLength(2));
    });

    test('tables keep their alignment and inline styles', () {
      final table =
          TextCore.markdown(
                '| a | *b* | c | d |\n|---|:--|:-:|--:|\n'
                '| `1` | [2](/u) | 3 \\| 4 |\n| x | y | z | w | extra |',
              ).blocks.single
              as MarkupTable;
      expect(table.align, MarkupAlign.values);
      expect(table.header[1], const [MarkupText('b', italic: true)]);
      expect(table.rows, const [
        [
          [MarkupText('1', code: true)],
          [MarkupText('2', link: '/u')],
          [MarkupText('3 | 4')],
          <MarkupInline>[],
        ],
        [
          [MarkupText('x')],
          [MarkupText('y')],
          [MarkupText('z')],
          [MarkupText('w')],
        ],
      ]);
    });

    test('code blocks keep their text and language', () {
      expect(
        TextCore.markdown(
          '```py title="x"\nif a:\n    b()\n```\n\n~~~\n<b>&amp;</b>\n~~~\n\n'
          '    indented\n      more',
        ).blocks,
        const [
          MarkupCode(language: 'py', text: 'if a:\n    b()'),
          MarkupCode(text: '<b>&amp;</b>'),
          MarkupCode(text: 'indented\n  more'),
        ],
      );
    });

    test('raw HTML is read as HTML', () {
      final doc = TextCore.markdown(
        '<p align="center">\n  <img src="logo.png" alt="Logo">\n</p>\n\n'
        '<h1 align="center">App</h1>\n\n'
        'Press <kbd>Ctrl</kbd>+<b>S</b><br>to save. <!-- hidden -->\n\n'
        '<details>\n<summary>More</summary>\n\n*Inside*\n\n</details>\n\n'
        '<script>alert(1)</script>',
      );
      expect(doc.title, 'App');
      expect(doc.blocks, const [
        MarkupParagraph([MarkupImage(alt: 'Logo', src: 'logo.png')]),
        MarkupHeading(1, [MarkupText('App')]),
        MarkupParagraph([
          MarkupText('Press '),
          MarkupText('Ctrl', code: true),
          MarkupText('+'),
          MarkupText('S', bold: true),
          MarkupBreak(),
          MarkupText('to save.'),
        ]),
        MarkupParagraph([MarkupText('More')]),
        MarkupParagraph([MarkupText('Inside', italic: true)]),
      ]);
    });

    test('entities, escapes and front matter', () {
      expect(runs('&copy; &amp; &#x1F600; &#8212; &nosuch; \\<b\\>'), const [
        MarkupText('© & 😀 — &nosuch; <b>'),
      ]);
      expect(
        TextCore.markdown('---\ntitle: Notes\n---\n# Notes').blocks.first,
        const MarkupCode(language: 'yaml', text: 'title: Notes'),
      );
    });
  }, skip: !available);

  test('reads HTML, however broken', () {
    final doc = TextCore.html('''
<!doctype html><html><head><title>Caf&eacute; menu</title>
<style>body { color: red }</style><script>alert("no")</script></head>
<body><h1>Menu</h1><p>Fresh <b>coffee <i>daily</b> &amp; more
<ul><li>Tea<li>Cake <img src="cake.png" alt="cake"></ul>
<table><tr><th>Item<th align=right>Price<tr><td>Tea<td>20</table>
<pre>  keep
    this</pre><div hidden>secret</div>
''');
    expect(doc.title, 'Café menu');
    expect(doc.blocks, [
      const MarkupHeading(1, [MarkupText('Menu')]),
      const MarkupParagraph([
        MarkupText('Fresh '),
        MarkupText('coffee ', bold: true),
        MarkupText('daily', bold: true, italic: true),
        MarkupText(' & more', italic: true),
      ]),
      const MarkupList(
        ordered: false,
        items: [
          MarkupListItem(
            blocks: [
              MarkupParagraph([MarkupText('Tea', italic: true)]),
            ],
          ),
          MarkupListItem(
            blocks: [
              MarkupParagraph([
                MarkupText('Cake ', italic: true),
                MarkupImage(alt: 'cake', src: 'cake.png'),
              ]),
            ],
          ),
        ],
      ),
      // The italic left open carries on, as in a browser, but not into
      // table cells.
      const MarkupTable(
        align: [MarkupAlign.none, MarkupAlign.right],
        header: [
          [MarkupText('Item')],
          [MarkupText('Price')],
        ],
        rows: [
          [
            [MarkupText('Tea')],
            [MarkupText('20')],
          ],
        ],
      ),
      const MarkupCode(text: '  keep\n    this'),
    ]);
  }, skip: !available);

  group('HTML', () {
    List<String> outline(String text) => _outline(TextCore.html(text).blocks);

    test('maps elements to blocks and styles', () {
      expect(
        outline(
          '<h2>Two</h2><p>a<br>b</p><blockquote><p>q</blockquote>'
          '<ol start=5><li>five<li>six</ol><dl><dt>Term<dd>Meaning</dl>'
          '<hr><section><div>in div</div></section><figure><img src=f.png alt=F>'
          '<figcaption>Cap</figcaption></figure>',
        ),
        [
          'h2 Two',
          'p a b',
          '> p q',
          '5. p five',
          '6. p six',
          'p Term',
          'p Meaning',
          'hr',
          'p in div',
          'p F',
          'p Cap',
        ],
      );
      expect(
        _only(
          TextCore.html(
            '<p><strong>b</strong> <em>i</em> <u>u</u> <del>d</del> '
            '<kbd>k</kbd> x<sup>2</sup> <a href="/l">l</a> '
            '<a href="javascript:x()">js</a></p>',
          ),
        ),
        const [
          MarkupText('b', bold: true),
          MarkupText(' '),
          MarkupText('i', italic: true),
          MarkupText(' u '),
          MarkupText('d', strike: true),
          MarkupText(' '),
          MarkupText('k', code: true),
          MarkupText(' x2 '),
          MarkupText('l', link: '/l'),
          MarkupText(' js'),
        ],
      );
    });

    test('closes what HTML lets authors leave open', () {
      expect(
        outline(
          '<p>one<p>two<div>three</div>four<ul><li>a<li>b<ul><li>b1</ul>'
          '<li>c</ul><h1>h<h2>i</h2><table><tr><td>1<td>2<tr><td>3</table>'
          '</span></div>stray',
        ),
        [
          'p one',
          'p two',
          'p three',
          'p four',
          '- p a',
          '- p b',
          '  - p b1',
          '- p c',
          'h1 h',
          'h2 i',
          'table  / 1|2 / 3|',
          'p stray',
        ],
      );
    });

    test('skips what a reader does not see', () {
      expect(
        outline(
          '<head><meta charset=utf-8><style>p{}</style></head><p>keep</p>'
          '<script>no()</script><noscript>no</noscript><template>no</template>'
          '<svg><text>no</text></svg><iframe>no</iframe><form><label>Name'
          '</label><input value=no><select><option>no</select>'
          '<button>no</button></form><p hidden>no</p>'
          '<div style="display: none">no</div>',
        ),
        ['p keep', 'p Name'],
      );
    });

    test('tables find their header, alignment and spans', () {
      final table = TextCore.html(
        '<table><caption>Prices</caption><thead><tr>'
        '<th align=left>Item<th style="text-align:center">Cost'
        '</thead><tbody><tr><td>Tea<td>2<tr><td colspan=2>Total'
        '</tbody><tfoot><tr><th>Sum<td>2</tfoot></table>',
      ).blocks;
      expect(table.first, _para('Prices'));
      expect(
        table.last,
        const MarkupTable(
          align: [MarkupAlign.left, MarkupAlign.center],
          header: [
            [MarkupText('Item')],
            [MarkupText('Cost')],
          ],
          rows: [
            [
              [MarkupText('Tea')],
              [MarkupText('2')],
            ],
            [
              [MarkupText('Total')],
              [],
            ],
            [
              [MarkupText('Sum', bold: true)],
              [MarkupText('2')],
            ],
          ],
        ),
      );
      expect(outline('<table><tr><td></td></tr></table>'), isEmpty);
    });

    test('white space collapses except in pre', () {
      expect(TextCore.html('<p>\n  a \t b&nbsp;&nbsp;c\n</p>').blocks, [
        _para('a b\u00A0\u00A0c'),
      ]);
      expect(
        TextCore.html(
          '<pre class="language-rust">\nfn main() {\n    x &lt; y;\n}\n</pre>',
        ).blocks,
        const [
          MarkupCode(language: 'rust', text: 'fn main() {\n    x < y;\n}'),
        ],
      );
    });

    test('entities and titles', () {
      final doc = TextCore.html(
        '<title> A &amp; B </title><p>&copy &euro; &#8364; &#x20AC; &#128; '
        '&hellip; &notit; &bogus;</p>',
      );
      expect(doc.title, 'A & B');
      expect(doc.blocks, [_para('© € € € € … ¬it; &bogus;')]);
      expect(TextCore.html('<h1>First</h1><h1>Second</h1>').title, 'First');
      expect(TextCore.html('<svg><title>icon</title></svg>').title, isNull);
    });
  }, skip: !available);

  test('reads on a background isolate', () async {
    final markdown = await TextCore.markdownAsync('# Title\n\ntext');
    expect(markdown.title, 'Title');
    expect(markdown.blocks.last, _para('text'));
    final html = await TextCore.htmlAsync('<p>a<br>b</p>');
    expect(html.blocks, [
      const MarkupParagraph([MarkupText('a'), MarkupBreak(), MarkupText('b')]),
    ]);
  }, skip: !available);

  test('keeps any text intact on its way through', () {
    const text = 'नमस्ते 你好 😀 "quotes" \\back\\ tab\tend é';
    expect(TextCore.markdown('`$text`').blocks, const [
      MarkupParagraph([MarkupText(text, code: true)]),
    ]);
    expect(TextCore.html('<pre>$text</pre>').blocks, const [
      MarkupCode(text: text),
    ]);
  }, skip: !available);

  test('copes with odd input and refuses huge input', () {
    expect(TextCore.markdown('').blocks, isEmpty);
    expect(TextCore.html('').title, isNull);
    expect(TextCore.markdown('a\x00b').blocks, [_para('a\u{FFFD}b')]);
    // Half a surrogate pair cannot be UTF-8 either.
    final lone = 'a${String.fromCharCode(0xD800)}b';
    expect(TextCore.html(lone).blocks, [_para('a\u{FFFD}b')]);
    for (final text in [
      '<<<',
      '*' * 1000,
      '[' * 1000,
      '<b>' * 1000,
      '> ' * 1000,
      '- ' * 1000,
      '<div>' * 1000,
    ]) {
      _check(TextCore.markdown(text).blocks);
      _check(TextCore.html(text).blocks);
    }
    // A document too long to read whole keeps its beginning and says so.
    final long = TextCore.markdown('- [ ] buy milk\n' * 60000);
    expect(long.truncated, isTrue);
    final list = long.blocks.single as MarkupList;
    expect(list.items.length, lessThan(60000));
    expect(list.items.first.blocks, [_para('buy milk')]);
    _check(long.blocks);
    expect(TextCore.html('<p>x</p>' * 200000).truncated, isTrue);
    expect(TextCore.markdown('- a\n' * 100).truncated, isFalse);
    expect(
      () => TextCore.markdown('a' * (33 << 20)),
      throwsA(
        isA<TextCoreException>().having((e) => e.code, 'code', 'TOO_LARGE'),
      ),
    );
  }, skip: !available);

  test('random input always reads into a sound model', () {
    // Pieces to string together: markup split at spaces and line ends,
    // then pieces that are or end in white space.
    final pieces = [
      ...r'''* ** _ ~~ ` [ ]( ) ![ < > & &amp; # | \ word é 😀 <b> </b> <p> <li>
<ul> <table> <tr> <td> <pre> <script> <a href= " ' <br> <img src=x alt= <!--
--> <h1> http://a.b me@x.y'''
          .split(RegExp(r'[ \n]')),
      ...'```\n|# |- |1. |> |\n|\n\n| |  |\t|[ ] |[x] |\x00'.split('|'),
    ];
    final random = Random(42);
    for (var i = 0; i < 300; i++) {
      final text = [
        for (var j = random.nextInt(60); j > 0; j--)
          pieces[random.nextInt(pieces.length)],
      ].join();
      for (final doc in [TextCore.markdown(text), TextCore.html(text)]) {
        _check(doc.blocks);
        expect(doc.title?.trim(), doc.title);
      }
    }
  }, skip: !available);
}
