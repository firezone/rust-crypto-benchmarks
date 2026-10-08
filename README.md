# Rust crypto benchmarks

How fast do Rust crypto libraries seal and open ChaCha20-Poly1305 messages the way WireGuard uses
them?

## Submit results from your machine

You need [Rust](https://rustup.rs); the right toolchain is installed automatically. Linux, macOS
and Windows are supported. On Windows, `ring` needs the MSVC build tools (which rustup offers to
install) and `aws-lc-rs` additionally needs CMake and NASM; without them those implementations
are skipped.

1. Clone the repository and run the benchmarks:

   ```sh
   git clone https://github.com/firezone/rust-crypto-benchmarks
   cd rust-crypto-benchmarks
   cargo run
   ```

2. Commit the file it saved in `results/`. The last lines of its output show the exact commands.
3. Push the commit to your fork and open a pull request.

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

- Every implementation is measured in 7 interleaved rounds: each round runs every implementation
  once, in an order rotated by one position per round, so drift over time (thermals, background
  work) spreads evenly instead of always hitting whichever runs last. Within a round, a
  hand-rolled timing loop warms up for 0.2 s (which also calibrates the batch size) and then takes
  15 batches of about 20 ms. The reported time is the median of the round medians; the "±" is half
  the range of the round medians, relative to that median. A run is rejected when the
  interquartile range of an implementation's round medians exceeds 10% of the median, because that
  means the machine was busy or throttled. Inputs and outputs go through `std::hint::black_box`.
- On hybrid CPUs, the core type a process lands on can matter more than the library. So on Linux
  and Windows every benchmark process is pinned to the same logical CPU: the one with the highest
  maximum frequency (Linux) or efficiency class (Windows), lowest index on ties. Windows processes
  are also exempted from power throttling (EcoQoS). macOS has no affinity API, so there the
  benchmark raises its QoS class to user-interactive, which keeps it on the performance cores.
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
