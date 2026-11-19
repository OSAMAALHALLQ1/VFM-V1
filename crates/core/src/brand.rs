//! The one and only place that defines product branding.
//!
//! No other module may hard-code the application name, executable name, app id
//! or file-association id. Phase 0 used a temporary `Player — Phase 0` title;
//! nothing here derives from the VLC product name or its cone logo.
//!
//! TODO(branding): the values below are placeholders. Replace them in this file
//! only, once the real product name is confirmed.

use std::path::PathBuf;

/// User-visible product name.
pub const APP_NAME: &str = "<APP_NAME>";
/// Reverse-DNS identifier used for settings, AppUserModelID and telemetry keys.
pub const APP_ID: &str = "dev.placeholder.player";
/// Executable name (without `.exe`).
pub const EXE_NAME: &str = "player";
/// Directory name under `%APPDATA%` used for settings and the SQLite database.
pub const CONFIG_DIR: &str = "PlayerApp";
/// ProgID used for Windows file associations.
pub const PROG_ID: &str = "PlayerApp.Media";
/// Media extensions this player registers for. Keep sorted.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "3gp", "aac", "avi", "flac", "m4a", "m4v", "mkv", "mov", "mp3", "mp4", "mpeg", "mpg", "ogg",
    "opus", "ts", "wav", "webm", "wma", "wmv",
];

/// Per-user configuration directory. Falls back to a relative directory when
/// the environment variable is absent (tests, non-Windows hosts).
pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join(CONFIG_DIR)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join(CONFIG_DIR)
    } else {
        PathBuf::from(CONFIG_DIR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branding_is_centralised_and_placeholder_is_explicit() {
        assert_eq!(APP_NAME, "<APP_NAME>");
        assert!(APP_ID.starts_with("dev.placeholder."));
        assert!(SUPPORTED_EXTENSIONS.contains(&"mp4"));
        assert!(SUPPORTED_EXTENSIONS.windows(2).all(|w| w[0] < w[1]));
    }
}
