//! Shared error type for the platform-independent core.

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("settings parse error: {0}")]
    SettingsParse(#[from] toml::de::Error),
    #[error("settings write error: {0}")]
    SettingsWrite(#[from] toml::ser::Error),
    #[error("invalid data: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
