//! Checks that the machine is in a state worth measuring: plugged in, not throttled to save
//! power, and otherwise idle. Every check is skipped quietly where the platform has no way to
//! answer it.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::schema::{Power, PowerSource};

/// Average utilisation of all cores above which the machine counts as busy.
pub const MAX_CPU_BUSY_PCT: f64 = 10.0;
const CPU_SAMPLE_TIME: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq)]
pub enum Verdict {
    Pass,
    /// Recorded, but no reason to refuse.
    Info,
    Warn,
    Fail,
}

pub struct Finding {
    pub name: &'static str,
    pub value: String,
    pub verdict: Verdict,
    /// What is wrong and what to do about it, for warnings and failures.
    pub advice: Option<String>,
}

pub fn power() -> (Power, Vec<Finding>) {
    let power = if cfg!(target_os = "macos") {
        macos_power()
    } else if cfg!(target_os = "linux") {
        linux_power()
    } else {
        Power::default()
    };

    let mut findings = Vec::new();
    if let Some(source) = power.source {
        let on_battery = source == PowerSource::Battery;
        findings.push(Finding {
            name: "power source",
            value: if on_battery { "battery" } else { "AC" }.to_owned(),
            verdict: if on_battery {
                Verdict::Warn
            } else {
                Verdict::Pass
            },
            advice: on_battery.then(|| {
                "running on battery: plug in for results comparable with other machines".to_owned()
            }),
        });
    }
    if let Some(on) = power.low_power_mode {
        findings.push(Finding {
            name: "low power mode",
            value: if on { "on" } else { "off" }.to_owned(),
            verdict: if on { Verdict::Fail } else { Verdict::Pass },
            advice: on.then(|| {
                "Low Power Mode throttles the CPU: turn it off in System Settings > Battery"
                    .to_owned()
            }),
        });
    }
    if let Some(profile) = &power.profile {
        let saver = matches!(profile.as_str(), "power-saver" | "low-power");
        findings.push(Finding {
            name: "power profile",
            value: profile.clone(),
            verdict: if saver { Verdict::Fail } else { Verdict::Pass },
            advice: saver.then(|| {
                "the power-saving profile throttles the CPU: switch to balanced or performance (`powerprofilesctl set balanced`)".to_owned()
            }),
        });
    }
    if let Some(governor) = &power.governor {
        // `powersave` is the normal default of intel_pstate and amd_pstate, so it is only recorded.
        findings.push(Finding {
            name: "cpufreq governor",
            value: governor.clone(),
            verdict: Verdict::Info,
            advice: None,
        });
    }
    (power, findings)
}

/// Samples overall CPU utilisation for a few seconds.
pub fn cpu_busy() -> (Option<f64>, Finding) {
    use sysinfo::{CpuRefreshKind, RefreshKind, System};

    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()),
    );
    sys.refresh_cpu_usage();
    std::thread::sleep(CPU_SAMPLE_TIME.max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL));
    sys.refresh_cpu_usage();

    let busy = (!sys.cpus().is_empty()).then(|| f64::from(sys.global_cpu_usage()));
    let finding = match busy {
        Some(pct) => {
            let too_busy = pct > MAX_CPU_BUSY_PCT;
            Finding {
                name: "cpu busy",
                value: format!("{pct:.1}% over {} s", CPU_SAMPLE_TIME.as_secs()),
                verdict: if too_busy { Verdict::Fail } else { Verdict::Pass },
                advice: too_busy.then(|| {
                    format!(
                        "the CPU is more than {MAX_CPU_BUSY_PCT}% busy: close browsers, IDEs, builds, video calls and other heavy applications"
                    )
                }),
            }
        }
        None => Finding {
            name: "cpu busy",
            value: "unknown".to_owned(),
            verdict: Verdict::Info,
            advice: None,
        },
    };
    (busy, finding)
}

fn macos_power() -> Power {
    let run = |args: &[&str]| -> Option<String> {
        let out = Command::new("pmset").args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };

    let settings = run(&["-g"]);
    let setting = |key: &str| {
        settings.as_deref()?.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some(key)).then(|| words.next().map(str::to_owned))?
        })
    };
    // `lowpowermode 1` on older releases, `powermode 1` (low) on newer ones.
    let low_power_mode = setting("lowpowermode")
        .or_else(|| setting("powermode"))
        .map(|v| v == "1");

    let source = run(&["-g", "batt"]).and_then(|batt| {
        let first = batt.lines().next()?;
        if first.contains("AC Power") {
            Some(PowerSource::Ac)
        } else if first.contains("Battery Power") {
            Some(PowerSource::Battery)
        } else {
            None
        }
    });

    Power {
        source,
        low_power_mode,
        profile: None,
        governor: None,
    }
}

fn linux_power() -> Power {
    let read = |path: &Path| {
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };

    let profile = Command::new("powerprofilesctl")
        .arg("get")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| read(Path::new("/sys/firmware/acpi/platform_profile")));

    let governor = read(Path::new(
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
    ));

    let mut mains = Vec::new();
    let mut discharging = false;
    if let Ok(supplies) = std::fs::read_dir("/sys/class/power_supply") {
        for supply in supplies.flatten() {
            let dir = supply.path();
            match read(&dir.join("type")).as_deref() {
                Some("Mains") => mains.push(read(&dir.join("online")).as_deref() == Some("1")),
                Some("Battery") => {
                    discharging |= read(&dir.join("status")).as_deref() == Some("Discharging")
                }
                _ => {}
            }
        }
    }
    let source = if discharging || (!mains.is_empty() && !mains.contains(&true)) {
        Some(PowerSource::Battery)
    } else if mains.contains(&true) {
        Some(PowerSource::Ac)
    } else {
        None
    };

    Power {
        source,
        low_power_mode: None,
        profile,
        governor,
    }
}
