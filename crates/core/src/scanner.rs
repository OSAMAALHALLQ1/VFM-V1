//! Incremental, cancellable folder scanning for the library.
//!
//! Runs on a background thread owned by the caller. Every media file is
//! upserted by URI, so repeated scans only add new files and refresh metadata.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::brand::SUPPORTED_EXTENSIONS;
use crate::error::{CoreError, Result};
use crate::model::{MediaItem, MediaKind};
use crate::store::Store;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanStats {
    pub visited: usize,
    pub added: usize,
    pub skipped: usize,
    pub cancelled: bool,
}

/// Shared cancellation flag. Clone it onto a UI thread and call `cancel()`.
#[derive(Debug, Clone, Default)]
pub struct ScanHandle(Arc<AtomicBool>);

impl ScanHandle {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

fn kind_for(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
        return None;
    }
    Some(
        if matches!(
            ext.as_str(),
            "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" | "wma"
        ) {
            MediaKind::Audio
        } else {
            MediaKind::Video
        },
    )
}

/// Walks `root` iteratively (no recursion depth limit), upserts every supported
/// file and links it to the persisted folder. `on_progress` is called every 32
/// visited entries; it must not perform blocking work.
pub fn scan_folder(
    store: &Store,
    root: &Path,
    recursive: bool,
    handle: &ScanHandle,
    on_progress: impl Fn(usize),
) -> Result<ScanStats> {
    if !root.is_dir() {
        return Err(CoreError::Invalid(format!(
            "not a directory: {}",
            root.display()
        )));
    }
    let folder_id = store.add_folder(&root.to_string_lossy(), recursive)?;
    let mut stats = ScanStats::default();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                stats.skipped += 1;
                continue;
            }
        };
        for entry in entries.flatten() {
            if handle.is_cancelled() {
                stats.cancelled = true;
                return Ok(stats);
            }
            stats.visited += 1;
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
                continue;
            }
            let Some(kind) = kind_for(&path) else {
                stats.skipped += 1;
                continue;
            };
            let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
            let uri = path.to_string_lossy().into_owned();
            let title = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| uri.clone());
            let item = MediaItem {
                id: 0,
                uri,
                kind,
                title,
                duration_ms: 0,
                size_bytes: size,
                added_at: 0,
            };
            let id = store.upsert_media(&item)?;
            store.link_folder_media(folder_id, id)?;
            stats.added += 1;
            if stats.visited % 32 == 0 {
                on_progress(stats.visited);
            }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("player-scan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scans_supported_files_recursively() -> Result<()> {
        let root = temp_dir("recursive");
        fs::write(root.join("a.mp4"), b"x").unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested").join("b.mkv"), b"x").unwrap();
        fs::write(root.join("notes.txt"), b"x").unwrap();
        let store = Store::in_memory()?;
        let stats = scan_folder(&store, &root, true, &ScanHandle::default(), |_| {})?;
        assert_eq!(stats.added, 2);
        assert_eq!(stats.skipped, 1);
        assert_eq!(store.list_media(10, 0)?.len(), 2);
        fs::remove_dir_all(root).unwrap();
        Ok(())
    }

    #[test]
    fn non_recursive_scan_ignores_subdirectories() -> Result<()> {
        let root = temp_dir("shallow");
        fs::write(root.join("a.mp4"), b"x").unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested").join("b.mp4"), b"x").unwrap();
        let store = Store::in_memory()?;
        let stats = scan_folder(&store, &root, false, &ScanHandle::default(), |_| {})?;
        assert_eq!(stats.added, 1);
        fs::remove_dir_all(root).unwrap();
        Ok(())
    }

    #[test]
    fn rescan_is_incremental_and_cancel_stops_early() -> Result<()> {
        let root = temp_dir("cancel");
        for i in 0..80 {
            fs::write(root.join(format!("{i:03}.mp4")), b"x").unwrap();
        }
        let store = Store::in_memory()?;
        let first = scan_folder(&store, &root, false, &ScanHandle::default(), |_| {})?;
        assert_eq!(first.added, 80);
        let second = scan_folder(&store, &root, false, &ScanHandle::default(), |_| {})?;
        assert_eq!(second.added, 80);
        assert_eq!(store.list_media(200, 0)?.len(), 80);
        let handle = ScanHandle::default();
        handle.cancel();
        let cancelled = scan_folder(&store, &root, false, &handle, |_| {})?;
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.added, 0);
        fs::remove_dir_all(root).unwrap();
        Ok(())
    }
}
