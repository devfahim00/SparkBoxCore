//! Runtime-overridable endpoints, user agents and signing material.
//!
//! Everything the upstream service can change on its side lives here or in
//! [`crate::signing`]. When MovieBox migrates hosts / headers / app versions,
//! updating this struct (or loading a JSON override at runtime via
//! [`Config::from_json`]) is usually all the core needs — the app keeps using
//! the same functions and the same JSON shapes.

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Default mobile BFF origin.
pub const DEFAULT_MOBILE_BASE: &str = "https://api3.aoneroom.com";
/// Default guest-token bootstrapper (ranking-list endpoint).
pub const DEFAULT_TOKEN_URL: &str = "https://apig.inmoviebox.com/wefeed-mobile-bff/tab/ranking-list?tabId=0&categoryType=4516404531735022304&page=1&perPage=1";
/// Default web (h5) origin, used as global fallback.
pub const DEFAULT_H5_BASE: &str = "https://movie-box.co";
/// Default `Referer` sent on h5 calls.
pub const DEFAULT_H5_REFERER: &str = "https://mzfi.me/";

/// HMAC-MD5 key (base64) extracted from the original plugin's request signer.
pub const DEFAULT_SECRET_B64: &str = "76iRl07s0xSN9jqmEWAt79EBJZulIQIsV64FZr2O";

/// User-Agent of the v3 Android client.
pub const DEFAULT_UA_V3: &str = "com.community.mbox.in/50020042 (Linux; U; Android 16; en_IN; sdk_gphone64_x86_64; Build/BP22.250325.006; Cronet/133.0.6876.3)";
/// User-Agent of the v4 Android client.
pub const DEFAULT_UA_V4: &str = "com.community.mbox.in/50020126 (Linux; U; Android 14; en_IN; Pixel 8; Build/UD1A.230803.041; Cronet/145.0.7582.0)";
/// Desktop browser User-Agent for web (h5) calls.
pub const DEFAULT_UA_DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";
/// Mobile browser User-Agent for the play endpoint.
pub const DEFAULT_UA_MOBILE: &str = "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Mobile Safari/537.36";

/// Endpoints, headers material and knobs the upstream controls.
///
/// All fields serialize camelCase so a config blob is readable from Dart/JS
/// and from Rust. Unknown fields are ignored; missing fields fall back to the
/// compiled defaults ([`Config::default`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Mobile BFF base, e.g. `https://api3.aoneroom.com`.
    pub mobile_base: String,
    /// Guest-token bootstrapper URL (any signed mobile GET that returns `x-user`).
    pub token_url: String,
    /// Web (h5) base, e.g. `https://movie-box.co`.
    pub h5_base: String,
    /// `Referer` header for h5 calls (the web player origin).
    pub h5_referer: String,
    /// Base64 HMAC-MD5 key used to sign mobile requests.
    pub secret_b64: String,
    /// User-Agent impersonating the v3 Android app.
    pub ua_v3: String,
    /// User-Agent impersonating the v4 Android app.
    pub ua_v4: String,
    /// Desktop browser User-Agent.
    pub ua_desktop: String,
    /// Mobile browser User-Agent.
    pub ua_mobile: String,
    /// Timezone sent in `x-client-info` (server uses it for release dates).
    pub timezone: String,
    /// Region sent in `x-client-info`.
    pub region: String,
    /// How long a cached mobile guest token stays fresh (ms).
    pub guest_token_ttl_ms: u64,
    /// Replace the whole v3 `x-client-info` JSON (advanced; overrides built-in).
    pub client_info_v3: Option<String>,
    /// Replace the whole v4 `x-client-info` JSON (advanced; overrides built-in).
    pub client_info_v4: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mobile_base: DEFAULT_MOBILE_BASE.to_string(),
            token_url: DEFAULT_TOKEN_URL.to_string(),
            h5_base: DEFAULT_H5_BASE.to_string(),
            h5_referer: DEFAULT_H5_REFERER.to_string(),
            secret_b64: DEFAULT_SECRET_B64.to_string(),
            ua_v3: DEFAULT_UA_V3.to_string(),
            ua_v4: DEFAULT_UA_V4.to_string(),
            ua_desktop: DEFAULT_UA_DESKTOP.to_string(),
            ua_mobile: DEFAULT_UA_MOBILE.to_string(),
            timezone: "Asia/Calcutta".to_string(),
            region: "IN".to_string(),
            guest_token_ttl_ms: 3_600_000,
            client_info_v3: None,
            client_info_v4: None,
        }
    }
}

impl Config {
    /// Serialize to a pretty JSON blob (camelCase keys).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("config serializes")
    }

    /// Parse a config JSON blob. Unknown fields are ignored; fields that are
    /// missing keep their defaults.
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_flutter_app() {
        let c = Config::default();
        assert_eq!(c.mobile_base, "https://api3.aoneroom.com");
        assert!(c.token_url.starts_with("https://apig.inmoviebox.com/"));
        assert_eq!(c.h5_base, "https://movie-box.co");
        assert_eq!(c.guest_token_ttl_ms, 3_600_000);
        assert!(c.client_info_v3.is_none() && c.client_info_v4.is_none());
    }

    #[test]
    fn json_round_trip_and_partial_override() {
        let json = r#"{"mobileBase":"https://new-host.example","h5Base":"https://new-h5.example"}"#;
        let c = Config::from_json(json).unwrap();
        assert_eq!(c.mobile_base, "https://new-host.example");
        assert_eq!(c.h5_base, "https://new-h5.example");
        // untouched fields keep defaults
        assert_eq!(c.secret_b64, DEFAULT_SECRET_B64);
        assert_eq!(c.timezone, "Asia/Calcutta");

        let back = Config::from_json(&c.to_json()).unwrap();
        assert_eq!(c, back);
    }
}
