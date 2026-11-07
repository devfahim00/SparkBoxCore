//! Mobile BFF (`wefeed-mobile-bff`) calls with the signed app headers — ported
//! from the private statics of the Dart `MovieBoxApi` class.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value};

use crate::client::MovieBoxClient;
use crate::error::{CoreError, Result};
use crate::models::{HomeSection, SearchItem, Season};
use crate::parse::{as_i64, deep_str, get_arr, parse_season, parse_subject, s, try_json};
use crate::signing::{self, client_info, signature};

/// Header lookup that ignores case (like the Dart `_header` helper).
fn header_value<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// `x-user` response header → guest-token JSON → `token` field.
pub(crate) fn token_from_x_user(h: &HeaderMap) -> Option<String> {
    let x_user = header_value(h, "x-user")?;
    let v: Value = serde_json::from_str(x_user).ok()?;
    let t = v.get("token")?.as_str()?;
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Insert a header only when both name and value parse (defensive; our values are ASCII).
fn ins(h: &mut HeaderMap, k: &str, v: String) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(k.as_bytes()),
        HeaderValue::from_str(&v),
    ) {
        h.insert(name, value);
    }
}

impl MovieBoxClient {
    /// Full signed header set the Android app sends.
    pub(crate) fn mobile_headers(
        &self,
        method: &str,
        url: &str,
        body: Option<&str>,
        v4: bool,
        token: Option<&str>,
    ) -> HeaderMap {
        let ct = if body.is_some() {
            "application/json; charset=utf-8"
        } else {
            "application/json"
        };
        let key = signing::decode_key(&self.config.secret_b64).unwrap_or_default();
        let mut h = HeaderMap::new();
        ins(
            &mut h,
            "user-agent",
            if v4 {
                self.config.ua_v4.clone()
            } else {
                self.config.ua_v3.clone()
            },
        );
        ins(&mut h, "accept", "application/json".into());
        ins(&mut h, "content-type", ct.into());
        ins(&mut h, "connection", "keep-alive".into());
        ins(
            &mut h,
            "x-client-token",
            signing::client_token(signing::now_ms()),
        );
        ins(
            &mut h,
            "x-tr-signature",
            signature(
                method,
                "application/json",
                ct,
                url,
                body,
                signing::now_ms(),
                &key,
            ),
        );
        ins(
            &mut h,
            "x-client-info",
            client_info(
                v4,
                &self.device_id,
                if v4 {
                    self.config.client_info_v4.as_deref()
                } else {
                    self.config.client_info_v3.as_deref()
                },
            ),
        );
        ins(&mut h, "x-client-status", "0".into());
        if let Some(t) = token {
            if !t.trim().is_empty() {
                ins(&mut h, "authorization", format!("Bearer {t}"));
            }
        }
        h
    }

    /// Mobile guest token from the `x-user` header of a signed ranking-list
    /// GET. Cached for [`crate::config::Config::guest_token_ttl_ms`].
    pub(crate) async fn guest_token(&self) -> Result<Option<String>> {
        let now = signing::now_ms();
        {
            let g = self.guest.lock().await;
            if let Some((t, at)) = g.as_ref() {
                if now.saturating_sub(*at) < self.config.guest_token_ttl_ms {
                    return Ok(Some(t.clone()));
                }
            }
        }
        let url = self.config.token_url.clone();
        let r = self
            .http
            .get(&url)
            .headers(self.mobile_headers("GET", &url, None, false, None))
            .send()
            .await?;
        let t = token_from_x_user(r.headers());
        if let Some(t) = &t {
            *self.guest.lock().await = Some((t.clone(), now));
        }
        Ok(t)
    }

    /// Cached guest token without refreshing (mirrors the Dart `_guest` read).
    pub(crate) async fn cached_guest(&self) -> Option<String> {
        self.guest.lock().await.as_ref().map(|(t, _)| t.clone())
    }

