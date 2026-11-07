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
    pub id: String,
    pub title: String,
    pub is_series: bool,
    pub detail_path: String,
    pub cover: String,
    pub backdrop: String,
    pub rating: String,
    pub year: String,
    pub genre: String,
    pub country: String,
    pub description: String,
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
    pub title: String,
    pub items: Vec<MediaItem>,
    /// Netflix-style numbered row.
    pub ranked: bool,
    /// Hero carousel source.
    pub is_banner: bool,
}

impl HomeSection {
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
    pub se: i64,
    pub max_ep: i64,
    pub episodes: Vec<i64>,
}

/// Play-flow detail: where to call the web (h5) play endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Detail {
    pub id: String,
    pub title: String,
    pub detail_path: String,
    pub h5_base: String,
    pub h5_token: Option<String>,
    /// Diagnostics appended to "no streams" errors.
    pub note: String,
}

/// An audio dub variant of a subject.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dub {
    pub name: String,
    pub subject_id: String,
    pub detail_path: String,
}

/// Cast member (name + avatar).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CastMember {
    pub name: String,
    pub avatar_url: String,
}

/// Stream container kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Hls,
    Dash,
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
    pub label: String,
    /// 0 = adaptive / unknown.
    pub quality: i64,
    pub url: String,
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
    pub item: MediaItem,
    pub dubs: Vec<Dub>,
    pub cast: Vec<CastMember>,
}

/// Lightweight search hit (legacy mobile endpoint).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchItem {
    pub id: String,
    pub title: String,
    pub is_series: bool,
    pub detail_path: String,
}

/// External subtitle track.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubtitleLink {
    pub label: String,
    pub url: String,
}

/// Paginated search outcome.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    pub items: Vec<MediaItem>,
    pub has_more: bool,
}

/// Everything [`MovieBoxClient::streams`] resolves: playable links + subtitles.
///
/// In the Flutter app the subtitle list arrived through a static side-channel
/// (`MovieBoxApi.subtitles`); here it is returned with the links so the core
/// stays stateless per call.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamsResult {
    pub links: Vec<StreamLink>,
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
