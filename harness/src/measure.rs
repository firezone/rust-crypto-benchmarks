use std::time::{Duration, Instant};

use crate::cycles::Counter;

pub struct Config {
    pub warmup: Duration,
    pub samples: usize,
    pub sample_time: Duration,
}

impl Config {
    pub fn full() -> Self {
        Self {
            warmup: Duration::from_millis(500),
            samples: 101,
            sample_time: Duration::from_millis(20),
        }
    }

    pub fn quick() -> Self {
        Self {
            warmup: Duration::from_millis(50),
            samples: 21,
            sample_time: Duration::from_millis(2),
        }
    }
}

pub struct Stats {
    pub median_ns: f64,
    pub q1_ns: f64,
    pub q3_ns: f64,
    pub min_ns: f64,
    pub samples: usize,
    pub iters_per_sample: u64,
    /// The median over the samples, when a cycle counter is available.
    pub cycles_per_iter: Option<f64>,
}

impl Stats {
    pub fn mib_per_s(&self, size: usize) -> f64 {
        size as f64 / (self.median_ns * 1e-9) / (1024.0 * 1024.0)
    }

    pub fn cycles_per_byte(&self, size: usize) -> Option<f64> {
        self.cycles_per_iter.map(|c| c / size as f64)
    }
}

/// Runs `f` for the warm-up period (doubling the batch size to estimate its cost), then takes
/// `samples` timed batches sized to last about `sample_time` each, counting cycles around every
/// batch when `counter` is given.
pub fn run(config: &Config, counter: Option<&Counter>, mut f: impl FnMut()) -> Stats {
    let mut batch = 1u64;
    let mut spent = Duration::ZERO;
    let mut per_iter = f64::INFINITY;
    while spent < config.warmup {
        let start = Instant::now();
        for _ in 0..batch {
            f();
        }
        let elapsed = start.elapsed();
        spent += elapsed;
        per_iter = per_iter.min(elapsed.as_nanos() as f64 / batch as f64);
        if elapsed < config.warmup / 8 {
            batch *= 2;
        }
    }

    let iters = ((config.sample_time.as_nanos() as f64 / per_iter) as u64).max(1);
    let samples = (0..config.samples)
        .map(|_| {
            let cycles_before = counter.map(Counter::read);
            let start = Instant::now();
            for _ in 0..iters {
                f();
            }
            let elapsed = start.elapsed();
            let cycles = counter
                .zip(cycles_before)
                .map(|(c, before)| c.read().wrapping_sub(before) as f64 / iters as f64);
            (elapsed.as_nanos() as f64 / iters as f64, cycles)
        })
        .collect::<Vec<_>>();
    let mut times = samples.iter().map(|s| s.0).collect::<Vec<_>>();
    times.sort_by(f64::total_cmp);
    let mut cycles = samples.iter().filter_map(|s| s.1).collect::<Vec<_>>();
    cycles.sort_by(f64::total_cmp);

    Stats {
        median_ns: quantile(&times, 0.5),
        q1_ns: quantile(&times, 0.25),
        q3_ns: quantile(&times, 0.75),
        min_ns: times[0],
        samples: times.len(),
        iters_per_sample: iters,
        cycles_per_iter: (!cycles.is_empty()).then(|| quantile(&cycles, 0.5)),
    }
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}
