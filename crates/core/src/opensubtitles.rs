//! OpenSubtitles.com REST client with offline and rate-limit handling.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::Result;

pub const DEFAULT_BASE_URL: &str = "https://api.opensubtitles.com/api/v1";

#[derive(Debug, Clone, PartialEq)]
pub enum OpenSubtitlesError {
    Offline(String),
    RateLimited,
    Unauthorized,
    NotFound,
    Invalid(String),
}

impl std::fmt::Display for OpenSubtitlesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Offline(e) => write!(f, "offline: {e}"),
            Self::RateLimited => write!(f, "rate limited"),
            Self::Unauthorized => write!(f, "invalid API key or login"),
            Self::NotFound => write!(f, "no subtitles found"),
            Self::Invalid(e) => write!(f, "invalid response: {e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitleCandidate {
    pub file_id: i64,
    pub file_name: String,
    pub language: String,
    pub release_name: String,
    pub download_count: i64,
}

#[derive(Debug, Clone)]
pub struct Client {
    pub api_key: String,
    pub user_agent: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            user_agent: format!("player/{}", env!("CARGO_PKG_VERSION")),
            base_url: DEFAULT_BASE_URL.into(),
            timeout: Duration::from_secs(8),
        }
    }

    /// `query` is a human-readable movie/episode name; `languages` is a
    /// comma-separated list of language codes.
    pub fn search(
        &self,
        query: &str,
        languages: &str,
    ) -> std::result::Result<Vec<SubtitleCandidate>, OpenSubtitlesError> {
        let url = search_url(&self.base_url, query, languages);
        let response = self.get(&url)?;
        let body: SearchResponse = serde_json::from_str(&response)
            .map_err(|e| OpenSubtitlesError::Invalid(e.to_string()))?;
        Ok(body
            .data
            .into_iter()
            .map(|item| SubtitleCandidate {
                file_id: item.attributes.file_id,
                file_name: item.attributes.file_name,
                language: item.attributes.language,
                release_name: item.attributes.release,
                download_count: item.attributes.download_count,
            })
            .collect())
    }

    /// Requests a download link. The API key is sent in the header, not the URL.
    pub fn download_link(&self, file_id: i64) -> std::result::Result<String, OpenSubtitlesError> {
        let url = format!("{}/download", self.base_url.trim_end_matches('/'));
        let response = ureq::post(&url)
            .timeout(self.timeout)
            .set("Api-Key", &self.api_key)
            .set("User-Agent", &self.user_agent)
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({ "file_id": file_id }))
            .map_err(classify_http)?;
        let body: DownloadResponse = response
            .into_string()
            .and_then(|s| {
                serde_json::from_str(&s).map_err(|e| std::io::Error::other(e.to_string()))
            })
            .map_err(|e| OpenSubtitlesError::Invalid(e.to_string()))?;
        Ok(body.link)
    }

    fn get(&self, url: &str) -> std::result::Result<String, OpenSubtitlesError> {
        ureq::get(url)
            .timeout(self.timeout)
            .set("Api-Key", &self.api_key)
            .set("User-Agent", &self.user_agent)
            .call()
            .map_err(classify_http)?
            .into_string()
            .map_err(|e| OpenSubtitlesError::Offline(e.to_string()))
    }
}

fn classify_http(error: ureq::Error) -> OpenSubtitlesError {
    match error {
        ureq::Error::Status(code, _) if code == 401 || code == 403 => {
            OpenSubtitlesError::Unauthorized
        }
        ureq::Error::Status(404, _) => OpenSubtitlesError::NotFound,
        ureq::Error::Status(429, _) => OpenSubtitlesError::RateLimited,
        ureq::Error::Status(code, response) => {
            OpenSubtitlesError::Invalid(format!("HTTP {code}: {}", response.status_text()))
        }
        ureq::Error::Transport(t) => OpenSubtitlesError::Offline(t.to_string()),
    }
}

pub fn search_url(base: &str, query: &str, languages: &str) -> String {
    let encoded = query
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect::<String>();
    format!(
        "{}/subtitles?query={encoded}&languages={languages}",
        base.trim_end_matches('/')
    )
}

/// OpenSubtitles' 64 KiB head+tail MD5 hash.
pub fn hash_file(path: &Path) -> Result<String> {
    const CHUNK: u64 = 65_536;
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut bytes = Vec::with_capacity(CHUNK as usize * 2);
    let mut head = vec![0u8; CHUNK as usize];
    let read = file.read(&mut head)?;
    bytes.extend_from_slice(&head[..read]);
    if len > CHUNK {
        file.seek(SeekFrom::End(-(CHUNK as i64)))?;
        let mut tail = vec![0u8; CHUNK as usize];
        let read = file.read(&mut tail)?;
        bytes.extend_from_slice(&tail[..read]);
    }
    let digest = md5::compute(&bytes);
    Ok(format!("{digest:x}"))
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    attributes: SearchAttributes,
}

#[derive(Debug, Deserialize)]
struct SearchAttributes {
    #[serde(rename = "file_id")]
    file_id: i64,
    #[serde(rename = "file_name")]
    file_name: String,
    language: String,
    release: String,
    #[serde(rename = "download_count")]
    download_count: i64,
}

#[derive(Debug, Deserialize)]
struct DownloadResponse {
    link: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_encodes_query_and_languages() {
        let url = search_url(DEFAULT_BASE_URL, "The Wand S01E11", "en,ar");
        assert_eq!(
            url,
            "https://api.opensubtitles.com/api/v1/subtitles?query=The+Wand+S01E11&languages=en,ar"
        );
    }

    #[test]
    fn hash_is_stable_and_depends_on_content() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("os-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let a = dir.join("a.mp4");
        let b = dir.join("b.mp4");
        std::fs::write(&a, vec![7u8; 4096])?;
        std::fs::write(&b, vec![9u8; 4096])?;
        let ha = hash_file(&a)?;
        assert_eq!(ha, hash_file(&a)?);
        assert_ne!(ha, hash_file(&b)?);
        assert_eq!(ha.len(), 32);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
