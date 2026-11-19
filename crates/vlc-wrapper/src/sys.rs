//! Raw libVLC 3.0.x C ABI, resolved at runtime from `libvlc.dll`.
//!
//! Every declaration here was checked against the `3.0.x` branch headers
//! (`include/vlc/libvlc*.h`). Nothing in this module is safe to call directly;
//! the safe API lives in the parent crate.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::ffi::{c_char, c_float, c_int, c_uint, c_void};

use libloading::Library;

use crate::VlcError;

macro_rules! opaque {
    ($($name:ident),*) => {$(
        #[repr(C)]
        pub struct $name {
            _private: [u8; 0],
        }
    )*};
}

opaque!(
    libvlc_instance_t,
    libvlc_media_t,
    libvlc_media_player_t,
    libvlc_event_manager_t
);

pub type libvlc_time_t = i64;
pub type libvlc_event_type_t = c_int;
/// `libvlc_media_slave_type_t` (a C `enum`, i.e. `c_int` on x64).
pub const libvlc_media_slave_type_subtitle: c_int = 0;

/// `libvlc_track_description_t`: a linked list owned by libVLC, released with
/// `libvlc_track_description_list_release`.
#[repr(C)]
pub struct libvlc_track_description_t {
    pub i_id: c_int,
    pub psz_name: *mut c_char,
    pub p_next: *mut libvlc_track_description_t,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct libvlc_media_stats_t {
    pub i_read_bytes: c_int,
    pub f_input_bitrate: c_float,
    pub i_demux_read_bytes: c_int,
    pub f_demux_bitrate: c_float,
    pub i_demux_corrupted: c_int,
    pub i_demux_discontinuity: c_int,
    pub i_decoded_video: c_int,
    pub i_decoded_audio: c_int,
    pub i_displayed_pictures: c_int,
    pub i_lost_pictures: c_int,
    pub i_played_abuffers: c_int,
    pub i_lost_abuffers: c_int,
    pub i_sent_packets: c_int,
    pub i_sent_bytes: c_int,
    pub f_send_bitrate: c_float,
}

/// Only the union members this crate reads are declared. The real union is
/// larger, but we only ever read through a pointer provided by libVLC, so
/// a smaller declaration is sound.
#[repr(C)]
#[derive(Clone, Copy)]
pub union libvlc_event_u {
    pub time_changed: i64,
    pub length_changed: i64,
    pub position_changed: c_float,
    pub buffering: c_float,
    pub vout_count: c_int,
}

#[repr(C)]
pub struct libvlc_event_t {
    pub type_: c_int,
    pub p_obj: *mut c_void,
    pub u: libvlc_event_u,
}

pub const libvlc_MediaPlayerOpening: c_int = 0x102;
pub const libvlc_MediaPlayerBuffering: c_int = 0x103;
pub const libvlc_MediaPlayerPlaying: c_int = 0x104;
pub const libvlc_MediaPlayerPaused: c_int = 0x105;
pub const libvlc_MediaPlayerStopped: c_int = 0x106;
pub const libvlc_MediaPlayerEndReached: c_int = 0x109;
pub const libvlc_MediaPlayerEncounteredError: c_int = 0x10A;
pub const libvlc_MediaPlayerTimeChanged: c_int = 0x10B;
pub const libvlc_MediaPlayerPositionChanged: c_int = 0x10C;
pub const libvlc_MediaPlayerLengthChanged: c_int = 0x111;
pub const libvlc_MediaPlayerVout: c_int = 0x112;

pub type libvlc_video_lock_cb =
    unsafe extern "C" fn(opaque: *mut c_void, planes: *mut *mut c_void) -> *mut c_void;
pub type libvlc_video_unlock_cb =
    unsafe extern "C" fn(opaque: *mut c_void, picture: *mut c_void, planes: *const *mut c_void);
pub type libvlc_video_display_cb = unsafe extern "C" fn(opaque: *mut c_void, picture: *mut c_void);
pub type libvlc_video_format_cb = unsafe extern "C" fn(
    opaque: *mut *mut c_void,
    chroma: *mut c_char,
    width: *mut c_uint,
    height: *mut c_uint,
    pitches: *mut c_uint,
    lines: *mut c_uint,
) -> c_uint;
pub type libvlc_video_cleanup_cb = unsafe extern "C" fn(opaque: *mut c_void);
pub type libvlc_callback_t = unsafe extern "C" fn(event: *const libvlc_event_t, data: *mut c_void);

macro_rules! api {
    ($( fn $name:ident($($arg:ty),* $(,)?) $(-> $ret:ty)?; )*) => {
        /// Function table resolved from `libvlc.dll`.
        ///
        /// Invariant: the pointers are only valid while the `Library` they were
        /// resolved from is loaded. [`crate::LibVlc`] owns both and never hands
        /// out the table without the library.
        #[allow(non_snake_case)]
        pub struct Api {
            $( pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?, )*
        }

        impl Api {
            /// # Safety
            /// `lib` must be a libVLC 3.x `libvlc.dll`, so that each exported
            /// symbol has exactly the signature declared above.
            pub unsafe fn resolve(lib: &Library) -> Result<Self, VlcError> {
                Ok(Self {
                    $( $name: {
                        // SAFETY: the caller guarantees the symbol has this signature.
                        let sym = unsafe {
                            lib.get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(stringify!($name))
                        }
                        .map_err(|source| VlcError::MissingSymbol { name: stringify!($name), source })?;
                        *sym
                    }, )*
                })
            }
        }
    };
}

api! {
    fn libvlc_new(c_int, *const *const c_char) -> *mut libvlc_instance_t;
    fn libvlc_release(*mut libvlc_instance_t);
    fn libvlc_get_version() -> *const c_char;
    fn libvlc_errmsg() -> *const c_char;

    fn libvlc_event_attach(*mut libvlc_event_manager_t, libvlc_event_type_t, libvlc_callback_t, *mut c_void) -> c_int;
    fn libvlc_event_detach(*mut libvlc_event_manager_t, libvlc_event_type_t, libvlc_callback_t, *mut c_void);

    fn libvlc_media_new_path(*mut libvlc_instance_t, *const c_char) -> *mut libvlc_media_t;
    fn libvlc_media_new_location(*mut libvlc_instance_t, *const c_char) -> *mut libvlc_media_t;
    fn libvlc_media_add_option(*mut libvlc_media_t, *const c_char);
    fn libvlc_media_release(*mut libvlc_media_t);
    fn libvlc_media_get_stats(*mut libvlc_media_t, *mut libvlc_media_stats_t) -> c_int;
    fn libvlc_media_player_add_slave(*mut libvlc_media_player_t, c_int, *const c_char, bool) -> c_int;

    fn libvlc_media_player_new(*mut libvlc_instance_t) -> *mut libvlc_media_player_t;
    fn libvlc_media_player_release(*mut libvlc_media_player_t);
    fn libvlc_media_player_set_media(*mut libvlc_media_player_t, *mut libvlc_media_t);
    fn libvlc_media_player_event_manager(*mut libvlc_media_player_t) -> *mut libvlc_event_manager_t;
    fn libvlc_media_player_is_playing(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_media_player_play(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_media_player_set_pause(*mut libvlc_media_player_t, c_int);
    fn libvlc_media_player_pause(*mut libvlc_media_player_t);
    fn libvlc_media_player_stop(*mut libvlc_media_player_t);
    fn libvlc_media_player_get_length(*mut libvlc_media_player_t) -> libvlc_time_t;
    fn libvlc_media_player_get_time(*mut libvlc_media_player_t) -> libvlc_time_t;
    fn libvlc_media_player_set_time(*mut libvlc_media_player_t, libvlc_time_t);
    fn libvlc_media_player_get_position(*mut libvlc_media_player_t) -> c_float;
    fn libvlc_media_player_set_position(*mut libvlc_media_player_t, c_float);
    fn libvlc_media_player_set_rate(*mut libvlc_media_player_t, c_float) -> c_int;
    fn libvlc_media_player_next_frame(*mut libvlc_media_player_t);
    fn libvlc_media_player_get_chapter(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_media_player_get_chapter_count(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_media_player_set_chapter(*mut libvlc_media_player_t, c_int);
    fn libvlc_media_player_next_chapter(*mut libvlc_media_player_t);
    fn libvlc_free(*mut c_void);
    fn libvlc_track_description_list_release(*mut libvlc_track_description_t);

    fn libvlc_audio_get_track_count(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_audio_get_track_description(*mut libvlc_media_player_t) -> *mut libvlc_track_description_t;
    fn libvlc_audio_get_track(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_audio_set_track(*mut libvlc_media_player_t, c_int) -> c_int;
    fn libvlc_audio_get_delay(*mut libvlc_media_player_t) -> i64;
    fn libvlc_audio_set_delay(*mut libvlc_media_player_t, i64) -> c_int;

    fn libvlc_video_get_track_count(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_video_get_track_description(*mut libvlc_media_player_t) -> *mut libvlc_track_description_t;
    fn libvlc_video_get_track(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_video_set_track(*mut libvlc_media_player_t, c_int) -> c_int;
    fn libvlc_video_get_spu_count(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_video_get_spu_description(*mut libvlc_media_player_t) -> *mut libvlc_track_description_t;
    fn libvlc_video_get_spu(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_video_set_spu(*mut libvlc_media_player_t, c_int) -> c_int;
    fn libvlc_video_get_spu_delay(*mut libvlc_media_player_t) -> i64;
    fn libvlc_video_set_spu_delay(*mut libvlc_media_player_t, i64) -> c_int;
    fn libvlc_video_get_aspect_ratio(*mut libvlc_media_player_t) -> *mut c_char;
    fn libvlc_video_set_aspect_ratio(*mut libvlc_media_player_t, *const c_char);
    fn libvlc_video_take_snapshot(
        *mut libvlc_media_player_t,
        c_uint,
        *const c_char,
        c_uint,
        c_uint,
    ) -> c_int;

    fn libvlc_audio_get_mute(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_audio_set_mute(*mut libvlc_media_player_t, c_int);
    fn libvlc_audio_get_volume(*mut libvlc_media_player_t) -> c_int;
    fn libvlc_audio_set_volume(*mut libvlc_media_player_t, c_int) -> c_int;

    fn libvlc_video_set_callbacks(
        *mut libvlc_media_player_t,
        libvlc_video_lock_cb,
        Option<libvlc_video_unlock_cb>,
        Option<libvlc_video_display_cb>,
        *mut c_void,
    );
    fn libvlc_video_set_format(*mut libvlc_media_player_t, *const c_char, c_uint, c_uint, c_uint);
    fn libvlc_video_set_format_callbacks(
        *mut libvlc_media_player_t,
        libvlc_video_format_cb,
        Option<libvlc_video_cleanup_cb>,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_msvc_x64_abi() {
        assert_eq!(size_of::<libvlc_media_stats_t>(), 15 * 4);
        assert_eq!(std::mem::offset_of!(libvlc_event_t, u), 16);
        assert_eq!(align_of::<libvlc_event_u>(), 8);
        // i_id(4) + padding(4) + two pointers(8 each) on MSVC x64.
        assert_eq!(size_of::<libvlc_track_description_t>(), 24);
        assert_eq!(std::mem::offset_of!(libvlc_track_description_t, p_next), 16);
    }
}
