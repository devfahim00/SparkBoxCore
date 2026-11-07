//! Data model shared between the core and the app.
//!
//! JSON shapes are camelCase and byte-compatible with the Dart models in the
//! Flutter app (`lib/models.dart`): a `MediaItem.toMap()` from Dart decodes
//! into [`MediaItem`] here and vice-versa, so the app can keep its existing
//! model code.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Rich media item used by the home / search / player UIs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaItem {
    /// Subject id (`subjectId` upstream).
    pub id: String,
    /// Display title.
    pub title: String,
    /// `subject_type == 2`.
    pub is_series: bool,
    /// Web detail path; the play flow needs it.
    pub detail_path: String,
    /// Portrait cover image URL.
    pub cover: String,
    /// Landscape backdrop image URL.
    pub backdrop: String,
    /// IMDb rating as a string, e.g. `"7.4"`.
    pub rating: String,
    /// Release year (first 4 chars of the release date).
    pub year: String,
    /// Comma-separated genre names.
    pub genre: String,
    /// Country of origin.
    pub country: String,
    /// Synopsis.
    pub description: String,
    /// Duration in minutes (rounded up from seconds).
    pub duration_min: i64,
    /// 1 movie, 2 series, 6 video.
    pub subject_type: i64,
}

impl MediaItem {
    /// Minimal item (search fallback shape).
    pub fn minimal(id: impl Into<String>, title: impl Into<String>, is_series: bool) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            is_series,
            subject_type: if is_series { 2 } else { 1 },
            ..Self::default()
        }
    }
}

/// One row of the home feed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HomeSection {
    /// Row title (a section titled "banner" is renamed "Top Picks").
    pub title: String,
    /// Items in this row.
    pub items: Vec<MediaItem>,
    /// Netflix-style numbered row.
    pub ranked: bool,
    /// Hero carousel source.
    pub is_banner: bool,
}

impl HomeSection {
    /// Build a section from its parts.
    pub fn new(
        title: impl Into<String>,
        items: Vec<MediaItem>,
        ranked: bool,
        is_banner: bool,
    ) -> Self {
        Self {
            title: title.into(),
            items,
            ranked,
            is_banner,
        }
    }
}

/// A season with its episode numbers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Season {
    /// 1-based season number.
    pub se: i64,
    /// Highest episode number (derived from `episodes`).
    pub max_ep: i64,
    /// Episode numbers — the explicit `allEp` list when the server sends one,
    /// else `1..=maxEp`.
    pub episodes: Vec<i64>,
}

/// Play-flow detail: where to call the web (h5) play endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Detail {
    /// Subject id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Web detail path used by the play endpoint.
    pub detail_path: String,
    /// Origin the play endpoint lives on (`scheme://host`).
    pub h5_base: String,
    /// Bearer token for play / CDN calls.
    pub h5_token: Option<String>,
    /// Diagnostics appended to "no streams" errors.
    pub note: String,
}

/// An audio dub variant of a subject.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dub {
    /// Language name (`lanName` upstream, default "Dub").
    pub name: String,
    /// Subject id of this dub variant.
    pub subject_id: String,
    /// Web detail path of this dub variant.
    pub detail_path: String,
}

/// Cast member (name + avatar).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CastMember {
    /// Person name.
    pub name: String,
    /// Avatar image URL.
    pub avatar_url: String,
}

/// Stream container kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    /// HTTP Live Streaming (`.m3u8`).
    Hls,
    /// MPEG-DASH (`.mpd`).
    Dash,
    /// Progressive MP4 download.
    #[default]
    Mp4,
}

impl StreamKind {
    /// Lowercase name, matching the Dart enum's `.name`.
    pub fn as_str(self) -> &'static str {
        match self {
            StreamKind::Hls => "hls",
            StreamKind::Dash => "dash",
            StreamKind::Mp4 => "mp4",
        }
    }
}

/// One playable stream URL plus the headers the player must send.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamLink {
    /// `"{variant} · {quality}p|auto} · {KIND}"`, e.g. `"Original · 1080p · HLS"`.
    pub label: String,
    /// Vertical resolution (first entry of `resolutions`); 0 = adaptive/unknown.
    pub quality: i64,
    /// Manifest (mpd/m3u8) or progressive mp4 URL.
    pub url: String,
    /// Container kind.
    pub kind: StreamKind,
    /// Headers required by the CDN (Referer/Origin/UA/signed token).
    pub headers: BTreeMap<String, String>,
}

