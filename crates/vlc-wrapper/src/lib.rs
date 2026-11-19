//! Safe wrapper over the subset of libVLC 3.0.x used by the player.
//!
//! # Thread safety
//! libVLC instances, media and players are internally locked and may be used
//! from any thread, so [`Instance`], [`Media`] and [`MediaPlayer`] are
//! `Send + Sync`. Two rules are enforced by construction:
//! * libVLC callbacks (video, events) never call back into libVLC; they only
//!   touch Rust-owned state or push into a channel.
//! * Callback state outlives the native player: [`MediaPlayer`]'s `Drop`
//!   stops and releases the player before freeing it.
//!
//! `MediaPlayer::stop` joins libVLC threads and can take tens of milliseconds;
//! the app calls it from a worker thread, never the UI thread.

mod loader;
mod sys;
pub mod video;

use std::ffi::{CStr, CString, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use loader::{LIBVLC_PATH_ENV, LibVlc};
pub use video::{FrameCounters, FrameLease, VideoFrames};

#[derive(Debug, thiserror::Error)]
pub enum VlcError {
    #[error("libVLC runtime not found (searched: {searched:?})")]
    RuntimeNotFound { searched: Vec<PathBuf> },
    #[error("libVLC plugins directory missing: {0}")]
    PluginsNotFound(PathBuf),
    #[error("failed to load {path}: {source}")]
    Load {
        path: PathBuf,
        source: libloading::Error,
    },
    #[error("libvlc.dll does not export {name}: {source}")]
    MissingSymbol {
        name: &'static str,
        source: libloading::Error,
    },
    #[error("unsupported libVLC version {0} (3.0.x required)")]
    UnsupportedVersion(String),
    #[error("libVLC initialisation failed: {0}")]
    Init(String),
    #[error("invalid media location: {0}")]
    InvalidLocation(String),
    #[error("libVLC call failed: {0}")]
    Call(String),
    #[error("a video sink is already installed on this player")]
    VideoSinkAlreadySet,
}

fn cstring(s: &str) -> Result<CString, VlcError> {
    CString::new(s).map_err(|_| VlcError::InvalidLocation(s.to_owned()))
}

/// A libVLC instance (`libvlc_instance_t`).
pub struct Instance {
    lib: Arc<LibVlc>,
    raw: *mut sys::libvlc_instance_t,
}

// SAFETY: libvlc_instance_t is reference counted and internally locked.
unsafe impl Send for Instance {}
// SAFETY: as above; all functions taking an instance are thread-safe.
unsafe impl Sync for Instance {}

impl Instance {
    /// `args` are libVLC command-line options, e.g. `--no-osd`.
    pub fn new(lib: Arc<LibVlc>, args: &[&str]) -> Result<Self, VlcError> {
        let owned = args
            .iter()
            .map(|a| cstring(a))
            .collect::<Result<Vec<_>, _>>()?;
        let ptrs: Vec<_> = owned.iter().map(|a| a.as_ptr()).collect();
        let argc =
            c_int::try_from(ptrs.len()).map_err(|_| VlcError::Init("too many args".into()))?;
        // SAFETY: `ptrs` holds `argc` valid NUL-terminated strings that outlive the call.
        let raw = unsafe { (lib.api().libvlc_new)(argc, ptrs.as_ptr()) };
        if raw.is_null() {
            return Err(VlcError::Init(lib.last_error()));
        }
        Ok(Self { lib, raw })
    }

    pub fn lib(&self) -> &Arc<LibVlc> {
        &self.lib
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: `raw` is a live instance owned by us; players/media hold their own refs.
        unsafe { (self.lib.api().libvlc_release)(self.raw) }
    }
}

/// Playback statistics accumulated by libVLC for one media.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MediaStats {
    pub decoded_video: u64,
    pub displayed_pictures: u64,
    pub lost_pictures: u64,
    pub input_bitrate_kbps: f32,
}

/// A media item (`libvlc_media_t`).
pub struct Media {
    lib: Arc<LibVlc>,
    raw: *mut sys::libvlc_media_t,
}

// SAFETY: libvlc_media_t is reference counted and internally locked.
unsafe impl Send for Media {}
// SAFETY: as above.
unsafe impl Sync for Media {}

