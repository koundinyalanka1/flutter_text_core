// Runs the Dart API against the real native library. Point
// TEXT_CORE_LIB_PATH at libtext_ffi (see scripts/build_libs.sh env).

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_text_core/flutter_text_core.dart';

void main() {
  final available = TextCore.isAvailable;
  late Directory dir;

  setUp(() => dir = Directory.systemTemp.createTempSync('text_core_test'));
  tearDown(() => dir.deleteSync(recursive: true));

  test('opens, edits and saves a Windows file as it came', () async {
    final file = File('${dir.path}/notes.txt')
      ..writeAsBytesSync('Rs. 100 paid\r\nrs. 50 due\r\n'.codeUnits);
    final doc = await TextCore.open(file.path);
    addTearDown(doc.close);
    final info = doc.info(withStats: true);
    expect(info.encoding, 'UTF-8');
    expect(info.lineEnding, LineEnding.crlf);
    expect(info.lines, 3);
    expect(info.words, 6);
    expect(doc.text, 'Rs. 100 paid\nrs. 50 due\n');
    expect(doc.find('rs.'), const [TextMatch(0, 3, 0), TextMatch(13, 16, 1)]);
    expect(doc.find('rs.', matchCase: true), const [TextMatch(13, 16, 1)]);
    expect(doc.replaceAll('rs.', '₹'), 2);
    expect(doc.lines(0, 2), ['₹ 100 paid', '₹ 50 due']);
    expect(await doc.save(file.path), 0);
    expect(file.readAsStringSync(), '₹ 100 paid\r\n₹ 50 due\r\n');
    expect(doc.info().modified, isFalse);
  }, skip: !available);

  test('warns about characters an encoding cannot save', () async {
    final file = File('${dir.path}/latin.txt')
      ..writeAsBytesSync([0x63, 0x61, 0x66, 0xE9]);
    final doc = await TextCore.open(file.path);
    addTearDown(doc.close);
    var info = doc.info(withStats: true);
    expect(info.encoding, 'windows-1252');
    expect(info.confident, isFalse);
    expect(doc.text, 'café');
    doc.setText('café ₹');
    expect(doc.info(withStats: true).unmappable, 1);
    doc.setFormat(encoding: 'UTF-8', bom: true, lineEnding: LineEnding.lf);
    info = doc.info(withStats: true);
    expect(info.unmappable, 0);
    expect(info.bom, isTrue);
    await doc.save(file.path);
    expect(file.readAsBytesSync().take(3), [0xEF, 0xBB, 0xBF]);
  }, skip: !available);

  test('re-reads an unedited file in another encoding', () async {
    final file = File('${dir.path}/ru.txt')
      ..writeAsBytesSync([0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
    final doc = await TextCore.open(file.path);
    addTearDown(doc.close);
    expect(doc.info().canReinterpret, isTrue);
    doc.reinterpret('windows-1251');
    expect(doc.text, 'Привет');
    doc.setText('changed');
    expect(
      () => doc.reinterpret('UTF-8'),
      throwsA(
        isA<TextCoreException>().having((e) => e.code, 'code', 'UNSUPPORTED'),
      ),
    );
  }, skip: !available);

  test('reports missing files and closed documents', () async {
    await expectLater(
      TextCore.open('${dir.path}/missing.txt'),
      throwsA(isA<TextCoreException>().having((e) => e.code, 'code', 'IO')),
    );
    final doc = TextCore.create();
    doc.close();
    doc.close();
    expect(() => doc.text, throwsStateError);
  }, skip: !available);

  test('formats JSON and explains errors', () {
    expect(
      TextCore.formatJson('{"a":[1,2]}'),
      '{\n  "a": [\n    1,\n    2\n  ]\n}',
    );
    expect(TextCore.formatJson('{ "a" : 1 }', indent: 0), '{"a":1}');
    expect(
      () => TextCore.formatJson('{"a" 1}'),
      throwsA(
        isA<TextCoreException>().having(
          (e) => e.message,
          'message',
          "line 1, column 6: expected ':' after the key",
        ),
      ),
    );
    final names = TextCore.encodings().map((e) => e.name).toList();
    expect(names.first, 'UTF-8');
    expect(names, contains('windows-1252'));
  }, skip: !available);

  test('a save keeps the version it replaces when backups are on', () async {
    final dir = await Directory.systemTemp.createTemp('text_backup');
    final backups = Directory('${dir.path}/Backups')..createSync();
    final file = File('${dir.path}/note.txt')..writeAsStringSync('first');
    expect(TextCore.setBackupDir(backups.path), isTrue);
    final doc = await TextCore.open(file.path);
    doc.setText('second');
    await doc.save(file.path);
    doc.close();
    expect(file.readAsStringSync(), 'second');
    final kept = backups.listSync().whereType<File>().toList();
    expect(kept.map((f) => f.readAsStringSync()), contains('first'));
    expect(TextCore.setBackupDir(''), isTrue);
    await dir.delete(recursive: true);
  }, skip: !available);

  test('stale temporary files from killed saves are removed', () async {
    final dir = await Directory.systemTemp.createTemp('text_stale');
    final stale = File('${dir.path}/.note.txt.12345.tmp')
      ..writeAsStringSync('x');
    final kept = File('${dir.path}/note.txt')..writeAsStringSync('mine');
    stale.setLastModifiedSync(
      DateTime.now().subtract(const Duration(hours: 2)),
    );
    expect(TextCore.removeStaleTempFiles(dir.path), 1);
    expect(stale.existsSync(), isFalse);
    expect(kept.existsSync(), isTrue);
    await dir.delete(recursive: true);
  }, skip: !available);
}
