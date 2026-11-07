//! Web (h5) API (`wefeed-h5api-bff`) — works from any region; content
//! localizes by IP. Ported from the h5 half of the Dart `MovieBoxApi`.

use serde_json::{Map, Value};

use crate::client::MovieBoxClient;
use crate::error::{CoreError, Result};
use crate::models::{
    CastMember, DetailRich, Dub, MediaItem, SearchResult, StreamKind, StreamLink, StreamsResult,
    SubtitleLink,
};
use crate::parse::{deep_str, first_int, get_arr, parse_subject, s, try_json};

impl MovieBoxClient {
    /// Host part of the h5 base (sent as the `host=` query param).
    fn h5_host(&self) -> String {
        url::Url::parse(&self.config.h5_base)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "movie-box.co".to_string())
    }

    /// h5 headers with an optional bearer token.
    fn h5_headers(
        &self,
        token: Option<&str>,
        with_content_type: bool,
    ) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        let ins = |h: &mut reqwest::header::HeaderMap, k: &str, v: String| {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(&v),
            ) {
                h.insert(name, value);
            }
        };
        ins(&mut h, "accept", "application/json".into());
        ins(&mut h, "origin", self.config.h5_base.clone());
        ins(&mut h, "referer", self.config.h5_referer.clone());
        if with_content_type {
            ins(&mut h, "content-type", "application/json".into());
        }
        ins(&mut h, "user-agent", self.config.ua_desktop.clone());
        ins(
            &mut h,
            "x-client-info",
            format!("{{\"timezone\":\"{}\"}}", self.config.timezone),
        );
        if let Some(t) = token {
            if !t.trim().is_empty() {
                ins(&mut h, "authorization", format!("Bearer {t}"));
            }
        }
        h
    }

    /// Guest token for the web (h5) API, taken from the `x-user` response
    /// header of the h5 home endpoint. Cached until a call comes back
    /// 400/401/441, then force-refreshed once.
    pub(crate) async fn h5_token(&self, force: bool) -> Result<Option<String>> {
        if !force {
            let cached = self.h5_guest.lock().await.clone();
            if let Some(t) = cached {
                return Ok(Some(t));
            }
        }
        let url = format!(
            "{}/wefeed-h5api-bff/home?host={}",
            self.config.h5_base,
            self.h5_host()
        );
        let r = self
            .http
            .get(&url)
            .headers(self.h5_headers(None, false))
            .send()
            .await?;
        let t = super::mobile::token_from_x_user(r.headers());
        if let Some(t) = &t {
            *self.h5_guest.lock().await = Some(t.clone());
        }
        Ok(t)
    }

    /// GET/POST against the h5 base with automatic token refresh.
    async fn h5_call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Map<String, Value>> {
        let url = format!("{}{}", self.config.h5_base, path);
        let do_call = |token: Option<String>| {
            let url = url.clone();
            let method = method.to_string();
            let body = body.cloned();
            async move {
                let h = self.h5_headers(token.as_deref(), true);
                let res = if method == "POST" {
                    let payload = serde_json::to_string(&body.unwrap_or(Value::Object(Map::new())))
                        .map_err(|e| CoreError::Api(format!("body encode failed: {e}")))?;
                    self.http.post(&url).headers(h).body(payload).send().await
                } else {
                    self.http.get(&url).headers(h).send().await
                };
                res.map_err(CoreError::from)
            }
        };

        // Take the cached token without holding the lock across the fetch
        // (h5_token locks the same mutex — a guard in the match scrutinee
        // would deadlock).
        let cached = self.h5_guest.lock().await.clone();
        let token = match cached {
            Some(t) => Some(t),
            None => self.h5_token(false).await?,
        };
        let mut r = do_call(token).await?;
        let code = r.status().as_u16();
        if code == 400 || code == 401 || code == 441 {
            *self.h5_guest.lock().await = None;
            let fresh = self.h5_token(true).await?;
            r = do_call(fresh).await?;
        }
        let status = r.status();
        let text = r.text().await?;
        if status.as_u16() != 200 {
            return Err(CoreError::Api(format!(
                "h5 HTTP {}: {}",
                status.as_u16(),
                truncate(&text, 140)
            )));
        }
        let v: Value = serde_json::from_str(&text)?;
        v.as_object()
            .cloned()
            .ok_or_else(|| CoreError::Api("h5 response was not a JSON object".into()))
    }

    /// h5 web home (works from any region; content localizes by IP).
    pub(crate) async fn home_h5(&self) -> Result<Vec<crate::models::HomeSection>> {
        let root = self
            .h5_call(
                "GET",
                &format!("/wefeed-h5api-bff/home?host={}", self.h5_host()),
                None,
            )
            .await?;
        let operating = get_arr(root.get("data").and_then(|d| d.get("operatingList")));
        let block_adult = self.block_adult();
        let mut out = Vec::new();
        for section in operating {
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
            if section_type == "BANNER" {
                let items = section
                    .get("banner")
                    .and_then(Value::as_object)
                    .and_then(|b| b.get("items"));
                for b in get_arr(items) {
                    let img = b
                        .get("image")
                        .and_then(Value::as_object)
                        .map(|i| s(i, "url"))
                        .unwrap_or_default();
                    if let Some(sub) = b.get("subject").and_then(Value::as_object) {
                        if let Some(m) = parse_subject(sub, &img, block_adult) {
                            // keep the banner image as backdrop when stills was empty
                            let m = if m.backdrop.is_empty() {
                                let mut m = m;
                                m.backdrop = img;
                                m
                            } else {
                                m
                            };
                            media.push(m);
                        }
                    }
                }
            } else {
                for sub in get_arr(section.get("subjects")) {
                    if let Some(m) = parse_subject(sub, "", block_adult) {
                        media.push(m);
                    }
                }
            }
            if !media.is_empty() {
                out.push(crate::models::HomeSection::new(
                    title,
                    media,
                    false,
                    section_type == "BANNER",
                ));
            }
        }
        if out.is_empty() {
            return Err(CoreError::Api("h5 home returned no sections".into()));
        }
        Ok(out)
    }

    /// Trending subjects (h5).
    pub(crate) async fn trending(&self, per_page: i64) -> Result<Vec<MediaItem>> {
        let root = self
            .h5_call(
                "GET",
                &format!("/wefeed-h5api-bff/subject/trending?tabId=0&page=1&perPage={per_page}"),
                None,
            )
            .await?;
        let subject_list = get_arr(root.get("data").and_then(|d| d.get("subjectList")));
        let block_adult = self.block_adult();
        Ok(subject_list
            .into_iter()
            .filter_map(|s| parse_subject(s, "", block_adult))
            .collect())
    }

    /// Word suggestions as-you-type (h5 search-suggest).
    pub(crate) async fn suggest_impl(&self, query: &str) -> Result<Vec<String>> {
        let root = self
            .h5_call(
                "POST",
                "/wefeed-h5api-bff/subject/search-suggest",
                Some(&serde_json::json!({ "keyword": query, "perPage": 10 })),
            )
            .await?;
        let items = get_arr(root.get("data").and_then(|d| d.get("items")));
        let mut seen = std::collections::HashSet::new();
        let mut words = Vec::new();
        for o in items {
            let w = s(o, "word");
            if !w.is_empty() && seen.insert(w.clone()) {
                words.push(w);
            }
        }
        Ok(words)
    }

    /// Rich, paginated search (h5).
    pub(crate) async fn search_rich_h5(
        &self,
        query: &str,
        page: i64,
        per_page: i64,
    ) -> Result<SearchResult> {
        let root = self
            .h5_call(
                "POST",
                "/wefeed-h5api-bff/subject/search",
                Some(&serde_json::json!({ "keyword": query, "page": page, "perPage": per_page })),
            )
            .await?;
        let data = root.get("data").and_then(Value::as_object);
        let block_adult = self.block_adult();
        let items = get_arr(data.and_then(|d| d.get("items")))
            .into_iter()
            .filter_map(|s| parse_subject(s, "", block_adult))
            .collect();
        let has_more = data
            .and_then(|d| d.get("pager"))
            .and_then(Value::as_object)
            .and_then(|p| p.get("hasMore"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(SearchResult { items, has_more })
    }

    /// Full detail from the h5 endpoint: description, stills, dubs and cast.
    pub(crate) async fn detail_rich_impl(&self, item: &MediaItem) -> Result<DetailRich> {
        let mut path = item.detail_path.clone();
        if path.is_empty() {
            // Resolve detailPath via the mobile API when missing.
            if let Ok(d) = self.detail_mobile(&item.id, "").await {
                path = d.detail_path;
            }
        }
        if path.is_empty() {
            return Ok(DetailRich {
                item: item.clone(),
                dubs: Vec::new(),
                cast: Vec::new(),
            });
        }
        let root = self
            .h5_call(
                "GET",
                &format!("/wefeed-h5api-bff/detail?detailPath={path}"),
                None,
            )
            .await?;
        let sub = root
            .get("data")
            .and_then(Value::as_object)
            .and_then(|d| d.get("subject"))
            .and_then(Value::as_object);
        let Some(sub) = sub else {
            return Ok(DetailRich {
                item: item.clone(),
                dubs: Vec::new(),
                cast: Vec::new(),
            });
        };
        let rich = match parse_subject(sub, &item.backdrop, self.block_adult()) {
            Some(mut m) => {
                m.detail_path = path.clone();
                m
            }
            None => {
                let mut m = item.clone();
                m.detail_path = path.clone();
                m
            }
        };
        let mut dubs = Vec::new();
        for d in get_arr(sub.get("dubs")) {
            let sid = s(d, "subjectId");
            if sid.is_empty() {
                continue;
            }
            let lan = s(d, "lanName");
            dubs.push(Dub {
                name: if lan.is_empty() {
                    "Dub".to_string()
                } else {
                    lan
                },
                subject_id: sid,
                detail_path: s(d, "detailPath"),
            });
        }
        let mut cast = Vec::new();
        for c in get_arr(sub.get("staffList")) {
            let name = s(c, "name");
            if name.is_empty() {
                continue;
            }
            cast.push(CastMember {
                name,
                avatar_url: s(c, "avatarUrl"),
            });
        }
        cast.truncate(15);
        Ok(DetailRich {
            item: rich,
            dubs,
            cast,
        })
    }

    /// `detailPath` lookup by subjectId via the web detail endpoint (the
    /// plugin's unverified fallback).
    pub(crate) async fn h5_detail_path_by_id(&self, id: &str) -> Result<String> {
        let url = format!(
            "{}/wefeed-h5api-bff/detail?subjectId={id}",
            self.config.h5_base
        );
        let r = self
            .http
            .get(&url)
            .headers(self.h5_headers(None, false))
            .send()
            .await?;
        let body = r.text().await?;
        let root = try_json(&body);
        let data = root
            .as_ref()
            .and_then(|r| r.get("data"))
            .and_then(Value::as_object);
        Ok(data
            .and_then(|d| deep_str(d, "detailPath", 3))
            .unwrap_or_default())
    }

    /// Dub variants from the web detail endpoint: (lanName, subjectId, detailPath).
    async fn fetch_dubs(&self, detail_path: &str) -> Vec<(String, String, String)> {
        let url = format!(
            "{}/wefeed-h5api-bff/detail?detailPath={detail_path}",
            self.config.h5_base
        );
        let req = async {
            let r = self
                .http
                .get(&url)
                .headers(self.h5_headers(None, false))
                .send()
                .await?;
            let body = r.text().await?;
            let root = try_json(&body);
            let data = root
                .as_ref()
                .and_then(|r| r.get("data"))
                .and_then(Value::as_object);
            let subject = data
                .and_then(|d| d.get("subject"))
                .and_then(Value::as_object);
            let arr = get_arr(
                subject
                    .and_then(|s| s.get("dubs"))
                    .or_else(|| data.and_then(|d| d.get("dubs"))),
            );
            let mut out = Vec::new();
            for it in arr {
                let sid = s(it, "subjectId");
                if sid.is_empty() {
                    continue;
                }
                let lan = s(it, "lanName");
                out.push((
                    if lan.is_empty() {
                        "Dub".to_string()
                    } else {
                        lan
                    },
                    sid,
                    s(it, "detailPath"),
                ));
            }
            Ok::<Vec<_>, CoreError>(out)
        };
        req.await.unwrap_or_default()
    }

    // -------------------------------------------------------------- streams

    /// Resolve playable streams for `d` at season/episode across every dub
    /// variant, plus external subtitle tracks. See [`MovieBoxClient::streams`].
    pub(crate) async fn streams_impl(
        &self,
        d: &crate::models::Detail,
        se: i64,
        ep: i64,
        is_series: bool,
    ) -> Result<StreamsResult> {
        *self.last_note.lock().unwrap() = String::new();
        let mut subtitles: Vec<SubtitleLink> = Vec::new();
        let mut cap_keys: Vec<(String, String)> = Vec::new();

        let mut variants: Vec<(String, String, String)> =
            vec![("Original".to_string(), d.id.clone(), d.detail_path.clone())];
        if !d.detail_path.is_empty() {
            for (lan, sid, path) in self.fetch_dubs(&d.detail_path).await {
                variants.push((
                    lan,
                    sid,
                    if path.is_empty() {
                        d.detail_path.clone()
                    } else {
                        path
                    },
                ));
            }
        }
        // Movies: try se/ep 0/0 first, then 1/1 (the plugin stores "|1|1" for movies).
        let attempts: &[(i64, i64)] = if is_series {
            &[(se, ep)]
        } else {
            &[(0, 0), (1, 1)]
        };
        let mut out: Vec<StreamLink> = Vec::new();
        for (name, sid, path) in &variants {
            for (s, e) in attempts {
                let found = self
                    .play_once(d, name, sid, path, *s, *e, &mut subtitles, &mut cap_keys)
                    .await;
                if !found.is_empty() {
                    out.extend(found);
                    break;
                }
            }
        }
        if out.is_empty() {
            let mut msg = format!("No streams (se={se} ep={ep})");
            if d.detail_path.is_empty() {
                msg.push_str(&format!(" | no detailPath, {}", d.note));
            }
            let note = self.last_note.lock().unwrap().clone();
            if !note.is_empty() {
                msg.push_str(&format!(" | {note}"));
            }
            return Err(CoreError::Api(msg));
        }
        let _ = self.load_captions(&cap_keys, &mut subtitles).await;
        out.sort_by(|a, b| {
            let ao = if a.label.starts_with("Original") {
                0
            } else {
                1
            };
            let bo = if b.label.starts_with("Original") {
                0
            } else {
                1
            };
            ao.cmp(&bo).then(b.quality.cmp(&a.quality))
        });
        Ok(StreamsResult {
            links: out,
            subtitles,
        })
    }

    /// One `subject/play` attempt for one variant/episode.
    #[allow(clippy::too_many_arguments)]
    async fn play_once(
        &self,
        d: &crate::models::Detail,
        name: &str,
        sid: &str,
        path: &str,
        se: i64,
        ep: i64,
        subtitles: &mut Vec<SubtitleLink>,
        cap_keys: &mut Vec<(String, String)>,
    ) -> Vec<StreamLink> {
        let attempt = async {
            let url = format!(
                "{}/wefeed-h5api-bff/subject/play?subjectId={}&se={}&ep={}&detailPath={}&streamSignType=1&supportCodecs%5Bhevc%5D=1&supportCodecs%5Bh264%5D=1",
                d.h5_base, sid, se, ep, path
            );
            let referer = format!(
                "{}/movies/{}?id={}&type=/movie/detail&detailSe=&detailEp=&lang=en",
                d.h5_base, path, sid
            );
            let mut h = reqwest::header::HeaderMap::new();
            let ins = |h: &mut reqwest::header::HeaderMap, k: &str, v: String| {
                if let (Ok(n), Ok(val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(&v),
                ) {
                    h.insert(n, val);
                }
            };
            ins(&mut h, "user-agent", self.config.ua_mobile.clone());
            ins(&mut h, "referer", referer.clone());
            ins(&mut h, "accept", "application/json".into());
            ins(
                &mut h,
                "x-client-info",
                format!("{{\"timezone\":\"{}\"}}", self.config.timezone),
            );
            ins(&mut h, "x-request-lang", "en".into());
            ins(&mut h, "x-vip-restrict", "0".into());
            ins(&mut h, "x-no-high-risk-restrict", "0".into());
            ins(&mut h, "x-source", String::new());
            if let Some(t) = &d.h5_token {
                if !t.trim().is_empty() {
                    ins(&mut h, "authorization", format!("Bearer {t}"));
                }
            }
            let r = self.http.get(&url).headers(h).send().await?;
            let status = r.status().as_u16();
            let text = r.text().await?;
            let root = try_json(&text).unwrap_or_default();
            let data = root.get("data").and_then(Value::as_object);

            let empty = |k: &str| get_arr(data.and_then(|d| d.get(k))).is_empty();
            if empty("dash") && empty("hls") && empty("streams") {
                *self.last_note.lock().unwrap() =
                    format!("play HTTP {status}: {}", truncate(&text, 160));
            }
            // Inline subtitles on the play response.
            for key in ["captions", "subtitles", "subtitleList"] {
                for c in get_arr(data.and_then(|d| d.get(key))) {
                    let mut u = s(c, "url");
                    if u.is_empty() {
                        u = s(c, "subtitleUrl");
                    }
                    if u.is_empty() || subtitles.iter().any(|x| x.url == u) {
                        continue;
                    }
                    let mut lang = s(c, "lanName");
                    if lang.is_empty() {
                        lang = s(c, "language");
                    }
                    if lang.is_empty() {
                        lang = s(c, "lan");
                    }
                    let label = if lang.is_empty() {
                        format!("Subtitle {}", subtitles.len() + 1)
                    } else {
                        lang
                    };
                    subtitles.push(SubtitleLink { label, url: u });
                }
            }
            // The play response carries three arrays: dash, hls (adaptive, high
            // res) and streams (progressive).
            let mut out = Vec::new();
            let mut first_stream_id = String::new();
            for (key, fixed) in [
                ("dash", Some(StreamKind::Dash)),
                ("hls", Some(StreamKind::Hls)),
                ("streams", None),
            ] {
                for st in get_arr(data.and_then(|d| d.get(key))) {
                    let u = s(st, "url");
                    if u.is_empty() {
                        continue;
                    }
                    if first_stream_id.is_empty() {
                        first_stream_id = s(st, "id");
                    }
                    let fmt = s(st, "format").to_lowercase();
                    let kind = fixed.unwrap_or(if fmt.contains("dash") || u.contains(".mpd") {
                        StreamKind::Dash
                    } else if fmt.contains("hls") || u.contains(".m3u8") {
                        StreamKind::Hls
                    } else {
                        StreamKind::Mp4
                    });
                    // "resolutions" may list several ("1080,720,480") for
                    // adaptive manifests — take the first (highest) one.
                    let q = first_int(&s(st, "resolutions")).unwrap_or(0);
                    let cookie = s(st, "signCookie");
                    let sign_key = s(st, "signHeaderKey");
                    let mut headers = std::collections::BTreeMap::new();
                    if kind == StreamKind::Mp4 {
                        headers.insert("Referer".to_string(), format!("{}/", self.config.h5_base));
                        headers.insert("User-Agent".to_string(), self.config.ua_desktop.clone());
                    } else {
                        headers.insert("Origin".to_string(), d.h5_base.clone());
                        headers.insert("Referer".to_string(), referer.clone());
                        headers.insert("User-Agent".to_string(), self.config.ua_mobile.clone());
                        if !cookie.is_empty() {
                            headers.insert(
                                if sign_key.is_empty() {
                                    "X-MB-Token".to_string()
                                } else {
                                    sign_key.clone()
                                },
                                cookie,
                            );
                        }
                    }
                    let label = format!(
                        "{} · {} · {}",
                        name,
                        if q > 0 {
                            format!("{q}p")
                        } else {
                            "auto".to_string()
                        },
                        kind.as_str().to_uppercase()
                    );
                    out.push(StreamLink {
                        label,
                        quality: q,
                        url: u,
                        kind,
                        headers,
                    });
                }
            }
            if !out.is_empty() {
                cap_keys.push((
                    sid.to_string(),
                    if first_stream_id.is_empty() {
                        format!("{sid}|{se}|{ep}")
                    } else {
                        first_stream_id
                    },
                ));
            }
            Ok::<Vec<StreamLink>, CoreError>(out)
        };
        match attempt.await {
            Ok(v) => v,
            Err(e) => {
                *self.last_note.lock().unwrap() = format!("play error: {e}");
                Vec::new()
            }
        }
    }

    /// External subtitle tracks for the resolved (subjectId, streamId) pairs,
    /// from the mobile caption endpoints.
    async fn load_captions(
        &self,
        cap_keys: &[(String, String)],
        subtitles: &mut Vec<SubtitleLink>,
    ) {
        let mut seen = std::collections::HashSet::new();
        let keys: Vec<&(String, String)> = cap_keys
            .iter()
            .filter(|k| seen.insert((*k).clone()))
            .collect();
        for (sid, stream) in keys {
            for path in [
                format!("/wefeed-mobile-bff/subject-api/get-stream-captions?subjectId={sid}&streamId={stream}"),
                format!("/wefeed-mobile-bff/subject-api/get-ext-captions?subjectId={sid}&resourceId={stream}&episode=0"),
            ] {
                let Some(root) = self.mobile_get(&path).await else { continue };
                let data = root.get("data").and_then(Value::as_object);
                let ext = get_arr(data.and_then(|d| d.get("extCaptions")));
                let list = if !ext.is_empty() { ext } else { get_arr(data.and_then(|d| d.get("captions"))) };
                for c in list {
                    let u = s(c, "url");
                    if u.is_empty() || subtitles.iter().any(|x| x.url == u) {
                        continue;
                    }
                    let mut lang = s(c, "lanName");
                    if lang.is_empty() {
                        lang = s(c, "lan");
                    }
                    if lang.is_empty() {
                        lang = s(c, "language");
                    }
                    if lang.is_empty() {
                        lang = "Unknown".to_string();
                    }
                    let mut label = lang.clone();
                    let mut n = 2;
                    while subtitles.iter().any(|x| x.label == label) {
                        label = format!("{lang} ({n})");
                        n += 1;
                    }
                    subtitles.push(SubtitleLink { label, url: u });
                }
            }
        }
    }
}

/// Dart-style `s.length > n ? s.substring(0, n) : s` on char boundaries.
fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