impl Media {
    pub fn from_path(instance: &Instance, path: &Path) -> Result<Self, VlcError> {
        let s = path
            .to_str()
            .ok_or_else(|| VlcError::InvalidLocation(path.display().to_string()))?;
        let c = cstring(s)?;
        // SAFETY: live instance and NUL-terminated UTF-8 path.
        let raw = unsafe { (instance.lib.api().libvlc_media_new_path)(instance.raw, c.as_ptr()) };
        Self::wrap(instance, raw, s)
    }

    /// `mrl` is a URL such as `https://…/index.m3u8` or `rtsp://…`.
    pub fn from_location(instance: &Instance, mrl: &str) -> Result<Self, VlcError> {
        let c = cstring(mrl)?;
        // SAFETY: live instance and NUL-terminated MRL.
        let raw =
            unsafe { (instance.lib.api().libvlc_media_new_location)(instance.raw, c.as_ptr()) };
        Self::wrap(instance, raw, mrl)
    }

    fn wrap(
        instance: &Instance,
        raw: *mut sys::libvlc_media_t,
        what: &str,
    ) -> Result<Self, VlcError> {
        if raw.is_null() {
            return Err(VlcError::InvalidLocation(format!(
                "{what}: {}",
                instance.lib.last_error()
            )));
        }
        Ok(Self {
            lib: instance.lib.clone(),
            raw,
        })
    }

    /// Per-media option, e.g. `:avcodec-hw=none`.
    pub fn add_option(&self, option: &str) -> Result<(), VlcError> {
        let c = cstring(option)?;
        // SAFETY: live media and NUL-terminated option (copied by libVLC).
        unsafe { (self.lib.api().libvlc_media_add_option)(self.raw, c.as_ptr()) };
        Ok(())
    }

    pub fn stats(&self) -> Option<MediaStats> {
        let mut s = sys::libvlc_media_stats_t::default();
        // SAFETY: live media; `s` is a valid out-pointer of the declared layout.
        let ok = unsafe { (self.lib.api().libvlc_media_get_stats)(self.raw, &mut s) };
        let n = |v: c_int| u64::try_from(v).unwrap_or(0);
        (ok != 0).then(|| MediaStats {
            decoded_video: n(s.i_decoded_video),
            displayed_pictures: n(s.i_displayed_pictures),
            lost_pictures: n(s.i_lost_pictures),
            // libVLC reports bytes per microsecond.
            input_bitrate_kbps: s.f_input_bitrate * 8000.0,
        })
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        // SAFETY: we own one reference to `raw`.
        unsafe { (self.lib.api().libvlc_media_release)(self.raw) }
    }
}

/// Events forwarded from libVLC's threads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerEvent {
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
}

const EVENTS: [c_int; 11] = [
    sys::libvlc_MediaPlayerOpening,
    sys::libvlc_MediaPlayerBuffering,
    sys::libvlc_MediaPlayerPlaying,
    sys::libvlc_MediaPlayerPaused,
    sys::libvlc_MediaPlayerStopped,
    sys::libvlc_MediaPlayerEndReached,
    sys::libvlc_MediaPlayerEncounteredError,
    sys::libvlc_MediaPlayerTimeChanged,
    sys::libvlc_MediaPlayerPositionChanged,
    sys::libvlc_MediaPlayerLengthChanged,
    sys::libvlc_MediaPlayerVout,
];

fn ms(v: i64) -> Duration {
    Duration::from_millis(u64::try_from(v).unwrap_or(0))
}

unsafe extern "C" fn event_cb(event: *const sys::libvlc_event_t, data: *mut c_void) {
    // SAFETY: libVLC passes a valid event for the duration of the call, and
    // `data` is the `Sender` boxed in MediaPlayer, alive until after detach.
    let (ev, tx) = unsafe { (&*event, &*data.cast::<Sender<PlayerEvent>>()) };
    // SAFETY: each union member is read only for the event type that defines it.
    let mapped = unsafe {
        match ev.type_ {
            sys::libvlc_MediaPlayerOpening => PlayerEvent::Opening,
            sys::libvlc_MediaPlayerBuffering => PlayerEvent::Buffering(ev.u.buffering),
            sys::libvlc_MediaPlayerPlaying => PlayerEvent::Playing,
            sys::libvlc_MediaPlayerPaused => PlayerEvent::Paused,
            sys::libvlc_MediaPlayerStopped => PlayerEvent::Stopped,
            sys::libvlc_MediaPlayerEndReached => PlayerEvent::EndReached,
            sys::libvlc_MediaPlayerEncounteredError => PlayerEvent::EncounteredError,
            sys::libvlc_MediaPlayerTimeChanged => PlayerEvent::TimeChanged(ms(ev.u.time_changed)),
            sys::libvlc_MediaPlayerPositionChanged => {
                PlayerEvent::PositionChanged(ev.u.position_changed)
            }
            sys::libvlc_MediaPlayerLengthChanged => {
                PlayerEvent::LengthChanged(ms(ev.u.length_changed))
            }
            sys::libvlc_MediaPlayerVout => PlayerEvent::VoutCount(ev.u.vout_count),
            _ => return,
        }
    };
    // A disconnected receiver just means nobody listens any more.
    let _ = tx.send(mapped);
}