    /// Mobile BFF `tab-operating` (the plugin's mainPage source).
    pub(crate) async fn home_mobile(&self) -> Result<Vec<HomeSection>> {
        let url = format!(
            "{}/wefeed-mobile-bff/tab-operating?page=1&tabId=0&version=",
            self.config.mobile_base
        );
        let mut h = self.mobile_headers("GET", &url, None, false, None);
        h.insert(
            HeaderName::from_static("x-play-mode"),
            HeaderValue::from_static("2"),
        );
        let body = self.http.get(&url).headers(h).send().await?.text().await?;
        let root = try_json(&body);
        let data = root
            .as_ref()
            .and_then(|r| r.get("data"))
            .and_then(Value::as_object);
        let items = get_arr(data.and_then(|d| d.get("items")));
        let block_adult = self.block_adult();

        let mut out = Vec::new();
        for section in items {
            let raw_title = s(section, "title");
            let title = if raw_title.to_lowercase() == "banner" {
                "Top Picks".to_string()
            } else {
                raw_title
            };
            if title.trim().is_empty() {
                continue;
            }
            let section_type = s(section, "type");
            let mut media = Vec::new();
            match section_type.as_str() {
                "BANNER" => {
                    let banners = section
                        .get("banner")
                        .and_then(Value::as_object)
                        .and_then(|b| b.get("banners"));
                    for b in get_arr(banners) {
                        if let Some(sub) = b.get("subject").and_then(Value::as_object) {
                            if let Some(m) = parse_subject(sub, "", block_adult) {
                                media.push(m);
                            }
                        }
                    }
                }
                "SUBJECTS_MOVIE" | "SUBJECTS_TV" | "APPOINTMENT_LIST" => {
                    for sub in get_arr(section.get("subjects")) {
                        if let Some(m) = parse_subject(sub, "", block_adult) {
                            media.push(m);
                        }
                    }
                }
                "CUSTOM" => {
                    let customs = section
                        .get("customData")
                        .and_then(Value::as_object)
                        .and_then(|d| d.get("items"));
                    for c in get_arr(customs) {
                        if let Some(sub) = c.get("subject").and_then(Value::as_object) {
                            if let Some(m) = parse_subject(sub, "", block_adult) {
                                media.push(m);
                            }
                        }
                    }
                }
                _ => {}
            }
            if !media.is_empty() {
                out.push(HomeSection::new(
                    title,
                    media,
                    false,
                    section_type == "BANNER",
                ));
            }
        }
        if out.is_empty() {
            return Err(CoreError::Api("tab-operating returned no sections".into()));
        }
        Ok(out)
    }

    /// Legacy mobile search (used as the h5 fallback path).
    pub(crate) async fn search_mobile(&self, query: &str) -> Result<Vec<SearchItem>> {
        let url = format!(
            "{}/wefeed-mobile-bff/subject-api/search/v2",
            self.config.mobile_base
        );
        let body = serde_json::json!({ "page": 1, "perPage": 20, "keyword": query }).to_string();
        let token = self.guest_token().await?;
        let text = self
            .http
            .post(&url)
            .headers(self.mobile_headers("POST", &url, Some(&body), false, token.as_deref()))
            .body(body)
            .send()
            .await?
            .text()
            .await?;
        let root = try_json(&text);
        let data = root
            .as_ref()
            .and_then(|r| r.get("data"))
            .and_then(Value::as_object);
        let mut out = Vec::new();
        for grp in get_arr(data.and_then(|d| d.get("results"))) {
            for sub in get_arr(grp.get("subjects")) {
                let id = s(sub, "subjectId");
                let title = s(sub, "title");
                if id.is_empty() || title.is_empty() {
                    continue;
                }
                out.push(SearchItem {
                    id,
                    title,
                    is_series: as_i64(sub.get("subjectType")) == Some(2),
                    detail_path: deep_str(sub, "detailPath", 3).unwrap_or_default(),
                });
            }
        }
        Ok(out)
    }

    /// Season list of a series (mobile season-info).
    pub(crate) async fn seasons_impl(&self, subject_id: &str) -> Result<Vec<Season>> {
        let url = format!(
            "{}/wefeed-mobile-bff/subject-api/season-info?subjectId={}",
            self.config.mobile_base, subject_id
        );
        let token = self.guest_token().await?;
        let text = self
            .http
            .get(&url)
            .headers(self.mobile_headers("GET", &url, None, false, token.as_deref()))
            .send()
            .await?
            .text()
            .await?;
        let root = try_json(&text);
        let data = root
            .as_ref()
            .and_then(|r| r.get("data"))
            .and_then(Value::as_object);
        let seasons_val = data
            .and_then(|d| d.get("seasons"))
            .or_else(|| root.as_ref().and_then(|r| r.get("seasons")));
        let mut out: Vec<Season> = get_arr(seasons_val)
            .into_iter()
            .filter_map(parse_season)
            .collect();
        out.sort_by_key(|s| s.se);
        Ok(out)
    }

