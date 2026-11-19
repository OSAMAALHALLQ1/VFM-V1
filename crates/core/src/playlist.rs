//! Saved collections and the sorting/filtering rules used by library views.
//!
//! Persistence lives in [`crate::store::Store`]; this module holds the logic
//! that can be unit-tested without a database.

use std::cmp::Reverse;

use crate::model::MediaItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    Name,
    Date,
    Duration,
    Size,
    UnwatchedFirst,
    WatchedFirst,
}

/// Sorts in place. `is_watched` receives a media id so the caller can use the
/// watch-progress table without coupling this module to SQLite.
pub fn sort_items(items: &mut [MediaItem], mode: SortMode, is_watched: impl Fn(i64) -> bool) {
    match mode {
        SortMode::Name => items.sort_by(|a, b| a.title.cmp(&b.title)),
        SortMode::Date => items.sort_by_key(|m| Reverse(m.added_at)),
        SortMode::Duration => items.sort_by_key(|m| Reverse(m.duration_ms)),
        SortMode::Size => items.sort_by_key(|m| Reverse(m.size_bytes)),
        SortMode::UnwatchedFirst => items.sort_by_key(|m| is_watched(m.id)),
        SortMode::WatchedFirst => items.sort_by_key(|m| Reverse(is_watched(m.id))),
    }
}

/// Case-insensitive substring search over title and URI.
pub fn filter_items(items: &[MediaItem], query: &str) -> Vec<MediaItem> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|m| {
            m.title.to_lowercase().contains(&needle) || m.uri.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, title: &str, duration: i64, size: i64) -> MediaItem {
        MediaItem {
            id,
            uri: format!("{title}.mp4"),
            kind: crate::model::MediaKind::Video,
            title: title.into(),
            duration_ms: duration,
            size_bytes: size,
            added_at: id,
        }
    }

    #[test]
    fn sorts_by_each_mode() {
        let mut items = vec![
            item(1, "b", 20, 30),
            item(2, "a", 10, 40),
            item(3, "c", 30, 20),
        ];
        sort_items(&mut items, SortMode::Name, |_| false);
        assert_eq!(items[0].title, "a");
        sort_items(&mut items, SortMode::Duration, |_| false);
        assert_eq!(items[0].title, "c");
        sort_items(&mut items, SortMode::Size, |_| false);
        assert_eq!(items[0].title, "a");
        sort_items(&mut items, SortMode::Date, |_| false);
        assert_eq!(items[0].title, "c");
        sort_items(&mut items, SortMode::UnwatchedFirst, |id| id == 2);
        assert_eq!(items[0].id, 3);
        sort_items(&mut items, SortMode::WatchedFirst, |id| id == 2);
        assert_eq!(items[0].id, 2);
    }

    #[test]
    fn search_matches_title_and_uri_case_insensitively() {
        let items = vec![item(1, "Summer Trip", 0, 0), item(2, "Winter", 0, 0)];
        assert_eq!(filter_items(&items, "summer").len(), 1);
        assert_eq!(filter_items(&items, "WINTER.MP4").len(), 1);
        assert_eq!(filter_items(&items, "  ").len(), 2);
    }
}