/// A media player (`libvlc_media_player_t`).
pub struct MediaPlayer {
    lib: Arc<LibVlc>,
    raw: *mut sys::libvlc_media_player_t,
    events_tx: Box<Sender<PlayerEvent>>,
    events_rx: Mutex<Option<Receiver<PlayerEvent>>>,
    video: Option<Box<video::VideoBridge>>,
    attached: usize,
}

// SAFETY: libvlc_media_player_t is internally locked; callback state is Send + Sync.
unsafe impl Send for MediaPlayer {}
// SAFETY: as above.
unsafe impl Sync for MediaPlayer {}

impl MediaPlayer {
    pub fn new(instance: &Instance) -> Result<Self, VlcError> {
        let api = instance.lib.api();
        // SAFETY: live instance.
        let raw = unsafe { (api.libvlc_media_player_new)(instance.raw) };
        if raw.is_null() {
            return Err(VlcError::Init(instance.lib.last_error()));
        }
        let (tx, rx) = mpsc::channel();
        let mut player = Self {
            lib: instance.lib.clone(),
            raw,
            events_tx: Box::new(tx),
            events_rx: Mutex::new(Some(rx)),
            video: None,
            attached: 0,
        };
        // SAFETY: live player; the manager lives as long as the player.
        let em = unsafe { (api.libvlc_media_player_event_manager)(raw) };
        for ty in EVENTS {
            // SAFETY: `events_tx` is boxed (stable address) and detached in Drop
            // before it is freed.
            let rc = unsafe { (api.libvlc_event_attach)(em, ty, event_cb, player.events_data()) };
            if rc != 0 {
                return Err(VlcError::Call(format!("libvlc_event_attach({ty:#x})")));
            }
            player.attached += 1;
        }
        Ok(player)
    }

    fn events_data(&self) -> *mut c_void {
        std::ptr::from_ref::<Sender<PlayerEvent>>(&self.events_tx)
            .cast_mut()
            .cast()
    }

