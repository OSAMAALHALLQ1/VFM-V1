//! Two 32-byte-aligned RV32 buffers shared between libVLC and a UI consumer.
//! FREE → WRITING → READY → READING → FREE; only the state owner touches pixels.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use crate::{VlcError, sys};

const FREE: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

#[repr(C, align(32))]
#[derive(Clone, Copy)]
struct Block([u8; 32]);

struct Buffer {
    blocks: Box<[Block]>,
    len: usize,
}

impl Buffer {
    fn new(len: usize) -> Self {
        Self {
            blocks: vec![Block([0; 32]); len.div_ceil(32)].into_boxed_slice(),
            len,
        }
    }

    fn ptr(&mut self) -> *mut u8 {
        self.blocks.as_mut_ptr().cast()
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: Block has no padding (32 bytes, alignment 32); the allocation
        // covers `len` initialized bytes and remains borrowed for this slice.
        unsafe { std::slice::from_raw_parts(self.blocks.as_ptr().cast(), self.len) }
    }
}

struct Slot {
    state: AtomicU8,
    sequence: AtomicU64,
    pixels: UnsafeCell<Buffer>,
}

// SAFETY: pixel access requires owning WRITING or READING, acquired with CAS.
// READY/FREE transitions use release/acquire; the allocation never moves.
unsafe impl Sync for Slot {}

struct Inner {
    width: u32,
    height: u32,
    slots: [Slot; 2],
    delivered: AtomicU64,
    overwritten: AtomicU64,
    starved: AtomicU64,
    watermark: AtomicU64,
}

/// Cloneable frame consumer and counters. Dimensions are fixed for this spike;
/// libVLC scales to the requested dimensions using `libvlc_video_set_format`.
#[derive(Clone)]
pub struct VideoFrames(Arc<Inner>);

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameCounters {
    pub delivered: u64,
    pub overwritten: u64,
    pub starved: u64,
}

impl VideoFrames {
    pub fn new(width: u32, height: u32) -> Result<Self, VlcError> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(VlcError::InvalidLocation(
                "video dimensions must be 1..=8192".into(),
            ));
        }
        let len = width as usize * height as usize * 4;
        Ok(Self(Arc::new(Inner {
            width,
            height,
            slots: std::array::from_fn(|_| Slot {
                state: AtomicU8::new(FREE),
                sequence: AtomicU64::new(0),
                pixels: UnsafeCell::new(Buffer::new(len)),
            }),
            delivered: AtomicU64::new(0),
            overwritten: AtomicU64::new(0),
            starved: AtomicU64::new(0),
            watermark: AtomicU64::new(0),
        })))
    }

    pub fn width(&self) -> u32 {
        self.0.width
    }
    pub fn height(&self) -> u32 {
        self.0.height
    }

    pub fn counters(&self) -> FrameCounters {
        FrameCounters {
            delivered: self.0.delivered.load(Ordering::Relaxed),
            overwritten: self.0.overwritten.load(Ordering::Relaxed),
            starved: self.0.starved.load(Ordering::Relaxed),
        }
    }

    /// Non-blocking: returns the latest available frame. Keep leases short;
    /// libVLC discards frames instead of waiting if both slots are unavailable.
    pub fn latest(&self) -> Option<FrameLease> {
        let a = self.0.slots[0].sequence.load(Ordering::Relaxed);
        let b = self.0.slots[1].sequence.load(Ordering::Relaxed);
        let order = if a >= b { [0, 1] } else { [1, 0] };
        for index in order {
            let slot = &self.0.slots[index];
            if slot
                .state
                .compare_exchange(READY, READING, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let sequence = slot.sequence.load(Ordering::Relaxed);
                if sequence <= self.0.watermark.fetch_max(sequence, Ordering::Relaxed) {
                    self.0.overwritten.fetch_add(1, Ordering::Relaxed);
                    slot.state.store(FREE, Ordering::Release);
                    continue;
                }
                return Some(FrameLease {
                    frames: self.clone(),
                    index,
                });
            }
        }
        None
    }
}

/// Exclusive, read-only ownership of a decoded frame. Dropping releases its slot.
pub struct FrameLease {
    frames: VideoFrames,
    index: usize,
}

impl FrameLease {
    /// RV32 on Windows x64: B, G, R, unused byte. Do not treat the last byte as alpha.
    pub fn bgra_bytes(&self) -> &[u8] {
        // SAFETY: the lease exclusively owns READING; writers cannot acquire it.
        unsafe { &*self.frames.0.slots[self.index].pixels.get() }.bytes()
    }
    pub fn sequence(&self) -> u64 {
        self.frames.0.slots[self.index]
            .sequence
            .load(Ordering::Relaxed)
    }
}

impl Drop for FrameLease {
    fn drop(&mut self) {
        self.frames.0.slots[self.index]
            .state
            .store(FREE, Ordering::Release);
    }
}

pub(crate) struct VideoBridge {
    frames: VideoFrames,
}
impl VideoBridge {
    pub(crate) fn new(frames: VideoFrames) -> Box<Self> {
        Box::new(Self { frames })
    }
}

struct Picture {
    frames: VideoFrames,
    index: Option<usize>,
    scratch: Option<Buffer>,
}

