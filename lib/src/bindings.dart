// Raw dart:ffi bindings for the Rust text_ffi library.
//
// Strings are UTF-8 and NUL-terminated. Strings the library returns are
// freed with [TextBindings.freeString]; `text_last_error` returns a borrowed
// pointer that must not be freed.
//
// This file is internal to the package (not exported), so its fields may use
// the private native signature typedefs.
// ignore_for_file: library_private_types_in_public_api

import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

typedef _StrC = Pointer<Utf8> Function();
typedef _FreeStringC = Void Function(Pointer<Utf8>);
typedef _FreeStringDart = void Function(Pointer<Utf8>);
typedef _OpenC = Int64 Function(Pointer<Utf8>, Pointer<Utf8>);
typedef _OpenDart = int Function(Pointer<Utf8>, Pointer<Utf8>);
typedef _HandleC = Int64 Function(Int64);
typedef _HandleDart = int Function(int);
typedef _NewC = Int64 Function();
typedef _NewDart = int Function();
typedef _InfoC = Pointer<Utf8> Function(Int64, Int32);
typedef _InfoDart = Pointer<Utf8> Function(int, int);
typedef _GetC = Pointer<Utf8> Function(Int64);
typedef _GetDart = Pointer<Utf8> Function(int);
typedef _SetC = Int64 Function(Int64, Pointer<Utf8>);
typedef _SetDart = int Function(int, Pointer<Utf8>);
typedef _LinesC = Pointer<Utf8> Function(Int64, Int64, Int64);
typedef _LinesDart = Pointer<Utf8> Function(int, int, int);
typedef _FindC = Pointer<Utf8> Function(Int64, Pointer<Utf8>, Int32, Int64);
typedef _FindDart = Pointer<Utf8> Function(int, Pointer<Utf8>, int, int);
typedef _ReplaceC = Int64 Function(Int64, Pointer<Utf8>, Pointer<Utf8>, Int32, Int64);
typedef _ReplaceDart = int Function(int, Pointer<Utf8>, Pointer<Utf8>, int, int);
typedef _FormatC = Int64 Function(Int64, Pointer<Utf8>, Int32, Pointer<Utf8>);
typedef _FormatDart = int Function(int, Pointer<Utf8>, int, Pointer<Utf8>);
typedef _JsonC = Pointer<Utf8> Function(Pointer<Utf8>, Int32);
typedef _JsonDart = Pointer<Utf8> Function(Pointer<Utf8>, int);
typedef _MarkupC = Pointer<Utf8> Function(Pointer<Utf8>);

/// The text library, resolved once per isolate.
class TextBindings {
  TextBindings._(DynamicLibrary lib)
    : _library = lib,
      lastError = lib.lookupFunction<_StrC, _StrC>('text_last_error'),
      freeString = lib.lookupFunction<_FreeStringC, _FreeStringDart>(
        'text_free_string',
      ),
      open = lib.lookupFunction<_OpenC, _OpenDart>('text_open'),
      create = lib.lookupFunction<_NewC, _NewDart>('text_new'),
      close = lib.lookupFunction<_HandleC, _HandleDart>('text_close'),
      infoJson = lib.lookupFunction<_InfoC, _InfoDart>('text_info_json'),
      get = lib.lookupFunction<_GetC, _GetDart>('text_get_json'),
      set = lib.lookupFunction<_SetC, _SetDart>('text_set_json'),
      linesJson = lib.lookupFunction<_LinesC, _LinesDart>('text_lines_json'),
      findJson = lib.lookupFunction<_FindC, _FindDart>('text_find_query_json'),
      replaceAll = lib.lookupFunction<_ReplaceC, _ReplaceDart>(
        'text_replace_all_json',
      ),
      setFormat = lib.lookupFunction<_FormatC, _FormatDart>('text_set_format'),
      reinterpret = lib.lookupFunction<_SetC, _SetDart>('text_reinterpret'),
      save = lib.lookupFunction<_SetC, _SetDart>('text_save'),
      writeCopy = lib.lookupFunction<_SetC, _SetDart>('text_write_copy'),
      encodingsJson = lib.lookupFunction<_StrC, _StrC>('text_encodings_json'),
      formatJson = lib.lookupFunction<_JsonC, _JsonDart>('text_format_json_input');

  final _StrC lastError;
  final _FreeStringDart freeString;
  final _OpenDart open;
  final _NewDart create;
  final _HandleDart close;
  final _InfoDart infoJson;
  final _GetDart get;
  final _SetDart set;
  final _LinesDart linesJson;
  final _FindDart findJson;
  final _ReplaceDart replaceAll;
  final _FormatDart setFormat;
  final _SetDart reinterpret;
  final _SetDart save;
  final _SetDart writeCopy;
  final _StrC encodingsJson;
  final _JsonDart formatJson;

  final DynamicLibrary _library;

  /// Optional: Markdown and HTML reading views arrived after the first
  /// native releases, so an older library still loads without them.
  late final _MarkupC? markdownJson = _optional(
    () => _library.lookupFunction<_MarkupC, _MarkupC>('text_markdown_json'),
  );
  late final _MarkupC? htmlJson = _optional(
    () => _library.lookupFunction<_MarkupC, _MarkupC>('text_html_json'),
  );

  static T? _optional<T>(T Function() lookup) {
    try {
      return lookup();
    } on ArgumentError {
      return null;
    }
  }

  static TextBindings? _instance;

  /// The bindings for this isolate. Throws [ArgumentError] when the native
  /// library cannot be loaded.
  ///
  /// `TEXT_CORE_LIB_PATH` overrides the library location (host tests).
  static TextBindings get instance => _instance ??= TextBindings._(_open());

  /// `text_set_backup_dir`, or null in a library older than it.
  static int Function(Pointer<Utf8>)? setBackupDir() {
    try {
      return _open().lookupFunction<
        Int64 Function(Pointer<Utf8>),
        int Function(Pointer<Utf8>)
      >('text_set_backup_dir');
    } on ArgumentError {
      return null;
    }
  }

  /// `text_remove_stale_temp_files`, or null in a library older than it.
  static int Function(Pointer<Utf8>, int)? removeStaleTempFiles() {
    try {
      return _open().lookupFunction<
        Int64 Function(Pointer<Utf8>, Int64),
        int Function(Pointer<Utf8>, int)
      >('text_remove_stale_temp_files');
    } on ArgumentError {
      return null;
    }
  }

  static DynamicLibrary _open() {
    final override = Platform.environment['TEXT_CORE_LIB_PATH'];
    if (override != null && override.isNotEmpty) {
      return DynamicLibrary.open(override);
    }
    if (Platform.isAndroid || Platform.isLinux) {
      return DynamicLibrary.open('libtext_ffi.so');
    }
    if (Platform.isMacOS || Platform.isIOS) {
      // CocoaPods links the vendored library into the app.
      final process = DynamicLibrary.process();
      if (process.providesSymbol('text_last_error')) return process;
      return DynamicLibrary.open('libtext_ffi.dylib');
    }
    if (Platform.isWindows) {
      return DynamicLibrary.open('text_ffi.dll');
    }
    return DynamicLibrary.process();
  }
}
