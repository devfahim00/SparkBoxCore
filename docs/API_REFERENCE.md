# API Reference

Complete reference for everything SparkBoxCore exposes publicly: the Rust
library (`sparkbox-core`) and the C-ABI FFI layer (`sparkbox-ffi`).

- [Core API (Rust)](#core-api-rust)
  - [`MovieBoxClient`](#movieboxclient)
  - [`Config`](#config)
  - [Models](#models)
  - [`CoreError`](#coreerror)
  - [`signing` module](#signing-module)
  - [`parse` module](#parse-module)
  - [`content_filter` module](#content_filter-module)
- [FFI API (C ABI)](#ffi-api)
  - [Envelope and conventions](#envelope-and-conventions)
  - [Function index](#function-index)

All types are re-exported at the crate root, so `use sparkbox_core::*` or
specific imports like `use sparkbox_core::{MovieBoxClient, MediaItem}` both
work. The crate constant `sparkbox_core::VERSION` holds the core version
string.

---

## Core API (Rust)

### `MovieBoxClient`

The public facade. Create one per app (or per isolate/worker) and reuse it —
guest tokens are cached internally. Thread-safe: all mutating state is behind
locks/atomics, so `&self` methods can be called from many tasks concurrently.

```rust
use sparkbox_core::MovieBoxClient;

let client = MovieBoxClient::new();
let sections = client.home().await?;   // Result<Vec<HomeSection>, CoreError>
```

#### Constructors and accessors

| Signature | Description |
| --- | --- |
| `MovieBoxClient::new() -> Self` | Default endpoints/headers, adult filter ON. |
| `MovieBoxClient::with_config(config: Config) -> Self` | Custom [`Config`](#config). |
| `Default::default() -> Self` | Same as [`new`](#movieboxclient). |
| `client.config() -> &Config` | The active configuration. |
| `client.device_id() -> &str` | This client's random 16-byte hex device id (sent in `x-client-info`). |
| `client.block_adult() -> bool` | Current adult-filter state (default `true`). |
| `client.set_block_adult(blocked: bool)` | Toggle the adult filter. Applies to everything parsed afterwards. |
| `MovieBoxClient::version() -> &'static str` | Associated: the core version. |
| `MovieBoxClient::type_label(subject_type: i64) -> &'static str` | Associated: `1→"MOVIE"`, `2→"SERIES"`, `6→"VIDEO"`, else `"MEDIA"`. |

#### Network methods (all `async`)

| Method | Returns | Description |
| --- | --- | --- |
| `home()` | `Result<Vec<HomeSection>>` | Home feed. Mobile API first, global web (h5) API as fallback; a "Trending Now" ranked row is prepended when trending resolves. `Err(Api("home feed unavailable"))` only when both sources fail. |
| `suggest(query: &str)` | `Result<Vec<String>>` | As-you-type word suggestions (web suggest endpoint), de-duplicated, order preserved. |
| `search_rich(query: &str, page: i64, per_page: i64)` | `Result<SearchResult>` | Paginated search (web API). Falls back to the legacy mobile search on any web error — in that case items are minimal and `has_more` is `false`. |
| `detail_rich(item: &MediaItem)` | `Result<DetailRich>` | Full detail: merged description/stills, dub variants, up to 15 cast members. Resolves a missing `detail_path` via the mobile API first. |
| `play_detail(item: &MediaItem)` | `Result<Detail>` | The play-flow handle for an item (h5 base + token + path). Feed its result to [`streams`](#streams). |
| `seasons(subject_id: &str)` | `Result<Vec<Season>>` | Seasons of a series, sorted by season number. Empty for movies. |
| `streams(d: &Detail, se: i64, ep: i64, is_series: bool)` | `Result<StreamsResult>` | Resolve playable links + subtitles. See [stream resolution](MOVIEBOX_PROTOCOL.md#stream-resolution-algorithm). Movies try `se/ep = 0/0` then `1/1` regardless of the passed values; series use the given numbers. Links sorted "Original" variant first, then quality desc. Fails with `Api("No streams (se=… ep=…) | …")` when nothing resolves. |

**Recommended play flow**

```rust
let hits = client.search_rich("avengers", 1, 20).await?;
if let Some(item) = hits.items.first() {
    let detail = client.play_detail(item).await?;
    let (se, ep) = if item.is_series {
        let s = &client.seasons(&item.id).await?[0];
        (s.se, s.episodes[0])
    } else {
        (0, 0) // movies: core tries 0/0 then 1/1 anyway
    };
    let streams = client.streams(&detail, se, ep, item.is_series).await?;
    let best = &streams.links[0];         // highest quality of "Original"
    // player: open best.url sending best.headers
}
```

---

### `Config`

Everything the upstream service controls, overridable at runtime. All fields
serialize camelCase; unknown JSON fields are ignored; missing fields keep the
defaults. Derives `Clone`, `PartialEq`, `Serialize`, `Deserialize`.

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mobile_base` | `String` | `https://api3.aoneroom.com` | Mobile BFF origin. |
| `token_url` | `String` | `https://apig.inmoviebox.com/wefeed-mobile-bff/tab/ranking-list?tabId=0&categoryType=4516404531735022304&page=1&perPage=1` | Guest-token bootstrap URL. |
| `h5_base` | `String` | `https://movie-box.co` | Web (h5) API origin. |
| `h5_referer` | `String` | `https://mzfi.me/` | `Referer` sent on web calls. |
| `secret_b64` | `String` | `76iRl07s0xSN9jqmEWAt79EBJZulIQIsV64FZr2O` | Base64 HMAC-MD5 signing key. |
| `ua_v3` / `ua_v4` | `String` | the two Android app UAs | User-Agents for signed calls. |
| `ua_desktop` / `ua_mobile` | `String` | browser UAs | User-Agents for web/play calls. |
| `timezone` | `String` | `Asia/Calcutta` | Timezone in `x-client-info` (affects release dates). |
| `region` | `String` | `IN` | Region in `x-client-info`. |
| `guest_token_ttl_ms` | `u64` | `3_600_000` | Mobile guest-token cache lifetime. |
| `client_info_v3` / `client_info_v4` | `Option<String>` | `None` | Replace the whole `x-client-info` JSON (advanced). |

| Method | Signature | Description |
| --- | --- | --- |
| Default | `Config::default() -> Self` | Compiled-in defaults. |
| serialize | `to_json(&self) -> String` | Pretty camelCase JSON blob. |
| parse | `from_json(json: &str) -> Result<Self>` | Parse a blob; unknown keys ignored, missing keys default. |

Default constants are also public: `DEFAULT_MOBILE_BASE`, `DEFAULT_TOKEN_URL`,
`DEFAULT_H5_BASE`, `DEFAULT_H5_REFERER`, `DEFAULT_SECRET_B64`, `DEFAULT_UA_V3`,
`DEFAULT_UA_V4`, `DEFAULT_UA_DESKTOP`, `DEFAULT_UA_MOBILE`.

---

### Models

All models derive `Debug`, `Clone`, `PartialEq`, `Serialize`, `Deserialize`,
serialize **camelCase** and are byte-compatible with the Flutter app's Dart
models (`MediaItem.toMap()` ↔ `MediaItem`). Missing JSON fields default, so
old payloads keep parsing.

#### `MediaItem`

Rich media item used by home/search/player UIs.

| Field | Type | JSON key | Notes |
| --- | --- | --- | --- |
| `id` | `String` | `id` | Subject id. |
| `title` | `String` | `title` | |
| `is_series` | `bool` | `isSeries` | `subject_type == 2`. |
| `detail_path` | `String` | `detailPath` | Web detail path (play flow needs it). |
| `cover` / `backdrop` | `String` | `cover` / `backdrop` | Image URLs. |
| `rating` | `String` | `rating` | e.g. `"7.4"`. |
| `year` | `String` | `year` | First 4 chars of the release date. |
| `genre` / `country` / `description` | `String` | same | |
| `duration_min` | `i64` | `durationMin` | Rounded up from seconds. |
| `subject_type` | `i64` | `subjectType` | 1 movie, 2 series, 6 video. |

Constructor: `MediaItem::minimal(id, title, is_series) -> Self` — everything
else empty (search-fallback shape).

#### `HomeSection`

| Field | Type | JSON key | Notes |
| --- | --- | --- | --- |
| `title` | `String` | `title` | "banner" sections are renamed "Top Picks". |
| `items` | `Vec<MediaItem>` | `items` | |
| `ranked` | `bool` | `ranked` | Numbered (Trending) row. |
| `is_banner` | `bool` | `isBanner` | Hero-carousel source. |

Constructor: `HomeSection::new(title, items, ranked, is_banner)`.

#### `Season`

| Field | Type | JSON key | Notes |
| --- | --- | --- | --- |
| `se` | `i64` | `se` | 1-based season number. |
| `max_ep` | `i64` | `maxEp` | Highest episode number (derived from `episodes`). |
| `episodes` | `Vec<i64>` | `episodes` | Explicit `allEp` list when the server sends one, else `1..=maxEp`. |

#### `Detail`

Play-flow handle produced by `play_detail`, consumed by `streams`.

| Field | Type | JSON key | Notes |
| --- | --- | --- | --- |
| `id` | `String` | `id` | Subject id. |
| `title` | `String` | `title` | |
| `detail_path` | `String` | `detailPath` | |
| `h5_base` | `String` | `h5Base` | Origin the play endpoint lives on. |
| `h5_token` | `Option<String>` | `h5Token` | Bearer for play/CDN calls. |
| `note` | `String` | `note` | Diagnostics, appended to "No streams" errors. |

#### `Dub`, `CastMember`, `SearchItem`, `SubtitleLink`

| Type | Fields (JSON keys) |
| --- | --- |
| `Dub` | `name` (`lanName`, default "Dub"), `subject_id` (`subjectId`), `detail_path` (`detailPath`) |
| `CastMember` | `name`, `avatar_url` (`avatarUrl`) |
| `SearchItem` | `id`, `title`, `is_series` (`isSeries`), `detail_path` (`detailPath`) — legacy-search hit |
| `SubtitleLink` | `label`, `url` |

#### `StreamKind`, `StreamLink`

`StreamKind` — `Hls`, `Dash`, `Mp4`; serializes lowercase (`"hls"`, `"dash"`,
`"mp4"`); `as_str()` returns the same strings.

`StreamLink` — one playable URL plus everything the player must send:

| Field | Type | JSON key | Notes |
| --- | --- | --- | --- |
| `label` | `String` | `label` | `"{variant} · {quality}p\|auto} · {KIND}"`, e.g. `"Original · 1080p · HLS"`. |
| `quality` | `i64` | `quality` | 0 = adaptive/unknown. |
| `url` | `String` | `url` | Manifest (mpd/m3u8) or progressive mp4. |
| `kind` | `StreamKind` | `kind` | `"hls"` / `"dash"` / `"mp4"`. |
| `headers` | `BTreeMap<String,String>` | `headers` | Referer/Origin/User-Agent (+ signed token header) the CDN requires. |

Method: `variant() -> &str` — first `label` segment (`"Original"`), matching
the Dart helper.

#### `DetailRich`, `SearchResult`, `StreamsResult`

| Type | Fields | Produced by |
| --- | --- | --- |
| `DetailRich` | `item: MediaItem`, `dubs: Vec<Dub>`, `cast: Vec<CastMember>` (≤ 15) | `detail_rich` |
| `SearchResult` | `items: Vec<MediaItem>`, `has_more: bool` | `search_rich` |
| `StreamsResult` | `links: Vec<StreamLink>` (sorted), `subtitles: Vec<SubtitleLink>` | `streams` |

---

### `CoreError`

```rust
pub enum CoreError {
    Api(String),                 // protocol failure, human-readable message
    Http(reqwest::Error),        // transport: DNS / connect / TLS / timeout
    Json(serde_json::Error),     // response body was not valid JSON
}
pub type Result<T> = std::result::Result<T, CoreError>;
```

`Api` messages match the strings the Dart client threw (`"h5 HTTP 400: …"`,
`"No streams (se=1 ep=1) | …"`), so app-side error UI keeps working unchanged.

---

### `signing` module

Low-level, pure functions — used by the client, public for testing and
protocol debugging. Details and worked examples:
[MOVIEBOX_PROTOCOL.md](MOVIEBOX_PROTOCOL.md#request-signing-mobile-api).

| Function | Signature | Description |
| --- | --- | --- |
| `now_ms` | `() -> u64` | Unix time in milliseconds. |
| `md5_hex` | `(&[u8]) -> String` | Lowercase hex MD5. |
| `client_token` | `(ts: u64) -> String` | `"{ms},{md5(reverse(str(ms)))}"`. |
| `canonical_query` | `(raw: &str) -> String` | Query with keys sorted, values percent-decoded, duplicates kept. |
| `canonical` | `(method, accept, content_type, url, body: Option<&str>, ts: u64) -> String` | The 7-line `\n`-joined canonical string (body MD5 caps at 102 400 bytes; empty path → `/`). |
| `signature` | `(method, accept, content_type, url, body, ts, key: &[u8]) -> String` | `"{ms}\|2\|{base64(hmac_md5(key, canonical))}"`. |
| `decode_key` | `(&str) -> Result<Vec<u8>, CoreErrorish>` | Base64-decode `Config::secret_b64`. |
| `client_info` | `(v4: bool, device_id: &str, override_json: Option<&str>) -> String` | The `x-client-info` JSON (override wins when set). |
| `random_device_id` | `() -> String` | 32-char random hex (16 bytes). |

### `parse` module

Tolerant upstream-JSON helpers (the Dart `_str`/`_obj`/`_arr`/`_deepStr`
ports). `s(o, k)` stringifies any scalar; `deep_str` searches nested objects up
to a depth (arrays skipped, literal `"null"` counts as missing);
`parse_subject(o, backdrop_hint, block_adult) -> Option<MediaItem>` and
`parse_season(o) -> Option<Season>` are the model parsers;
`type_label(subject_type)` maps 1/2/6 → `MOVIE`/`SERIES`/`VIDEO`; `first_int`
pulls the first digit run out of a string (resolution parsing).

### `content_filter` module

`is_adult(item: &MediaItem) -> bool` — the pure adult heuristic (genre →
title → description regexes; empty description never matches). The on/off
switch lives on the client (`set_block_adult`).

---

## FFI API

The `sparkbox-ffi` crate builds a `cdylib`/`staticlib`
(`libsparkbox_ffi.so` / `libsparkbox_ffi.dylib` / `sparkbox_ffi.dll` /
`libsparkbox_ffi.a`) with the C ABI declared in
[`bindings/sparkbox_core.h`](../bindings/sparkbox_core.h).

### Envelope and conventions

- Every function returns a **NUL-terminated UTF-8 JSON string** allocated by
  the core:

  ```json
  {"ok": true,  "data": …}      // success — data shape per function
  {"ok": false, "error": "…"}   // failure — CoreError display text
  ```

- Free every returned pointer with `sparkbox_free_string` (exactly once).
- All functions are **blocking** (network I/O on an internal runtime) — call
  them off the UI thread / main isolate. The bundled
  `bindings/sparkbox_core.dart` already does this.
- Arguments that are "JSON in" take a serialized model map with exactly the
  JSON keys documented above (`MediaItem` in, `Detail` in).
- Panics never cross the boundary; they surface as `{"ok":false,"error":"core
  panicked"}`.

### Function index

| Function | In | `data` payload on success |
| --- | --- | --- |
| `sparkbox_core_version()` | — | `{"version":"0.1.0","blockAdult":true}` |
| `sparkbox_set_block_adult(blocked: int)` | 0/1 | `{"blockAdult":…}` |
| `sparkbox_home()` | — | `[HomeSection]` |
| `sparkbox_suggest(query: cstr)` | query | `[String]` |
| `sparkbox_search(query: cstr, page: int, per_page: int)` | | `{"items":[MediaItem],"hasMore":bool}` |
| `sparkbox_detail_rich(item_json: cstr)` | `MediaItem` | `{"item":MediaItem,"dubs":[Dub],"cast":[CastMember]}` |
| `sparkbox_play_detail(item_json: cstr)` | `MediaItem` | `Detail` |
| `sparkbox_seasons(subject_id: cstr)` | subject id | `[Season]` |
| `sparkbox_streams(detail_json: cstr, se: int, ep: int, is_series: int)` | `Detail`, 0/1 | `{"links":[StreamLink],"subtitles":[SubtitleLink]}` |
| `sparkbox_type_label(subject_type: int)` | 1/2/6… | `{"label":"MOVIE"}` |
| `sparkbox_stream_kinds()` | — | `["hls","dash","mp4"]` |
| `sparkbox_free_string(ptr)` | returned ptr | — (frees) |

Minimal C example:

```c
#include "sparkbox_core.h"

char *raw = sparkbox_search("avengers", 1, 20);
printf("%s\n", raw);              /* {"ok":true,"data":{...}} */
sparkbox_free_string(raw);
```

Integration walkthroughs for Flutter/Dart, C, and other Rust projects:
[INTEGRATION.md](INTEGRATION.md).
