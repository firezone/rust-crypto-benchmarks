//! Shared harness for benchmarking the primitives WireGuard needs.
//!
//! Each crate under `impls/` implements the traits below for the operations its
//! library supports and hands them to a [`Suite`], which checks every operation
//! against known-answer vectors before benchmarking it.

mod json;
mod measure;
mod meta;
mod vectors;

use std::hint::black_box;
use std::path::PathBuf;
use std::process::ExitCode;

pub use measure::Config;

/// Length of a Poly1305 tag.
pub const TAG_LEN: usize = 16;

/// The data-plane AEAD.
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

/// The AEAD protecting cookie replies.
pub trait XChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self;

    /// Same layout as [`ChaCha20Poly1305::seal_in_place`].
    fn seal_in_place(&self, nonce: &[u8; 24], aad: &[u8], in_out: &mut [u8]);
}

/// Diffie-Hellman with a long-lived (static) secret, as in the WireGuard handshake.
pub trait X25519 {
    fn new(secret: &[u8; 32]) -> Self;

    fn diffie_hellman(&self, public: &[u8; 32]) -> [u8; 32];
}

/// Unkeyed BLAKE2s with a 32-byte digest.
pub trait Blake2s {
    fn hash(data: &[u8]) -> [u8; 32];
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

pub struct Measurement {
    pub op: &'static str,
    pub size: usize,
    pub stats: measure::Stats,
}

struct Op {
    name: &'static str,
    verify: fn() -> Result<(), String>,
    bench: fn(&Config) -> Vec<Measurement>,
}

pub struct Suite {
    library: &'static str,
    manifest: Manifest,
    ops: Vec<Op>,
}

/// Data-plane sizes: a small packet, the IPv6 minimum MTU and the payload of a 1500-byte link MTU.
pub const DATA_SIZES: [usize; 3] = [64, 1280, 1420];
/// An encrypted cookie in a WireGuard cookie reply.
pub const COOKIE_SIZE: usize = 32;
pub const BLAKE2S_SIZES: [usize; 2] = [64, 1280];

impl Manifest {
    pub fn name(&self) -> &'static str {
        std::path::Path::new(self.dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(self.dir)
    }
}

impl Suite {
    pub fn new(library: &'static str, manifest: Manifest) -> Self {
        Self {
            library,
            manifest,
            ops: Vec::new(),
        }
    }

    pub fn chacha20poly1305<T: ChaCha20Poly1305>(mut self) -> Self {
        self.ops.push(Op {
            name: "ChaCha20-Poly1305",
            verify: vectors::chacha20poly1305::<T>,
            bench: bench_chacha20poly1305::<T>,
        });
        self
    }

    pub fn xchacha20poly1305<T: XChaCha20Poly1305>(mut self) -> Self {
        self.ops.push(Op {
            name: "XChaCha20-Poly1305",
            verify: vectors::xchacha20poly1305::<T>,
            bench: bench_xchacha20poly1305::<T>,
        });
        self
    }

    pub fn x25519<T: X25519>(mut self) -> Self {
        self.ops.push(Op {
            name: "X25519",
            verify: vectors::x25519::<T>,
            bench: bench_x25519::<T>,
        });
        self
    }

    pub fn blake2s<T: Blake2s>(mut self) -> Self {
        self.ops.push(Op {
            name: "BLAKE2s-256",
            verify: vectors::blake2s::<T>,
            bench: bench_blake2s::<T>,
        });
        self
    }

    /// Verifies every registered operation, then benchmarks them and writes the JSON report.
    ///
    /// Arguments: `--out <path>` (default: print JSON to stdout), `--quick` (also `BENCH_QUICK=1`),
    /// `--verify-only`.
    pub fn run(self) -> ExitCode {
        let mut out = None::<PathBuf>;
        let mut quick = std::env::var("BENCH_QUICK").is_ok_and(|v| !v.is_empty() && v != "0");
        let mut verify_only = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--out" => out = args.next().map(PathBuf::from),
                "--quick" => quick = true,
                "--verify-only" => verify_only = true,
                other => {
                    eprintln!("unknown argument: {other}");
                    return ExitCode::FAILURE;
                }
            }
        }

