//! [`MovieBoxClient`] — the public facade of the core, mirroring the static
//! surface of the Dart `MovieBoxApi` class.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::Config;
use crate::error::{CoreError, Result};
use crate::models::{Detail, DetailRich, HomeSection, MediaItem, SearchResult, StreamsResult};
use crate::parse::type_label;

/// Async MovieBox client: home feed, search, detail, seasons, streams.
///
/// Cheap to clone conceptually — create one per app and reuse it; guest tokens
/// (mobile + web) are cached internally and refreshed automatically.
///
/// ```
/// use sparkbox_core::MovieBoxClient;
///
/// # async fn demo() -> sparkbox_core::Result<()> {
/// let client = MovieBoxClient::new();
/// client.set_block_adult(true);
/// let sections = client.home().await?;            // Vec<HomeSection>
/// let results = client.search_rich("avengers", 1, 20).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct MovieBoxClient {
    pub(crate) http: reqwest::Client,
    pub(crate) config: Config,
    pub(crate) device_id: String,
    block_adult: AtomicBool,
    /// Cached mobile guest token + fetch time (ms).
    pub(crate) guest: tokio::sync::Mutex<Option<(String, u64)>>,
    /// Cached web (h5) guest token.
    pub(crate) h5_guest: tokio::sync::Mutex<Option<String>>,
    /// Diagnostics of the last `streams()` attempt, used in its error text.
    pub(crate) last_note: std::sync::Mutex<String>,
}

impl Default for MovieBoxClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MovieBoxClient {
    /// Client with the default (compiled-in) endpoints and headers.
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    /// Client with a custom [`Config`] — override hosts, user agents or the
    /// signing key when the upstream changes, without touching code.
    pub fn with_config(config: Config) -> Self {
        // Modest transport timeouts so a dead endpoint fails over quickly
        // (the Dart client had none and could hang forever on bad networks).
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest::Client builds");
        Self {
            http,
            config,
            device_id: crate::signing::random_device_id(),
            block_adult: AtomicBool::new(true),
            guest: tokio::sync::Mutex::new(None),
            h5_guest: tokio::sync::Mutex::new(None),
            last_note: std::sync::Mutex::new(String::new()),
        }
    }

    /// Active [`Config`].
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// This client run's random device id (sent inside `x-client-info`).
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// "Block adult content" switch (default ON). Applies to every subject
    /// parsed after the change.
    pub fn set_block_adult(&self, blocked: bool) {
        self.block_adult.store(blocked, Ordering::SeqCst);
    }

    /// Current "Block adult content" state.
    pub fn block_adult(&self) -> bool {
        self.block_adult.load(Ordering::SeqCst)
    }

    /// `1 -> MOVIE, 2 -> SERIES, 6 -> VIDEO, _ -> MEDIA`.
    pub fn type_label(subject_type: i64) -> &'static str {
        type_label(subject_type)
    }

    /// Core version (`"0.1.0"`), so the app can log which core it runs.
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    // ------------------------------------------------------------------ home

    /// Home feed: mobile `tab-operating` first (region-locked to the app's
    /// market), h5 `/home` as a global fallback. A "Trending Now" row is
    /// prepended when trending resolves.
    pub async fn home(&self) -> Result<Vec<HomeSection>> {
        let server_sections = match self.home_mobile().await {
            Ok(s) => s,
            Err(_) => match self.home_h5().await {
                Ok(s) => s,
                Err(_) => return Err(CoreError::Api("home feed unavailable".into())),
            },
        };
        let trending = self.trending(20).await.unwrap_or_default();
        let mut out = Vec::new();
        if !trending.is_empty() {
            out.push(HomeSection::new("Trending Now", trending, true, false));
        }
        out.extend(server_sections.into_iter().filter(|s| !s.items.is_empty()));
        Ok(out)
    }

    // ---------------------------------------------------------------- search

    /// Word suggestions as-you-type (h5 search-suggest).
    pub async fn suggest(&self, query: &str) -> Result<Vec<String>> {
        self.suggest_impl(query).await
    }

    /// Rich, paginated search (h5). Falls back to the mobile search endpoint
    /// if the h5 call fails (e.g. token problems).
    pub async fn search_rich(&self, query: &str, page: i64, per_page: i64) -> Result<SearchResult> {
        match self.search_rich_h5(query, page, per_page).await {
            Ok(res) => Ok(res),
            Err(_) => {
                // Legacy fallback: basic items, no pagination.
                let legacy = self.search_mobile(query).await?;
                Ok(SearchResult {
                    items: legacy
                        .into_iter()
                        .map(|s| {
                            MediaItem::minimal(s.id, s.title, s.is_series)
                                .tap_detail_path(s.detail_path)
                        })
                        .collect(),
                    has_more: false,
                })
            }
        }
    }

    // ---------------------------------------------------------------- detail

    /// Full detail from the h5 endpoint: description, stills, dubs and cast.
    pub async fn detail_rich(&self, item: &MediaItem) -> Result<DetailRich> {
        self.detail_rich_impl(item).await
    }

    /// Build the play-flow [`Detail`] for a [`MediaItem`] (h5 path when
    /// `detail_path` is known, mobile `subject-api/get` otherwise).
    pub async fn play_detail(&self, item: &MediaItem) -> Result<Detail> {
        if !item.detail_path.is_empty() {
            return Ok(Detail {
                id: item.id.clone(),
                title: item.title.clone(),
                detail_path: item.detail_path.clone(),
                h5_base: self.config.h5_base.clone(),
                h5_token: self.h5_token(false).await?,
                note: String::new(),
            });
        }
        self.detail_mobile(item.id.as_str(), "").await
    }

    /// Seasons of a series (mobile season-info). Empty for movies.
    pub async fn seasons(&self, subject_id: &str) -> Result<Vec<crate::models::Season>> {
        self.seasons_impl(subject_id).await
    }

    // --------------------------------------------------------------- streams

    /// Resolve playable streams for `detail` at season/episode, across every
    /// dub variant, plus external subtitle tracks.
    ///
    /// Movies try `se/ep = 0/0` then `1/1`; series use the given numbers.
    /// Links are sorted "Original" variant first, then by quality desc.
    pub async fn streams(
        &self,
        d: &Detail,
        se: i64,
        ep: i64,
        is_series: bool,
    ) -> Result<StreamsResult> {
        self.streams_impl(d, se, ep, is_series).await
    }
}

/// Small builder helper used by the search fallback (keeps call sites tight).
trait TapDetailPath {
    fn tap_detail_path(self, path: String) -> Self;
}

impl TapDetailPath for MediaItem {
    fn tap_detail_path(mut self, path: String) -> Self {
        self.detail_path = path;
        self
    }
}
