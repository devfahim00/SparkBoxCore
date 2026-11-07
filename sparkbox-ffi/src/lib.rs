//! C-ABI bindings for SparkBoxCore — JSON in, JSON out.
//!
//! Every function returns a NUL-terminated UTF-8 JSON string allocated by the
//! core (free it with [`sparkbox_free_string`]):
//!
//! ```json
//! {"ok": true,  "data": ... }   // success
//! {"ok": false, "error": "..." } // failure
//! ```
//!
//! The `data` payloads serialize exactly like the Dart models' `toMap()`
//! (camelCase), so the Flutter app can feed them straight into its existing
//! `MediaItem.fromMap` etc.
//!
//! These are **blocking** calls (they run the async core on an internal
//! runtime). From Dart, call them on a background isolate — the bundled
//! `bindings/sparkbox_core.dart` helper already does that.
//!
//! Build: `cargo build --release -p sparkbox-ffi`
//! → `libsparkbox_ffi.so` (Android/Linux), `libsparkbox_ffi.dylib` (macOS),
//!   `sparkbox_ffi.dll` (Windows), `libsparkbox_ffi.a` (iOS/static).

use std::ffi::{c_char, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::OnceLock;

use sparkbox_core::{CoreError, MediaItem, MovieBoxClient, StreamKind};
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static CLIENT: OnceLock<MovieBoxClient> = OnceLock::new();

fn rt() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

fn client() -> &'static MovieBoxClient {
    CLIENT.get_or_init(MovieBoxClient::new)
}

/// Run `f`, converting panics into an error JSON instead of aborting the host.
fn guard<F: FnOnce() -> *mut c_char>(f: F) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(p) => p,
        Err(_) => leak(r#"{"ok":false,"error":"core panicked"}"#.to_string()),
    }
}