unsafe extern "C" fn lock_cb(opaque: *mut c_void, planes: *mut *mut c_void) -> *mut c_void {
    // SAFETY: opaque is a boxed bridge retained until stop/release joins vout.
    let bridge = unsafe { &*opaque.cast::<VideoBridge>() };
    let frames = &bridge.frames;
    let mut index = None;
    for state in [FREE, READY] {
        for (i, slot) in frames.0.slots.iter().enumerate() {
            if slot
                .state
                .compare_exchange(state, WRITING, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                if state == READY {
                    frames.0.overwritten.fetch_add(1, Ordering::Relaxed);
                }
                index = Some(i);
                break;
            }
        }
        if index.is_some() {
            break;
        }
    }
    let mut picture = Box::new(Picture {
        frames: frames.clone(),
        index,
        scratch: None,
    });
    let ptr = if let Some(i) = index {
        // SAFETY: the WRITING owner exclusively accesses this stable allocation.
        unsafe { &mut *frames.0.slots[i].pixels.get() }.ptr()
    } else {
        frames.0.starved.fetch_add(1, Ordering::Relaxed);
        let buffer = picture.scratch.insert(Buffer::new(
            frames.width() as usize * frames.height() as usize * 4,
        ));
        buffer.ptr()
    };
    // SAFETY: libVLC passes a valid plane array; ptr remains allocated through display.
    unsafe { *planes = ptr.cast() };
    Box::into_raw(picture).cast()
}

unsafe extern "C" fn display_cb(_opaque: *mut c_void, picture: *mut c_void) {
    // SAFETY: each picture is created by lock, then displayed exactly once by
    // libVLC 3.0 vmem.c Prepare/Display. Display follows completion of the copy.
    let picture = unsafe { Box::from_raw(picture.cast::<Picture>()) };
    let sequence = picture.frames.0.delivered.fetch_add(1, Ordering::Relaxed) + 1;
    if let Some(index) = picture.index {
        let slot = &picture.frames.0.slots[index];
        slot.sequence.store(sequence, Ordering::Relaxed);
        slot.state.store(READY, Ordering::Release);
    }
}

pub(crate) fn install(api: &sys::Api, mp: *mut sys::libvlc_media_player_t, bridge: &VideoBridge) {
    let opaque = std::ptr::from_ref(bridge).cast_mut().cast::<c_void>();
    // SAFETY: mp is live; opaque is stable until player stop/release. Pitch is
    // width*4 and both pixel buffers are 32-byte aligned as required by libVLC.
    unsafe {
        (api.libvlc_video_set_callbacks)(mp, lock_cb, None, Some(display_cb), opaque);
        (api.libvlc_video_set_format)(
            mp,
            c"RV32".as_ptr(),
            bridge.frames.width(),
            bridge.frames.height(),
            bridge.frames.width() * 4,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deliver(bridge: &VideoBridge, value: u8) {
        let opaque = std::ptr::from_ref(bridge).cast_mut().cast();
        let mut plane = std::ptr::null_mut();
        // SAFETY: simulate the vmem lock → copy → display contract with live bridge.
        unsafe {
            let picture = lock_cb(opaque, &mut plane);
            assert_eq!(plane as usize % 32, 0);
            std::ptr::write_bytes(
                plane,
                value,
                bridge.frames.width() as usize * bridge.frames.height() as usize * 4,
            );
            display_cb(opaque, picture);
        }
    }

    #[test]
    fn reader_is_not_overwritten_and_backlog_is_bounded() {
        let frames = VideoFrames::new(4, 4).unwrap();
        let bridge = VideoBridge::new(frames.clone());
        deliver(&bridge, 1);
        let first = frames.latest().unwrap();
        deliver(&bridge, 2);
        deliver(&bridge, 3);
        assert_eq!(first.bgra_bytes(), &[1; 64]);
        let latest = frames.latest().unwrap();
        assert_eq!(latest.bgra_bytes(), &[3; 64]);
        deliver(&bridge, 4);
        assert_eq!(frames.counters().starved, 1);
        assert_eq!(frames.counters().overwritten, 1);
        drop(first);
        drop(latest);
        deliver(&bridge, 5);
        assert_eq!(frames.latest().unwrap().bgra_bytes(), &[5; 64]);
    }

    #[test]
    fn latest_never_returns_an_obsolete_frame() {
        let frames = VideoFrames::new(4, 4).unwrap();
        let bridge = VideoBridge::new(frames.clone());
        deliver(&bridge, 1);
        deliver(&bridge, 2);
        assert_eq!(frames.latest().unwrap().sequence(), 2);
        assert!(frames.latest().is_none());
        assert_eq!(frames.counters().overwritten, 1);
    }

    #[test]
    fn concurrent_delivery_never_tears_a_read_lease() {
        let frames = VideoFrames::new(4, 4).unwrap();
        let writer_frames = frames.clone();
        let writer = std::thread::spawn(move || {
            let bridge = VideoBridge::new(writer_frames);
            for value in 0..10_000 {
                deliver(&bridge, (value % 255) as u8);
            }
        });
        let mut sequence = 0;
        while !writer.is_finished() {
            if let Some(frame) = frames.latest() {
                assert!(frame.sequence() > sequence);
                sequence = frame.sequence();
                let bytes = frame.bgra_bytes();
                assert!(bytes.iter().all(|v| *v == bytes[0]));
            }
            std::thread::yield_now();
        }
        writer.join().unwrap();
        assert_eq!(frames.counters().delivered, 10_000);
    }

    #[test]
    fn dimensions_are_validated() {
        assert!(VideoFrames::new(0, 1080).is_err());
        assert!(VideoFrames::new(9000, 1).is_err());
    }
}
