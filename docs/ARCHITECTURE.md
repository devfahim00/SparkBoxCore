# Architecture

This document explains how SparkBoxCore is built, why it is built this way, and
how each piece maps back to the Flutter app it was extracted from. It is aimed
at contributors and at anyone who needs to reason about the core before
modifying it.

- [Goals and non-goals](#goals-and-non-goals)
- [System context](#system-context)
- [Workspace structure](#workspace-structure)
- [Module responsibilities](#module-responsibilities)
- [Provenance: Dart → Rust mapping](#provenance-dart--rust-mapping)
- [Runtime state model](#runtime-state-model)
- [Data flows](#data-flows)
- [Error strategy](#error-strategy)
- [Deliberate deviations from the Dart client](#deliberate-deviations-from-the-dart-client)
- [Security considerations](#security-considerations)

## Goals and non-goals

**Goals**

1. **Isolate volatility.** Everything that can break when the upstream service
   changes — endpoints, headers, signing, parsing — lives in this one library.
2. **Behavior parity.** Same requests, same parsing, same fallbacks and same
   quirks as the original Dart client, so the app shows the same content
   before and after the migration. Where the Dart code had quirks, they were
   ported *bug-for-bug* (see [deviations](#deliberate-deviations-from-the-dart-client)
   for the short list of intentional differences).
3. **Stable boundary.** Apps consume the core through a C ABI with a JSON
   envelope whose payload keys match the app's existing Dart models
   byte-for-byte. The app's model code does not change.
4. **Cheap fixes.** Host or user-agent changes should be fixable with a
   runtime `Config` override — no recompile, no app update.

**Non-goals**

- No UI, no persistence, no player logic. The app owns those.
- No account/login support (the upstream app used guest tokens; the SparkBox
  app never logs in).
- No caching layer beyond the two guest-token caches.
- Not a general MovieBox SDK: only what the SparkBox app actually uses.

## System context

```
┌──────────────────────────────────────────────────────────────────┐
│                        SparkBox app (Flutter)                    │
│   UI · player · downloads · library/settings/playback state      │
│   lib/ui/* · lib/services/* (stores) · lib/updater.dart          │
└───────────────────────────┬──────────────────────────────────────┘
                            │  FFI (C ABI), JSON in / JSON out
                            │  bindings/sparkbox_core.dart
┌───────────────────────────▼──────────────────────────────────────┐
│                     SparkBoxCore  (this repo)                    │
│                                                                  │
│   sparkbox-core (library)          sparkbox-ffi (cdylib)         │
│   ┌──────────────────────────┐     ┌─────────────────────────┐   │
│   │ MovieBoxClient facade    │◄────│ sparkbox_home() …       │   │
│   │ signing · config · parse │     │ JSON envelope + free()  │   │
│   │ mobile.rs · h5.rs        │     └─────────────────────────┘   │
│   │ models · content_filter  │                                   │
│   └──────────────────────────┘                                   │
└──────────────┬─────────────────────────────────┬─────────────────┘
               │ signed HTTPS (HMAC-MD5)         │ plain HTTPS + bearer
┌──────────────▼──────────────┐   ┌──────────────▼──────────────────┐
│   MovieBox mobile BFF       │   │   MovieBox web (h5) API         │
│   api3.aoneroom.com         │   │   movie-box.co                  │
│   apig.inmoviebox.com       │   │   (global fallback, any region) │
│   (region-locked)           │   │                                 │
└─────────────────────────────┘   └─────────────────────────────────┘
```

The two upstream surfaces exist because the mobile BFF is region-locked to the
app's market: when it fails (or is unavailable from the user's network), the
core falls back to the web API, which works from any region and localizes
content by IP. The Flutter app used exactly this dual-path strategy; the core
inherits it.

## Workspace structure

A two-crate Cargo workspace:

| Crate | Type | Role |
| --- | --- | --- |
| `sparkbox-core` | `lib` | The entire protocol implementation. Pure Rust, no UI dependencies. Consumable directly from Rust. |
| `sparkbox-ffi` | `cdylib` + `staticlib` | A thin C-ABI layer exposing the client through JSON strings, so any language with plain FFI (Dart, C, Java, Python…) can drive the core. |

Keeping the FFI layer separate means the library's Rust API can evolve freely
while the C ABI stays frozen — apps only ever see the ABI.

## Module responsibilities

| Module | Responsibility |
| --- | --- |
| `client.rs` | `MovieBoxClient`: constructor, config access, adult-filter switch, and the high-level facade methods (`home`, `search_rich`, `play_detail`, `streams`, …) that delegate to `mobile.rs` / `h5.rs`. |
| `config.rs` | `Config`: every upstream-controlled constant (hosts, user agents, signing key, client-info blobs, token TTL), with `to_json` / `from_json` for runtime overrides. |
| `signing.rs` | The mobile-BFF request signer: `client_token`, `canonical_query`, `canonical`, `signature` (HMAC-MD5), `client_info` blobs, device-id generation. Pure functions, fully covered by regression vectors. |
| `mobile.rs` | Mobile BFF transport: signed header assembly, guest-token bootstrap/caching, home feed, legacy search, seasons, subject detail (with the v4→v3 retry), caption fetch helper. |
| `h5.rs` | Web API transport: h5 token bootstrap/refresh (400/401/441 → force refresh → retry once), home/trending/suggest/search/detail endpoints, the full stream-resolution pipeline (`streams_impl`, `play_once`, `load_captions`). |
| `parse.rs` | Tolerant JSON → model conversion (`s`, `deep_str`, `get_arr`, `parse_subject`, `parse_season`, …), ported from the Dart `_str`/`_obj`/`_arr`/`_deepStr` helpers. |
| `models.rs` | The data model. Every type serializes camelCase, byte-compatible with the Dart `toMap()` shapes. |
| `content_filter.rs` | The adult-content heuristic (regex over genre/title/description), ported bug-for-bug. |

## Provenance: Dart → Rust mapping

The extraction point is RMB `flutter` @ `1d675b8`.

| Dart source (RMB `flutter`) | Rust module | Notes |
| --- | --- | --- |
| `lib/api/moviebox_api.dart` — signing block | `signing.rs` | Constants moved to `Config`. |
| `lib/api/moviebox_api.dart` — `_homeMobile`, `search`, `seasons`, `detail`, `guestToken`, `_mobileGet` | `mobile.rs` | Static methods → client methods; static token fields → client state. |
| `lib/api/moviebox_api.dart` — `_h5Call`, `_homeH5`, `_trending`, `suggest`, `searchRich`, `detailRich`, `_fetchDubs`, `_playOnce`, `_loadCaptions`, `streams` | `h5.rs` | `streams()` returns links *and* subtitles (see [deviations](#deliberate-deviations-from-the-dart-client)). |
| `lib/api/moviebox_api.dart` — `_arr`/`_obj`/`_str`/`_deepStr`/`parseSubject`/`typeLabel` | `parse.rs` | |
| `lib/models.dart` | `models.rs` | Field names and JSON keys identical; Dart tuples `(name, avatarUrl)` became `CastMember`. |
| `lib/services/content_filter.dart` | `content_filter.rs` + `MovieBoxClient::set_block_adult` | The Dart singleton's file persistence stays in the app; the core only holds the on/off state. |
| *(new)* | `config.rs`, `client.rs`, `sparkbox-ffi` | Not in Dart: the runtime-config mechanism, the facade, and the C ABI. |

What was **deliberately left in the app**: `lib/ui/*` (all UI),
`library_store.dart`, `playback_store.dart`, `settings_store.dart`
(platform-file persistence of watch state), `download_manager.dart` (platform
HTTP downloads), `updater.dart` (GitHub-release check + APK install — UI-bound).

## Runtime state model

`MovieBoxClient` is a plain struct — no globals, no `static mut`. Create one
per app (or per isolate/worker) and reuse it.

| State | Type | Purpose |
| --- | --- | --- |
| `http` | `reqwest::Client` | Shared connection pool, 10 s connect / 30 s total timeouts. |
| `config` | `Config` | Endpoint/header material (see `sparkbox-core/src/config.rs`). |
| `device_id` | `String` | Random 16-byte hex, generated per client (per app run in the Dart version). Sent inside `x-client-info`. |
| `block_adult` | `AtomicBool` | Adult-filter switch, default ON. Read at parse time. |
| `guest` | `tokio::sync::Mutex<Option<(String, u64)>>` | Mobile guest token + fetch time; fresh for `Config::guest_token_ttl_ms` (default 1 h). |
| `h5_guest` | `tokio::sync::Mutex<Option<String>>` | Web (h5) guest token; cached until a call returns 400/401/441, then force-refreshed once. |
| `last_note` | `std::sync::Mutex<String>` | Diagnostics of the last `play` attempt, appended to "No streams" errors. |

Token locks are tokio mutexes because refresh happens across `.await` points;
`last_note` is a std mutex (never held across an await).

## Data flows

### Home feed

```
home()
 ├─ home_mobile()  GET {mobile_base}/wefeed-mobile-bff/tab-operating
 │                 signed v3 headers + x-play-mode:2
 │                 sections: BANNER / SUBJECTS_MOVIE / SUBJECTS_TV /
 │                 APPOINTMENT_LIST / CUSTOM        ── fails? ──┐
 └─ home_h5()      GET {h5_base}/wefeed-h5api-bff/home          ▼
 │                 (web fallback, any region)          both failed
 │                                                  → Err("home feed unavailable")
 ├─ trending(20)   GET /wefeed-h5api-bff/subject/trending   (errors ignored)
 └─ [ "Trending Now" (ranked) ] + server sections (non-empty only)
```

### Search

```
search_rich(query, page, per_page)
 ├─ h5:  POST /wefeed-h5api-bff/subject/search      → items + pager.hasMore
 └─ fallback on any h5 error:
        POST {mobile_base}/wefeed-mobile-bff/subject-api/search/v2
        (signed, guest token) → minimal items, has_more = false
```

### Play resolution (the core pipeline)

```
play_detail(item)
 ├─ item.detail_path set → Detail { h5_base = config.h5_base, h5_token = cached }
 └─ else → detail_mobile(id)
        GET subject-api/get  (v4 headers)
        no detailUrl? → retry (v3 + guest token + x-play-mode:2)
        origin/path derived from data.detailUrl:
          origin = scheme://host   ·   path = last URL segment
        path still empty? → detailPath from data → path hint → web lookup by id
        h5_token = x-user token of the last response

streams(detail, se, ep, is_series)
 ├─ variants = [Original] + dubs(detail_path)          (h5 detail endpoint)
 ├─ attempts  = series ? [(se, ep)] : [(0,0), (1,1)]   (movie quirk)
 ├─ for each variant × attempt:
 │     play_once()  GET {h5_base}/wefeed-h5api-bff/subject/play
 │                   → dash[] hls[] streams[] → StreamLink + player headers
 │                   → inline captions/subtitles/subtitleList arrays
 │     first non-empty result wins for the variant
 ├─ all empty → Err("No streams (se=… ep=…) | …diagnostics…")
 ├─ load_captions()  mobile caption endpoints for each (subjectId, streamId)
 └─ sort: "Original" first, then quality desc
```

Every step of these flows, including exact URLs, headers and response shapes,
is specified in [MOVIEBOX_PROTOCOL.md](MOVIEBOX_PROTOCOL.md).

## Error strategy

- One error enum, `CoreError`, with three variants: `Api(String)` (protocol
  failures, human-readable, message-compatible with the Dart client's thrown
  strings), `Http(reqwest::Error)` (transport), `Json(serde_json::Error)`
  (malformed bodies).
- **Fail-soft where the Dart client failed soft**: trending errors during
  `home()`, dub-list errors during `streams()`, and caption errors are
  swallowed; the operation continues with what it has.
- **Fail-hard where the Dart client failed hard**: both home sources dead,
  no stream links at all, detail without data — return `Err` with the familiar
  message text.
- Over FFI, `Display` of the error is placed into `{"ok":false,"error":"…"}`;
  panics are caught at the boundary and never cross the ABI.

## Deliberate deviations from the Dart client

The port is bug-for-bug compatible in behavior. The differences below are
intentional improvements that do not change what the user sees:

| # | Dart | Rust | Why |
| --- | --- | --- | --- |
| 1 | `MovieBoxApi.subtitles` — a public static list filled as a side effect of `streams()`. | `streams()` returns `StreamsResult { links, subtitles }`. | No hidden global state; results can't be read at the wrong time. |
| 2 | Captions were fetched in parallel (`Future.wait`); ordering of subtitle labels depended on completion order. | Captions are fetched sequentially, deterministically. | Same set of subtitles, stable order — reproducible and testable. |
| 3 | `http.Client` with no timeouts. | 10 s connect / 30 s total. | A dead endpoint previously hung the app forever. |
| 4 | Hosts, UAs, signing key hard-coded `static const`. | `Config`, overridable at runtime via JSON. | Host migrations without an app update. |
| 5 | `deviceId` regenerated per app *run*. | Generated per *client* (same format, 16-byte hex). | A library must not rely on process lifetime; the server only sees a random id either way. |
| 6 | `_deepStr` would return a Dart `Map.toString()` blob when the field held an object. | Object/array values are skipped during the nested search. | The Dart behavior could never produce a usable value; it only mattered for malformed responses. |
| 7 | Search fallback produced `MediaItem` with only 4 fields set. | Same, via `MediaItem::minimal` + `detail_path`. | Identical shape. |

Everything else — the `18+` regex quirk, the movie `0/0 → 1/1` attempt order,
the `"Original"` label prefix rule, the port-dropping origin derivation, the
100 KB body-hash cap — matches the Dart code exactly, and tests pin it.

## Security considerations

- **No `unsafe` in `sparkbox-core`.** The only `unsafe` in the workspace is the
  `CString::from_raw` in `sparkbox_free_string`, required by the C ABI.
- Tokens and the signing key are never logged (the crate performs no logging
  at all) and never serialized into any returned model. `Detail.h5_token` is
  the one place a token is returned — deliberately, because the *player* needs
  it for CDN requests; it is the same token the web player holds in the
  browser.
- The signing key ships in the binary (it is extracted from a public plugin
  and is not a secret against the server, only a compatibility requirement).
- Panics are contained at the FFI boundary (`catch_unwind`) so a core bug
  cannot take down the host app.
