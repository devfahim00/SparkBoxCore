# The MovieBox Wire Protocol

This document specifies, at the byte level, everything SparkBoxCore knows
about the upstream MovieBox service: the two API surfaces, request signing,
token lifecycle, every endpoint the core calls, and the exact algorithms used
to turn responses into home feeds, search results and playable streams.

It exists so that **when the upstream changes, this document plus the core are
the only things that need updating** — and so a future maintainer can verify a
fix without reverse-engineering the original plugin.

- [Overview: two API surfaces](#overview-two-api-surfaces)
- [Constants](#constants)
- [Request signing (mobile API)](#request-signing-mobile-api)
- [Token lifecycle](#token-lifecycle)
- [Endpoint catalog](#endpoint-catalog)
- [Home feed composition](#home-feed-composition)
- [Detail resolution](#detail-resolution)
- [Stream resolution algorithm](#stream-resolution-algorithm)
- [Subtitle discovery](#subtitle-discovery)
- [Adult-content heuristic](#adult-content-heuristic)
- [Maintenance runbook](#maintenance-runbook)

## Overview: two API surfaces

| Surface | Base | Auth | Region | Used for |
| --- | --- | --- | --- | --- |
| **Mobile BFF** | `https://api3.aoneroom.com` (config: `mobileBase`) | HMAC-MD5 signed headers + optional bearer guest token | Region-locked to the app's market | Home feed, legacy search, seasons, subject detail, captions |
| **Web (h5) API** | `https://movie-box.co` (config: `h5Base`) | Bearer guest token (from `x-user` response header) | Global (content localizes by IP) | Home fallback, trending, suggest, rich search, detail, dubs, **play** |

The guest-token bootstrapper lives on a third host,
`https://apig.inmoviebox.com` (config: `tokenUrl`) — any signed mobile GET
that returns an `x-user` header works; the core uses the `ranking-list`
endpoint.

Everything below is implemented in `sparkbox-core/src/mobile.rs` (mobile) and
`sparkbox-core/src/h5.rs` (web), with signing in `signing.rs` and parsing in
`parse.rs`.

## Constants

| Constant | Default | Config field |
| --- | --- | --- |
| Mobile BFF base | `https://api3.aoneroom.com` | `mobileBase` |
| Token bootstrapper | `https://apig.inmoviebox.com/wefeed-mobile-bff/tab/ranking-list?tabId=0&categoryType=4516404531735022304&page=1&perPage=1` | `tokenUrl` |
| Web (h5) base | `https://movie-box.co` | `h5Base` |
| h5 `Referer` | `https://mzfi.me/` | `h5Referer` |
| Signing key (base64) | `76iRl07s0xSN9jqmEWAt79EBJZulIQIsV64FZr2O` (30 bytes decoded) | `secretB64` |
| UA · v3 Android app | `com.community.mbox.in/50020042 (…Android 16… Cronet/133.0.6876.3)` | `uaV3` |
| UA · v4 Android app | `com.community.mbox.in/50020126 (…Android 14… Pixel 8… Cronet/145.0.7582.0)` | `uaV4` |
| UA · desktop browser | `Mozilla/5.0 (Windows NT 10.0; Win64; x64) … Chrome/154…` | `uaDesktop` |
| UA · mobile browser | `Mozilla/5.0 (Linux; Android 15; Pixel 9) … Mobile Safari/537.36` | `uaMobile` |
| `x-client-info` timezone | `Asia/Calcutta` | `timezone` |
| `x-client-info` region | `IN` | `region` |
| Guest-token TTL | 3 600 000 ms | `guestTokenTtlMs` |

## Request signing (mobile API)

Every mobile-BFF request carries four custom headers. Two of them are
signatures and must match the server's expectations exactly.

### `x-client-token`

```
x-client-token = "<ms>,<md5( reverse( str(ms) ) )>"
```

where `ms` is the current unix time in **milliseconds** and `reverse` reverses
the decimal string. Example (`ms = 1700000000123`):

```
str(ms)     = "1700000000123"
reversed    = "3210000000071"
md5         = 8bb2b37cedae787f581503a05e847ec5
header      = "1700000000123,8bb2b37cedae787f581503a05e847ec5"
```

### `x-tr-signature`

```
x-tr-signature = "<ms>|2|<base64( HMAC-MD5( key, canonical ) )>"
```

- `key` = base64-decoded `secretB64` (30 bytes).
- The `|2|` infix is a scheme version marker.

### The canonical string

Seven lines joined with `\n` (empty lines stay empty):

```
METHOD            uppercase HTTP method
accept            e.g. "application/json" ("" for caption calls)
contentType       e.g. "application/json" or "application/json; charset=utf-8"
bodyLen           byte length of the body, "" when there is no body
ms                unix milliseconds (same value as in the header)
bodyMd5           lowercase hex MD5 of the body — first 102 400 bytes only; "" when no body
path              URL path + "?" + sorted query ("/" when the URL has no path)
```

Worked example — the signed search POST (`ms = 1700000000123`):

```
canonical =
"POST\n"
"application/json\n"
"application/json; charset=utf-8\n"
"44\n"
"1700000000123\n"
"ba5194dd00d7e7748a69d4894cfa05f1\n"
"/wefeed-mobile-bff/subject-api/search/v2"

x-tr-signature = "1700000000123|2|4ZEZqtUBNjMkA1irNODHpA=="
```

The caption endpoints sign with **empty** accept/content-type lines:

```
canonical = "GET\n\n\n\n1700000000123\n\n/wefeed-mobile-bff/subject-api/get-stream-captions?streamId=abc&subjectId=123"
signature = "1700000000123|2|lzJBSVjtU5mxZz70LaKt2A=="
```

All of these vectors are pinned as regression tests in
`sparkbox-core/tests/core_tests.rs`, derived independently with Python
(`hashlib`/`hmac`/`base64`).

### Sorted query ("canonical query")

The query part of the URL is rebuilt before signing, mimicking Android's
`Uri.getQueryParameters` behavior the original signer relied on:

1. split the raw query on `&`; skip empty segments;
2. split each pair at the **first** `=` (no `=` → whole segment is the key,
   value is empty);
3. percent-decode key and value (`+` stays `+`);
4. group values by key, **sort the keys alphabetically** (multi-value keys
   keep their original relative order);
5. re-join as `k=v&k=v…` (values re-encoded never — they are emitted decoded).

Example:

```
raw      : tabId=0&categoryType=4516404531735022304&page=1&perPage=1
canonical: categoryType=4516404531735022304&page=1&perPage=1&tabId=0
```

### `x-client-info`

A JSON blob describing the (fake) Android client. Key order is significant to
byte-identical reproduction and is preserved (`serde_json` built with
`preserve_order`). Two shapes:

**v4** (app version 4.0.02.0831.03, code 50020126, Pixel 8 / Android 14):

```json
{"package_name":"com.community.mbox.in","version_name":"4.0.02.0831.03","version_code":50020126,"os":"android","os_version":"14","install_ch":"official","device_id":"<16-byte-hex>","install_store":"official","gaid":"1b2212c1-dadf-43c3-a0c8-bd6ce48ae22d","brand":"Google","model":"Pixel 8","X-Play-Mode":"1","X-Idle-Data":"1","X-Family-Mode":"0","X-Content-Mode":"0","system_language":"en","net":"NETWORK_WIFI","region":"IN","timezone":"Asia/Calcutta","sp_code":""}
```

**v3** (app version 3.0.03.0529.03, code 50020042, SM-S918B / Android 16):

```json
{"package_name":"com.community.mbox.in","version_name":"3.0.03.0529.03","version_code":50020042,"os":"android","os_version":"16","device_id":"<16-byte-hex>","install_store":"ps","gaid":"d7578036d13336cc","brand":"google","model":"SM-S918B","system_language":"en","net":"NETWORK_WIFI","region":"IN","timezone":"Asia/Calcutta","sp_code":""}
```

`device_id` is a random 16-byte hex string generated once per client. The
whole blob can be replaced at runtime via `Config.client_info_v3` /
`Config.client_info_v4`.

### Full mobile header set

```
user-agent:      uaV3 or uaV4
accept:          application/json
content-type:    application/json            (charset=utf-8 when a body is sent)
connection:      keep-alive
x-client-token:  <see above>
x-tr-signature:  <see above>
x-client-info:   <see above>
x-client-status: 0
authorization:   Bearer <guest token>        (only when a token is known)
x-play-mode:     2                           (only on tab-operating and the
                                              v3 detail retry)
```

### h5 header set (no signing)

```
accept:         application/json
origin:         https://movie-box.co
referer:        https://mzfi.me/                 (config h5Referer)
content-type:   application/json                 (on POST)
user-agent:     <desktop browser UA>
x-client-info:  {"timezone":"Asia/Calcutta"}
authorization:  Bearer <h5 token>                (when a token is known)
```

### Play-call header set

```
user-agent:                <mobile browser UA>
referer:                   {h5_base}/movies/{detailPath}?id={subjectId}&type=/movie/detail&detailSe=&detailEp=&lang=en
accept:                    application/json
x-client-info:             {"timezone":"Asia/Calcutta"}
x-request-lang:            en
x-vip-restrict:            0
x-no-high-risk-restrict:   0
x-source:                  (empty)
authorization:             Bearer <h5 token>
```

## Token lifecycle

Two independent guest tokens — no accounts, no refresh tokens, no expiry
fields.

**Mobile guest token**

1. `GET tokenUrl` (any signed mobile GET returning `x-user`) with v3 headers,
   no bearer.
2. The response carries `x-user: {"token": "…", …}` (JSON in a header).
3. The token is cached with its fetch time and reused for
   `guestTokenTtlMs` (default 1 h). No other refresh logic — after the TTL the
   next call simply bootstraps again.
4. The token is attached as `authorization: Bearer …` on signed mobile calls
   that need it (search, seasons, detail retry, captions).

**Web (h5) token**

1. `GET {h5_base}/wefeed-h5api-bff/home?host=movie-box.co` with h5 headers, no
   bearer.
2. Read the token from the `x-user` response header, cache it indefinitely.
3. Every h5 call sends it as a bearer. If a call answers **400 / 401 / 441**:
   drop the cache, force a fresh bootstrap, retry the call **once**. If the
   retry is not 200, fail with `h5 HTTP <code>: <body prefix>`.

`x-user` parsing is case-insensitive on the header name; a missing, invalid or
empty token simply yields "no token".

## Endpoint catalog

### Mobile BFF (`{mobile_base}`)

| # | Method & path | Auth | Purpose |
| --- | --- | --- | --- |
| M1 | `GET /wefeed-mobile-bff/tab/ranking-list?tabId=0&categoryType=…&page=1&perPage=1` *(host `apig.inmoviebox.com`, config `tokenUrl`)* | signed v3 | Guest-token bootstrap (token in `x-user` response header). |
| M2 | `GET /wefeed-mobile-bff/tab-operating?page=1&tabId=0&version=` | signed v3 + `x-play-mode: 2` | Home feed (region-locked). Response: `data.items[]`, each `{title, type, banner.banners[].subject \| subjects[] \| customData.items[].subject}`. |
| M3 | `POST /wefeed-mobile-bff/subject-api/search/v2` | signed v3 + bearer | Legacy search. Body `{"page":1,"perPage":20,"keyword":"…"}`. Response: `data.results[].subjects[]` (`subjectId`, `title`, `subjectType`, nested `detailPath`). |
| M4 | `GET /wefeed-mobile-bff/subject-api/season-info?subjectId={id}` | signed v3 + bearer | Seasons. Response: `data.seasons[]` (or root `seasons[]`): `{se, maxEp, allEp}`. |
| M5 | `GET /wefeed-mobile-bff/subject-api/get?subjectId={id}` | signed **v4**; retry signed v3 + bearer + `x-play-mode: 2` when no `detailUrl` | Subject detail → `detailPath`/`detailUrl` origin. |
| M6 | `GET /wefeed-mobile-bff/subject-api/get-stream-captions?subjectId={id}&streamId={sid}` | signed v4 + bearer, **empty-signature** variant first, regular signature as fallback | Captions for one stream. Response: `data.captions[]` / `data.extCaptions[]`. |
| M7 | `GET /wefeed-mobile-bff/subject-api/get-ext-captions?subjectId={id}&resourceId={sid}&episode=0` | same as M6 | Extended captions. |

### Web API (`{h5_base}`)

| # | Method & path | Body / params | Purpose |
| --- | --- | --- | --- |
| H1 | `GET /wefeed-h5api-bff/home?host=movie-box.co` | — | h5 token bootstrap + home fallback. Response: `data.operatingList[]` (`title`, `type`, `banner.items[]`, `subjects[]`). |
| H2 | `GET /wefeed-h5api-bff/subject/trending?tabId=0&page=1&perPage={n}` | — | Trending row. Response: `data.subjectList[]`. |
| H3 | `POST /wefeed-h5api-bff/subject/search-suggest` | `{"keyword":"…","perPage":10}` | Search suggestions. Response: `data.items[].word`. |
| H4 | `POST /wefeed-h5api-bff/subject/search` | `{"keyword":"…","page":1,"perPage":20}` | Rich search. Response: `data.items[]` + `data.pager.hasMore`. |
| H5 | `GET /wefeed-h5api-bff/detail?detailPath={path}` | — | Rich detail: `data.subject` (full subject + `dubs[]` + `staffList[]`). |
| H6 | `GET /wefeed-h5api-bff/detail?subjectId={id}` | — | detailPath lookup by id (fallback). |
| H7 | `GET /wefeed-h5api-bff/subject/play?subjectId={id}&se={se}&ep={ep}&detailPath={path}&streamSignType=1&supportCodecs%5Bhevc%5D=1&supportCodecs%5Bh264%5D=1` | — | **Play manifest.** Response: `data.dash[]`, `data.hls[]`, `data.streams[]`, inline `captions`/`subtitles`/`subtitleList`. |

### Subject object (h5 shape)

Parsed by `parse_subject` into a `MediaItem`:

```
subjectId        → id                    (required, non-empty)
title            → title                 (required, non-empty)
subjectType      → subject_type          (1 movie, 2 series, 6 video; default 1)
cover.url        → cover
stills.url       → backdrop              (falls back to a banner hint)
duration         → duration_min          ((duration + 59) / 60, seconds → minutes)
releaseDate      → year                  (first 4 chars)
detailPath       → detail_path
imdbRatingValue  → rating
genre            → genre
countryName      → country
description      → description
```

An item missing `subjectId` or `title` is dropped, as is any item the adult
filter hides while the filter is ON.

## Home feed composition

1. Try **M2** (mobile). Sections with `type`:
   - `BANNER` → `banner.banners[].subject`; a section literally titled
     "banner" (case-insensitive) is renamed **"Top Picks"**;
   - `SUBJECTS_MOVIE` / `SUBJECTS_TV` / `APPOINTMENT_LIST` → `subjects[]`;
   - `CUSTOM` → `customData.items[].subject`.
   Empty-titled sections and sections that end up with zero items are dropped.
   If no sections survive → mobile home failed.
2. If mobile failed → **H1** (web). Same section rules, except `BANNER`
   iterates `banner.items[]` and uses each banner's `image.url` as a backdrop
   hint. Still nothing → `Err("home feed unavailable")`.
3. Prepend a **"Trending Now"** ranked row from **H2** (20 items) when
   trending returns anything (errors ignored).

## Detail resolution

`play_detail(item)`:

1. If `item.detail_path` is already set → `Detail { h5_base = config.h5_base,
   h5_token = cached h5 token }`.
2. Otherwise **M5** with v4 headers; if `data.detailUrl` is missing/empty →
   retry with v3 + bearer + `x-play-mode: 2`; keep the retry response whenever
   it has a `data` object.
3. From `data.detailUrl` derive:
   - `h5_base` = `scheme://host` (port dropped; fallback `config.h5_base`);
   - `detail_path` = last path segment after stripping the query and any
     trailing `/` (fallbacks, in order: `data.detailPath` via nested search →
     the caller's path hint → **H6** lookup by subject id).
4. `h5_token` = the `x-user` token of whichever response won, else the cached
   mobile guest token (any age), else a fresh bootstrap.

`detail_rich(item)` additionally calls **H5** and merges description/stills,
dubs (`dubs[].lanName|subjectId|detailPath`, default name "Dub") and up to 15
cast members (`staffList[].name|avatarUrl`).

## Stream resolution algorithm

`streams(detail, se, ep, is_series)`:

1. **Variants** = `[("Original", detail.id, detail.detail_path)]` plus every
   dub from H5's `dubs` (a dub without its own path reuses the original
   `detail_path`). Dub-list failures are ignored (Original still plays).
2. **Attempts** = `[(se, ep)]` for series; `[(0, 0), (1, 1)]` for movies
   (quirk kept from the plugin: movies are stored as "season 1, episode 1"
   but play at 0/0).
3. For each variant × attempt, call **H7** (`play_once`):
   - If `dash` + `hls` + `streams` are all empty, record a diagnostic note
     (`play HTTP <code>: <first 160 chars>`) and try the next attempt.
   - Collect inline subtitles (see [below](#subtitle-discovery)).
   - Turn each entry of `dash` / `hls` / `streams` into a `StreamLink`:
     - `kind`: fixed DASH / HLS for the first two arrays; for `streams[]`,
       inferred from `format` or the URL (`.mpd` → dash, `.m3u8` → hls, else
       mp4);
     - `quality`: first integer in `resolutions` (e.g. `"1080,720,480"` →
       1080), 0 = adaptive/unknown;
     - `label`: `"{variant} · {quality}p|auto · {KIND}"`;
     - `headers` (what the player must send):
       - mp4 → `Referer: {h5_base}/`, desktop UA;
       - dash/hls → `Origin: {h5_base}`, the play referer, mobile UA, plus
         `signHeaderKey` (or `X-MB-Token`) : `signCookie` when the entry
         carries a signed cookie.
   - First attempt that yields links wins for that variant (no double-play).
4. All variants empty → `Err("No streams (se=… ep=…)" + diagnostics)`.
5. Load external captions (below), then sort: links whose label starts with
   `"Original"` first, remaining by quality descending.

## Subtitle discovery

Two sources, merged and de-duplicated by URL (first occurrence wins):

1. **Inline** on every play response (H7): the arrays `captions`, `subtitles`,
   `subtitleList` — each entry's `url` (or `subtitleUrl`) and language
   (`lanName` → `language` → `lan`, else `"Subtitle N"`).
2. **External** via M6/M7 for every (subjectId, streamId) pair collected
   during play (`streamId` = the `id` of the first stream entry seen, else
   `"{subjectId}|{se}|{ep}"`). The list prefers `extCaptions` over `captions`
   when both exist. Language fallback `lanName` → `lan` → `language` →
   `"Unknown"`; duplicate labels get `" (2)"`, `" (3)"`… suffixes.

Caption endpoints are called with the empty-accept/content-type signature
first; if the server rejects it, the regular signature is retried.

## Adult-content heuristic

The API has no adult flag, so items are matched on text — three
case-insensitive regexes, applied in order (genre → title → description):

```
genre  : \b(adult|erotic|erotica|porn|pornography|hentai|xxx|sexploitation|softcore|hardcore|18\+)\b
title  : (\bxxx\b|\b18\s?\+|\berotic|\bporn|\bhentai\b|\bsex\s?(tape|scene|slave|doll|club|tour)\b|\bnude\b|\buncensored\b|\bnaked\b|\bsoftcore\b|\bsexy\b|\bhorny\b|\bseduc)
desc   : \b(erotic|pornograph|explicit sex|sexual encounters|hardcore|softcore|nudity|adult film)\b
```

Known quirk (kept bug-for-bug): `\b18\+` can only match when a word character
follows the `+`, so the genre `"18+"` alone does **not** match but `"18+action"`
does. An empty description never matches. The switch (`block_adult`, default
ON) toggles the whole filter; when ON, matching items are silently dropped at
parse time — home rows, search results, trending, everywhere.

## Maintenance runbook

When the upstream misbehaves, work through this table:

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| All mobile calls return 401/403, h5 still works | Signing rejected (app version banned, key rotated) | Bump `uaV3`/`uaV4` + `clientInfoV3/4` in `Config`; if the key changed, update `secretB64` and re-derive the vectors in `tests/core_tests.rs`. |
| Connection errors on every call | Host migration | Update `mobileBase` / `h5Base` / `tokenUrl` in `Config` (runtime JSON is enough — no rebuild). |
| Home works, search empty | h5 search endpoint moved/renamed | Adjust the path in `h5.rs::search_rich_h5`; the mobile fallback will have masked the failure meanwhile. |
| Streams resolve but playback 403s | CDN header/signing change | Compare the `headers` map built in `play_once` against what the website sends (`signCookie` / `signHeaderKey` / referer shape). |
| No streams at all | Play endpoint or detailPath derivation changed | Read the diagnostics appended to the `No streams` error — they contain the raw play-response prefix; fix the URL builder or parser in `h5.rs`. |
| Response shape drift (fields renamed) | Upstream refactor | Update `parse.rs` / the endpoint parsers; models and FFI stay untouched. |
| Subtitles missing | Caption endpoint/signature change | Check M6/M7 paths and the empty-signature variant in `mobile_get`. |

**Golden rule:** if you touch `signing.rs` or anything that affects request
bytes, re-derive the regression vectors with an independent implementation
(Python) before shipping — the vectors are the repo's tripwire.
