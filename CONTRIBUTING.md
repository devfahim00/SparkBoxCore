# Contributing to SparkBoxCore

Thanks for helping maintain the core. This repo is small on purpose — its
entire job is to *contain* the volatility of the upstream MovieBox service —
so changes should stay surgical, well-tested and consistent with the
behavior-parity goal.

- [Development setup](#development-setup)
- [Everyday commands](#everyday-commands)
- [Code style](#code-style)
- [Where changes land](#where-changes-land)
- [Testing expectations](#testing-expectations)
- [Submitting a pull request](#submitting-a-pull-request)

## Development setup

```bash
git clone https://github.com/devfahim00/SparkBoxCore.git
cd SparkBoxCore
cargo test --workspace        # must pass before anything else
```

Requirements: Rust 1.80+ (rustup recommended). Android NDK + [cargo-ndk](https://github.com/bbqsrc/cargo-ndk)
only if you touch the FFI crate and need to verify mobile builds locally —
CI builds them regardless.

## Everyday commands

```bash
cargo fmt --all                        # format (CI enforces)
cargo clippy --workspace --all-targets -- -D warnings   # lint (CI enforces)
cargo test --workspace                 # 32 tests incl. signing vectors
cargo run -p sparkbox-core --example smoke   # live read-only smoke test
cargo build --release -p sparkbox-ffi  # native library
```

The smoke example hits the live API. It is read-only (search/detail/play
lookup, no playback) and is the fastest way to see whether the protocol still
works end-to-end.

## Code style

- Formatting and lints are non-negotiable: `cargo fmt` and clippy with
  `-D warnings` must be clean (CI blocks otherwise).
- Every public item carries a doc comment (`#![warn(missing_docs)]` is on).
  Doc examples compile and run as tests — keep them `no_run` when they need
  the network.
- No `unsafe` in `sparkbox-core`. The only permitted `unsafe` in the workspace
  is the `sparkbox_free_string` deallocation in `sparkbox-ffi`.
- No logging, no panics across the FFI boundary, no new global mutable state.
- Keep dependencies minimal: anything added to `sparkbox-core` ships inside
  every app that embeds the core.

## Where changes land

The decision table below is the heart of this repo. When the upstream service
changes, the fix must land in the narrowest possible layer:

| Change | Layer | File(s) |
| --- | --- | --- |
| Host, UA, client-info, token TTL | Configuration | `sparkbox-core/src/config.rs` (often just a default, or nothing at all if shipped as runtime JSON) |
| Signing / canonicalization | Signer | `sparkbox-core/src/signing.rs` **+ re-derived vectors** in `sparkbox-core/tests/core_tests.rs` |
| Response field renames / shape drift | Parsing | `sparkbox-core/src/parse.rs` |
| Endpoint paths, request bodies, fallback chains | Transport | `sparkbox-core/src/mobile.rs` (mobile BFF), `sparkbox-core/src/h5.rs` (web API) |
| High-level behavior (ordering, retries, labels) | Facade / flows | `sparkbox-core/src/client.rs`, `h5.rs::streams_impl` |
| New app-facing capability | FFI + bindings | `sparkbox-ffi/src/lib.rs`, `bindings/sparkbox_core.h`, `bindings/sparkbox_core.dart` — additive only |
| Documentation of protocol behavior | Docs | `docs/MOVIEBOX_PROTOCOL.md` (keep it in sync with the code!) |

**Golden rule:** if a fix requires touching the app (anything outside this
repo), stop and reconsider — the architecture failed its purpose. Bring the
volatile part into the core instead.

When editing protocol behavior, update `docs/MOVIEBOX_PROTOCOL.md` in the
same PR; it is the reference future maintainers will trust.

## Testing expectations

- Bug fixes come with a test that fails before the fix (use the
  tolerant-parser and pure-function style of the existing suite).
- Anything that changes **request bytes** (signing, URL construction,
  headers) requires re-deriving the affected regression vectors with an
  *independent* implementation (e.g. Python `hashlib`/`hmac`/`base64`) —
  never with the crate itself — and documenting the derivation in the test.
- New parsers get a fixture built from a real (anonymized/truncated) upstream
  response.
- Live-API checks belong in the `smoke` example, not in `cargo test`.

## Submitting a pull request

1. Branch from `main`: `fix/…`, `feat/…`, `docs/…`, `chore/…`.
2. Make the change in the narrowest layer (see table above).
3. `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace` — all green.
4. Update `CHANGELOG.md` (one bullet, present tense, under an *Unreleased*
   heading) and, for protocol changes, `docs/MOVIEBOX_PROTOCOL.md`.
5. PR description: what changed, *why* (which upstream symptom), how it was
   verified (tests + `smoke` output if live).
6. Keep PRs single-purpose; a signing fix and a refactor do not belong in the
   same PR.

CI runs format + clippy + tests + FFI builds (Linux and all Android ABIs) on
every push and PR; it must be green before merge.
