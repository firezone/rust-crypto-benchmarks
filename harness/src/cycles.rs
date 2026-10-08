//! The CPU cycles the benchmark thread spends, from a hardware counter the platform exposes
//! without privileges: `perf_event_open` on Linux and `proc_pid_rusage` on Apple silicon.
//! Windows has none that user programs can read (`QueryThreadCycleTime` returns the TSC, which
//! ticks at a constant rate whatever the core's clock does), and neither have Intel Macs.

use std::hint::black_box;

pub struct Counter(imp::Counter);

impl Counter {
    pub const METHOD: &str = imp::METHOD;

    /// Fails with the reason this platform cannot count cycles.
    pub fn open() -> Result<Self, String> {
        let counter = imp::Counter::open()?;
        let before = counter.read();
        let mut x = 0u64;
        for i in 0..100_000u64 {
            x = black_box(x.wrapping_add(i));
        }
        if counter.read() == before {
            return Err("the counter does not advance".to_owned());
        }
        Ok(Self(counter))
    }

    pub fn read(&self) -> u64 {
        self.0.read()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs::File;
    use std::io::{self, Read};
    use std::os::fd::FromRawFd;

    pub const METHOD: &str = "perf_event_open, user mode only";

    /// The first 64 bytes of `struct perf_event_attr`, which the kernel accepts as the complete
    /// version 0 of it.
    #[repr(C)]
    struct Attr {
        type_: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        flags: u64,
        wakeup_events: u32,
        bp_type: u32,
        bp_addr: u64,
    }

    const PERF_TYPE_HARDWARE: u32 = 0;
    const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
    const EXCLUDE_KERNEL: u64 = 1 << 5;
    const EXCLUDE_HV: u64 = 1 << 6;
    const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 8;

    pub struct Counter(File);

    impl Counter {
        pub fn open() -> Result<Self, String> {
            let attr = Attr {
                type_: PERF_TYPE_HARDWARE,
                size: size_of::<Attr>() as u32,
                config: PERF_COUNT_HW_CPU_CYCLES,
                sample_period: 0,
                sample_type: 0,
                read_format: 0,
                flags: EXCLUDE_KERNEL | EXCLUDE_HV,
                wakeup_events: 0,
                bp_type: 0,
                bp_addr: 0,
            };
            // SAFETY: `attr` declares its own size and outlives the call; pid 0 and cpu -1
            // count the calling thread wherever it runs.
            let fd = unsafe {
                libc::syscall(
                    libc::SYS_perf_event_open,
                    &raw const attr,
                    0 as libc::pid_t,
                    -1 as libc::c_int,
                    -1 as libc::c_int,
                    PERF_FLAG_FD_CLOEXEC,
                )
            };
            if fd < 0 {
                let err = io::Error::last_os_error();
                let hint = match err.raw_os_error() {
                    Some(libc::EACCES | libc::EPERM) => {
                        " (set kernel.perf_event_paranoid to 2 or lower)"
                    }
                    Some(libc::ENOENT | libc::EOPNOTSUPP) => {
                        " (no hardware counters, as in most virtual machines)"
                    }
                    _ => "",
                };
                return Err(format!("perf_event_open: {err}{hint}"));
            }
            // SAFETY: `fd` is a descriptor this process just received and owns.
            Ok(Self(unsafe { File::from_raw_fd(fd as i32) }))
        }

        pub fn read(&self) -> u64 {
            let mut buf = [0u8; 8];
            match (&self.0).read_exact(&mut buf) {
                Ok(()) => u64::from_ne_bytes(buf),
                Err(_) => 0,
            }
        }
    }
}

#[cfg(target_vendor = "apple")]
mod imp {
    pub const METHOD: &str = "proc_pid_rusage, whole process";

    pub struct Counter(libc::pid_t);

    impl Counter {
        pub fn open() -> Result<Self, String> {
            // SAFETY: Always safe to call.
            Ok(Self(unsafe { libc::getpid() }))
        }

        pub fn read(&self) -> u64 {
            // SAFETY: `info` is a valid `rusage_info_v4`, which is what `RUSAGE_INFO_V4` fills.
            unsafe {
                let mut info: libc::rusage_info_v4 = std::mem::zeroed();
                if libc::proc_pid_rusage(self.0, libc::RUSAGE_INFO_V4, (&raw mut info).cast()) == 0
                {
                    info.ri_cycles
                } else {
                    0
                }
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
mod imp {
    pub const METHOD: &str = "";

    pub struct Counter;

    impl Counter {
        pub fn open() -> Result<Self, String> {
            Err("no cycle counter is readable without privileges on this system".to_owned())
        }

        pub fn read(&self) -> u64 {
            0
        }
    }
}
