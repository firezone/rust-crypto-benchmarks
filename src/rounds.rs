//! Interleaved rounds: every implementation is measured once per round, in a rotating order, so
//! slow drift (thermals, background work) spreads evenly over all of them instead of landing on
//! whichever ran last.

use crate::schema::{Measurement, Op, Report, SIZE};

pub const DEFAULT_ROUNDS: u32 = 7;
pub const DEFAULT_QUICK_ROUNDS: u32 = 3;

/// One round's measurement budget, passed to the implementation binaries. The total over all
/// rounds stays close to a single long measurement of 101 samples (21 with `--quick`).
pub struct Budget {
    pub warmup_ms: u64,
    pub samples: u32,
    pub sample_ms: u64,
}

impl Budget {
    pub fn per_round(rounds: u32, quick: bool) -> Self {
        let (total_samples, warmup_ms, sample_ms, min_samples) = if quick {
            (21u32, 20, 2, 3)
        } else {
            (101, 200, 20, 5)
        };
        Self {
            warmup_ms,
            samples: total_samples.div_ceil(rounds).max(min_samples),
            sample_ms,
        }
    }

    pub fn args(&self) -> [String; 6] {
        [
            "--warmup-ms".to_owned(),
            self.warmup_ms.to_string(),
            "--samples".to_owned(),
            self.samples.to_string(),
            "--sample-ms".to_owned(),
            self.sample_ms.to_string(),
        ]
    }
}

/// The order of `items` in round `round`: rotated by one position per round.
pub fn order<T>(items: &[T], round: u32) -> impl Iterator<Item = &T> {
    let start = if items.is_empty() {
        0
    } else {
        round as usize % items.len()
    };
    items[start..].iter().chain(&items[..start])
}

/// Combines one implementation's per-round reports into one.
pub fn aggregate(mut rounds: Vec<Report>) -> Report {
    let mut report = rounds.remove(0);
    let ops = report.results.iter().map(|m| m.op).collect::<Vec<Op>>();
    let all = std::iter::once(&report).chain(&rounds).collect::<Vec<_>>();

    report.results = ops
        .into_iter()
        .filter_map(|op| {
            let per_round = all
                .iter()
                .filter_map(|r| r.results.iter().find(|m| m.op == op))
                .collect::<Vec<_>>();
            let medians = per_round.iter().map(|m| m.median_ns).collect::<Vec<_>>();
            let mut sorted = medians.clone();
            sorted.sort_by(f64::total_cmp);
            let median = quantile(&sorted, 0.5);
            if median <= 0.0 {
                return None;
            }
            let spread = (sorted[sorted.len() - 1] - sorted[0]) / 2.0 / median * 100.0;
            Some(Measurement {
                op,
                size: SIZE,
                median_ns: median,
                q1_ns: quantile(&sorted, 0.25),
                q3_ns: quantile(&sorted, 0.75),
                min_ns: per_round
                    .iter()
                    .map(|m| m.min_ns)
                    .fold(f64::INFINITY, f64::min),
                mib_per_s: SIZE as f64 / (median * 1e-9) / (1024.0 * 1024.0),
                samples: per_round.iter().map(|m| m.samples).sum(),
                iters_per_sample: None,
                round_medians_ns: Some(medians),
                spread_pct: Some((spread * 10.0).round() / 10.0),
            })
        })
        .collect();
    report
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_order() {
        let items = ["a", "b", "c"];
        let rounds = (0..4)
            .map(|r| order(&items, r).copied().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(rounds, ["abc", "bca", "cab", "abc"]);
    }

    #[test]
    fn budget_matches_a_single_long_run() {
        assert_eq!(Budget::per_round(1, false).samples, 101);
        assert_eq!(Budget::per_round(7, false).samples, 15);
        assert_eq!(Budget::per_round(50, false).samples, 5);
    }
}
