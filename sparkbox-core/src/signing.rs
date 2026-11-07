//! MovieBox mobile-BFF request signing, ported 1:1 from the Dart client
//! (`lib/api/moviebox_api.dart`, originally from the CloudStream
//! `MovieBoxProviderIN` plugin's signer).
//!
//! Two layers:
//! - `x-client-token` — `"<ms>,<md5(reverse(ms))>"`;
//! - `x-tr-signature` — `"<ms>|2|<base64(hmac_md5(key, canonical))>"` where the
//!   canonical string is `METHOD\naccept\ncontentType\nbodyLen\nms\nbodyMd5\npath?sortedQuery`.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use url::Url;

/// Current unix time in milliseconds.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Lowercase hex MD5 of `bytes`.
pub fn md5_hex(bytes: &[u8]) -> String {
    format!("{:x}", Md5::digest(bytes))
}

/// `x-client-token` for a given timestamp: `"<ms>,<md5(str(ms) reversed)>"`.
pub fn client_token(ts: u64) -> String {
    let reversed: String = ts.to_string().chars().rev().collect();
    format!("{ts},{}", md5_hex(reversed.as_bytes()))
}

/// Percent-decode a single query component (`+` stays `+`, like Dart's
/// `Uri.decodeComponent`).
fn decode_component(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// Query string with parameter names sorted alphabetically and all values kept
/// (decoded), matching Android's `Uri.getQueryParameters` behaviour that the
/// original signer relied on.
pub fn canonical_query(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let mut by_name: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        by_name
            .entry(decode_component(k))
            .or_default()
            .push(decode_component(v));
    }
    let mut out = Vec::new();
    for (k, vs) in by_name {
        for v in vs {
            out.push(format!("{k}={v}"));
        }
    }
    out.join("&")
}

/// The canonical string that gets HMAC'd.
///
/// `path` is the URL path (defaulting to `/`) plus the sorted query. The body
/// hash is the MD5 of the first 102 400 body bytes; empty when there is no
/// body.
pub fn canonical(
    method: &str,
    accept: &str,
    content_type: &str,
    url: &str,
    body: Option<&str>,
    ts: u64,
) -> String {
    let parsed = Url::parse(url).ok();
    let query = canonical_query(parsed.as_ref().and_then(|u| u.query()).unwrap_or(""));
    let mut path = parsed
        .map(|u| u.path().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "/".to_string());
    if !query.is_empty() {
        path = format!("{path}?{query}");
    }
    let body_bytes = body.map(str::as_bytes);
    let body_hash = match body_bytes {
        Some(b) => md5_hex(if b.len() > 102_400 { &b[..102_400] } else { b }),
        None => String::new(),
    };
    let body_len = body_bytes.map(|b| b.len().to_string()).unwrap_or_default();
    [
        method.to_uppercase(),
        accept.to_string(),
        content_type.to_string(),
        body_len,
        ts.to_string(),
        body_hash,
        path,
    ]
    .join("\n")
}

/// `x-tr-signature` for a request. `key` is the base64-decoded signing key
/// (see [`Config::secret_b64`]).
pub fn signature(
    method: &str,
    accept: &str,
    content_type: &str,
    url: &str,
    body: Option<&str>,
    ts: u64,
    key: &[u8],
) -> String {
    let canon = canonical(method, accept, content_type, url, body, ts);
    let mut mac = Hmac::<Md5>::new_from_slice(key).expect("HMAC-MD5 accepts any key length");
    mac.update(canon.as_bytes());
    let sig = mac.finalize().into_bytes();
    format!("{ts}|2|{}", B64.encode(sig))
}

/// Decode the base64 signing key from a config.
pub fn decode_key(secret_b64: &str) -> Result<Vec<u8>, CoreErrorish> {
    B64.decode(secret_b64.trim())
        .map_err(|_| CoreErrorish("invalid base64 secret in config"))
}

