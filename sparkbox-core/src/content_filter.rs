//! "Block adult content" heuristic, ported from
//! `lib/services/content_filter.dart`.
//!
//! The MovieBox API has no reliable adult flag, so items are matched on their
//! genre / title / description. The on/off switch itself lives with the app
//! ([`crate::client::MovieBoxClient::set_block_adult`]); this module only
//! holds the pure predicate.

use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};

use crate::models::MediaItem;

fn ci(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .expect("static regex compiles")
}

static GENRE: LazyLock<Regex> = LazyLock::new(|| {
    ci(
        r"\b(adult|erotic|erotica|porn|pornography|hentai|xxx|sexploitation|softcore|hardcore|18\+)\b",
    )
});

static TITLE: LazyLock<Regex> = LazyLock::new(|| {
    ci(
        r"(\bxxx\b|\b18\s?\+|\berotic|\bporn|\bhentai\b|\bsex\s?(tape|scene|slave|doll|club|tour)\b|\bnude\b|\buncensored\b|\bnaked\b|\bsoftcore\b|\bsexy\b|\bhorny\b|\bseduc)",
    )
});

static DESC: LazyLock<Regex> = LazyLock::new(|| {
    ci(
        r"\b(erotic|pornograph|explicit sex|sexual encounters|hardcore|softcore|nudity|adult film)\b",
    )
});

/// True when `item` matches the adult heuristic (regardless of the setting).
pub fn is_adult(item: &MediaItem) -> bool {
    if GENRE.is_match(&item.genre) {
        return true;
    }
    if TITLE.is_match(&item.title) {
        return true;
    }
    if !item.description.is_empty() && DESC.is_match(&item.description) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(genre: &str, title: &str, desc: &str) -> MediaItem {
        MediaItem {
            title: title.into(),
            genre: genre.into(),
            description: desc.into(),
            ..MediaItem::default()
        }
    }

    #[test]
    fn genre_matches() {
        assert!(is_adult(&item("Adult, Drama", "Some Title", "")));
        assert!(is_adult(&item("Erotica", "Some Title", "")));
        // Quirk kept bug-for-bug from the Dart regex: a bare "18+" does NOT
        // match (`\b` cannot follow `+` at a non-word char), only "18+…" with
        // a word char right after the plus does.
        assert!(!is_adult(&item("18+", "Some Title", "")));
        assert!(is_adult(&item("18+action", "Some Title", "")));
        assert!(is_adult(&item("Hardcore", "Some Title", "")));
        assert!(!is_adult(&item("Action, Thriller", "Some Title", "")));
    }

    #[test]
    fn title_matches() {
        assert!(is_adult(&item("", "Sexy Nights 2", "")));
        assert!(is_adult(&item("", "The XXX Tape", "")));
        assert!(is_adult(&item("", "Movie 18 + cut", "")));
        assert!(is_adult(&item("", "Uncensored Story", "")));
        assert!(!is_adult(&item("", "Inception", "")));
        // "sex tape" as two words with optional space
        assert!(is_adult(&item("", "A Sex Tape", "")));
    }

    #[test]
    fn description_matches() {
        assert!(is_adult(&item(
            "",
            "Innocent Title",
            "contains explicit sex scenes"
        )));
        assert!(is_adult(&item(
            "",
            "Innocent Title",
            "full nudity throughout"
        )));
        assert!(!is_adult(&item("", "Innocent Title", "a heist movie")));
        // empty description never matches
        assert!(!is_adult(&item("", "Innocent Title", "")));
    }
}
