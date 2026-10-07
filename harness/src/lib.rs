//! Shared harness for benchmarking ChaCha20-Poly1305 as WireGuard uses it.
//!
//! Each crate under `impls/` implements [`ChaCha20Poly1305`] for its library and calls [`run`],
//! which checks the implementation against known-answer vectors before benchmarking it.
//!
//! Progress goes to stderr, one event per line, which the runner turns into its status display:
//! `verify: ok`, `verify: FAILED: <reason>` and `bench <op>`.

mod json;
mod measure;
mod vectors;

use std::hint::black_box;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

pub use measure::Config;

/// Length of a Poly1305 tag.
pub const TAG_LEN: usize = 16;

/// The message size every operation is measured at: the IPv6 minimum MTU.
pub const SIZE: usize = 1280;

/// The AEAD protecting WireGuard transport data.
pub trait ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self;

    /// Encrypts `in_out[..len - 16]` in place and writes the tag to `in_out[len - 16..]`.
    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]);

    /// Decrypts `in_out[..len - 16]` in place after verifying the tag in `in_out[len - 16..]`.
    fn open_in_place(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<(), OpenError>;
}

#[derive(Debug)]
pub struct OpenError;

/// The implementation's own `Cargo.toml` and `Cargo.lock`, used to report exact crate versions.
pub struct Manifest {
    /// The crate's directory; its name identifies the implementation in the results.
    pub dir: &'static str,
    pub cargo_toml: &'static str,
    pub cargo_lock: &'static str,
}

#[macro_export]
macro_rules! manifest {
    () => {
        $crate::Manifest {
            dir: env!("CARGO_MANIFEST_DIR"),
            cargo_toml: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")),
            cargo_lock: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock")),
        }
    };
}

impl Manifest {
    pub fn name(&self) -> &'static str {
        std::path::Path::new(self.dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(self.dir)
    }
}

pub struct Measurement {
    pub op: &'static str,
    pub stats: measure::Stats,
}

/// Verifies `T`, then benchmarks it and writes the JSON report.
///
/// Arguments: `--out <path>` (default: print JSON to stdout), `--quick`, `--verify-only`, and
/// `--warmup-ms`, `--samples` and `--sample-ms` to override the measurement budget.
pub fn run<T: ChaCha20Poly1305>(library: &'static str, manifest: Manifest) -> ExitCode {
    let mut out = None::<PathBuf>;
    let mut config = Config::full();
    let mut verify_only = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut number = || {
            args.next()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| *n > 0)
        };
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--quick" => config = Config::quick(),
            "--verify-only" => verify_only = true,
            "--warmup-ms" => match number() {
                Some(ms) => config.warmup = Duration::from_millis(ms),
                None => return usage(&arg),
            },
            "--samples" => match number() {
                Some(n) => config.samples = n as usize,
                None => return usage(&arg),
            },
            "--sample-ms" => match number() {
                Some(ms) => config.sample_time = Duration::from_millis(ms),
                None => return usage(&arg),
            },
            other => return usage(other),
        }
    }
    prefer_fast_cores();

    if let Err(e) = vectors::chacha20poly1305::<T>() {
        eprintln!("verify: FAILED: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("verify: ok");
    if verify_only {
        return ExitCode::SUCCESS;
    }

    let cipher = T::new(&[0x42; 32]);
    let nonce = [0u8; 12];
    let plaintext = input(SIZE);
    let mut sealed = plaintext.clone();
    cipher.seal_in_place(&nonce, &[], &mut sealed);
    let mut buf = plaintext.clone();

    // Every iteration copies a fresh message into the working buffer. Opening needs this to get a
    // valid ciphertext each time; sealing does it too so both directions carry the same (small,
    // implementation-independent) overhead.
    eprintln!("bench seal");
    let seal = measure::run(&config, || {
        buf.copy_from_slice(black_box(&plaintext));
        cipher.seal_in_place(black_box(&nonce), black_box(&[]), black_box(&mut buf));
    });
    eprintln!("bench open");
    let open = measure::run(&config, || {
        buf.copy_from_slice(black_box(&sealed));
        let res = cipher.open_in_place(black_box(&nonce), black_box(&[]), black_box(&mut buf));
        assert!(res.is_ok());
    });

    let measurements = [
        Measurement {
            op: "seal",
            stats: seal,
        },
        Measurement {
            op: "open",
            stats: open,
        },
    ];
    let report = json::report(library, &manifest, &config, &measurements);
    match out {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, report) {
                eprintln!("failed to write {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
        None => print!("{report}"),
    }
    ExitCode::SUCCESS
}

fn usage(arg: &str) -> ExitCode {
    eprintln!("invalid argument: {arg}");
    ExitCode::FAILURE
}

/// macOS has no CPU affinity, but the highest QoS class keeps a thread on the performance cores.
/// Elsewhere the runner pins the process to a core.
fn prefer_fast_cores() {
    #[cfg(target_vendor = "apple")]
    {
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
        }
        // SAFETY: Only changes the scheduling class of the calling thread.
        if unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) } != 0 {
            eprintln!("warning: could not raise the thread's QoS class");
        }
    }
}

/// A message of `len` bytes followed by room for the tag.
fn input(len: usize) -> Vec<u8> {
    let mut message: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    message.extend_from_slice(&[0; TAG_LEN]);
    message
}
