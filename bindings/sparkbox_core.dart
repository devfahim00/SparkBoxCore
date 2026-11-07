// sparkbox_core.dart — ready-made Dart/Flutter bindings for SparkBoxCore.
//
// Drop-in for the Flutter app (RMB `flutter` branch): the JSON keys match the
// app's existing models, so `MediaItem.fromMap(map)` keeps working unchanged.
//
// Requires in pubspec.yaml:  ffi: ^2.1.0
//
// The native library must be packaged with the app:
//   Android: src/main/jniLibs/<abi>/libsparkbox_ffi.so   (or a plugin that links it)
//   Linux:   libsparkbox_ffi.so next to the executable
//   Windows: sparkbox_ffi.dll next to the executable
//
// All calls run on a background isolate (the C functions are blocking) and
// throw [SparkBoxCoreException] when the core returns {"ok":false}.
//
// Usage:
//   final sections = await SparkBoxCore.home();
//   final item = MediaItem.fromMap(sections.first.items.first); // app-side model
//   final detail = await SparkBoxCore.playDetail(item.toMap());
//   final streams = await SparkBoxCore.streams(detail, 1, 1, item.isSeries);

import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

class SparkBoxCoreException implements Exception {
  SparkBoxCoreException(this.message);
  final String message;
  @override
  String toString() => 'SparkBoxCoreException: $message';
}

class SparkBoxCore {
  SparkBoxCore._();

  /// Library file name per platform (override for custom packaging).
  static String? libOverride;

  static DynamicLibrary _lib() {
    if (libOverride != null) return DynamicLibrary.open(libOverride!);
    if (Platform.isAndroid || Platform.isLinux) {
      return DynamicLibrary.open('libsparkbox_ffi.so');
    }
    if (Platform.isWindows) return DynamicLibrary.open('sparkbox_ffi.dll');
    if (Platform.isMacOS) return DynamicLibrary.open('libsparkbox_ffi.dylib');
    throw UnsupportedError('SparkBoxCore: unsupported platform');
  }

  // ---------------------------------------------------------------- plumbing

  static Map<String, dynamic> _unwrap(Object? env) {
    final map = env as Map<String, dynamic>;
    if (map['ok'] != true) {
      throw SparkBoxCoreException(map['error']?.toString() ?? 'unknown error');
    }
    return map['data'] as Map<String, dynamic>;
  }

  static List<dynamic> _unwrapList(Object? env) {
    final map = env as Map<String, dynamic>;
    if (map['ok'] != true) {
      throw SparkBoxCoreException(map['error']?.toString() ?? 'unknown error');
    }
    return map['data'] as List<dynamic>;
  }

  /// Runs [f] on a fresh isolate, opening the library there (the C functions
  /// block on network I/O and must not run on the main isolate).
  static Future<T> _run<T>(T Function() f) => Isolate.run(f);

  static void _free(DynamicLibrary lib, Pointer<Utf8> ptr) {
    lib
        .lookupFunction<
            Void Function(Pointer<Utf8>),
            void Function(Pointer<Utf8>)>('sparkbox_free_string')(ptr);
  }

  // ------------------------------------------------------------------- calls

  /// Core version + adult-filter state.
  static Future<Map<String, dynamic>> version() => _run(() {
        final lib = _lib();
        final f = lib.lookupFunction<
            Pointer<Utf8> Function(),
            Pointer<Utf8> Function()>('sparkbox_core_version');
        final ptr = f();
        final env = jsonDecode(ptr.toDartString()) as Map<String, dynamic>;
        _free(lib, ptr);
        return env['data'] as Map<String, dynamic>;
      });

  /// Toggle the adult-content filter (default ON).
  static Future<void> setBlockAdult(bool blocked) => _run(() {
        final lib = _lib();
        final f = lib.lookupFunction<
            Pointer<Utf8> Function(Int32),
            Pointer<Utf8> Function(int)>('sparkbox_set_block_adult');
        final ptr = f(blocked ? 1 : 0);
        final env = jsonDecode(ptr.toDartString()) as Map<String, dynamic>;
        _free(lib, ptr);
        if (env['ok'] != true) {
          throw SparkBoxCoreException(env['error']?.toString() ?? 'unknown');
        }
      });

  /// Home feed → `List<Map<String, dynamic>>` of HomeSection maps
  /// (keys: title, items, ranked, isBanner).
  static Future<List<Map<String, dynamic>>> home() => _run(() {
        final lib = _lib();
        final f = lib.lookupFunction<
            Pointer<Utf8> Function(),
            Pointer<Utf8> Function()>('sparkbox_home');
        final ptr = f();
        final list = _unwrapList(jsonDecode(ptr.toDartString()));
        _free(lib, ptr);
        return list.cast<Map<String, dynamic>>();
      });

