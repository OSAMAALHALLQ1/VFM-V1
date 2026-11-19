//! Typed playback events and the UI-facing player state machine.
//!
//! libVLC events arrive on a worker thread. The worker maps them to
//! [`PlaybackEvent`], applies them here, and publishes an immutable snapshot to
//! the UI. Nothing in this module touches libVLC or any OS API.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::TrackInfo;
use crate::settings::Settings;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Opening,
    Playing,
    Paused,
    Ended,
}

impl PlaybackState {
    pub fn needs_restart(self) -> bool {
        matches!(self, Self::Stopped | Self::Ended)
    }
}

/// Events the worker can apply to [`PlayerState`], in libVLC-independent form.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackEvent {
    Opening,
    Buffering(f32),
    Playing,
    Paused,
    Stopped,
    EndReached,
    EncounteredError,
    TimeChanged(Duration),
    PositionChanged(f32),
    LengthChanged(Duration),
    VoutCount(i32),
    Tracks {
        audio: Vec<TrackInfo>,
        video: Vec<TrackInfo>,
        subtitle: Vec<TrackInfo>,
    },
    Chapters {
        current: i32,
        count: i32,
    },
    VolumeChanged(i32),
    MuteChanged(bool),
    RateChanged(f32),
    AudioDelayChanged(i64),
    SubtitleDelayChanged(i64),
    AspectRatioChanged(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub status: PlaybackState,
    pub time: Duration,
    pub length: Option<Duration>,
    pub position: f32,
    pub buffering: f32,
    pub volume: i32,
    pub muted: bool,
    pub rate: f32,
    /// Microseconds, matching libVLC's delay API (`i64`).
    pub audio_delay_us: i64,
    pub subtitle_delay_us: i64,
    pub audio_tracks: Vec<TrackInfo>,
    pub video_tracks: Vec<TrackInfo>,
    pub subtitle_tracks: Vec<TrackInfo>,
    pub audio_track: i32,
    pub video_track: i32,
    pub subtitle_track: i32,
    pub chapter: i32,
    pub chapter_count: i32,
    pub vout_count: i32,
    pub aspect_ratio: Option<String>,
    /// `(A, B)` in playback time; `A < B` is required.
    pub ab_repeat: Option<(Duration, Duration)>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl PlayerState {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            status: PlaybackState::Stopped,
            time: Duration::ZERO,
            length: None,
            position: 0.0,
            buffering: 0.0,
            volume: settings.volume,
            muted: settings.muted,
            rate: settings.rate,
            audio_delay_us: 0,
            subtitle_delay_us: 0,
            audio_tracks: Vec::new(),
            video_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            audio_track: -1,
            video_track: -1,
            subtitle_track: -1,
            chapter: 0,
            chapter_count: 0,
            vout_count: 0,
            aspect_ratio: None,
            ab_repeat: None,
        }
    }

    /// Clears per-media data. Settings-derived values (volume, rate, A–B loop)
    /// survive so the next item inherits user preferences.
    pub fn reset_for_media(&mut self) {
        self.time = Duration::ZERO;
        self.length = None;
        self.position = 0.0;
        self.buffering = 0.0;
        self.audio_tracks.clear();
        self.video_tracks.clear();
        self.subtitle_tracks.clear();
        self.audio_track = -1;
        self.video_track = -1;
        self.subtitle_track = -1;
        self.chapter = 0;
        self.chapter_count = 0;
        self.vout_count = 0;
        self.aspect_ratio = None;
    }

    /// Applies one event. Returns `true` when any field actually changed.
    pub fn apply(&mut self, event: PlaybackEvent) -> bool {
        let before = self.clone();
        match event {
            PlaybackEvent::Opening => self.status = PlaybackState::Opening,
            PlaybackEvent::Buffering(v) => self.buffering = v.clamp(0.0, 1.0),
            PlaybackEvent::Playing => {
                self.status = PlaybackState::Playing;
                self.buffering = 1.0;
            }
            PlaybackEvent::Paused => self.status = PlaybackState::Paused,
            PlaybackEvent::Stopped => self.status = PlaybackState::Stopped,
            PlaybackEvent::EndReached => self.status = PlaybackState::Ended,
            PlaybackEvent::EncounteredError => self.status = PlaybackState::Stopped,
            PlaybackEvent::TimeChanged(t) => {
                if let Some(len) = self.length {
                    self.time = t.min(len);
                } else {
                    self.time = t;
                }
            }
            PlaybackEvent::PositionChanged(p) => self.position = p.clamp(0.0, 1.0),
            PlaybackEvent::LengthChanged(len) => {
                self.length = (len > Duration::ZERO).then_some(len);
                if let Some(len) = self.length {
                    self.time = self.time.min(len);
                }
            }
            PlaybackEvent::VoutCount(n) => self.vout_count = n,
            PlaybackEvent::Tracks {
                audio,
                video,
                subtitle,
            } => {
                self.audio_tracks = audio;
                self.video_tracks = video;
                self.subtitle_tracks = subtitle;
            }
            PlaybackEvent::Chapters { current, count } => {
                self.chapter = current;
                self.chapter_count = count;
            }
            PlaybackEvent::VolumeChanged(v) => self.volume = v.clamp(0, 200),
            PlaybackEvent::MuteChanged(m) => self.muted = m,
            PlaybackEvent::RateChanged(r) => {
                if r.is_finite() && r > 0.0 {
                    self.rate = r;
                }
            }
            PlaybackEvent::AudioDelayChanged(d) => self.audio_delay_us = d,
            PlaybackEvent::SubtitleDelayChanged(d) => self.subtitle_delay_us = d,
            PlaybackEvent::AspectRatioChanged(r) => self.aspect_ratio = r,
        }
        self != &before
    }

    /// Seeks back to A when playback has passed B. Used by the worker on each
    /// time-changed event; returns `None` when A–B repeat is inactive.
    pub fn ab_repeat_seek(&self) -> Option<Duration> {
        let (a, b) = self.ab_repeat?;
        (a < b && self.time >= b && self.length.is_some_and(|len| b <= len)).then_some(a)
    }

    pub fn set_ab_repeat(&mut self, a: Option<Duration>, b: Option<Duration>) {
        self.ab_repeat = match (a, b) {
            (Some(a), Some(b)) if a < b => Some((a, b)),
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_events_drive_status() {
        let mut s = PlayerState::default();
        assert_eq!(s.status, PlaybackState::Stopped);
        assert!(s.apply(PlaybackEvent::Opening));
        assert_eq!(s.status, PlaybackState::Opening);
        s.apply(PlaybackEvent::Playing);
        assert_eq!(s.status, PlaybackState::Playing);
        assert!(!s.apply(PlaybackEvent::Playing));
        s.apply(PlaybackEvent::Paused);
        assert_eq!(s.status, PlaybackState::Paused);
        s.apply(PlaybackEvent::EndReached);
        assert_eq!(s.status, PlaybackState::Ended);
        s.apply(PlaybackEvent::EncounteredError);
        assert_eq!(s.status, PlaybackState::Stopped);
    }

    #[test]
    fn time_is_clamped_to_a_known_length() {
        let mut s = PlayerState::default();
        s.apply(PlaybackEvent::LengthChanged(Duration::from_secs(100)));
        s.apply(PlaybackEvent::TimeChanged(Duration::from_secs(150)));
        assert_eq!(s.time, Duration::from_secs(100));
        s.apply(PlaybackEvent::PositionChanged(2.0));
        assert_eq!(s.position, 1.0);
        s.apply(PlaybackEvent::LengthChanged(Duration::ZERO));
        assert_eq!(s.length, None);
    }

    #[test]
    fn reset_keeps_user_settings_but_clears_media_data() {
        let mut s = PlayerState {
            volume: 42,
            ab_repeat: Some((Duration::from_secs(1), Duration::from_secs(2))),
            ..PlayerState::default()
        };
        s.apply(PlaybackEvent::TimeChanged(Duration::from_secs(90)));
        s.audio_tracks.push(TrackInfo {
            id: 1,
            name: "en".into(),
        });
        s.reset_for_media();
        assert_eq!(s.volume, 42);
        assert!(s.ab_repeat.is_some());
        assert_eq!(s.time, Duration::ZERO);
        assert!(s.audio_tracks.is_empty());
    }

    #[test]
    fn ab_repeat_only_fires_after_b() {
        let mut s = PlayerState::default();
        s.apply(PlaybackEvent::LengthChanged(Duration::from_secs(100)));
        s.set_ab_repeat(Some(Duration::from_secs(10)), Some(Duration::from_secs(20)));
        s.apply(PlaybackEvent::TimeChanged(Duration::from_secs(15)));
        assert_eq!(s.ab_repeat_seek(), None);
        s.apply(PlaybackEvent::TimeChanged(Duration::from_secs(21)));
        assert_eq!(s.ab_repeat_seek(), Some(Duration::from_secs(10)));
        s.set_ab_repeat(None, None);
        assert_eq!(s.ab_repeat_seek(), None);
    }

    #[test]
    fn inverted_ab_bounds_are_rejected() {
        let mut s = PlayerState::default();
        s.set_ab_repeat(Some(Duration::from_secs(20)), Some(Duration::from_secs(10)));
        assert_eq!(s.ab_repeat, None);
    }
}
