use std::time::{Duration, Instant};

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
}

impl Stats {
    pub fn mib_per_s(&self, size: usize) -> f64 {
        size as f64 / (self.median_ns * 1e-9) / (1024.0 * 1024.0)
    }
}

/// Runs `f` for the warm-up period (doubling the batch size to estimate its cost), then takes
/// `samples` timed batches sized to last about `sample_time` each.
pub fn run(config: &Config, mut f: impl FnMut()) -> Stats {
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
    let mut times: Vec<f64> = (0..config.samples)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iters {
                f();
            }
            start.elapsed().as_nanos() as f64 / iters as f64
        })
        .collect();
    times.sort_by(f64::total_cmp);

    Stats {
        median_ns: quantile(&times, 0.5),
        q1_ns: quantile(&times, 0.25),
        q3_ns: quantile(&times, 0.75),
        min_ns: times[0],
        samples: times.len(),
        iters_per_sample: iters,
    }
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}
