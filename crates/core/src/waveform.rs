//! Waveform peak extraction — a separate, cancellable decode pass.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::conv::IntoSample;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::sample::Sample;

use crate::error::Result;

/// Downsampled absolute peaks in `0.0..=1.0`, one per requested bin.
/// Files without an audio track produce an all-zero waveform.
pub fn compute_peaks(path: &Path, bins: usize) -> Result<Vec<f32>> {
    let bins = bins.max(1);
    let file = File::open(path)?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| crate::error::CoreError::Invalid(format!("audio probe: {e}")))?;
    let mut format = probed.format;
    let Some(track) = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .cloned()
    else {
        return Ok(vec![0.0; bins]);
    };
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| crate::error::CoreError::Invalid(format!("audio decoder: {e}")))?;

    let mut peaks = vec![0.0f32; bins];
    let mut total_frames = 0usize;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        accumulate(&decoded, &mut peaks);
        total_frames += decoded.frames();
    }
    if total_frames == 0 {
        return Ok(peaks);
    }
    for peak in &mut peaks {
        *peak = peak.clamp(0.0, 1.0);
    }
    Ok(peaks)
}

fn accumulate(decoded: &AudioBufferRef<'_>, peaks: &mut [f32]) {
    match decoded {
        AudioBufferRef::U8(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::U16(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::U24(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::U32(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::S8(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::S16(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::S24(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::S32(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::F32(buf) => feed(buf.planes().planes().first().copied(), peaks),
        AudioBufferRef::F64(buf) => feed(buf.planes().planes().first().copied(), peaks),
    }
}

fn feed<S: Sample + Copy + IntoSample<f32>>(plane: Option<&[S]>, peaks: &mut [f32]) {
    let Some(samples) = plane else { return };
    let total = samples.len();
    if total == 0 {
        return;
    }
    for (i, &sample) in samples.iter().enumerate() {
        let value: f32 = sample.into_sample();
        let bin = i * peaks.len() / total;
        peaks[bin] = peaks[bin].max(value.abs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_only_file_produces_zero_waveform() -> Result<()> {
        let peaks = compute_peaks(Path::new("../../media/h264_1080p60.mp4"), 32)?;
        assert_eq!(peaks.len(), 32);
        assert!(peaks.iter().all(|p| *p == 0.0));
        Ok(())
    }
}