    /// Takes the event receiver. There is exactly one; later calls return `None`.
    pub fn take_events(&self) -> Option<Receiver<PlayerEvent>> {
        self.events_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Install the frame buffers once, before the first `play`.
    pub fn set_video_frames(&mut self, frames: VideoFrames) -> Result<(), VlcError> {
        if self.video.is_some() {
            return Err(VlcError::VideoSinkAlreadySet);
        }
        self.stop();
        let bridge = video::VideoBridge::new(frames);
        video::install(self.lib.api(), self.raw, &bridge);
        self.video = Some(bridge);
        Ok(())
    }

    pub fn set_media(&self, media: &Media) {
        // SAFETY: live player and media; libVLC retains the media.
        unsafe { (self.lib.api().libvlc_media_player_set_media)(self.raw, media.raw) }
    }

    pub fn play(&self) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_media_player_play)(self.raw) };
        if rc == 0 {
            Ok(())
        } else {
            Err(VlcError::Call(self.lib.last_error()))
        }
    }

    pub fn toggle_pause(&self) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_pause)(self.raw) }
    }

    pub fn set_pause(&self, paused: bool) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_set_pause)(self.raw, c_int::from(paused)) }
    }

    /// Blocks until libVLC's input and output threads have stopped.
    pub fn stop(&self) {
        // SAFETY: live player; never called from a libVLC callback.
        unsafe { (self.lib.api().libvlc_media_player_stop)(self.raw) }
    }

    pub fn is_playing(&self) -> bool {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_is_playing)(self.raw) != 0 }
    }

    /// `None` when no media is loaded or the length is unknown (live streams).
    pub fn length(&self) -> Option<Duration> {
        // SAFETY: live player.
        let v = unsafe { (self.lib.api().libvlc_media_player_get_length)(self.raw) };
        (v > 0).then(|| ms(v))
    }

    pub fn time(&self) -> Option<Duration> {
        // SAFETY: live player.
        let v = unsafe { (self.lib.api().libvlc_media_player_get_time)(self.raw) };
        (v >= 0).then(|| ms(v))
    }

    pub fn set_time(&self, t: Duration) {
        let v = i64::try_from(t.as_millis()).unwrap_or(i64::MAX);
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_set_time)(self.raw, v) }
    }

    pub fn position(&self) -> f32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_get_position)(self.raw) }
    }

    pub fn set_position(&self, pos: f32) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_set_position)(self.raw, pos.clamp(0.0, 1.0)) }
    }

    pub fn set_rate(&self, rate: f32) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_media_player_set_rate)(self.raw, rate) };
        if rc == 0 {
            Ok(())
        } else {
            Err(VlcError::Call(format!("set_rate({rate})")))
        }
    }

    /// 0..=200 (percent, 100 = unity gain).
    pub fn volume(&self) -> i32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_audio_get_volume)(self.raw) }
    }

    pub fn set_volume(&self, volume: i32) {
        // SAFETY: live player. Fails harmlessly (-1) when no audio output exists yet.
        unsafe { (self.lib.api().libvlc_audio_set_volume)(self.raw, volume.clamp(0, 200)) };
    }

    pub fn is_muted(&self) -> bool {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_audio_get_mute)(self.raw) == 1 }
    }

    pub fn set_muted(&self, muted: bool) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_audio_set_mute)(self.raw, c_int::from(muted)) }
    }

    // ---- Tracks, delays, video controls ------------------------------------

    /// Copies a libVLC-owned linked list into Rust-owned values, then releases
    /// the list through libVLC's allocator.
    ///
    /// # Safety
    /// `head` must be a list returned by one of libVLC's `*_get_*_description`
    /// calls on this player, and must not be used afterwards.
    unsafe fn collect_tracks(
        api: &sys::Api,
        head: *mut sys::libvlc_track_description_t,
    ) -> Vec<Track> {
        let mut tracks = Vec::new();
        let mut node = head;
        while !node.is_null() {
            // SAFETY: libVLC keeps every node alive until the list is released.
            let current = unsafe { &*node };
            let name = if current.psz_name.is_null() {
                String::new()
            } else {
                // SAFETY: libVLC returns a NUL-terminated description string.
                unsafe { CStr::from_ptr(current.psz_name) }
                    .to_string_lossy()
                    .into_owned()
            };
            tracks.push(Track {
                id: current.i_id,
                name,
            });
            node = current.p_next;
        }
        // SAFETY: `head` is the owner pointer returned by libVLC.
        unsafe { (api.libvlc_track_description_list_release)(head) };
        tracks
    }

    pub fn audio_tracks(&self) -> Vec<Track> {
        // SAFETY: live player; the returned list is handed to `collect_tracks`.
        let head = unsafe { (self.lib.api().libvlc_audio_get_track_description)(self.raw) };
        // SAFETY: `head` came from the matching libVLC getter.
        unsafe { Self::collect_tracks(self.lib.api(), head) }
    }

    pub fn video_tracks(&self) -> Vec<Track> {
        // SAFETY: as above.
        let head = unsafe { (self.lib.api().libvlc_video_get_track_description)(self.raw) };
        // SAFETY: as above.
        unsafe { Self::collect_tracks(self.lib.api(), head) }
    }

    pub fn subtitle_tracks(&self) -> Vec<Track> {
        // SAFETY: as above.
        let head = unsafe { (self.lib.api().libvlc_video_get_spu_description)(self.raw) };
        // SAFETY: as above.
        unsafe { Self::collect_tracks(self.lib.api(), head) }
    }

    pub fn audio_track(&self) -> i32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_audio_get_track)(self.raw) }
    }

    pub fn set_audio_track(&self, id: i32) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_audio_set_track)(self.raw, id) };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call(format!("set_audio_track({id})")))
    }

    pub fn video_track(&self) -> i32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_video_get_track)(self.raw) }
    }

    pub fn set_video_track(&self, id: i32) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_video_set_track)(self.raw, id) };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call(format!("set_video_track({id})")))
    }

    pub fn subtitle_track(&self) -> i32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_video_get_spu)(self.raw) }
    }

    pub fn set_subtitle_track(&self, id: i32) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_video_set_spu)(self.raw, id) };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call(format!("set_subtitle_track({id})")))
    }

    /// Audio delay in microseconds (negative = audio earlier).
    pub fn audio_delay_us(&self) -> i64 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_audio_get_delay)(self.raw) }
    }

    pub fn set_audio_delay_us(&self, us: i64) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_audio_set_delay)(self.raw, us) };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call(format!("set_audio_delay({us})")))
    }

    /// Subtitle delay in microseconds (positive = subtitles later).
    pub fn subtitle_delay_us(&self) -> i64 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_video_get_spu_delay)(self.raw) }
    }

    pub fn set_subtitle_delay_us(&self, us: i64) -> Result<(), VlcError> {
        // SAFETY: live player.
        let rc = unsafe { (self.lib.api().libvlc_video_set_spu_delay)(self.raw, us) };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call(format!("set_subtitle_delay({us})")))
    }

    pub fn aspect_ratio(&self) -> Option<String> {
        // SAFETY: live player.
        let ptr = unsafe { (self.lib.api().libvlc_video_get_aspect_ratio)(self.raw) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: libVLC returned a NUL-terminated string owned by libVLC.
        let value = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: the string came from libVLC's allocator and is no longer used.
        unsafe { (self.lib.api().libvlc_free)(ptr.cast()) };
        Some(value)
    }

    pub fn set_aspect_ratio(&self, ratio: Option<&str>) {
        let api = self.lib.api();
        match ratio {
            Some(r) => {
                if let Ok(c) = cstring(r) {
                    // SAFETY: live player; NUL-terminated string borrowed for the call.
                    unsafe { (api.libvlc_video_set_aspect_ratio)(self.raw, c.as_ptr()) }
                }
            }
            None => {
                // SAFETY: NULL resets libVLC to the default aspect ratio.
                unsafe { (api.libvlc_video_set_aspect_ratio)(self.raw, std::ptr::null()) }
            }
        }
    }

    /// Displays the next frame while paused.
    pub fn next_frame(&self) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_next_frame)(self.raw) }
    }

    pub fn chapter(&self) -> i32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_get_chapter)(self.raw) }
    }

    pub fn chapter_count(&self) -> i32 {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_get_chapter_count)(self.raw) }
    }

    pub fn set_chapter(&self, chapter: i32) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_set_chapter)(self.raw, chapter) }
    }

    pub fn next_chapter(&self) {
        // SAFETY: live player.
        unsafe { (self.lib.api().libvlc_media_player_next_chapter)(self.raw) }
    }

    /// Saves a snapshot at native resolution; `0` width/height means native.
    pub fn snapshot(&self, path: &Path) -> Result<(), VlcError> {
        let c = cstring(&path.to_string_lossy())?;
        // SAFETY: live player and NUL-terminated path.
        let rc =
            unsafe { (self.lib.api().libvlc_video_take_snapshot)(self.raw, 0, c.as_ptr(), 0, 0) };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call("video_take_snapshot".into()))
    }

    /// Loads an external subtitle file. Works before and during playback.
    pub fn add_subtitle_file(&self, path: &Path) -> Result<(), VlcError> {
        let uri = file_uri(path)?;
        let c = cstring(&uri)?;
        // SAFETY: live player, valid enum value and NUL-terminated URI.
        let rc = unsafe {
            (self.lib.api().libvlc_media_player_add_slave)(
                self.raw,
                sys::libvlc_media_slave_type_subtitle,
                c.as_ptr(),
                true,
            )
        };
        (rc == 0)
            .then_some(())
            .ok_or_else(|| VlcError::Call("media_player_add_slave".into()))
    }
}

/// A named track reported by libVLC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: i32,
    pub name: String,
}

fn file_uri(path: &Path) -> Result<String, VlcError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| VlcError::InvalidLocation(e.to_string()))?
            .join(path)
    };
    let normalised = absolute.to_string_lossy().replace('\\', "/");
    Ok(format!("file:///{}", normalised.trim_start_matches('/')))
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        let api = self.lib.api();
        // SAFETY: live player. After stop, no vout exists, so no video callback
        // runs. Detaching takes the event manager lock, so no event callback is
        // running once it returns. Only then is callback state freed (field drop
        // after this function body).
        unsafe {
            (api.libvlc_media_player_stop)(self.raw);
            let em = (api.libvlc_media_player_event_manager)(self.raw);
            for ty in EVENTS.into_iter().take(self.attached) {
                (api.libvlc_event_detach)(em, ty, event_cb, self.events_data());
            }
            (api.libvlc_media_player_release)(self.raw);
        }
    }
}