        let mut failed = false;
        for op in &self.ops {
            match (op.verify)() {
                Ok(()) => eprintln!("[{}] {}: test vectors OK", self.manifest.name(), op.name),
                Err(e) => {
                    eprintln!(
                        "[{}] {}: TEST VECTOR MISMATCH: {e}",
                        self.manifest.name(),
                        op.name
                    );
                    failed = true;
                }
            }
        }
        if failed {
            eprintln!(
                "[{}] refusing to benchmark an incorrect implementation",
                self.manifest.name()
            );
            return ExitCode::FAILURE;
        }
        if verify_only {
            return ExitCode::SUCCESS;
        }

        let config = if quick {
            Config::quick()
        } else {
            Config::full()
        };
        let mut measurements = Vec::new();
        for op in &self.ops {
            for m in (op.bench)(&config) {
                eprintln!(
                    "[{}] {:<26} {:>5} B  {:>12.1} ns/op{}",
                    self.manifest.name(),
                    m.op,
                    m.size,
                    m.stats.median_ns,
                    m.stats
                        .mib_per_s(m.size)
                        .map(|t| format!("  {t:>9.1} MiB/s"))
                        .unwrap_or_default()
                );
                measurements.push(m);
            }
        }

        let report = json::report(
            self.library,
            &self.manifest,
            &meta::collect(quick),
            &config,
            &measurements,
        );
        match out {
            Some(path) => {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Err(e) = std::fs::write(&path, report) {
                    eprintln!("failed to write {}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            }
            None => println!("{report}"),
        }
        ExitCode::SUCCESS
    }
}

fn input(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Every iteration copies a fresh message into the working buffer before sealing or opening it.
/// Opening needs this to get a valid ciphertext each time; sealing does it too so both directions
/// carry the same (small, implementation-independent) overhead.
fn bench_chacha20poly1305<T: ChaCha20Poly1305>(config: &Config) -> Vec<Measurement> {
    let cipher = T::new(&[0x42; 32]);
    let nonce = [0u8; 12];
    let mut out = Vec::new();

    for size in DATA_SIZES {
        let mut plaintext = input(size);
        plaintext.extend_from_slice(&[0; TAG_LEN]);
        let mut sealed = plaintext.clone();
        cipher.seal_in_place(&nonce, &[], &mut sealed);
        let mut buf = vec![0u8; size + TAG_LEN];

        let stats = measure::run(config, || {
            buf.copy_from_slice(black_box(&plaintext));
            cipher.seal_in_place(black_box(&nonce), black_box(&[]), black_box(&mut buf));
        });
        out.push(Measurement {
            op: "chacha20poly1305_seal",
            size,
            stats,
        });

        let stats = measure::run(config, || {
            buf.copy_from_slice(black_box(&sealed));
            let res = cipher.open_in_place(black_box(&nonce), black_box(&[]), black_box(&mut buf));
            assert!(res.is_ok());
        });
        out.push(Measurement {
            op: "chacha20poly1305_open",
            size,
            stats,
        });
    }
    out
}

fn bench_xchacha20poly1305<T: XChaCha20Poly1305>(config: &Config) -> Vec<Measurement> {
    let cipher = T::new(&[0x42; 32]);
    let nonce = [7u8; 24];
    let mac1 = [9u8; 16];
    let mut plaintext = input(COOKIE_SIZE);
    plaintext.extend_from_slice(&[0; TAG_LEN]);
    let mut buf = plaintext.clone();

    let stats = measure::run(config, || {
        buf.copy_from_slice(black_box(&plaintext));
        cipher.seal_in_place(black_box(&nonce), black_box(&mac1), black_box(&mut buf));
    });
    vec![Measurement {
        op: "xchacha20poly1305_seal",
        size: COOKIE_SIZE,
        stats,
    }]
}

fn bench_x25519<T: X25519>(config: &Config) -> Vec<Measurement> {
    let secret = T::new(&vectors::X25519_ALICE_SECRET);
    let public = vectors::X25519_BOB_PUBLIC;

    let stats = measure::run(config, || {
        black_box(secret.diffie_hellman(black_box(&public)));
    });
    vec![Measurement {
        op: "x25519",
        size: 0,
        stats,
    }]
}

fn bench_blake2s<T: Blake2s>(config: &Config) -> Vec<Measurement> {
    BLAKE2S_SIZES
        .into_iter()
        .map(|size| {
            let data = input(size);
            let stats = measure::run(config, || {
                black_box(T::hash(black_box(&data)));
            });
            Measurement {
                op: "blake2s256",
                size,
                stats,
            }
        })
        .collect()
}