  /// Keyword suggestions.
  static Future<List<String>> suggest(String query) => _run(() {
        final lib = _lib();
        final f = lib.lookupFunction<
            Pointer<Utf8> Function(Pointer<Utf8>),
            Pointer<Utf8> Function(Pointer<Utf8>)>('sparkbox_suggest');
        final q = query.toNativeUtf8();
        final ptr = f(q);
        malloc.free(q);
        final list = _unwrapList(jsonDecode(ptr.toDartString()));
        _free(lib, ptr);
        return list.cast<String>();
      });

  /// Rich search → `{items: List<Map>, hasMore: bool}`.
  static Future<Map<String, dynamic>> search(String query,
      {int page = 1, int perPage = 20}) {
    return _run(() {
      final lib = _lib();
      final f = lib.lookupFunction<
          Pointer<Utf8> Function(Pointer<Utf8>, Int32, Int32),
          Pointer<Utf8> Function(
              Pointer<Utf8>, int, int)>('sparkbox_search');
      final q = query.toNativeUtf8();
      final ptr = f(q, page, perPage);
      malloc.free(q);
      final data = _unwrap(jsonDecode(ptr.toDartString()));
      _free(lib, ptr);
      return data;
    });
  }

  /// Full detail for a MediaItem map →
  /// `{item: Map, dubs: List<Map>, cast: List<Map>}`.
  static Future<Map<String, dynamic>> detailRich(Map<String, dynamic> item) {
    return _run(() {
      final lib = _lib();
      final f = lib.lookupFunction<
          Pointer<Utf8> Function(Pointer<Utf8>),
          Pointer<Utf8> Function(Pointer<Utf8>)>('sparkbox_detail_rich');
      final arg = jsonEncode(item).toNativeUtf8();
      final ptr = f(arg);
      malloc.free(arg);
      final data = _unwrap(jsonDecode(ptr.toDartString()));
      _free(lib, ptr);
      return data;
    });
  }

  /// Play-flow Detail for a MediaItem map (feed into [streams]).
  static Future<Map<String, dynamic>> playDetail(Map<String, dynamic> item) {
    return _run(() {
      final lib = _lib();
      final f = lib.lookupFunction<
          Pointer<Utf8> Function(Pointer<Utf8>),
          Pointer<Utf8> Function(Pointer<Utf8>)>('sparkbox_play_detail');
      final arg = jsonEncode(item).toNativeUtf8();
      final ptr = f(arg);
      malloc.free(arg);
      final data = _unwrap(jsonDecode(ptr.toDartString()));
      _free(lib, ptr);
      return data;
    });
  }

  /// Seasons of a series → List of `{se, maxEp, episodes}` maps.
  static Future<List<Map<String, dynamic>>> seasons(String subjectId) {
    return _run(() {
      final lib = _lib();
      final f = lib.lookupFunction<
          Pointer<Utf8> Function(Pointer<Utf8>),
          Pointer<Utf8> Function(Pointer<Utf8>)>('sparkbox_seasons');
      final arg = subjectId.toNativeUtf8();
      final ptr = f(arg);
      malloc.free(arg);
      final list = _unwrapList(jsonDecode(ptr.toDartString()));
      _free(lib, ptr);
      return list.cast<Map<String, dynamic>>();
    });
  }

  /// Streams for a Detail map → `{links: List<Map>, subtitles: List<Map>}`.
  /// Each link: `{label, quality, url, kind, headers: Map<String, String>}`.
  static Future<Map<String, dynamic>> streams(Map<String, dynamic> detail,
      int se, int ep, bool isSeries) {
    return _run(() {
      final lib = _lib();
      final f = lib.lookupFunction<
          Pointer<Utf8> Function(Pointer<Utf8>, Int32, Int32, Int32),
          Pointer<Utf8> Function(
              Pointer<Utf8>, int, int, int)>('sparkbox_streams');
      final arg = jsonEncode(detail).toNativeUtf8();
      final ptr = f(arg, se, ep, isSeries ? 1 : 0);
      malloc.free(arg);
      final data = _unwrap(jsonDecode(ptr.toDartString()));
      _free(lib, ptr);
      return data;
    });
  }

  /// `"MOVIE" | "SERIES" | "VIDEO" | "MEDIA"` for a subjectType int.
  static Future<String> typeLabel(int subjectType) => _run(() {
        final lib = _lib();
        final f = lib.lookupFunction<
            Pointer<Utf8> Function(Int32),
            Pointer<Utf8> Function(int)>('sparkbox_type_label');
        final ptr = f(subjectType);
        final data = _unwrap(jsonDecode(ptr.toDartString()));
        _free(lib, ptr);
        return data['label'] as String;
      });
}
