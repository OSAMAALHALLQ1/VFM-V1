//! Subtitle data model and parsers (SRT, WebVTT, basic ASS/SSA).
//!
//! The parsed cue list is the single source for Phase 5's own Slint renderer
//! and Phase 6's draggable subtitle timeline.

use std::time::Duration;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubtitleFormat {
    Srt,
    WebVtt,
    Ass,
    Ssa,
    #[default]
    Unknown,
}

impl SubtitleFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::WebVtt => "vtt",
            Self::Ass => "ass",
            Self::Ssa => "ssa",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubtitlePosition {
    Bottom,
    Middle,
    Top,
    #[default]
    BottomCustom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleStyle {
    pub font_family: String,
    /// `true` = size is a fraction of video height; `false` = exact pixels.
    pub relative_size: bool,
    pub size: f32,
    pub color: String,
    pub outline_px: u32,
    pub background: bool,
    pub position: SubtitlePosition,
    /// Vertical anchor as a fraction of the video height (0 = top, 1 = bottom).
    pub y: f32,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_family: "Inter".into(),
            relative_size: true,
            size: 0.05,
            color: "#FFFFFF".into(),
            outline_px: 2,
            background: true,
            position: SubtitlePosition::Bottom,
            y: 0.86,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub index: usize,
    pub start: Duration,
    pub end: Duration,
    pub text: String,
    pub layer: i32,
    pub style_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleDocument {
    pub format: SubtitleFormat,
    pub cues: Vec<Cue>,
    pub styles: Vec<(String, SubtitleStyle)>,
}

impl SubtitleDocument {
    /// Cues active at `time` after applying the user's delay (positive delay
    /// makes subtitles appear later, matching libVLC's convention).
    pub fn active_cues(&self, time: Duration, delay: Duration) -> Vec<&Cue> {
        let adjusted = time.saturating_sub(delay);
        self.cues
            .iter()
            .filter(|cue| cue.start <= adjusted && adjusted < cue.end)
            .collect()
    }

    /// Shifts every cue by a signed millisecond delta (Phase 6 timeline drag).
    /// Positive values move subtitles later; negative values move them earlier.
    pub fn shift(&mut self, delta_ms: i64) {
        for cue in &mut self.cues {
            cue.start = shift_duration(cue.start, delta_ms);
            cue.end = shift_duration(cue.end, delta_ms);
        }
        self.cues.retain(|cue| cue.end > cue.start);
        self.cues.sort_by_key(|cue| cue.start);
    }
}

fn shift_duration(d: Duration, delta_ms: i64) -> Duration {
    if delta_ms >= 0 {
        d.saturating_add(Duration::from_millis(delta_ms as u64))
    } else {
        d.saturating_sub(Duration::from_millis(delta_ms.unsigned_abs()))
    }
}

/// Decodes UTF-8, UTF-16 (with BOM) and falls back to Windows-1252 bytes.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (pairs, _) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = pairs
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (pairs, _) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = pairs
            .iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

pub fn parse(bytes: &[u8]) -> Result<SubtitleDocument> {
    let text = decode(bytes);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with("WEBVTT") {
        Ok(parse_vtt(trimmed))
    } else if trimmed.starts_with("[Script Info]") || trimmed.contains("Dialogue:") {
        parse_ass(trimmed)
    } else if trimmed.contains("-->") {
        Ok(parse_srt(trimmed))
    } else {
        Err(CoreError::Invalid("unrecognised subtitle format".into()))
    }
}

fn ms(h: u64, m: u64, s: u64, millis: u64) -> Duration {
    Duration::from_millis(h * 3_600_000 + m * 60_000 + s * 1000 + millis)
}

fn parse_timestamp(raw: &str) -> Option<Duration> {
    let raw = raw.trim().replace(',', ".");
    let (clock, millis) = raw.split_once('.').unwrap_or((&raw, "0"));
    let parts: Vec<u64> = clock
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let millis: u64 = millis
        .chars()
        .take(3)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    match parts.as_slice() {
        [h, m, s] => Some(ms(*h, *m, *s, millis)),
        [m, s] => Some(ms(0, *m, *s, millis)),
        _ => None,
    }
}

fn split_timing(line: &str) -> Option<(Duration, Duration)> {
    let (start, end) = line.split_once("-->")?;
    Some((
        parse_timestamp(start)?,
        parse_timestamp(end.split_whitespace().next()?)?,
    ))
}

fn parse_srt(text: &str) -> SubtitleDocument {
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect();
        let timing_index = lines.iter().position(|l| l.contains("-->"));
        let Some(i) = timing_index else { continue };
        let Some((start, end)) = split_timing(lines[i]) else {
            continue;
        };
        let body = lines[i + 1..].join("\n");
        cues.push(Cue {
            index: cues.len() + 1,
            start,
            end,
            text: body,
            layer: 0,
            style_name: String::new(),
        });
    }
    SubtitleDocument {
        format: SubtitleFormat::Srt,
        cues,
        styles: Vec::new(),
    }
}

fn parse_vtt(text: &str) -> SubtitleDocument {
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        let Some(i) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((start, end)) = split_timing(lines[i]) else {
            continue;
        };
        let body = lines[i + 1..].join("\n");
        cues.push(Cue {
            index: cues.len() + 1,
            start,
            end,
            text: body,
            layer: 0,
            style_name: String::new(),
        });
    }
    SubtitleDocument {
        format: SubtitleFormat::WebVtt,
        cues,
        styles: Vec::new(),
    }
}

