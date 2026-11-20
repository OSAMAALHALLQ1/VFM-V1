//! Media metadata and embedded cover art via `lofty`.

use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;

use crate::error::Result;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: u32,
    pub duration_secs: u32,
    pub track_number: u32,
    pub cover: Option<Vec<u8>>,
}

/// Reads container/tag metadata. Missing tags simply yield empty fields, and
/// unreadable files return `Ok(Metadata::default())` so the UI never blocks on
/// a corrupt artifact.
pub fn read(path: &Path) -> Result<Metadata> {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return Ok(Metadata::default());
    };
    let mut out = Metadata {
        duration_secs: tagged.properties().duration().as_secs() as u32,
        ..Metadata::default()
    };
    let Some(tag) = tagged.primary_tag() else {
        return Ok(out);
    };
    out.title = tag.title().map(|s| s.to_string()).unwrap_or_default();
    out.artist = tag.artist().map(|s| s.to_string()).unwrap_or_default();
    out.album = tag.album().map(|s| s.to_string()).unwrap_or_default();
    out.album_artist = out.artist.clone();
    out.genre = tag.genre().map(|s| s.to_string()).unwrap_or_default();
    out.year = tag.year().unwrap_or(0);
    out.track_number = tag.track().unwrap_or(0);
    out.cover = tag.pictures().first().map(|p| p.data().to_vec());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() -> Result<()> {
        let meta = read(Path::new("does/not/exist.mp3"))?;
        assert_eq!(meta, Metadata::default());
        Ok(())
    }
}
