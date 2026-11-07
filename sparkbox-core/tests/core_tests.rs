// Request-level regression vectors, computed independently with Python
// (hashlib/hmac/base64) against the exact algorithm from the Dart client.
// If the server rejects signed requests after a core change, re-derive these
// vectors first — the canonicalization, not the transport, is what usually
// drifts.

use sparkbox_core::config::{Config, DEFAULT_SECRET_B64};
use sparkbox_core::signing::{canonical, canonical_query, client_token, decode_key, signature};

const KEY_LEN: usize = 30;

#[test]
fn request_signature_vector() {
    let key = decode_key(DEFAULT_SECRET_B64).unwrap();
    assert_eq!(key.len(), KEY_LEN);

    // POST search with a body, exactly as the client sends it.
    let url = "https://api3.aoneroom.com/wefeed-mobile-bff/subject-api/search/v2";
    let body = r#"{"keyword":"avengers","page":1,"perPage":20}"#;
    let ts = 1_700_000_000_123u64;

    let canon = canonical(
        "POST",
        "application/json",
        "application/json; charset=utf-8",
        url,
        Some(body),
        ts,
    );
    assert_eq!(
        canon,
        "POST\napplication/json\napplication/json; charset=utf-8\n44\n1700000000123\nba5194dd00d7e7748a69d4894cfa05f1\n/wefeed-mobile-bff/subject-api/search/v2"
    );

    let sig = signature(
        "POST",
        "application/json",
        "application/json; charset=utf-8",
        url,
        Some(body),
        ts,
        &key,
    );
    assert_eq!(sig, "1700000000123|2|4ZEZqtUBNjMkA1irNODHpA==");
}

#[test]
fn caption_signature_vector() {
    // The caption endpoints sign with EMPTY accept / content-type.
    let key = decode_key(DEFAULT_SECRET_B64).unwrap();
    let url = "https://api3.aoneroom.com/wefeed-mobile-bff/subject-api/get-stream-captions?subjectId=123&streamId=abc";
    let ts = 1_700_000_000_123u64;

    let canon = canonical("GET", "", "", url, None, ts);
    assert_eq!(
        canon,
        "GET\n\n\n\n1700000000123\n\n/wefeed-mobile-bff/subject-api/get-stream-captions?streamId=abc&subjectId=123"
    );

    let sig = signature("GET", "", "", url, None, ts, &key);
    // vector derived over the SORTED query (streamId=abc&subjectId=123)
    assert_eq!(sig, "1700000000123|2|lzJBSVjtU5mxZz70LaKt2A==");
}

#[test]
fn client_token_vector() {
    assert_eq!(
        client_token(1_700_000_000_000),
        "1700000000000,e41bb805cc23fdc5541917da93d608d5"
    );
}

#[test]
fn token_url_canonical_query_vector() {
    let cfg = Config::default();
    let raw = cfg.token_url.split_once('?').unwrap().1;
    assert_eq!(
        canonical_query(raw),
        "categoryType=4516404531735022304&page=1&perPage=1&tabId=0"
    );
    // and the full canonical for the token bootstrap GET
    let key = decode_key(&cfg.secret_b64).unwrap();
    let ts = 1_700_000_000_555u64;
    let canon = canonical(
        "GET",
        "application/json",
        "application/json",
        &cfg.token_url,
        None,
        ts,
    );
    assert_eq!(
        canon,
        "GET\napplication/json\napplication/json\n\n1700000000555\n\n/wefeed-mobile-bff/tab/ranking-list?categoryType=4516404531735022304&page=1&perPage=1&tabId=0"
    );
    let _ = signature(
        "GET",
        "application/json",
        "application/json",
        &cfg.token_url,
        None,
        ts,
        &key,
    );
}

#[test]
fn canonical_query_edge_cases() {
    // unsorted + duplicate keys + empty value + percent decoding + '+' kept
    assert_eq!(
        canonical_query("b=2&a=1&a=3&c=&e=%5Bx%5D&f=a+b"),
        "a=1&a=3&b=2&c=&e=[x]&f=a+b"
    );
    assert_eq!(canonical_query(""), "");
    assert_eq!(canonical_query("x"), "x=");
}

#[test]
fn huge_body_hash_uses_first_100k_bytes_only() {
    // The signer hashes at most 102400 body bytes, but reports the REAL length.
    use sparkbox_core::signing::md5_hex;

    let big = "x".repeat(102_500);
    let url = "https://api3.aoneroom.com/wefeed-mobile-bff/subject-api/search/v2";
    let c = canonical("POST", "a", "b", url, Some(&big), 1);

    // real body length is reported…
    assert!(
        c.contains("\n102500\n"),
        "length must be 102500, got: {c:?}"
    );
    // …but the hash covers only the first 102400 bytes
    assert!(c.contains(&md5_hex(&[b'x'; 102_400])));
    assert!(!c.contains(&md5_hex(&[b'x'; 102_500])));
    // and it never equals the hash of a genuinely 102400-byte body with its own length
    let c_exact = canonical("POST", "a", "b", url, Some(&"x".repeat(102_400)), 1);
    assert!(c_exact.contains("\n102400\n"));
    // same truncated hash, different length line
    assert!(c_exact.contains(&md5_hex(&[b'x'; 102_400])));
}

#[test]
fn url_without_path_defaults_to_root() {
    let c = canonical("GET", "", "", "https://api3.aoneroom.com", None, 5);
    assert!(
        c.ends_with("\n/"),
        "canonical should end with \\n/, got: {c:?}"
    );
    let c2 = canonical("GET", "", "", "https://api3.aoneroom.com?q=1", None, 5);
    assert!(c2.ends_with("/?q=1"), "got: {c2:?}");
}

// ------------------------------------------------------------ ffi envelope
// The ffi crate is exercised indirectly here only for the pure helpers that
// live in the core; full cdylib symbols are checked by `cargo build -p
// sparkbox-ffi` and by the Dart binding file contract documented in README.
