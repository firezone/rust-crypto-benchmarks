//! What the results say about the machine they were taken on.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Machine {
    pub arch: &'static str,
    pub os: &'static str,
    pub cpu: String,
    pub cpu_count: Option<u32>,
    pub cpu_features: Vec<String>,
    pub rustc: String,
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub c_compiler: Option<String>,
}

impl Machine {
    pub fn detect() -> Self {
        Self {
            arch: std::env::consts::ARCH,
            os: std::env::consts::OS,
            cpu: cpu_model().unwrap_or_else(|| "unknown".to_owned()),
            cpu_count: std::thread::available_parallelism()
                .ok()
                .and_then(|n| u32::try_from(n.get()).ok()),
            cpu_features: cpu_features(),
            rustc: output("rustc", &["-V"]).unwrap_or_else(|| "unknown".to_owned()),
            commit: output("git", &["rev-parse", "HEAD"])
                .or_else(|| std::env::var("GITHUB_SHA").ok().filter(|s| !s.is_empty())),
            dirty: dirty(),
            c_compiler: c_compiler(),
        }
    }

    /// A slug of the CPU model and OS, e.g. `amd-epyc-7763-linux` or `apple-m3-max-macos`.
    /// Deliberately not the hostname: the results are published.
    pub fn default_label(&self) -> String {
        label(&self.cpu, self.os)
    }
}

fn label(cpu: &str, os: &str) -> String {
    let cpu = cpu.to_lowercase().replace("(r)", " ").replace("(tm)", " ");
    let cpu = cpu.split('@').next().unwrap_or_default();
    let words = cpu
        .split_whitespace()
        .filter(|w| !matches!(*w, "processor" | "cpu") && !w.ends_with("-core"))
        .chain([os]);

    let mut label = String::new();
    for word in words {
        for c in word.chars() {
            if c.is_ascii_alphanumeric() {
                label.push(c);
            } else if !label.is_empty() && !label.ends_with('-') {
                label.push('-');
            }
        }
        if !label.is_empty() && !label.ends_with('-') {
            label.push('-');
        }
    }
    label.truncate(64);
    label.trim_end_matches('-').to_owned()
}

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    (out.status.success() && !text.is_empty()).then(|| text.to_owned())
}

/// Whether tracked files differ from `HEAD`; `None` when git is unavailable.
fn dirty() -> Option<bool> {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()?;
    out.status.success().then_some(!out.stdout.is_empty())
}

/// The version banner of the C compiler `cc-rs` picks, which builds `ring` and `aws-lc-rs`.
fn c_compiler() -> Option<String> {
    if let Some(cc) = std::env::var("CC").ok().filter(|cc| !cc.is_empty()) {
        return c_compiler_version(&cc);
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["cl", "cc"]
    } else {
        &["cc", "clang", "gcc"]
    };
    candidates.iter().find_map(|cc| c_compiler_version(cc))
}

/// The first line of the compiler's version banner. `cl` has no version flag and prints its
/// banner to stderr instead.
fn c_compiler_version(cc: &str) -> Option<String> {
    let is_cl = Path::new(cc)
        .file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("cl"));
    let text = if is_cl {
        Command::new(cc).output().ok()?.stderr
    } else {
        let out = Command::new(cc).arg("--version").output().ok()?;
        if !out.status.success() {
            return None;
        }
        out.stdout
    };
    String::from_utf8(text)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

fn cpu_model() -> Option<String> {
    let field = |text: &str, key: &str| {
        text.lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };

    // aarch64 Linux has no "model name" in /proc/cpuinfo, but `lscpu` decodes the part number.
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| field(&t, "model name"))
        .or_else(|| output("lscpu", &[]).and_then(|t| field(&t, "Model name")))
        .or_else(|| output("sysctl", &["-n", "machdep.cpu.brand_string"]))
        .or_else(|| {
            use sysinfo::{CpuRefreshKind, RefreshKind, System};
            let sys = System::new_with_specifics(
                RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing()),
            );
            let brand = sys.cpus().first()?.brand().trim().to_owned();
            (!brand.is_empty()).then_some(brand)
        })
}

/// Features that select the SIMD backends of the libraries under test.
fn cpu_features() -> Vec<String> {
    #[allow(unused_mut)]
    let mut features: Vec<&str> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        macro_rules! detect {
            ($($f:tt),*) => { $(if std::arch::is_x86_feature_detected!($f) { features.push($f); })* };
        }
        detect!("ssse3", "avx", "avx2", "avx512f", "avx512vl");
    }
    #[cfg(target_arch = "aarch64")]
    {
        macro_rules! detect {
            ($($f:tt),*) => { $(if std::arch::is_aarch64_feature_detected!($f) { features.push($f); })* };
        }
        detect!("neon", "sve", "sve2");
    }
    features.into_iter().map(str::to_owned).collect()
}

/// The current time as `2026-10-07T12:03:01Z`.
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86400, secs % 86400);

    // Howard Hinnant's `civil_from_days`.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_from_cpu_models() {
        for (cpu, os, expected) in [
            (
                "AMD EPYC 7763 64-Core Processor",
                "linux",
                "amd-epyc-7763-linux",
            ),
            ("Apple M3 Max", "macos", "apple-m3-max-macos"),
            (
                "Intel(R) Xeon(R) Processor @ 2.80GHz",
                "linux",
                "intel-xeon-linux",
            ),
            (
                "Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz",
                "linux",
                "intel-core-i7-8700k-linux",
            ),
            ("Neoverse-N2", "linux", "neoverse-n2-linux"),
        ] {
            let label = label(cpu, os);
            assert_eq!(label, expected);
            assert!(crate::schema::is_label(&label));
        }
    }
}
