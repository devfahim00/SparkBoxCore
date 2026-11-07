# SparkBoxCore

**SparkBox-এর core** — MovieBox API-র সবকিছু (request signing, endpoints, parsing, stream + subtitle resolution, adult filter) Rust-এ আলাদা করে লেখা library।

The Flutter app ([RMB](https://github.com/devfahim00/RMB) · `flutter` branch) talks to MovieBox **only through this core**. The goal:

> যদি কখনো MovieBox-এর API, host, header বা অন্য কিছু বদলায় — **শুধু এই core repo আপডেট করলেই চলবে।** অ্যাপের Flutter code, UI, models — কিছুই ছোঁয়া লাগবে না।

Extracted 1:1 from the Dart client (`lib/api/moviebox_api.dart` + `lib/models.dart` + `lib/services/content_filter.dart`, originally from the CloudStream `MovieBoxProviderIN` plugin). JSON shapes are **byte-compatible** with the app's Dart models — `MediaItem.toMap()` in Dart decodes into the Rust `MediaItem` and back, unchanged.

---

## Layout

```
sparkbox-core/    the library crate (pure Rust, no UI deps)
  src/signing.rs        HMAC-MD5 request signing + canonicalization
  src/config.rs         endpoints, user agents, signing key — overridable at runtime
  src/mobile.rs         mobile BFF: guest token, home, search, seasons, detail
  src/h5.rs             web (h5) API: home, trending, suggest, search, detail, play, subtitles
  src/parse.rs          tolerant upstream-JSON → model parsing
  src/content_filter.rs adult-content heuristic (block-adult switch)
  src/client.rs         MovieBoxClient facade
  tests/core_tests.rs   signing regression vectors (computed independently)
  examples/smoke.rs     live API smoke test (home → search → detail → streams)
sparkbox-ffi/     C-ABI bindings (cdylib/staticlib) — JSON in, JSON out
bindings/         sparkbox_core.h (C header) + sparkbox_core.dart (Flutter helper)
```

## Quick start (Rust)

```bash
cargo test --workspace                       # 32 tests incl. signing vectors
cargo run -p sparkbox-core --example smoke   # live: home → search → streams
cargo build --release -p sparkbox-ffi        # .so / .a for app integration
```

```rust
use sparkbox_core::MovieBoxClient;

let client = MovieBoxClient::new();          // block-adult defaults ON
let home = client.home().await?;             // Vec<HomeSection>
let hits = client.search_rich("avengers", 1, 20).await?;
for item in &hits.items {
    let d = client.play_detail(item).await?;
    let s = client.streams(&d, 1, 1, item.is_series).await?;
    // s.links: Vec<StreamLink> (url + kind + required player headers)
    // s.subtitles: Vec<SubtitleLink>
}
```

## Using from the Flutter app (Dart FFI)

1. Get the native library:
   - CI builds it for you: **Actions → latest run → artifacts** (`sparkbox-ffi-android-jniLibs` / `sparkbox-ffi-linux-x86_64`), or
   - build locally: `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 -o <app>/android/app/src/main/jniLibs build --release -p sparkbox-ffi`
2. Copy `bindings/sparkbox_core.dart` into `lib/` (needs `flutter pub add ffi`).
3. Call it — keys match the app's existing models:

```dart
final sections = await SparkBoxCore.home();                 // [{title, items, ...}]
final media = MediaItem.fromMap(sections.first['items'][0]); // app model, unchanged

final detail = await SparkBoxCore.playDetail(media.toMap());
final streams = await SparkBoxCore.streams(detail, se, ep, media.isSeries);
// streams['links'][0] = {label, quality, url, kind, headers} — feed to media_kit
```

All FFI functions are blocking; the Dart helper already moves them to a background isolate.

## When MovieBox changes something

| What changed | Where to fix | App change needed |
| --- | --- | --- |
| Mobile/web host moved | `Config` (`mobileBase`, `h5Base`, `tokenUrl`) — can even be loaded as JSON at runtime | none |
| New app version / UA blocked | `Config` (`uaV3`, `uaV4`, `clientInfoV3/4` overrides) | none |
| Signing scheme changed | `sparkbox-core/src/signing.rs` (+ re-derive the vectors in `tests/core_tests.rs`) | none |
| Response shape changed | `sparkbox-core/src/parse.rs` / `mobile.rs` / `h5.rs` | none |
| New endpoint / feature | core only — bump version, rebuild, swap the `.so` | none (new features: bindings only) |

The FFI/JSON contract (`bindings/sparkbox_core.h`) is the stable boundary: the app compiled against it keeps working across core updates.

## FFI contract

Every function returns `{"ok":true,"data":…}` or `{"ok":false,"error":"…"}`; free returned strings with `sparkbox_free_string`. Full surface: `sparkbox_core_version`, `sparkbox_set_block_adult`, `sparkbox_home`, `sparkbox_suggest`, `sparkbox_search`, `sparkbox_detail_rich`, `sparkbox_play_detail`, `sparkbox_seasons`, `sparkbox_streams`, `sparkbox_type_label`, `sparkbox_stream_kinds` — see `bindings/sparkbox_core.h`.

## Notes

- **Unofficial API.** Endpoints, header formats and signing may change server-side and break this; the whole point of this repo is that the fix then lands *here* and nowhere else. Use only for content you are allowed to access.
- Transport timeouts (10 s connect / 30 s total) were added — the Dart client could hang forever on dead endpoints.
- The mobile BFF is region-locked; `home()`/search automatically fall back to the global web (h5) API, exactly like the Flutter app.
- Provenance: extracted from RMB `flutter` @ `1d675b8`.

## বাংলায় সংক্ষেপে

- অ্যাপের `MovieBoxApi` + models + content filter — সব Dart থেকে Rust-এ পোর্ট করা হয়েছে, behavior একই (bug-for-bug, যাতে একই কনটেন্ট দেখায়)।
- Flutter অ্যাপ এখন `bindings/sparkbox_core.dart` দিয়ে core কল করবে; JSON key গুলো `MediaItem.toMap()`-এর মতোই, তাই অ্যাপের model code অপরিবর্তিত থাকবে।
- API বদলালে শুধু এই repo-তে fix করে `.so` আবার build করলেই হবে — `Config` দিয়ে host/UA তো runtime-এও বদলানো যায়, code ছোঁয়ারও দরকার নেই।
- Signing-এর regression vector গুলো `tests/core_tests.rs`-এ আছে — কিছু ভাঙলে আগে ওগুলো দেখুন।
