//! Keeps every benchmark process on the same, fastest core. On hybrid CPUs the scheduler would
//! otherwise place each implementation on whichever core type is free, which swamps the
//! differences between implementations.
//!
//! Linux and Windows pin the process to one logical CPU. macOS has no affinity API; there the
//! harness raises its own QoS class to stay on the performance cores.

use std::process::{Child, Command};

use crate::schema::{Placement, PlacementMethod};

/// Chooses where to run, honouring `--cpu` if given.
pub fn choose(requested: Option<u32>) -> Result<Option<Placement>, String> {
    imp::choose(requested)
}

/// Applies the placement to a command before it is spawned.
pub fn before_spawn(command: &mut Command, placement: Option<&Placement>) {
    imp::before_spawn(command, placement)
}

/// Applies the placement to a freshly spawned process.
pub fn after_spawn(child: &Child, placement: Option<&Placement>) -> std::io::Result<()> {
    imp::after_spawn(child, placement)
}

/// A human-readable description for the CLI.
pub fn describe(placement: Option<&Placement>) -> String {
    let Some(p) = placement else {
        return "not pinned (unsupported on this system)".to_owned();
    };
    match p.method {
        PlacementMethod::Qos => "QoS user-interactive (prefers performance cores)".to_owned(),
        PlacementMethod::Affinity => {
            let mut text = format!("pinned to CPU {}", p.cpu.unwrap_or_default());
            if let Some(mhz) = p.cpu_max_mhz {
                text.push_str(&format!(
                    ", max {} MHz",
                    crate::ui::thousands(f64::from(mhz))
                ));
            }
            if let Some(class) = p.efficiency_class {
                text.push_str(&format!(", efficiency class {class}"));
            }
            text
        }
    }
}

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
fn affinity(cpu: u32, cpu_max_mhz: Option<u32>, efficiency_class: Option<u8>) -> Placement {
    Placement {
        method: PlacementMethod::Affinity,
        cpu: Some(cpu),
        cpu_max_mhz,
        efficiency_class,
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    use super::affinity;
    use crate::schema::Placement;

    /// The CPUs this process may run on.
    fn allowed() -> Vec<u32> {
        // SAFETY: `set` is a plain bitset that `sched_getaffinity` fills in.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            if libc::sched_getaffinity(0, size_of::<libc::cpu_set_t>(), &mut set) != 0 {
                return vec![0];
            }
            (0..libc::CPU_SETSIZE as u32)
                .filter(|&cpu| libc::CPU_ISSET(cpu as usize, &set))
                .collect()
        }
    }

    fn max_khz(cpu: u32) -> Option<u32> {
        std::fs::read_to_string(format!(
            "/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq"
        ))
        .ok()?
        .trim()
        .parse()
        .ok()
    }

    pub fn choose(requested: Option<u32>) -> Result<Option<Placement>, String> {
        let allowed = allowed();
        let cpu = match requested {
            Some(cpu) if allowed.contains(&cpu) => cpu,
            Some(cpu) => {
                return Err(format!(
                    "CPU {cpu} is not available to this process (available: {})",
                    allowed
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            // Highest maximum frequency, lowest index on ties.
            None => allowed
                .iter()
                .copied()
                .max_by_key(|&cpu| (max_khz(cpu).unwrap_or(0), std::cmp::Reverse(cpu)))
                .unwrap_or(0),
        };
        Ok(Some(affinity(
            cpu,
            max_khz(cpu).map(|khz| khz / 1000),
            None,
        )))
    }

    pub fn before_spawn(command: &mut Command, placement: Option<&Placement>) {
        let Some(cpu) = placement.and_then(|p| p.cpu) else {
            return;
        };
        // SAFETY: The closure only makes async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(move || {
                let mut set: libc::cpu_set_t = std::mem::zeroed();
                libc::CPU_SET(cpu as usize, &mut set);
                if libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }

    pub fn after_spawn(_: &Child, _: Option<&Placement>) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};

    use windows_sys::Win32::System::SystemInformation::{
        GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessAffinityMask,
        SetProcessInformation,
    };

    use super::affinity;
    use crate::schema::Placement;

    /// `(logical processor index, efficiency class)` of every CPU in processor group 0, which
    /// is where a process runs unless it asks for another group.
    fn cpus() -> Vec<(u32, u8)> {
        let mut len = 0u32;
        // SAFETY: A null buffer of length 0 only queries the required length.
        unsafe {
            GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut len, std::ptr::null_mut(), 0)
        };
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` is 8-byte aligned and at least `len` bytes long.
        let ok = unsafe {
            GetSystemCpuSetInformation(
                buf.as_mut_ptr().cast(),
                len,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok == 0 {
            return Vec::new();
        }

        let base = buf.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        let mut cpus = Vec::new();
        while offset + size_of::<SYSTEM_CPU_SET_INFORMATION>() <= len as usize {
            // SAFETY: Windows wrote a sequence of variable-sized entries, each starting with
            // its size, into the first `len` bytes of `buf`.
            let info = unsafe {
                std::ptr::read_unaligned(base.add(offset).cast::<SYSTEM_CPU_SET_INFORMATION>())
            };
            if info.Size == 0 {
                break;
            }
            // SAFETY: CPU sets are the only kind of entry Windows defines.
            let set = unsafe { info.Anonymous.CpuSet };
            if set.Group == 0 && set.LogicalProcessorIndex < 64 {
                cpus.push((u32::from(set.LogicalProcessorIndex), set.EfficiencyClass));
            }
            offset += info.Size as usize;
        }
        cpus
    }

    pub fn choose(requested: Option<u32>) -> Result<Option<Placement>, String> {
        let cpus = cpus();
        let chosen = match requested {
            Some(cpu) => match cpus.iter().find(|(index, _)| *index == cpu) {
                Some(found) => *found,
                None if cpus.is_empty() && cpu < 64 => (cpu, 0),
                None => return Err(format!("CPU {cpu} does not exist in processor group 0")),
            },
            // Highest efficiency class (the most performant cores), lowest index on ties.
            None => cpus
                .iter()
                .copied()
                .max_by_key(|&(index, class)| (class, std::cmp::Reverse(index)))
                .unwrap_or((0, 0)),
        };
        let class = (!cpus.is_empty()).then_some(chosen.1);
        Ok(Some(affinity(chosen.0, None, class)))
    }

    pub fn before_spawn(_: &mut Command, _: Option<&Placement>) {}

    pub fn after_spawn(child: &Child, placement: Option<&Placement>) -> std::io::Result<()> {
        let handle = child.as_raw_handle();
        // Opt out of EcoQoS, which would treat the benchmark as background work.
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: 0,
        };
        // SAFETY: `handle` is the child's process handle with full access, and `state`
        // outlives the call.
        unsafe {
            SetProcessInformation(
                handle,
                ProcessPowerThrottling,
                (&raw const state).cast(),
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            );
        }
        if let Some(cpu) = placement.and_then(|p| p.cpu) {
            // SAFETY: As above; the mask selects one existing CPU of group 0.
            if unsafe { SetProcessAffinityMask(handle, 1usize << cpu) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[cfg(target_vendor = "apple")]
mod imp {
    use std::process::{Child, Command};

    use crate::schema::{Placement, PlacementMethod};

    pub fn choose(_: Option<u32>) -> Result<Option<Placement>, String> {
        Ok(Some(Placement {
            method: PlacementMethod::Qos,
            cpu: None,
            cpu_max_mhz: None,
            efficiency_class: None,
        }))
    }

    pub fn before_spawn(_: &mut Command, _: Option<&Placement>) {}

    pub fn after_spawn(_: &Child, _: Option<&Placement>) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", windows, target_vendor = "apple")))]
mod imp {
    use std::process::{Child, Command};

    use crate::schema::Placement;

    pub fn choose(_: Option<u32>) -> Result<Option<Placement>, String> {
        Ok(None)
    }

    pub fn before_spawn(_: &mut Command, _: Option<&Placement>) {}

    pub fn after_spawn(_: &Child, _: Option<&Placement>) -> std::io::Result<()> {
        Ok(())
    }
}
