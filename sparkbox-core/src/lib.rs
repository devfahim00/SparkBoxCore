//! # SparkBoxCore
//!
//! Core MovieBox API client for [SparkBox](https://github.com/devfahim00/SparkBox),
//! extracted from the Flutter app (`RMB` repo, `flutter` branch) into its own
//! Rust workspace so the app and the protocol layer can evolve independently.
//!
//! Everything that talks to the outside world lives here:
//! - request signing ([`signing`]) and app headers,
//! - endpoint / user-agent configuration ([`config::Config`]),
//! - home feed, search, detail, seasons ([`MovieBoxClient`]),
//! - stream + subtitle resolution across dub variants,
//! - the adult-content heuristic ([`content_filter`]).
//!
//! JSON shapes are camelCase and byte-compatible with the Dart models
//! (`MediaItem.toMap()` ↔ [`models::MediaItem`]).
//!
//! ```no_run
//! # async fn demo() -> sparkbox_core::Result<()> {
//! use sparkbox_core::MovieBoxClient;
//!
//! let client = MovieBoxClient::new();
//! let sections = client.home().await?;                     // home feed
//! let hits = client.search_rich("avengers", 1, 20).await?; // search + pager
//! for item in &hits.items {
//!     let detail = client.play_detail(item).await?;
//!     let streams = client.streams(&detail, 1, 1, item.is_series).await?;
//!     println!("{} → {} links, {} subtitles",
//!              item.title, streams.links.len(), streams.subtitles.len());
//! }
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod content_filter;
pub mod error;
pub mod models;
pub mod parse;
pub mod signing;

mod client;
mod h5;
mod mobile;

pub use client::MovieBoxClient;
pub use config::Config;
pub use error::{CoreError, Result};
pub use models::{
    CastMember, Detail, DetailRich, Dub, HomeSection, MediaItem, SearchItem, SearchResult, Season,
    StreamKind, StreamLink, StreamsResult, SubtitleLink,
};

/// Version of this core build (Cargo package version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
