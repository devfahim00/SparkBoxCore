# Integration Guide

How to consume SparkBoxCore from the SparkBox Flutter app, from C, or from
another Rust project — and how to keep the integration stable across core
updates.

- [The integration model](#the-integration-model)
- [Building the native library](#building-the-native-library)
- [Flutter / Dart integration](#flutter--dart-integration)
- [C integration](#c-integration)
- [Using sparkbox-core from Rust](#using-sparkbox-core-from-rust)
- [Runtime configuration](#runtime-configuration)
- [Threading and isolates](#threading-and-isolates)
- [Updating the core in a shipped app](#updating-the-core-in-a-shipped-app)

## The integration model

```
your app  ──(FFI: JSON strings, C ABI)──►  sparkbox-ffi  ──►  sparkbox-core  ──HTTPS──►  MovieBox
```

- The app never links `sparkbox-core` directly; it talks to `sparkbox-ffi`.
- The only contract is the JSON envelope and payload shapes documented in
  [API_REFERENCE.md § FFI](API_REFERENCE.md#ffi-api) — payload keys are
  identical to the app's existing Dart models, so `MediaItem.fromMap()`
  etc. keep working unchanged.
- Everything volatile (endpoints, signing, parsing) stays behind that
  boundary in the core.

## Building the native library

Prerequisites: Rust 1.80+ ([rustup](https://rustup.rs)), plus the Android
NDK for Android targets.

```bash
git clone https://github.com/devfahim00/SparkBoxCore.git
cd SparkBoxCore
```

| Target | Command | Output |
| --- | --- | --- |
| Linux desktop | `cargo build --release -p sparkbox-ffi` | `target/release/libsparkbox_ffi.so` (+ `.a`) |
| Windows desktop | `cargo build --release -p sparkbox-ffi` (from Windows/MSVC) | `target/release/sparkbox_ffi.dll` |
| macOS desktop | `cargo build --release -p sparkbox-ffi` | `target/release/libsparkbox_ffi.dylib` |
| Android (all ABIs) | `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 -o ./jniLibs build --release -p sparkbox-ffi` | `jniLibs/<abi>/libsparkbox_ffi.so` |
| iOS (static) | `cargo build --release -p sparkbox-ffi --target aarch64-apple-ios` | `libsparkbox_ffi.a` |

[cargo-ndk](https://github.com/bbqsrc/cargo-ndk) handles the Android NDK
glue: `cargo install cargo-ndk`.

**No builds needed if you use CI artifacts**: every push runs
[CI](https://github.com/devfahim00/SparkBoxCore/actions/workflows/ci.yml)
which publishes `sparkbox-ffi-linux-x86_64` (`.so` + `.a`) and
`sparkbox-ffi-android-jniLibs` (all four ABIs) as workflow artifacts —
*Actions → latest successful run → Artifacts*.

## Flutter / Dart integration

The repo ships a drop-in Dart helper: [`bindings/sparkbox_core.dart`](../bindings/sparkbox_core.dart).

**Step 1 — package the native library**

- Android: place each `libsparkbox_ffi.so` under
  `android/app/src/main/jniLibs/<abi>/` (the `cargo ndk -o` flag above writes
  them there directly).
- Linux: `libsparkbox_ffi.so` next to the executable.
- Windows: `sparkbox_ffi.dll` next to the executable.
- Custom location: set `SparkBoxCore.libOverride = 'path/to/libsparkbox_ffi.so';`
  before the first call.

**Step 2 — add the helper and its dependency**

```bash
cp bindings/sparkbox_core.dart  <app>/lib/
cd <app> && flutter pub add ffi
```

**Step 3 — call it**

```dart
import 'sparkbox_core.dart';

// All methods are static Futures; each call runs on its own background
// isolate (the C functions block) and throws SparkBoxCoreException on
// {"ok":false}.

final sections = await SparkBoxCore.home();            // List<Map> — HomeSection shape
final words    = await SparkBoxCore.suggest('aven');   // List<String>
final result   = await SparkBoxCore.search('avengers', 1, 20);
//   result['items'] → List<Map> (MediaItem shape) · result['hasMore'] → bool

final rich     = await SparkBoxCore.detailRich(item);  // {'item':…, 'dubs':[…], 'cast':[…]}
final detail   = await SparkBoxCore.playDetail(item);  // Detail map — keep for playback
final seasons  = await SparkBoxCore.seasons(subjectId);// List<Map> — {se, maxEp, episodes}

final streams  = await SparkBoxCore.streams(detail, se, ep, isSeries);
//   streams['links'][0]     → {label, quality, url, kind, headers}
//   streams['subtitles'][i] → {label, url}

await SparkBoxCore.setBlockAdult(true);                // adult filter (default ON)
final info     = await SparkBoxCore.version();         // {version, blockAdult}
final label    = await SparkBoxCore.typeLabel(2);      // "SERIES"
```

Because payload keys match the app's Dart models 1:1, existing model classes
keep working:

```dart
final media = MediaItem.fromMap(sections.first['items'].first);
final detail = await SparkBoxCore.playDetail(media.toMap());
```

**Playing a link** — give the URL *and* the required headers to your player:

```dart
final link = streams['links'].first;
// media_kit example:
// await player.open(Media(link['url'], httpHeaders: Map<String,String>.from(link['headers'])));
```

The headers contain the `Referer`/`Origin`/`User-Agent` (and, when present,
the signed token header) the CDN requires — without them the CDN returns 403.

## C integration

```c
#include <stdio.h>
#include "sparkbox_core.h"

int main(void) {
    char *raw = sparkbox_search("avengers", 1, 20);
    printf("%s\n", raw);            /* {"ok":true,"data":{"items":[…],"hasMore":false}} */
    sparkbox_free_string(raw);

    char *detail = sparkbox_play_detail(
        "{\"id\":\"70612\",\"title\":\"Test\",\"isSeries\":false,"
        "\"detailPath\":\"movie/test-70612\",\"cover\":\"\",\"backdrop\":\"\","
        "\"rating\":\"\",\"year\":\"\",\"genre\":\"\",\"country\":\"\","
        "\"description\":\"\",\"durationMin\":0,\"subjectType\":1}");
    printf("%s\n", detail);
    sparkbox_free_string(detail);
    return 0;
}
```

Link against `libsparkbox_ffi.so` (or the `.a`) and include
`bindings/sparkbox_core.h`. The same surface works from Java (JNI), C#,
Python (ctypes) — any language that can call a C function and parse JSON.

## Using sparkbox-core from Rust

Skip the FFI layer entirely and depend on the library:

```toml
[dependencies]
sparkbox-core = { git = "https://github.com/devfahim00/SparkBoxCore.git", tag = "v0.1.0" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust
use sparkbox_core::MovieBoxClient;

#[tokio::main]
async fn main() -> sparkbox_core::Result<()> {
    let client = MovieBoxClient::new();
    for section in client.home().await? {
        println!("{} — {} items", section.title, section.items.len());
    }
    Ok(())
}
```

The full Rust API is documented in
[API_REFERENCE.md § Core API](API_REFERENCE.md#core-api-rust).

## Runtime configuration

To survive host migrations without an app release, fetch a config blob from
your own server at startup and build the client from it (Rust shown; over FFI
the same is achieved by adding a config-aware constructor when you need it):

```rust
let json = my_server.fetch("sparkbox-config.json").await?;   // your infra
let cfg  = sparkbox_core::Config::from_json(&json)?;
let client = MovieBoxClient::with_config(cfg);
```

Only override what changed — missing fields keep the compiled defaults:

```json
{ "mobileBase": "https://api3.aoneroom.com", "h5Base": "https://movie-box.co" }
```

Field/key reference: [API_REFERENCE.md § Config](API_REFERENCE.md#config).

## Threading and isolates

- The C functions block for the duration of the network call (up to the
  30 s total timeout). **Never** call them on the UI thread / main isolate.
- The bundled Dart helper runs every call inside `Isolate.run`, so Flutter
  apps get this right by default.
- Internally the core is safe for concurrent use: tokens are behind tokio
  mutexes, the filter switch is an atomic. One client instance can serve many
  parallel calls.
- In Rust, any Tokio-compatible runtime works (the library itself is
  runtime-agnostic).

## Updating the core in a shipped app

1. Fix/extend the core (see
   [MOVIEBOX_PROTOCOL.md § Maintenance runbook](MOVIEBOX_PROTOCOL.md#maintenance-runbook)
   for the symptom→fix table).
2. `cargo test --workspace` — the signing vectors must stay green.
3. Bump the version (workspace `Cargo.toml`) and update `CHANGELOG.md`.
4. CI builds and publishes the new artifacts; copy them into the app (or let
   your app CI fetch them) and rebuild the app.
5. App code changes needed: **none**, unless you introduced a *new* FFI
   function — then only the binding line for it.
