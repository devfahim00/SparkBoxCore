# Changelog

## Unreleased

### Documentation

- Full professional English documentation suite:
  - README rewritten end-to-end (quick start, Flutter/C integration,
    configuration, maintenance runbook, versioning policy).
  - `docs/ARCHITECTURE.md` — design goals, system context, module map,
    Dart → Rust provenance, runtime state model, data flows, deliberate
    deviations from the Dart client.
  - `docs/MOVIEBOX_PROTOCOL.md` — complete wire-protocol specification:
    signing (with worked examples), token lifecycle, endpoint catalog,
    home/detail/stream/subtitle algorithms, adult heuristic, maintenance
    runbook.
  - `docs/API_REFERENCE.md` — every public type, function and FFI symbol.
  - `docs/INTEGRATION.md` — Flutter/Dart, C and Rust integration guides,
    build matrix (desktop/Android/iOS), threading and update strategy.
- Add `CONTRIBUTING.md` (style rules, layering table, testing expectations,
  PR checklist).
- Add MIT `LICENSE`.

## 0.1.0 — first extraction

Core extracted from the SparkBox Flutter app (RMB `flutter` @ `1d675b8`) into Rust.

- `MovieBoxClient` facade: `home`, `suggest`, `search_rich`, `detail_rich`,
  `play_detail`, `seasons`, `streams` (+ subtitles), `type_label`.
- Mobile BFF client with 1:1 request signing (`x-client-token`, `x-tr-signature`
  HMAC-MD5 over the canonical string, `x-client-info`), guest-token caching and
  the v4→v3 detail retry.
- Web (h5) client with token bootstrap/refresh and the region-independent
  home/trending/suggest/search/detail/play endpoints.
- Stream resolution across dub variants (movies try se/ep 0/0 then 1/1),
  dash/hls/mp4 links with per-link player headers, mobile caption endpoints for
  external subtitles, "Original first, then quality desc" ordering.
- Adult-content heuristic ported bug-for-bug, switchable at runtime.
- `Config` with runtime JSON override for hosts, user agents, signing key and
  client-info blobs.
- C-ABI FFI crate (`sparkbox-ffi`, cdylib/staticlib) with a JSON envelope;
  C header + Flutter/Dart binding helper under `bindings/`.
- 32 tests, including independently derived signing regression vectors.
