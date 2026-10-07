# Rust crypto benchmarks

How fast do Rust crypto libraries seal and open ChaCha20-Poly1305 messages the way WireGuard uses
them? This compares them so we can choose what [sanstun](https://github.com/firezone/sanstun)
should use instead of `ring`. Transport data dominates a tunnel's throughput, so that is all this
measures: sealing and opening 1280-byte messages in place.

Results from every machine are collected at <https://firezone.github.io/rust-crypto-benchmarks/>.
CI adds runs from GitHub's x86_64 and aarch64 runners on every push to `main` and weekly.

## Submit results from your machine

You need [Rust](https://rustup.rs); the right toolchain is installed automatically.

1. Clone the repository and run the benchmarks:

   ```sh
   git clone https://github.com/firezone/rust-crypto-benchmarks
   cd rust-crypto-benchmarks
   cargo run
   ```

2. Commit the file it saved in `results/`. The last lines of its output show the exact commands.
3. Push the commit to your fork and open a pull request. CI checks the file.

`cargo run` refuses to measure a machine in a power-saving mode or one that is busy with other
work: close heavy applications and plug in laptops first. Implementations that do not build on
your machine (for example `aws-lc-rs` without a C toolchain) are skipped with a warning. The
results file names your machine after its CPU and OS (never its hostname); pass `--machine LABEL`
to choose another name. See `cargo run -- --help` for all options.

## Layout

```
src/            the runner: `cargo run`, `cargo run -- merge`, `cargo run -- validate`
harness/        the ChaCha20Poly1305 trait, test vectors, timing loop and per-implementation report
impls/<name>/   one standalone crate per library and version, with its own Cargo.lock
results/        one JSON file per run, from CI and from contributors
site/           the static results page (no build step, no external dependencies)
lint.sh         rustfmt, clippy and tests for every crate
```

The runner is the root package. Two semver-compatible versions of a crate cannot share a
`Cargo.lock`, so the implementations are not part of its workspace: each is its own crate that
depends on `harness` by path and pins its library with `=x.y.z`. The runner builds each one in
release mode, runs it and collects its report into
`results/<timestamp>-<machine>.json`. Each implementation is checked against RFC 8439 section 2.8.2
and a 1280-byte known answer before it is timed; a mismatch fails the run.

## Adding a library or a new version

1. Copy the closest directory under `impls/`, e.g. `cp -r impls/rustcrypto-0.11 impls/rustcrypto-0.12`.
   The directory name is the implementation's name in the results.
2. Set `name` in its `Cargo.toml` to the directory name with `.` replaced by `_`, and bump the pinned
   version in `[dependencies]` (keep each dependency on one line).
3. Adapt `src/main.rs` if the API changed, and run `cargo update --manifest-path impls/<name>/Cargo.toml`.
4. `cargo run -- --quick <name>` and `./lint.sh`, then commit including `Cargo.lock`.

Keep the old directory to compare versions side by side. A library limited to some architectures
declares them in its `Cargo.toml`:

```toml
[package.metadata.bench]
arches = ["x86_64", "aarch64"]
```

## Method

- A hand-rolled timing loop: about 0.5 s of warm-up that also calibrates the batch size, then 101
  batches of about 20 ms each. The median of the per-message batch times is reported with the
  quartiles and minimum. Inputs and outputs go through `std::hint::black_box`.
- Neither `-C target-cpu` nor `-C target-feature` is set: libraries choose their SIMD backends by
  runtime detection, as they would in a real deployment. Release builds use `lto = "fat"` and
  `codegen-units = 1` (see `.cargo/config.toml`), and the toolchain is pinned in `rust-toolchain.toml`.
- Sealing and opening happen in place, as a WireGuard implementation does them. Every iteration
  first copies a fresh message into the working buffer, which costs the same for every
  implementation. libcrux only offers out-of-place encryption, so its adapter additionally copies
  the input to a scratch buffer, which is the cost an in-place caller would pay.
- Before benchmarking, the runner records the power source, power profile or Low Power Mode,
  cpufreq governor and how busy the CPU is over 3 seconds. It refuses to run in a power-saving mode
  or above 10% CPU load unless given `--force`, in which case the run is flagged on the site.