/// Tiny local error so callers can map key-decode failures to [`crate::error::CoreError`].
#[derive(Debug)]
pub struct CoreErrorish(pub &'static str);

impl std::fmt::Display for CoreErrorish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The `x-client-info` JSON. Key order matches the Dart `jsonEncode` output
/// (serde_json is built with `preserve_order`).
///
/// `override_json` (from [`Config::client_info_v3`] / [`Config::client_info_v4`])
/// wins when set.
pub fn client_info(v4: bool, device_id: &str, override_json: Option<&str>) -> String {
    if let Some(json) = override_json {
        let json = json.trim();
        if !json.is_empty() {
            return json.to_string();
        }
    }
    let m = if v4 {
        serde_json::json!({
            "package_name": "com.community.mbox.in",
            "version_name": "4.0.02.0831.03",
            "version_code": 50020126,
            "os": "android",
            "os_version": "14",
            "install_ch": "official",
            "device_id": device_id,
            "install_store": "official",
            "gaid": "1b2212c1-dadf-43c3-a0c8-bd6ce48ae22d",
            "brand": "Google",
            "model": "Pixel 8",
            "X-Play-Mode": "1",
            "X-Idle-Data": "1",
            "X-Family-Mode": "0",
            "X-Content-Mode": "0",
            "system_language": "en",
            "net": "NETWORK_WIFI",
            "region": "IN",
            "timezone": "Asia/Calcutta",
            "sp_code": "",
        })
    } else {
        serde_json::json!({
            "package_name": "com.community.mbox.in",
            "version_name": "3.0.03.0529.03",
            "version_code": 50020042,
            "os": "android",
            "os_version": "16",
            "device_id": device_id,
            "install_store": "ps",
            "gaid": "d7578036d13336cc",
            "brand": "google",
            "model": "SM-S918B",
            "system_language": "en",
            "net": "NETWORK_WIFI",
            "region": "IN",
            "timezone": "Asia/Calcutta",
            "sp_code": "",
        })
    };
    m.to_string()
}

/// Random 16-byte hex device id, stable for the client's lifetime (the Dart
/// version regenerated it per app run; keep the same length and format).
pub fn random_device_id() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors computed independently (Python hashlib/hmac) — see tests/core_tests.rs
    // for the full request-level vectors.

    #[test]
    fn md5_known_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn client_token_reverses_timestamp() {
        assert_eq!(
            client_token(1_700_000_000_000),
            "1700000000000,e41bb805cc23fdc5541917da93d608d5"
        );
    }

    #[test]
    fn canonical_query_sorts_and_decodes() {
        // token URL query (values are numeric strings)
        let raw = "tabId=0&categoryType=4516404531735022304&page=1&perPage=1";
        assert_eq!(
            canonical_query(raw),
            "categoryType=4516404531735022304&page=1&perPage=1&tabId=0"
        );
        // unsorted, duplicate keys, empty value, percent-encoded value, '+' untouched
        assert_eq!(
            canonical_query("b=2&a=1&a=3&c=&e=%5Bx%5D&f=a+b"),
            "a=1&a=3&b=2&c=&e=[x]&f=a+b"
        );
        assert_eq!(canonical_query(""), "");
    }

    #[test]
    fn client_info_shape() {
        let v4 = client_info(true, "deadbeef", None);
        assert!(v4.starts_with("{\"package_name\":\"com.community.mbox.in\""));
        assert!(v4.contains("\"version_code\":50020126"));
        assert!(v4.contains("\"device_id\":\"deadbeef\""));
        assert!(v4.contains("\"X-Play-Mode\":\"1\""));
        assert!(v4.contains("\"install_store\":\"official\""));

        let v3 = client_info(false, "deadbeef", None);
        assert!(v3.contains("\"version_code\":50020042"));
        assert!(v3.contains("\"install_store\":\"ps\""));
        assert!(!v3.contains("X-Play-Mode"));

        // explicit override wins
        let over = client_info(true, "deadbeef", Some(r#"{"x":1}"#));
        assert_eq!(over, r#"{"x":1}"#);
    }

    #[test]
    fn device_id_is_16_bytes_hex() {
        let id = random_device_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, random_device_id());
    }
}
