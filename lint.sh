#!/usr/bin/env bash
# Runs rustfmt, clippy and the unit tests on the runner, the harness and every implementation.
set -euo pipefail
cd "$(dirname "$0")"

# One shared target directory instead of one per excluded crate.
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target/lint}

for manifest in Cargo.toml harness/Cargo.toml impls/*/Cargo.toml; do
    dir=$(dirname "$manifest")
    echo "Checking ${dir/#./runner}"
    cargo fmt --check --manifest-path "$manifest"
    cargo clippy --locked --all-targets --manifest-path "$manifest" -- -D warnings
done
cargo test --locked --quiet
