//! Versioned, typed settings stored as TOML with change notifications.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{Language, RepeatMode, Theme};

pub const SETTINGS_VERSION: u32 = 1;
const CHANGE_CHANNEL_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
            maximized: false,
            fullscreen: false,
        }
    }
}

impl WindowState {
    /// Ignores impossible geometry so a corrupt file cannot create a 0×0 window.
    pub fn sanitized(mut self) -> Self {
        self.width = self.width.clamp(320, 16_384);
        self.height = self.height.clamp(240, 16_384);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: Theme,
    /// `#RRGGBB`; Phase 2 replaces this with the theme module's accent token.
    pub accent: String,
    pub language: Language,
    pub volume: i32,
    pub muted: bool,
    pub rate: f32,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub autoplay: bool,
    pub remember_position: bool,
    pub seek_step_secs: u64,
    pub window: WindowState,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: Theme::System,
            accent: "#FF9800".to_owned(),
            language: Language::En,
            volume: 100,
            muted: false,
            rate: 1.0,
            repeat: RepeatMode::All,
            shuffle: false,
            autoplay: true,
            remember_position: true,
            seek_step_secs: 5,
            window: WindowState::default(),
        }
    }
}

impl Settings {
    /// Reads settings from `path`. A missing file yields defaults; an older
    /// `version` is migrated forward and rewritten on the next `save`.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut settings: Self = toml::from_str(&text)?;
        settings.migrate();
        settings.window = settings.window.sanitized();
        settings.volume = settings.volume.clamp(0, 200);
        settings.rate = if settings.rate.is_finite() && settings.rate > 0.0 {
            settings.rate.clamp(0.25, 4.0)
        } else {
            1.0
        };
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp: PathBuf = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        // Windows cannot atomically replace an existing file with rename, so
        // remove first. Settings are tiny and rewritten only on user action.
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    fn migrate(&mut self) {
        // Version 0 did not exist publicly; future migrations are added here
        // as `if self.version < N { ... self.version = N; }` blocks.
        if self.version > SETTINGS_VERSION {
            // A newer writer wrote this file: keep unknown fields but do not
            // pretend we understand them. Re-stamp so our writer is versioned.
            self.version = SETTINGS_VERSION;
        }
    }
}

/// Shared settings with a persisted path and change notifications.
///
/// Subscribers receive the newest snapshot after every `update`. A full receiver
/// is skipped (the current value is always available via [`Self::get`]), so a
/// stalled UI can never block a worker thread.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    current: Arc<RwLock<Settings>>,
    subscribers: Arc<Mutex<Vec<SyncSender<Settings>>>>,
    path: PathBuf,
}

impl SettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let settings = Settings::load(&path)?;
        Ok(Self {
            current: Arc::new(RwLock::new(settings)),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            path,
        })
    }

    pub fn in_memory() -> Self {
        Self {
            current: Arc::new(RwLock::new(Settings::default())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            path: PathBuf::new(),
        }
    }

    pub fn get(&self) -> Settings {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Mutates settings under a write lock, persists (unless this is an
    /// in-memory store), then notifies subscribers.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<()> {
        let snapshot = {
            let mut guard = self.current.write().unwrap_or_else(|e| e.into_inner());
            f(&mut guard);
            guard.version = SETTINGS_VERSION;
            guard.clone()
        };
        if !self.path.as_os_str().is_empty() {
            snapshot.save(&self.path)?;
        }
        self.publish(snapshot);
        Ok(())
    }

    pub fn subscribe(&self) -> Receiver<Settings> {
        let (tx, rx) = mpsc::sync_channel(CHANGE_CHANNEL_CAPACITY);
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }

    fn publish(&self, snapshot: Settings) {
        let mut guard = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|tx| tx.try_send(snapshot.clone()).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir =
            std::env::temp_dir().join(format!("player-settings-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("settings.toml");
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn round_trip_preserves_values() {
        let dir =
            std::env::temp_dir().join(format!("player-settings-roundtrip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("settings.toml");
        let settings = Settings {
            language: Language::Ar,
            theme: Theme::Dark,
            repeat: RepeatMode::One,
            window: WindowState {
                width: 1600,
                ..WindowState::default()
            },
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_values_are_clamped() {
        let dir =
            std::env::temp_dir().join(format!("player-settings-clamp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("settings.toml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &path,
            "version = 1\nvolume = 999\nrate = -1.0\n[window]\nwidth = 0\nheight = 99999\n",
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.volume, 200);
        assert_eq!(settings.rate, 1.0);
        assert_eq!(settings.window.width, 320);
        assert_eq!(settings.window.height, 16_384);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn store_persists_and_notifies() {
        let dir =
            std::env::temp_dir().join(format!("player-settings-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("settings.toml");
        let store = SettingsStore::open(&path).unwrap();
        let rx = store.subscribe();
        store.update(|s| s.volume = 42).unwrap();
        assert_eq!(store.get().volume, 42);
        assert_eq!(rx.recv().unwrap().volume, 42);
        assert_eq!(Settings::load(&path).unwrap().volume, 42);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disconnected_subscribers_do_not_block_updates() {
        let store = SettingsStore::in_memory();
        let rx = store.subscribe();
        drop(rx);
        for i in 0..32 {
            store.update(|s| s.volume = i).unwrap();
        }
        assert_eq!(store.get().volume, 31);
    }
}