impl StreamLink {
    /// Audio-variant name = first segment of the label.
    pub fn variant(&self) -> &str {
        self.label.split(" · ").next().unwrap_or(&self.label)
    }
}

/// Full detail: item + dubs + cast.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailRich {
    /// The (merged, enriched) subject.
    pub item: MediaItem,
    /// Audio dub variants.
    pub dubs: Vec<Dub>,
    /// Cast members (at most 15).
    pub cast: Vec<CastMember>,
}

/// Lightweight search hit (legacy mobile endpoint).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchItem {
    /// Subject id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// `subject_type == 2`.
    pub is_series: bool,
    /// Web detail path (nested lookup).
    pub detail_path: String,
}

/// External subtitle track.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubtitleLink {
    /// Language label (may carry a `" (n)"` suffix when duplicated).
    pub label: String,
    /// Subtitle file URL.
    pub url: String,
}

/// Paginated search outcome.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    /// Hits for this page.
    pub items: Vec<MediaItem>,
    /// `pager.hasMore` from the web API; always `false` on the legacy fallback.
    pub has_more: bool,
}

/// Everything [`crate::MovieBoxClient::streams`] resolves: playable links + subtitles.
///
/// In the Flutter app the subtitle list arrived through a static side-channel
/// (`MovieBoxApi.subtitles`); here it is returned with the links so the core
/// stays stateless per call.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamsResult {
    /// Playable links, "Original" variant first then quality desc.
    pub links: Vec<StreamLink>,
    /// External subtitle tracks, de-duplicated by URL.
    pub subtitles: Vec<SubtitleLink>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_item_json_matches_dart_to_map() {
        let item = MediaItem {
            id: "123".into(),
            title: "Test".into(),
            is_series: true,
            detail_path: "movie/abc".into(),
            cover: "https://c/1.jpg".into(),
            backdrop: "https://c/2.jpg".into(),
            rating: "8.1".into(),
            year: "2024".into(),
            genre: "Action".into(),
            country: "IN".into(),
            description: "desc".into(),
            duration_min: 121,
            subject_type: 2,
        };
        let json = serde_json::to_string(&item).unwrap();
        for key in [
            "\"id\"",
            "\"title\"",
            "\"isSeries\"",
            "\"detailPath\"",
            "\"cover\"",
            "\"backdrop\"",
            "\"rating\"",
            "\"year\"",
            "\"genre\"",
            "\"country\"",
            "\"description\"",
            "\"durationMin\"",
            "\"subjectType\"",
        ] {
            assert!(json.contains(key), "missing key {key} in {json}");
        }
        let back: MediaItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item, back);
    }

    #[test]
    fn media_item_partial_json_gets_defaults() {
        // What a legacy-mobile search fallback produces: id/title/isSeries/subjectType only.
        let item: MediaItem =
            serde_json::from_str(r#"{"id":"9","title":"X","isSeries":false,"subjectType":1}"#)
                .unwrap();
        assert_eq!(item.id, "9");
        assert_eq!(item.title, "X");
        assert!(!item.is_series);
        assert_eq!(item.detail_path, "");
        assert_eq!(item.duration_min, 0);
    }

    #[test]
    fn stream_kind_serializes_lowercase_like_dart_enum_name() {
        assert_eq!(serde_json::to_string(&StreamKind::Hls).unwrap(), "\"hls\"");
        assert_eq!(
            serde_json::to_string(&StreamKind::Dash).unwrap(),
            "\"dash\""
        );
        assert_eq!(serde_json::to_string(&StreamKind::Mp4).unwrap(), "\"mp4\"");
        let k: StreamKind = serde_json::from_str("\"dash\"").unwrap();
        assert_eq!(k, StreamKind::Dash);
        assert_eq!(k.as_str(), "dash");
    }

    #[test]
    fn stream_link_variant_is_first_label_segment() {
        let link = StreamLink {
            label: "Original · 1080p · HLS".into(),
            quality: 1080,
            url: "https://x/hls.m3u8".into(),
            kind: StreamKind::Hls,
            headers: BTreeMap::new(),
        };
        assert_eq!(link.variant(), "Original");
    }
}
