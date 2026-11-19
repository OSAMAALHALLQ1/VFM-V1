use std::fmt::Write;
use std::time::Duration;

/// `HH:MM:SS`, the format used by the control bar (e.g. `00:02:10`).
/// Hours keep growing past 99 rather than wrapping.
pub fn format_hms(d: Duration) -> String {
    let s = d.as_secs();
    let mut out = String::with_capacity(8);
    let _ = write!(out, "{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60);
    out
}

/// Fraction `pos / len` clamped to `0.0..=1.0`; `0.0` when the length is unknown.
pub fn progress(pos: Duration, len: Option<Duration>) -> f32 {
    match len {
        Some(len) if !len.is_zero() => {
            (pos.as_secs_f64() / len.as_secs_f64()).clamp(0.0, 1.0) as f32
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hms_matches_design_examples() {
        assert_eq!(format_hms(Duration::from_secs(130)), "00:02:10");
        assert_eq!(format_hms(Duration::from_secs(25 * 60 + 40)), "00:25:40");
    }

    #[test]
    fn hms_truncates_subseconds_and_handles_bounds() {
        assert_eq!(format_hms(Duration::ZERO), "00:00:00");
        assert_eq!(format_hms(Duration::from_millis(59_999)), "00:00:59");
        assert_eq!(format_hms(Duration::from_secs(3600)), "01:00:00");
        assert_eq!(format_hms(Duration::from_secs(100 * 3600 + 1)), "100:00:01");
    }

    #[test]
    fn progress_clamps_and_handles_unknown_length() {
        let s = Duration::from_secs;
        assert_eq!(progress(s(5), Some(s(10))), 0.5);
        assert_eq!(progress(s(20), Some(s(10))), 1.0);
        assert_eq!(progress(s(5), None), 0.0);
        assert_eq!(progress(s(5), Some(Duration::ZERO)), 0.0);
    }
}