fn strip_ass_tags(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '{' => in_tag = true,
            '}' => in_tag = false,
            '\\' if in_tag => out.push('\n'),
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("\\N", "\n")
        .replace("\n\n", "\n")
        .trim()
        .to_owned()
}

fn parse_ass(text: &str) -> Result<SubtitleDocument> {
    let format = if text.contains(">: SSA") || text.contains("SSA v4") {
        SubtitleFormat::Ssa
    } else {
        SubtitleFormat::Ass
    };
    let mut cues = Vec::new();
    let mut in_events = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if !in_events || !line.to_ascii_lowercase().starts_with("dialogue:") {
            continue;
        }
        let (_, fields) = line.split_once(':').unwrap_or(("", ""));
        let parts: Vec<&str> = fields.split(',').collect();
        if parts.len() < 10 {
            continue;
        }
        let layer = parts[0].trim().parse().unwrap_or(0);
        let start = parse_timestamp(parts[1]).unwrap_or_default();
        let end = parse_timestamp(parts[2]).unwrap_or_default();
        let style = parts[3].trim().to_owned();
        let body = strip_ass_tags(&parts[9..].join(","));
        cues.push(Cue {
            index: cues.len() + 1,
            start,
            end,
            text: body,
            layer,
            style_name: style,
        });
    }
    if cues.is_empty() {
        return Err(CoreError::Invalid(
            "ASS file contained no dialogue events".into(),
        ));
    }
    Ok(SubtitleDocument {
        format,
        cues,
        styles: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_and_applies_delay() {
        let srt = b"1\n00:00:01,000 --> 00:00:03,500\nHello\nworld\n\n2\n00:01:00,000 --> 00:01:02,000\nBye\n";
        let doc = parse(srt).unwrap();
        assert_eq!(doc.format, SubtitleFormat::Srt);
        assert_eq!(doc.cues.len(), 2);
        assert_eq!(doc.cues[0].text, "Hello\nworld");
        assert_eq!(doc.cues[0].start, Duration::from_secs(1));
        assert_eq!(
            doc.active_cues(Duration::from_secs(2), Duration::ZERO)
                .len(),
            1
        );
        // Positive delay means the cue appears 1 s later.
        assert!(
            doc.active_cues(Duration::from_secs(2), Duration::from_secs(2))
                .is_empty()
        );
        assert_eq!(
            doc.active_cues(Duration::from_secs(3), Duration::from_secs(2))
                .len(),
            1
        );
    }

    #[test]
    fn parses_webvtt_header_and_cue_settings() {
        let vtt = b"WEBVTT\n\n00:00:02.000 --> 00:00:04.000 align:start\nHi there\n";
        let doc = parse(vtt).unwrap();
        assert_eq!(doc.format, SubtitleFormat::WebVtt);
        assert_eq!(doc.cues[0].text, "Hi there");
        assert_eq!(doc.cues[0].start, Duration::from_secs(2));
    }

    #[test]
    fn parses_ass_dialogue_and_strips_tags() {
        let ass = b"[Script Info]\nScriptType: v4.00+\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\i1}Italics\\Nnext line\n";
        let doc = parse(ass).unwrap();
        assert_eq!(doc.format, SubtitleFormat::Ass);
        assert_eq!(doc.cues[0].text, "Italics\nnext line");
        assert_eq!(doc.cues[0].style_name, "Default");
    }

    #[test]
    fn decodes_utf16_bom() {
        let text = "مرحبا";
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&bytes), text);
    }

    #[test]
    fn shift_moves_cues_and_drops_negative_ones() {
        let srt = b"1\n00:00:05,000 --> 00:00:07,000\nLater\n";
        let mut doc = parse(srt).unwrap();
        doc.shift(4_000);
        assert_eq!(doc.cues[0].start, Duration::from_secs(9));
        doc.shift(-3_000);
        assert_eq!(doc.cues[0].start, Duration::from_secs(6));
        doc.shift(-20_000);
        assert!(doc.cues.is_empty());
    }
}
