//! Data types shared by the library, queue, store and player state.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    #[default]
    Video,
    Audio,
    Stream,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Stream => "stream",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "audio" => Self::Audio,
            "stream" => Self::Stream,
            _ => Self::Video,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    #[default]
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Ar,
}

/// One row of the `media_items` table. `uri` is the stable identity used by the
/// library, queue and resume bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: i64,
    pub uri: String,
    pub kind: MediaKind,
    pub title: String,
    pub duration_ms: i64,
    pub size_bytes: i64,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchProgress {
    pub media_id: i64,
    pub position_ms: i64,
    pub completed: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub media_id: i64,
    pub position_ms: i64,
    pub label: String,
    pub created_at: i64,
}

/// A named collection of settings; the payload is a JSON object owned by the
/// feature that defines the preset (Phase 6), stored verbatim here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub id: i64,
    pub name: String,
    pub payload: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackInfo {
    /// libVLC track id;`-1` means "disabled" and `-2` is libVLC's "unset" value.
    pub id: i32,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_kind_round_trips_through_storage_string() {
        for kind in [MediaKind::Video, MediaKind::Audio, MediaKind::Stream] {
            assert_eq!(MediaKind::parse(kind.as_str()), kind);
        }
        assert_eq!(MediaKind::parse("unknown"), MediaKind::Video);
    }
}