    /// Mobile detail call: gives detailPath/detailUrl and the guest token used
    /// for web calls.
    pub(crate) async fn detail_mobile(
        &self,
        id: &str,
        path_hint: &str,
    ) -> Result<crate::models::Detail> {
        let url = format!(
            "{}/wefeed-mobile-bff/subject-api/get?subjectId={}",
            self.config.mobile_base, id
        );
        // First try: v4 headers, no token.
        let resp = self
            .http
            .get(&url)
            .headers(self.mobile_headers("GET", &url, None, true, None))
            .send()
            .await?;
        let mut status = resp.status().as_u16();
        let mut x_user = token_from_x_user(resp.headers());
        let mut root = try_json(&resp.text().await?);

        let data_has_detail_url = |root: &Option<Map<String, Value>>| {
            root.as_ref()
                .and_then(|r| r.get("data"))
                .and_then(Value::as_object)
                .and_then(|d| deep_str(d, "detailUrl", 3))
                .is_some_and(|v| !v.is_empty())
        };
        if !data_has_detail_url(&root) {
            // Retry: v3 headers + guest token + play-mode 2 (like the plugin).
            let t = self.guest_token().await?;
            let mut h = self.mobile_headers("GET", &url, None, false, t.as_deref());
            h.insert(
                HeaderName::from_static("x-play-mode"),
                HeaderValue::from_static("2"),
            );
            if let Ok(r2) = self.http.get(&url).headers(h).send().await {
                let status2 = r2.status().as_u16();
                let x_user2 = token_from_x_user(r2.headers());
                let text2 = r2.text().await.unwrap_or_default();
                let root2 = try_json(&text2);
                // Dart keeps the retry result whenever its `data` exists.
                if root2
                    .as_ref()
                    .and_then(|r| r.get("data"))
                    .and_then(Value::as_object)
                    .is_some()
                {
                    status = status2;
                    x_user = x_user2;
                    root = root2;
                }
            }
        }

        let data = root
            .as_ref()
            .and_then(|r| r.get("data"))
            .and_then(Value::as_object)
            .ok_or_else(|| CoreError::Api(format!("Detail failed (HTTP {status})")))?;

        let subject = data
            .get("subject")
            .and_then(Value::as_object)
            .unwrap_or(data);
        let detail_url = deep_str(data, "detailUrl", 3).unwrap_or_default();

        // The plugin derives both from data.detailUrl: origin = scheme://host,
        // detailPath = last path segment (query stripped).
        let (origin, mut path) = parse_detail_url(&detail_url, &self.config.h5_base);
        if path.is_empty() {
            path = deep_str(data, "detailPath", 3).unwrap_or_default();
        }
        if path.is_empty() {
            path = path_hint.to_string();
        }
        if path.is_empty() {
            // Last resort (unverified endpoint): ask the web detail API by subjectId.
            if let Ok(p) = self.h5_detail_path_by_id(id).await {
                path = p;
            }
        }

        let h5_token = match x_user {
            Some(t) => Some(t),
            None => self.cached_guest().await.or(self.guest_token().await?),
        };
        Ok(crate::models::Detail {
            id: id.to_string(),
            title: s(subject, "title"),
            detail_path: path,
            h5_base: origin,
            h5_token,
            note: String::new(),
        })
    }

    /// Mobile-API GET with the app's signed headers. The plugin signs caption
    /// calls with empty accept / content-type; falls back to the regular
    /// signature if the server rejects that.
    pub(crate) async fn mobile_get(&self, path: &str) -> Option<Map<String, Value>> {
        let url = format!("{}{}", self.config.mobile_base, path);
        let token = self.guest_token().await.ok()?;
        let key = signing::decode_key(&self.config.secret_b64).ok()?;
        for empty_sig in [true, false] {
            let mut h = self.mobile_headers("GET", &url, None, true, token.as_deref());
            if empty_sig {
                let sig = signature("GET", "", "", &url, None, signing::now_ms(), &key);
                if let Ok(value) = HeaderValue::from_str(&sig) {
                    h.insert(HeaderName::from_static("x-tr-signature"), value);
                }
            }
            match self.http.get(&url).headers(h).send().await {
                Ok(r) => {
                    if !r.status().is_success() {
                        continue;
                    }
                    if let Some(j) = r.text().await.ok().and_then(|t| try_json(&t)) {
                        return Some(j);
                    }
                }
                Err(_) => continue,
            }
        }
        None
    }
}

/// `origin` (`scheme://host`) and last path segment of a detail URL.
fn parse_detail_url(detail_url: &str, fallback_base: &str) -> (String, String) {
    if detail_url.is_empty() {
        return (fallback_base.to_string(), String::new());
    }
    let origin = match url::Url::parse(detail_url) {
        Ok(u) if !u.scheme().is_empty() && u.host_str().is_some_and(|h| !h.is_empty()) => {
            format!("{}://{}", u.scheme(), u.host_str().unwrap_or_default())
        }
        _ => fallback_base.to_string(),
    };
    let mut path = detail_url.split('?').next().unwrap_or_default().to_string();
    while path.ends_with('/') {
        path.pop();
    }
    if let Some(slash) = path.rfind('/') {
        path = path[slash + 1..].to_string();
    }
    (origin, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_url_origin_and_path() {
        // normal case
        let (o, p) = parse_detail_url(
            "https://m.example.com/movies/some-slug?x=1",
            "https://fallback",
        );
        assert_eq!(o, "https://m.example.com");
        assert_eq!(p, "some-slug");
        // trailing slash stripped before the last-segment cut
        let (_, p) = parse_detail_url(
            "https://m.example.com/movies/some-slug/",
            "https://fallback",
        );
        assert_eq!(p, "some-slug");
        // port is dropped from origin, like the Dart code (scheme://host only)
        let (o, _) = parse_detail_url("https://m.example.com:8443/a/b", "https://fallback");
        assert_eq!(o, "https://m.example.com");
        // not a URL → fallback origin, empty path
        let (o, p) = parse_detail_url("garbage", "https://fallback");
        assert_eq!(o, "https://fallback");
        assert_eq!(p, "garbage"); // no slash → whole string is the "segment"
                                  // empty
        let (o, p) = parse_detail_url("", "https://fallback");
        assert_eq!(o, "https://fallback");
        assert_eq!(p, "");
    }
}
