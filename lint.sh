#!/usr/bin/env bash
# Runs rustfmt and clippy on the harness and every implementation.
set -euo pipefail
cd "$(dirname "$0")"

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
for manifest in harness/Cargo.toml impls/*/Cargo.toml; do
    echo "Checking ${manifest%/Cargo.toml}"
    cargo fmt --check --manifest-path "$manifest"
    cargo clippy --locked --all-targets --manifest-path "$manifest" -- -D warnings
done