/// Take ownership of a Rust string as a C string (caller frees with
/// [`sparkbox_free_string`]).
fn leak(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn ok<T: serde::Serialize>(data: &T) -> *mut c_char {
    let body = serde_json::json!({ "ok": true, "data": data });
    leak(body.to_string())
}

fn err(e: &CoreError) -> *mut c_char {
    let body = serde_json::json!({ "ok": false, "error": e.to_string() });
    leak(body.to_string())
}

/// Read a C string argument; `null` and invalid UTF-8 become `""`.
fn arg(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Parse an argument that must be JSON (`MediaItem` / `Detail` maps).
fn arg_json<T: serde::de::DeserializeOwned>(p: *const c_char, what: &str) -> Result<T, CoreError> {
    let raw = arg(p);
    if raw.trim().is_empty() {
        return Err(CoreError::Api(format!("{what} JSON is empty")));
    }
    serde_json::from_str(&raw).map_err(|e| CoreError::Api(format!("{what} JSON invalid: {e}")))
}

// ------------------------------------------------------------------ exported

/// Core version + current adult-filter state: `{"ok":true,"data":{"version":"…","blockAdult":true}}`.
#[no_mangle]
pub extern "C" fn sparkbox_core_version() -> *mut c_char {
    guard(|| {
        let data = serde_json::json!({
            "version": sparkbox_core::VERSION,
            "blockAdult": client().block_adult(),
        });
        ok(&data)
    })
}

/// Toggle the adult-content filter (default ON).
#[no_mangle]
pub extern "C" fn sparkbox_set_block_adult(blocked: c_int) -> *mut c_char {
    guard(|| {
        client().set_block_adult(blocked != 0);
        ok(&serde_json::json!({ "blockAdult": blocked != 0 }))
    })
}

/// Home feed → `Vec<HomeSection>` (each: `title/items/ranked/isBanner`).
#[no_mangle]
pub extern "C" fn sparkbox_home() -> *mut c_char {
    guard(|| match rt().block_on(client().home()) {
        Ok(sections) => ok(&sections),
        Err(e) => err(&e),
    })
}

/// Search suggestions → `Vec<String>`.
#[no_mangle]
pub extern "C" fn sparkbox_suggest(query: *const c_char) -> *mut c_char {
    guard(|| {
        let q = arg(query);
        match rt().block_on(client().suggest(&q)) {
            Ok(words) => ok(&words),
            Err(e) => err(&e),
        }
    })
}

/// Rich search → `{"items": [...MediaItem], "hasMore": bool}`.
#[no_mangle]
pub extern "C" fn sparkbox_search(
    query: *const c_char,
    page: c_int,
    per_page: c_int,
) -> *mut c_char {
    guard(|| {
        let q = arg(query);
        match rt().block_on(client().search_rich(&q, page as i64, per_page as i64)) {
            Ok(res) => ok(&res),
            Err(e) => err(&e),
        }
    })
}

/// Full detail for a `MediaItem` map → `{"item":…, "dubs":[…], "cast":[…]}`.
#[no_mangle]
pub extern "C" fn sparkbox_detail_rich(item_json: *const c_char) -> *mut c_char {
    guard(|| {
        let item: MediaItem = match arg_json(item_json, "MediaItem") {
            Ok(i) => i,
            Err(e) => return err(&e),
        };
        match rt().block_on(client().detail_rich(&item)) {
            Ok(rich) => ok(&rich),
            Err(e) => err(&e),
        }
    })
}

/// Play-flow `Detail` for a `MediaItem` map → `Detail` (feed to `sparkbox_streams`).
#[no_mangle]
pub extern "C" fn sparkbox_play_detail(item_json: *const c_char) -> *mut c_char {
    guard(|| {
        let item: MediaItem = match arg_json(item_json, "MediaItem") {
            Ok(i) => i,
            Err(e) => return err(&e),
        };
        match rt().block_on(client().play_detail(&item)) {
            Ok(d) => ok(&d),
            Err(e) => err(&e),
        }
    })
}

/// Seasons of a series → `Vec<Season>` (each: `se/maxEp/episodes`).
#[no_mangle]
pub extern "C" fn sparkbox_seasons(subject_id: *const c_char) -> *mut c_char {
    guard(|| {
        let id = arg(subject_id);
        match rt().block_on(client().seasons(&id)) {
            Ok(seasons) => ok(&seasons),
            Err(e) => err(&e),
        }
    })
}

/// Resolve streams for a `Detail` map → `{"links": [...StreamLink], "subtitles": [...SubtitleLink]}`.
#[no_mangle]
pub extern "C" fn sparkbox_streams(
    detail_json: *const c_char,
    se: c_int,
    ep: c_int,
    is_series: c_int,
) -> *mut c_char {
    guard(|| {
        let d: sparkbox_core::Detail = match arg_json(detail_json, "Detail") {
            Ok(v) => v,
            Err(e) => return err(&e),
        };
        match rt().block_on(client().streams(&d, se as i64, ep as i64, is_series != 0)) {
            Ok(res) => ok(&res),
            Err(e) => err(&e),
        }
    })
}

/// `subjectType` → `"MOVIE" | "SERIES" | "VIDEO" | "MEDIA"` inside `data`.
#[no_mangle]
pub extern "C" fn sparkbox_type_label(subject_type: c_int) -> *mut c_char {
    guard(|| ok(&serde_json::json!({ "label": MovieBoxClient::type_label(subject_type as i64) })))
}

/// Kind names for reference: `["hls","dash","mp4"]` — `StreamLink.kind` uses these strings.
#[no_mangle]
pub extern "C" fn sparkbox_stream_kinds() -> *mut c_char {
    guard(|| {
        ok(&serde_json::json!([
            StreamKind::Hls.as_str(),
            StreamKind::Dash.as_str(),
            StreamKind::Mp4.as_str(),
        ]))
    })
}

/// Free a string returned by any `sparkbox_*` function. `null` is a no-op.
///
/// # Safety
/// `ptr` must come from one of the `sparkbox_*` functions and must not be
/// freed twice.
#[no_mangle]
pub unsafe extern "C" fn sparkbox_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
