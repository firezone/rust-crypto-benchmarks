use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Meta {
    pub arch: &'static str,
    pub os: &'static str,
    pub cpu: String,
    pub cpu_features: Vec<&'static str>,
    pub rustc: &'static str,
    pub date: String,
    pub commit: String,
    pub quick: bool,
}

pub fn collect(quick: bool) -> Meta {
    Meta {
        arch: std::env::consts::ARCH,
        os: std::env::consts::OS,
        cpu: cpu_model().unwrap_or_else(|| "unknown".to_owned()),
        cpu_features: cpu_features(),
        rustc: env!("HARNESS_RUSTC_VERSION"),
        date: now_rfc3339(),
        commit: std::env::var("BENCH_GIT_COMMIT")
            .ok()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "unknown".to_owned()),
        quick,
    }
}

fn cpu_model() -> Option<String> {
    let field = |text: &str, key: &str| {
        text.lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let run = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
    };

    // aarch64 Linux has no "model name" in /proc/cpuinfo, but `lscpu` decodes the part number.
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| field(&t, "model name"))
        .or_else(|| run("lscpu", &[]).and_then(|t| field(&t, "Model name")))
        .or_else(|| {
            run("sysctl", &["-n", "machdep.cpu.brand_string"])
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
}

/// Features that select the SIMD backends of the libraries under test.
fn cpu_features() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut features = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        macro_rules! detect {
            ($($f:tt),*) => { $(if std::arch::is_x86_feature_detected!($f) { features.push($f); })* };
        }
        detect!(
            "sse4.1",
            "avx",
            "avx2",
            "bmi2",
            "adx",
            "avx512f",
            "avx512vl",
            "avx512ifma"
        );
    }
    #[cfg(target_arch = "aarch64")]
    {
        macro_rules! detect {
            ($($f:tt),*) => { $(if std::arch::is_aarch64_feature_detected!($f) { features.push($f); })* };
        }
        detect!("neon", "aes", "sha2", "sha3", "sve", "sve2");
    }
    features
}

fn now_rfc3339() -> String {
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
