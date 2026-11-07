// Live smoke test against the MovieBox API (the Rust port of tool/api_test.dart).
// Run: cargo run -p sparkbox-core --example smoke
//
// Performs: home → suggest + search → detail rich → seasons (series) → streams
// (movie) with headers, printing a compact report. Purely read-only.

use sparkbox_core::{MediaItem, MovieBoxClient};

fn pretty(title: &str, body: impl std::fmt::Display) {
    println!("===== {title} =====");
    println!("{body}");
    println!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = MovieBoxClient::new();
    println!(
        "SparkBoxCore v{} · device id {} · block adult {}",
        sparkbox_core::VERSION,
        client.device_id(),
        client.block_adult()
    );
    println!();

    // 1. Home feed
    let home = client.home().await?;
    pretty(
        &format!("HOME ({} sections)", home.len()),
        home.iter()
            .map(|s| {
                format!(
                    "  [{}] {} — {} items",
                    if s.is_banner {
                        "BANNER"
                    } else if s.ranked {
                        "RANKED"
                    } else {
                        "ROW   "
                    },
                    s.title,
                    s.items.len()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let first = home
        .iter()
        .flat_map(|s| s.items.iter())
        .next()
        .ok_or("home feed had no items")?;

    // 2. Suggestions + search
    let words = client.suggest("avengers").await?;
    pretty("SUGGEST \"avengers\"", words.join(" | "));

    let results = client.search_rich("avengers", 1, 5).await?;
    pretty(
        &format!(
            "SEARCH \"avengers\" ({} items, hasMore={})",
            results.items.len(),
            results.has_more
        ),
        results
            .items
            .iter()
            .map(|m| {
                format!(
                    "  {} ({}) [{}] rating={}",
                    m.title,
                    m.year,
                    MovieBoxClient::type_label(m.subject_type),
                    m.rating
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // 3. Detail rich (dubs + cast) for a search hit
    let target = results
        .items
        .iter()
        .find(|m| !m.detail_path.is_empty())
        .unwrap_or(first);
    let rich = client.detail_rich(target).await?;
    let desc: String = rich.item.description.chars().take(100).collect();
    pretty(
        &format!("DETAIL RICH — {}", rich.item.title),
        format!(
            "  dubs: {}\n  cast: {}\n  desc: {}…",
            rich.dubs
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            rich.cast
                .iter()
                .take(5)
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            desc
        ),
    );

    // 4. Seasons (series) or streams (movie)
    if rich.item.is_series || rich.item.subject_type == 2 {
        let seasons = client.seasons(&rich.item.id).await?;
        pretty(
            "SEASONS",
            seasons
                .iter()
                .map(|s| {
                    format!(
                        "  Season {}: {} eps (max {})",
                        s.se,
                        s.episodes.len(),
                        s.max_ep
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    // 5. Play stream for a movie
    let mut movie: &MediaItem = target;
    if movie.is_series || movie.subject_type == 2 {
        movie = results
            .items
            .iter()
            .find(|m| !m.is_series && m.subject_type != 2)
            .unwrap_or(target);
    }
    let d = client.play_detail(movie).await?;
    let streams = client
        .streams(&d, 0, 0, movie.is_series || movie.subject_type == 2)
        .await?;
    pretty(
        &format!("STREAMS — {} ({} links)", movie.title, streams.links.len()),
        streams
            .links
            .iter()
            .map(|s| {
                format!(
                    "  {}  kind={}  {}",
                    s.label,
                    s.kind.as_str(),
                    &s.url[..s.url.len().min(72)]
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if let Some(link) = streams.links.first() {
        pretty(
            "STREAM HEADERS",
            link.headers
                .iter()
                .map(|(k, v)| {
                    format!(
                        "  {k}: {}{}",
                        &v[..v.len().min(60)],
                        if v.len() > 60 { "…" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !streams.subtitles.is_empty() {
        pretty(
            &format!("SUBTITLES ({} tracks)", streams.subtitles.len()),
            streams
                .subtitles
                .iter()
                .map(|s| format!("  {} → {}", s.label, &s.url[..s.url.len().min(64)]))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    Ok(())
}
