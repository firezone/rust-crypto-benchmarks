# Rust crypto benchmarks

Compares Rust cryptography libraries on exactly the primitives WireGuard needs, so we can choose
what [sanstun](https://github.com/firezone/sanstun) should use instead of `ring`:

| Operation | Sizes | Why |
| --- | --- | --- |
| ChaCha20-Poly1305 seal and open (in place) | 64, 1280, 1420 bytes | Transport data packets |
| XChaCha20-Poly1305 seal | 32 bytes, 16 bytes AD | Cookie reply |
| X25519 with a static secret | | Handshake |
| BLAKE2s-256 | 64, 1280 bytes | Handshake hashing, MACs and HKDF |

Results are published at <https://firezone.github.io/rust-crypto-benchmarks/>, refreshed on every
push to `main` and weekly, on x86_64 (`ubuntu-24.04`) and aarch64 (`ubuntu-24.04-arm`) GitHub
runners.

## Layout

```
harness/           traits, test vectors, timing loop and JSON output
impls/<name>/      one standalone crate per library and version, with its own Cargo.lock
run.sh             builds and runs every crate under impls/, writes results/<arch>/<name>.json
merge-results.sh   merges results/ into site/results.json
site/              static results page (no build step, no external dependencies)
lint.sh            rustfmt and clippy for every crate
```

Two semver-compatible versions of a crate cannot share a `Cargo.lock`, so there is deliberately no
Cargo workspace: every implementation is its own crate, depends on `harness` by path and pins its
libraries with `=x.y.z`. The JSON records the versions that were actually resolved.

Each implementation is a small binary whose `main` registers the operations its library offers:

```rust
fn main() -> ExitCode {
    harness::Suite::new("graviola", harness::manifest!())
        .chacha20poly1305::<ChaCha20Poly1305>()
        .xchacha20poly1305::<XChaCha20Poly1305>()
        .x25519::<X25519>()
        .run()
}
```

Before timing anything, the harness checks every registered operation against RFC 8439 section 2.8.2,
draft-irtf-cfrg-xchacha-03 section A.3.1, RFC 7748 sections 5.2 and 6.1 and RFC 7693 appendix B,
plus known answers for the benchmark sizes. A mismatch aborts that implementation and makes
`run.sh` exit non-zero.

## Running

```sh
./run.sh --quick          # all implementations, few samples: a smoke test in a minute or two
./run.sh                  # what CI runs
./run.sh ring-0.17        # only some implementations
./merge-results.sh && python3 -m http.server -d site   # view the results page locally
```

`run.sh` builds everything first and benchmarks afterwards, so the compiler does not compete with
the measurements. An implementation that fails to build, or whose `arches` metadata excludes the
host, is skipped with a warning.

## Adding a library or a new version

1. Copy the closest directory under `impls/`, e.g. `cp -r impls/rustcrypto-0.11 impls/rustcrypto-0.12`.
   The directory name is the implementation's name in the results.
2. Set `name` in its `Cargo.toml` to the directory name with `.` replaced by `_`, and bump the pinned
   versions in `[dependencies]` (keep each dependency on one line).
3. Adapt `src/main.rs` if the API changed, and run `cargo update --manifest-path impls/<name>/Cargo.toml`
   to refresh its lockfile.
4. `./run.sh --quick <name>` and `./lint.sh`, then commit including `Cargo.lock`.

Keep the old directory around to compare versions side by side. A library limited to some
architectures declares them in its `Cargo.toml`:

```toml
[package.metadata.bench]
arches = ["x86_64", "aarch64"]
```

## Method

- A hand-rolled timing loop: about 0.5 s of warm-up that also calibrates the batch size, then 101
  batches of about 20 ms each. The median of the per-operation batch times is reported along with the
  quartiles and minimum. Inputs and outputs go through `std::hint::black_box`.
- Neither `-C target-cpu` nor `-C target-feature` is set: libraries choose their SIMD backends by
  runtime detection, as they would in a real deployment. Release builds use `lto = "fat"` and
  `codegen-units = 1` (see `.cargo/config.toml`), and the toolchain is pinned in `rust-toolchain.toml`.
- The AEAD interfaces are in place, as a WireGuard implementation uses them. Every iteration first
  copies a fresh message into the working buffer, which costs the same for every implementation.
  libcrux only offers out-of-place encryption, so its adapter additionally copies the input to a
  scratch buffer, which is the cost an in-place caller would pay.
- Keys are set up once outside the loop. X25519 parses the static secret once and the peer's public
  key on every operation.
- `ring` only supports ephemeral X25519 secrets, so it cannot take a static secret or be checked
  against test vectors; it is benchmarked for ChaCha20-Poly1305 only.
