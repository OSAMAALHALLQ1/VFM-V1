//! Small statistics helpers for frame-pacing telemetry.

use serde::Serialize;

/// Turns a monotonically increasing counter into per-interval deltas.
/// A counter that goes backwards (new media, reset) yields 0 and re-bases.
#[derive(Debug, Default, Clone, Copy)]
pub struct Delta {
    last: Option<u64>,
}

impl Delta {
    pub fn step(&mut self, now: u64) -> u64 {
        let d = match self.last {
            Some(prev) if now >= prev => now - prev,
            _ => 0,
        };
        self.last = Some(now);
        d
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Summary {
    pub count: usize,
    pub mean: f64,
    pub min: f64,
    pub p95: f64,
    pub max: f64,
}

/// `None` for an empty slice. p95 uses the nearest-rank method.
pub fn summarize(values: &[f64]) -> Option<Summary> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let rank = ((0.95 * n as f64).ceil() as usize).clamp(1, n);
    Some(Summary {
        count: n,
        mean: sorted.iter().sum::<f64>() / n as f64,
        min: sorted[0],
        p95: sorted[rank - 1],
        max: sorted[n - 1],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_first_sample_is_zero_then_differences() {
        let mut d = Delta::default();
        assert_eq!(d.step(100), 0);
        assert_eq!(d.step(160), 60);
        assert_eq!(d.step(160), 0);
    }

    #[test]
    fn delta_rebases_on_counter_reset() {
        let mut d = Delta::default();
        d.step(500);
        assert_eq!(d.step(10), 0);
        assert_eq!(d.step(40), 30);
    }

    #[test]
    fn summarize_empty_is_none() {
        assert_eq!(summarize(&[]), None);
    }

    #[test]
    fn summarize_computes_nearest_rank_p95() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = summarize(&v).unwrap();
        assert_eq!(s.count, 100);
        assert_eq!(s.mean, 50.5);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.p95, 95.0);
        assert_eq!(s.max, 100.0);
    }

    #[test]
    fn summarize_single_value() {
        let s = summarize(&[7.0]).unwrap();
        assert_eq!((s.min, s.p95, s.max), (7.0, 7.0, 7.0));
    }
}
