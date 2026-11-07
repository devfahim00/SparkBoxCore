# SparkBoxCore

[![CI](https://github.com/devfahim00/SparkBoxCore/actions/workflows/ci.yml/badge.svg)](https://github.com/devfahim00/SparkBoxCore/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Rust 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange)

**SparkBoxCore** is the standalone, protocol-level core of
[SparkBox](https://github.com/devfahim00/SparkBox) — everything the app needs to
talk to the MovieBox service and rewritten in
Rust:

- **request signing** (HMAC-MD5 over a canonical string, client tokens),
- **endpoint and user-agent configuration**, overridable at runtime,
- **home feed, search, detail, seasons** across the mobile and web APIs,
- **stream + subtitle resolution** across every dub variant,
- the **adult-content filter**.

The app — its UI, state and models — never talks to MovieBox directly. It calls
this core through a stable JSON-over-FFI boundary.

> **The problem this repo solves:** when MovieBox changes its API, hosts,
> headers or signing scheme, the fix lands *here* — in one small, heavily
> tested library — and the app itself is untouched. Update the core, swap the
> native library, ship.

The port is behavior-identical to the original Dart client (extracted at RMB
`flutter` @ `1d675b8`, originally from the CloudStream `MovieBoxProviderIN`
plugin): same requests, same parsing, same fallbacks, same quirks. JSON shapes
are **byte-compatible** with the app's Dart models — a `MediaItem.toMap()`
produced in Dart decodes into the Rust `MediaItem` and back, unchanged.

---

## Contents

- [Repository layout](#repository-layout)
- [Quick start (Rust)](#quick-start-rust)
- [Using from Flutter / Dart](#using-from-flutter--dart)
- [Using from C or any FFI language](#using-from-c-or-any-ffi-language)
- [Configuration](#configuration)
- [When MovieBox changes something](#when-moviebox-changes-something)
- [Documentation](#documentation)
- [Error handling](#error-handling)
- [Testing](#testing)
- [Versioning and compatibility](#versioning-and-compatibility)
- [Legal notice](#legal-notice)
- [License](#license)

## Repository layout

```
sparkbox-core/          The library crate (pure Rust, no UI dependencies)
  src/lib.rs              Crate root and re-exports
  src/client.rs           MovieBoxClient — the public facade
  src/config.rs           Endpoints, user agents, signing key (runtime-overridable)
  src/signing.rs          HMAC-MD5 request signing and canonicalization
  src/mobile.rs           Mobile BFF: guest token, home, search, seasons, detail
  src/h5.rs               Web (h5) API: home, trending, suggest, search, play, subtitles
  src/parse.rs            Tolerant upstream-JSON → model parsing
  src/content_filter.rs   Adult-content heuristic
  src/models.rs           Data models (byte-compatible with the Dart models)
  tests/core_tests.rs     Independently derived signing regression vectors
  examples/smoke.rs       Live API smoke test (home → search → detail → streams)

sparkbox-ffi/           C-ABI bindings (cdylib + staticlib), JSON in / JSON out
bindings/               sparkbox_core.h (C header) + sparkbox_core.dart (Flutter helper)
.github/workflows/      CI: fmt, clippy, tests, desktop + Android FFI builds
docs/                   Full documentation suite (see below)
```

## Quick start (Rust)

Requires Rust 1.80 or newer.

```bash
git clone https://github.com/devfahim00/SparkBoxCore.git
cd SparkBoxCore

cargo test --workspace                        # 32 tests, incl. signing vectors
cargo run -p sparkbox-core --example smoke    # live: home → search → streams
cargo build --release -p sparkbox-ffi         # native library for app integration
```

As a dependency in another crate:

```toml
[dependencies]
sparkbox-core = { git = "https://github.com/devfahim00/SparkBoxCore.git", tag = "v0.1.0" }
```

Every network call is async (works with any Tokio-compatible runtime):

```rust
use sparkbox_core::MovieBoxClient;

#[tokio::main]
async fn main() -> sparkbox_core::Result<()> {
    let client = MovieBoxClient::new();          // adult filter defaults ON

    // Home feed — mobile API first, global web API as fallback.
    for section in client.home().await? {
        println!("== {} ({} items)", section.title, section.items.len());
    }

    // Search with pagination.
    let hits = client.search_rich("avengers", 1, 20).await?;
    println!("{} hits, more pages: {}", hits.items.len(), hits.has_more);

    // Resolve playable streams for the first hit.
    if let Some(item) = hits.items.first() {
        let detail = client.play_detail(item).await?;
        let streams = client.streams(&detail, 1, 1, item.is_series).await?;
        for link in &streams.links {
            // link.url + link.headers are everything the player needs
            println!("{} [{}] {}", link.label, link.kind.as_str(), link.url);
        }
        for sub in &streams.subtitles {
            println!("subtitle: {} {}", sub.label, sub.url);
        }
    }
    Ok(())
}
```

A series typically goes `seasons()` → pick season/episode → `streams()`:

```rust
let seasons = client.seasons(&item.id).await?;          // Vec<Season>
let s = &seasons[0];                                     // se, max_ep, episodes
let streams = client.streams(&detail, s.se, s.episodes[0], true).await?;
```

Full API reference: [docs/API_REFERENCE.md](docs/API_REFERENCE.md).

## Using from Flutter / Dart

The repo ships ready-made Dart bindings (`bindings/sparkbox_core.dart`) that
wrap the native library and move every call onto a background isolate.

1. **Get the native library**
   - from CI: *Actions → latest run → artifacts*
     (`sparkbox-ffi-android-jniLibs`, `sparkbox-ffi-linux-x86_64`), or
   - build locally with [cargo-ndk](https://github.com/bbqsrc/cargo-ndk):

     ```bash
     cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 \
       -o <app>/android/app/src/main/jniLibs build --release -p sparkbox-ffi
     ```

2. **Add the Dart helper** — copy `bindings/sparkbox_core.dart` into the app's
   `lib/` and `flutter pub add ffi`.

3. **Call it** — the JSON keys match the app's existing models exactly:

```dart
final sections = await SparkBoxCore.home();                  // [{title, items, ...}]
final media = MediaItem.fromMap(sections.first['items'][0]); // app model, unchanged

final detail = await SparkBoxCore.playDetail(media.toMap());
final streams = await SparkBoxCore.streams(detail, se, ep, media.isSeries);
// streams['links'][0] = {label, quality, url, kind, headers} — feed to media_kit
```

Step-by-step guide for Flutter, C and other languages:
[docs/INTEGRATION.md](docs/INTEGRATION.md).

## Using from C or any FFI language

```c
#include "sparkbox_core.h"

char *res = sparkbox_search("avengers", 1, 20);
// res = {"ok":true,"data":{"items":[...],"hasMore":false}}
sparkbox_free_string(res);
```

Every function returns one JSON string with a uniform envelope
(`{"ok":true,"data":…}` / `{"ok":false,"error":"…"}`) and frees with
`sparkbox_free_string`. The complete surface — 11 functions — is documented in
[docs/API_REFERENCE.md](docs/API_REFERENCE.md#ffi-api) and declared in
[`bindings/sparkbox_core.h`](bindings/sparkbox_core.h).

## Configuration

Everything the upstream service controls — hosts, user agents, the signing
key, client-info blobs — lives in one struct, [`Config`](docs/API_REFERENCE.md#config).
Change it in code or load it as JSON at runtime (e.g. fetched from your own
server) so host migrations don't even require a rebuild:

```rust
use sparkbox_core::{Config, MovieBoxClient};

let cfg = Config::from_json(r#"{
    "mobileBase": "https://new-mobile-host.example",
    "h5Base":     "https://new-web-host.example"
}"#)?;
let client = MovieBoxClient::with_config(cfg);   // everything else stays default
```

A typical deployment fetches the config blob from its own server at startup and
constructs the client with it, so a host migration needs no app update at all.
Every field, its default and the exact JSON key names are listed in
[docs/API_REFERENCE.md](docs/API_REFERENCE.md#config).

## When MovieBox changes something

| What changed upstream | Where the fix lands | App change needed |
| --- | --- | --- |
| Mobile / web host moved | `Config` (`mobileBase`, `h5Base`, `tokenUrl`) — even loadable as runtime JSON | none |
| App version or UA blocked | `Config` (`uaV3`, `uaV4`, `clientInfoV3/4` overrides) | none |
| Signing scheme changed | `sparkbox-core/src/signing.rs` + re-derive the vectors in `tests/core_tests.rs` | none |
| Response shape changed | `sparkbox-core/src/parse.rs`, `mobile.rs`, `h5.rs` | none |
| New endpoint / feature | core only — bump version, rebuild, swap the library | none (new features: bindings only) |

The FFI/JSON contract in `bindings/sparkbox_core.h` is the stable boundary:
an app compiled against it keeps working across core updates. The protocol
itself — every endpoint, header and fallback chain — is documented in
[docs/MOVIEBOX_PROTOCOL.md](docs/MOVIEBOX_PROTOCOL.md).

## Documentation

| Document | Contents |
| --- | --- |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Design, module map, data flows, Dart → Rust provenance, deliberate deviations |
| [docs/MOVIEBOX_PROTOCOL.md](docs/MOVIEBOX_PROTOCOL.md) | The full MovieBox wire protocol: endpoints, signing, tokens, stream resolution, maintenance runbook |
| [docs/API_REFERENCE.md](docs/API_REFERENCE.md) | Complete reference for every public type, function and FFI symbol |
| [docs/INTEGRATION.md](docs/INTEGRATION.md) | Integrating the core into Flutter, C, and other Rust projects |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Development setup, code style, review checklist |
| [CHANGELOG.md](CHANGELOG.md) | Release history |

## Error handling

All fallible functions return `Result<T, CoreError>`:

- `CoreError::Api(String)` — protocol-level failures, carrying the same
  messages the Dart client threw (`"h5 HTTP 400: …"`,
  `"No streams (se=1 ep=1) | …"`), so user-visible error text stays familiar;
- `CoreError::Http(reqwest::Error)` — transport failures (DNS, connect, TLS,
  timeouts);
- `CoreError::Json(serde_json::Error)` — a response body was not valid JSON.

Over the FFI boundary every error becomes `{"ok":false,"error":"…"}`.

## Testing

```bash
cargo test --workspace       # 32 tests: 23 unit + 7 integration + 2 doc-tests
cargo run -p sparkbox-core --example smoke   # optional live smoke test
```

The integration suite in `tests/core_tests.rs` contains request-signing
regression vectors that were **derived independently** (Python `hashlib` /
`hmac` / `base64`, not with this crate). If the server ever rejects signed
requests after a core change, re-derive those vectors first — the
canonicalization, not the transport, is what usually drifts.

CI runs format checks, clippy with `-D warnings`, the full test suite, and
builds the FFI library for Linux and all four Android ABIs on every push.

## Versioning and compatibility

- Semantic versioning. While `0.x`, breaking changes bump the minor version.
- The **FFI/JSON contract** (`bindings/sparkbox_core.h`, the envelope and
  payload shapes) is treated as more stable than the Rust API: it only gains
  fields or functions, never renames/removes them within a major version.
- JSON payloads are camelCase and byte-compatible with the Flutter app's Dart
  models; new fields appear with defaults so old payloads keep parsing.

## Legal notice

SparkBoxCore is an **unofficial** client for a third-party service. Endpoints,
header formats and signing are not public contracts and may change or break
server-side at any time — which is precisely why this code is isolated here.
Use it only to access content you are legally allowed to access. The authors
are not affiliated with the upstream service.

## License

[MIT](LICENSE) © devfahim00
