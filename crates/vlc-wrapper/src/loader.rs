//! Locating and loading the libVLC runtime (`libvlc.dll`, `libvlccore.dll`, `plugins/`).

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;

use crate::{VlcError, sys};

/// Environment variable that overrides runtime discovery (developer / CI use).
pub const LIBVLC_PATH_ENV: &str = "LIBVLC_PATH";

#[cfg(windows)]
const LIBVLC: &str = "libvlc.dll";
#[cfg(windows)]
const LIBVLCCORE: &str = "libvlccore.dll";
#[cfg(not(windows))]
const LIBVLC: &str = "libvlc.so.5";
#[cfg(not(windows))]
const LIBVLCCORE: &str = "libvlccore.so.9";

/// A loaded libVLC runtime. Cheap to share via [`Arc`]; every wrapper object
/// keeps one alive, so the function table can never outlive the DLLs.
pub struct LibVlc {
    api: sys::Api,
    dir: PathBuf,
    version: String,
    // Field order matters: `libvlc` is dropped (unloaded) before `libvlccore`.
    _libvlc: Library,
    _libvlccore: Library,
}

impl std::fmt::Debug for LibVlc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibVlc")
            .field("dir", &self.dir)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl LibVlc {
    /// Search order: `$LIBVLC_PATH`, `<exe>/vlc/`, `<exe>/`, `%ProgramFiles%\VideoLAN\VLC`.
    pub fn candidate_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(p) = std::env::var_os(LIBVLC_PATH_ENV).filter(|p| !p.is_empty()) {
            dirs.push(PathBuf::from(p));
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            dirs.push(exe_dir.join("vlc"));
            dirs.push(exe_dir);
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            dirs.push(PathBuf::from(pf).join("VideoLAN").join("VLC"));
        }
        dirs
    }

    /// Load the first complete runtime found in [`Self::candidate_dirs`].
    pub fn discover() -> Result<Arc<Self>, VlcError> {
        let searched = Self::candidate_dirs();
        let dir = searched
            .iter()
            .find(|d| d.join(LIBVLC).is_file() && d.join(LIBVLCCORE).is_file())
            .ok_or_else(|| VlcError::RuntimeNotFound {
                searched: searched.clone(),
            })?;
        Self::load_from(dir)
    }

    /// Load the runtime from an explicit directory.
    pub fn load_from(dir: &Path) -> Result<Arc<Self>, VlcError> {
        let dir = std::path::absolute(dir).map_err(|_| VlcError::RuntimeNotFound {
            searched: vec![dir.to_path_buf()],
        })?;
        if !dir.join("plugins").is_dir() {
            return Err(VlcError::PluginsNotFound(dir.join("plugins")));
        }
        let libvlccore = load_library(&dir.join(LIBVLCCORE))?;
        let libvlc = load_library(&dir.join(LIBVLC))?;

        // SAFETY: `libvlc` is a libvlc.dll; its major version is verified right
        // below before any other function from the table is called.
        let api = unsafe { sys::Api::resolve(&libvlc)? };
        // SAFETY: libvlc_get_version takes no arguments and returns a pointer
        // to a static NUL-terminated string.
        let version = unsafe { CStr::from_ptr((api.libvlc_get_version)()) }
            .to_string_lossy()
            .into_owned();
        if !version.starts_with("3.0.") {
            return Err(VlcError::UnsupportedVersion(version));
        }
        Ok(Arc::new(Self {
            api,
            dir,
            version,
            _libvlc: libvlc,
            _libvlccore: libvlccore,
        }))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Full version string, e.g. `3.0.24 Vetinari`.
    pub fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn api(&self) -> &sys::Api {
        &self.api
    }

    /// Last libVLC error message for the calling thread, if any.
    pub(crate) fn last_error(&self) -> String {
        // SAFETY: libvlc_errmsg returns NULL or a thread-local NUL-terminated
        // string that stays valid until the next libVLC call on this thread;
        // it is copied immediately.
        let msg = unsafe { (self.api.libvlc_errmsg)() };
        if msg.is_null() {
            "unknown libVLC error".to_owned()
        } else {
            // SAFETY: non-null and NUL-terminated per the libVLC contract above.
            unsafe { CStr::from_ptr(msg) }
                .to_string_lossy()
                .into_owned()
        }
    }
}

#[cfg(windows)]
fn load_library(path: &Path) -> Result<Library, VlcError> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library as WinLibrary,
    };
    // Dependencies resolve from the DLL's own directory and System32 only,
    // never from PATH or the CWD, so a stray libvlccore.dll cannot be picked up.
    // SAFETY: libVLC's DllMain only initialises its own globals.
    unsafe {
        WinLibrary::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .map(Library::from)
    .map_err(|source| VlcError::Load {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(windows))]
fn load_library(path: &Path) -> Result<Library, VlcError> {
    // SAFETY: libVLC's initialisers only touch its own globals.
    unsafe { Library::new(path) }.map_err(|source| VlcError::Load {
        path: path.to_path_buf(),
        source,
    })
}
