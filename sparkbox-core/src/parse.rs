//! Tolerant parsing of upstream MovieBox JSON into core models, ported from
//! the `_arr` / `_obj` / `_str` / `_deepStr` / `parseSubject` helpers in
//! `lib/api/moviebox_api.dart`.

use serde_json::{Map, Value};

use crate::content_filter::is_adult;
use crate::models::{MediaItem, Season};

/// Parse a JSON object out of a response body (`_tryJson`).
pub fn try_json(text: &str) -> Option<Map<String, Value>> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.as_object().cloned()
}

/// Objects of an array value; non-objects are skipped (`_arr`).
pub fn get_arr(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

/// `o[k]` when it is an object (`_obj`).
pub fn get_obj<'a>(o: Option<&'a Value>, k: &str) -> Option<&'a Map<String, Value>> {
    o?.get(k)?.as_object()
}

/// `o[k][field]` as a string, `""` when missing (used for `cover.url` etc.).
pub fn sub_str(o: &Map<String, Value>, k: &str, field: &str) -> String {
    o.get(k)
        .and_then(Value::as_object)
        .and_then(|m| m.get(field))
        .map(value_to_string)
        .unwrap_or_default()
}

/// `o[k]?.toString() ?? ''` — any scalar becomes its string form.
pub fn s(o: &Map<String, Value>, k: &str) -> String {
    o.get(k).map(value_to_string).unwrap_or_default()
}

/// Dart `toString()` semantics for JSON scalars.
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// `(o[k] as num?)?.toInt()`.
pub fn as_i64(v: Option<&Value>) -> Option<i64> {
    let n = v?.as_number()?;
    n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))
}

