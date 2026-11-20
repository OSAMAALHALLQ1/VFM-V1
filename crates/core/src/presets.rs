//! Named presets with auto-apply rules, persisted as JSON in SQLite.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::store::Store;
use crate::subtitles::SubtitlePosition;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitlePreset {
    pub delay_ms: i64,
    pub font_family: String,
    pub size: f32,
    pub relative_size: bool,
    pub color: String,
    pub outline_px: u32,
    pub position: SubtitlePosition,
    pub y: f32,
    pub default_track: i32,
    pub language: String,
}

impl Default for SubtitlePreset {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            font_family: "Inter".into(),
            size: 0.05,
            relative_size: true,
            color: "#FFFFFF".into(),
            outline_px: 2,
            position: SubtitlePosition::Bottom,
            y: 0.86,
            default_track: -1,
            language: "en".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioPreset {
    pub delay_ms: i64,
    pub default_track: i32,
    pub language: String,
    pub equalizer_preset: String,
    pub filters: Vec<String>,
}

impl Default for AudioPreset {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            default_track: -1,
            language: "en".into(),
            equalizer_preset: "Flat".into(),
            filters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoPreset {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub hue: i32,
    pub gamma: f32,
    pub zoom: f32,
    pub aspect_ratio: String,
}

impl Default for VideoPreset {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0,
            gamma: 1.0,
            zoom: 1.0,
            aspect_ratio: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackPreset {
    pub speed: f32,
    pub sleep_minutes: u32,
    pub show_subtitle_timeline: bool,
    pub waveform_seekbar: bool,
    pub metadata_panel: bool,
}

impl Default for PlaybackPreset {
    fn default() -> Self {
        Self {
            speed: 1.0,
            sleep_minutes: 0,
            show_subtitle_timeline: true,
            waveform_seekbar: false,
            metadata_panel: false,
        }
    }
}

/// Everything a named preset controls. `target` is a folder or show path:
/// when non-empty, the preset auto-applies to every episode under it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetPayload {
    pub target: String,
    pub subtitle: SubtitlePreset,
    pub audio: AudioPreset,
    pub video: VideoPreset,
    pub playback: PlaybackPreset,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Preset {
    pub id: i64,
    pub name: String,
    pub payload: PresetPayload,
}

/// Resolves the preset that should auto-apply to `uri`: the most specific
/// (longest) folder target that is a prefix of the media path.
pub fn auto_apply_for<'a>(uri: &str, presets: &'a [Preset]) -> Option<&'a Preset> {
    presets
        .iter()
        .filter(|p| !p.payload.target.is_empty() && uri.starts_with(&p.payload.target))
        .max_by_key(|p| p.payload.target.len())
}

pub fn load_all(store: &Store) -> Result<Vec<Preset>> {
    store
        .presets()?
        .into_iter()
        .map(|row| {
            let payload: PresetPayload = serde_json_like_parse(&row.payload)?;
            Ok(Preset {
                id: row.id,
                name: row.name,
                payload,
            })
        })
        .collect()
}

fn serde_json_like_parse(text: &str) -> Result<PresetPayload> {
    // serde_json is not a dependency of core; presets are stored as TOML,
    // which serde::Deserialize handles with the same struct definitions.
    toml::from_str(text).map_err(|_| CoreError::Invalid("corrupt preset payload".into()))
}

pub fn save(store: &Store, name: &str, payload: &PresetPayload) -> Result<i64> {
    if name.trim().is_empty() {
        return Err(CoreError::Invalid("preset name is empty".into()));
    }
    let text = toml::to_string(payload)
        .map_err(|_| CoreError::Invalid("cannot serialise preset".into()))?;
    store.save_preset(name, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_round_trips_through_toml() -> Result<()> {
        let store = Store::in_memory()?;
        let payload = PresetPayload {
            target: "D:/Shows/The Wand".into(),
            subtitle: SubtitlePreset {
                delay_ms: 250,
                ..SubtitlePreset::default()
            },
            video: VideoPreset {
                contrast: 1.2,
                ..VideoPreset::default()
            },
            ..PresetPayload::default()
        };
        let id = save(&store, "The Wand", &payload)?;
        let presets = load_all(&store)?;
        assert_eq!(presets[0].id, id);
        assert_eq!(presets[0].payload, payload);
        Ok(())
    }

    #[test]
    fn most_specific_folder_target_wins() {
        let a = Preset {
            name: "Shows".into(),
            payload: PresetPayload {
                target: "D:/Shows".into(),
                ..PresetPayload::default()
            },
            ..Preset::default()
        };
        let b = Preset {
            name: "The Wand".into(),
            payload: PresetPayload {
                target: "D:/Shows/The Wand".into(),
                ..PresetPayload::default()
            },
            ..Preset::default()
        };
        let presets = vec![a, b];
        let found = auto_apply_for("D:/Shows/The Wand/S01E02.mkv", &presets).unwrap();
        assert_eq!(found.name, "The Wand");
        assert!(auto_apply_for("D:/Movies/movie.mkv", &presets).is_none());
    }
}
