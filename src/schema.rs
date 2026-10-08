//! The result files under `results/`: one file per run, describing the machine and every
//! implementation's measurements.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rounds::{MAX_DRIFT_PCT, drift_pct};

/// Version 2 added interleaved rounds (`meta.rounds`, `round_medians_ns`, `spread_pct`) and
/// `meta.placement`. Version 1 files remain valid.
pub const SCHEMA_VERSION: u32 = 2;
pub const SIZE: u64 = 1280;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunFile {
    pub schema: u32,
    /// A human-readable label for the machine, e.g. `apple-m3-max-macos`.
    pub machine: String,
    pub meta: Meta,
    pub implementations: Vec<Report>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    /// RFC 3339 UTC timestamp of the end of the run.
    pub date: String,
    pub arch: String,
    pub os: String,
    pub cpu: String,
    pub cpu_count: Option<u32>,
    pub cpu_features: Vec<String>,
    pub rustc: String,
    /// The first line of the C compiler's version banner, which `ring` and `aws-lc-rs` are built
    /// with; absent when none was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c_compiler: Option<String>,
    /// The repository commit the run was made from; absent when git was unavailable.
    pub commit: Option<String>,
    /// Whether tracked files differed from `commit`; absent when git was unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    pub quick: bool,
    /// Absent for runs made before the preflight checks existed.
    pub preflight: Option<Preflight>,
    /// Where the benchmark processes ran; absent before version 2 and on unsupported systems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// How many interleaved rounds every implementation was measured in (version 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub method: PlacementMethod,
    /// The logical CPU every benchmark process was pinned to.
    pub cpu: Option<u32>,
    pub cpu_max_mhz: Option<u32>,
    /// Windows' ranking of core types: higher is more performant.
    pub efficiency_class: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlacementMethod {
    /// Pinned to one logical CPU (Linux, Windows).
    Affinity,
    /// Highest thread QoS class, which prefers performance cores (macOS).
    Qos,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preflight {
    /// Average utilisation of all cores while idling before the benchmarks.
    pub cpu_busy_pct: Option<f64>,
    pub power: Power,
    /// Checks that failed but were overridden, by `--force` or because the run was in CI.
    pub forced: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Power {
    pub source: Option<PowerSource>,
    pub low_power_mode: Option<bool>,
    pub profile: Option<String>,
    pub governor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerSource {
    Ac,
    Battery,
}

/// What one implementation binary writes.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    #[serde(rename = "impl")]
    pub name: String,
    pub library: String,
    pub crates: Vec<Crate>,
    /// How CPU cycles were counted, when `cycles_per_byte` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_counter: Option<String>,
    pub config: Config,
    pub results: Vec<Measurement>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crate {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub samples: u32,
    pub sample_time_ms: u64,
    pub warmup_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub op: Op,
    pub size: u64,
    /// In a run file of version 2 and later: the median of `round_medians_ns`, with `q1_ns` and
    /// `q3_ns` its quartiles. Otherwise, statistics over the samples of a single measurement.
    pub median_ns: f64,
    pub q1_ns: f64,
    pub q3_ns: f64,
    pub min_ns: f64,
    pub mib_per_s: f64,
    pub samples: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iters_per_sample: Option<u64>,
    /// The median of each interleaved round, in the order the rounds ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_medians_ns: Option<Vec<f64>>,
    /// Half the range of `round_medians_ns`, relative to `median_ns`: the "±" of the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_pct: Option<f64>,
    /// The median CPU cycles per message byte, where a cycle counter was available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycles_per_byte: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Seal,
    Open,
}

impl Op {
    pub fn title(self) -> &'static str {
        match self {
            Op::Seal => "seal",
            Op::Open => "open",
        }
    }
}

/// Lowercase letters and digits, optionally separated by `-`, `.` or `_`, e.g.
/// `apple-m3-max-macos` or `github-ubuntu-24.04-x86_64`.
pub fn is_label(label: &str) -> bool {
    let alnum = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    label.len() <= 64
        && label.starts_with(alnum)
        && label.ends_with(alnum)
        && label
            .chars()
            .all(|c| alnum(c) || matches!(c, '-' | '.' | '_'))
}

/// Reads and checks one result file, as submitted to the repository.
pub fn load(path: &Path) -> Result<RunFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let run: RunFile = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    check(&run)?;
    Ok(run)
}

fn check(run: &RunFile) -> Result<(), String> {
    let mut problems = Vec::new();
    if !(1..=SCHEMA_VERSION).contains(&run.schema) {
        problems.push(format!("schema must be between 1 and {SCHEMA_VERSION}"));
    }
    if !is_label(&run.machine) {
        problems.push(
            "machine must be lowercase letters and digits separated by `-`, `.` or `_`, at most 64 characters"
                .to_owned(),
        );
    }
    let meta = &run.meta;
    if !is_timestamp(&meta.date) {
        problems.push("meta.date must look like 2026-10-07T12:03:01Z".to_owned());
    }
    for (field, value) in [
        ("arch", &meta.arch),
        ("os", &meta.os),
        ("cpu", &meta.cpu),
        ("rustc", &meta.rustc),
    ] {
        if value.trim().is_empty() {
            problems.push(format!("meta.{field} must not be empty"));
        }
    }
    if meta.quick {
        problems.push("quick runs are smoke tests: please submit a full run".to_owned());
    }
    if run.implementations.is_empty() {
        problems.push("implementations must not be empty".to_owned());
    }
    for report in &run.implementations {
        let name = &report.name;
        if report.results.is_empty() {
            problems.push(format!("{name}: no results"));
        }
        for m in &report.results {
            if m.size != SIZE {
                problems.push(format!("{name} {}: size must be {SIZE}", m.op.title()));
            }
            if run.schema >= 2 {
                let rounds = m.round_medians_ns.as_deref().unwrap_or_default();
                if rounds.is_empty() || m.spread_pct.is_none_or(|s| !s.is_finite() || s < 0.0) {
                    problems.push(format!(
                        "{name} {}: round_medians_ns and spread_pct are required",
                        m.op.title()
                    ));
                }
                if rounds.iter().any(|t| !t.is_finite() || *t <= 0.0) {
                    problems.push(format!(
                        "{name} {}: round medians must be positive",
                        m.op.title()
                    ));
                }
                let drift = drift_pct(rounds);
                if drift > MAX_DRIFT_PCT {
                    problems.push(format!(
                        "{name} {}: the round medians disagree, their interquartile range is {drift:.1}% of the median (at most {MAX_DRIFT_PCT}%); the machine was busy or throttled, run again",
                        m.op.title()
                    ));
                }
            }
            if m.cycles_per_byte
                .is_some_and(|c| !c.is_finite() || c <= 0.0)
            {
                problems.push(format!(
                    "{name} {}: cycles_per_byte must be a positive number",
                    m.op.title()
                ));
            }
            let timings = [m.median_ns, m.q1_ns, m.q3_ns, m.min_ns, m.mib_per_s];
            if !timings.iter().all(|t| t.is_finite() && *t > 0.0) {
                problems.push(format!(
                    "{name} {}: timings must be positive numbers",
                    m.op.title()
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}