/// Find a string field in this object or nested objects (arrays are skipped so
/// we never pick another title's value). A literal `"null"` counts as missing.
pub fn deep_str(o: &Map<String, Value>, key: &str, depth: u8) -> Option<String> {
    let direct = o.get(key).map(value_to_string).unwrap_or_default();
    if !direct.is_empty() && direct != "null" {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    for child in o.values() {
        if let Some(cm) = child.as_object() {
            if let Some(found) = deep_str(cm, key, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// First run of digits in `s` (`RegExp(r'\d+').firstMatch`), e.g. the highest
/// resolution out of `"1080,720,480"`.
pub fn first_int(s: &str) -> Option<i64> {
    let mut start: Option<usize> = None;
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if b.is_ascii_digit() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(st) = start {
            return s[st..i].parse().ok();
        }
    }
    start.and_then(|st| s[st..].parse().ok())
}

/// `1 -> MOVIE, 2 -> SERIES, 6 -> VIDEO, _ -> MEDIA`.
pub fn type_label(subject_type: i64) -> &'static str {
    match subject_type {
        1 => "MOVIE",
        2 => "SERIES",
        6 => "VIDEO",
        _ => "MEDIA",
    }
}

/// Parse one subject object (h5 shape) into a [`MediaItem`]; `None` when it has
/// no id/title or the content filter hides it.
pub fn parse_subject(
    o: &Map<String, Value>,
    backdrop_hint: &str,
    block_adult: bool,
) -> Option<MediaItem> {
    let id = s(o, "subjectId");
    if id.is_empty() {
        return None;
    }
    let title = s(o, "title");
    if title.is_empty() {
        return None;
    }
    let subject_type = as_i64(o.get("subjectType")).unwrap_or(1);
    let cover = sub_str(o, "cover", "url");
    let stills = sub_str(o, "stills", "url");
    let duration = as_i64(o.get("duration")).unwrap_or(0);
    let release = s(o, "releaseDate");
    let item = MediaItem {
        id,
        title,
        is_series: subject_type == 2,
        detail_path: s(o, "detailPath"),
        cover,
        backdrop: if !stills.is_empty() {
            stills
        } else {
            backdrop_hint.to_string()
        },
        rating: s(o, "imdbRatingValue"),
        year: release.chars().take(4).collect(),
        genre: s(o, "genre"),
        country: s(o, "countryName"),
        description: s(o, "description"),
        duration_min: (duration + 59) / 60,
        subject_type,
    };
    if block_adult && is_adult(&item) {
        None
    } else {
        Some(item)
    }
}

/// Parse one `season-info` entry: `se` (negative/missing → `None`), an explicit
/// episode list from `allEp` ("1,2,3") when present, else `1..=maxEp`.
pub fn parse_season(o: &Map<String, Value>) -> Option<Season> {
    let se = as_i64(o.get("se"))?;
    if se < 0 {
        return None;
    }
    let mut listed: Vec<i64> = s(o, "allEp")
        .split([',', ';', ' '])
        .filter_map(|t| t.trim().parse::<i64>().ok())
        .collect();
    listed.sort_unstable();
    listed.dedup();
    let max_ep = as_i64(o.get("maxEp")).unwrap_or(0);
    let episodes = if !listed.is_empty() {
        listed
    } else {
        (1..=max_ep.max(0)).collect()
    };
    if episodes.is_empty() {
        return None;
    }
    let max = *episodes.iter().max().unwrap();
    Some(Season {
        se,
        max_ep: max,
        episodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_to_string_scalars() {
        assert_eq!(value_to_string(&json!("x")), "x");
        assert_eq!(value_to_string(&json!(50020126)), "50020126");
        assert_eq!(
            value_to_string(&json!(4516404531735022304u64)),
            "4516404531735022304"
        );
        assert_eq!(value_to_string(&json!(true)), "true");
        assert_eq!(value_to_string(&Value::Null), "");
    }

    #[test]
    fn deep_str_finds_nested_and_skips_arrays() {
        let o = json!({
            "a": { "detailPath": "movie/inner" },
            "list": [ { "detailPath": "movie/array-item" } ],
        })
        .as_object()
        .unwrap()
        .clone();
        // nested object is found…
        assert_eq!(
            deep_str(&o, "detailPath", 3).as_deref(),
            Some("movie/inner")
        );
        // arrays are skipped, so if only the array has it we get None
        let o2 = json!({ "list": [ { "detailPath": "movie/array-item" } ] })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(deep_str(&o2, "detailPath", 3), None);
        // literal "null" counts as missing
        let o3 = json!({ "detailPath": Value::Null, "x": { "detailPath": "null" } })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(deep_str(&o3, "detailPath", 3), None);
    }

    #[test]
    fn first_int_extracts_leading_digits() {
        assert_eq!(first_int("1080,720,480"), Some(1080));
        assert_eq!(first_int("720"), Some(720));
        assert_eq!(first_int("auto"), None);
        assert_eq!(first_int(""), None);
        assert_eq!(first_int("a4k"), Some(4));
    }

    #[test]
    fn type_labels() {
        assert_eq!(type_label(1), "MOVIE");
        assert_eq!(type_label(2), "SERIES");
        assert_eq!(type_label(6), "VIDEO");
        assert_eq!(type_label(99), "MEDIA");
    }

    #[test]
    fn parse_subject_full() {
        let o = json!({
            "subjectId": "70612",
            "title": "Test Movie",
            "subjectType": 1,
            "cover": { "url": "https://img/cover.jpg" },
            "stills": { "url": "https://img/still.jpg" },
            "duration": 7262,
            "releaseDate": "2023-11-05",
            "detailPath": "movie/test-70612",
            "imdbRatingValue": "7.4",
            "genre": "Action, Adventure",
            "countryName": "United States",
            "description": "A test description.",
        })
        .as_object()
        .unwrap()
        .clone();
        let m = parse_subject(&o, "", false).unwrap();
        assert_eq!(m.id, "70612");
        assert!(!m.is_series);
        assert_eq!(m.cover, "https://img/cover.jpg");
        assert_eq!(m.backdrop, "https://img/still.jpg");
        assert_eq!(m.duration_min, 122); // (7262+59)/60
        assert_eq!(m.year, "2023");
        assert_eq!(m.rating, "7.4");
        assert_eq!(m.subject_type, 1);

        // backdrop falls back to the hint when stills is missing
        let mut o2 = o.clone();
        o2.remove("stills");
        let m2 = parse_subject(&o2, "https://img/hint.jpg", false).unwrap();
        assert_eq!(m2.backdrop, "https://img/hint.jpg");
    }

    #[test]
    fn parse_subject_requires_id_and_title() {
        let no_id = json!({ "title": "T" }).as_object().unwrap().clone();
        let no_title = json!({ "subjectId": "1" }).as_object().unwrap().clone();
        assert_eq!(parse_subject(&no_id, "", false), None);
        assert_eq!(parse_subject(&no_title, "", false), None);
    }

    #[test]
    fn parse_subject_respects_adult_filter() {
        let o = json!({ "subjectId": "1", "title": "Sexy Uncensored Nights", "genre": "Drama" })
            .as_object()
            .unwrap()
            .clone();
        assert!(parse_subject(&o, "", true).is_none());
        assert!(parse_subject(&o, "", false).is_some());
    }

    #[test]
    fn parse_season_all_ep_list_and_range() {
        // explicit list wins over maxEp
        let o = json!({ "se": 1, "allEp": "3,1,2,2", "maxEp": 9 })
            .as_object()
            .unwrap()
            .clone();
        let season = parse_season(&o).unwrap();
        assert_eq!(season.episodes, vec![1, 2, 3]);
        assert_eq!(season.max_ep, 3);
        // no list → 1..=maxEp
        let o2 = json!({ "se": 2, "allEp": "", "maxEp": 4 })
            .as_object()
            .unwrap()
            .clone();
        let s2 = parse_season(&o2).unwrap();
        assert_eq!(s2.episodes, vec![1, 2, 3, 4]);
        assert_eq!(s2.max_ep, 4);
        // negative / missing se, or no episodes at all
        assert_eq!(parse_season(json!({ "se": -1 }).as_object().unwrap()), None);
        assert_eq!(
            parse_season(json!({ "maxEp": 0, "allEp": "" }).as_object().unwrap()),
            None
        );
    }
}
