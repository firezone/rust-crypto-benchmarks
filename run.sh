#!/usr/bin/env bash
# Builds every crate under impls/, verifies it against the test vectors and benchmarks it,
# writing results/<arch>/<impl>.json.
#
# Usage: ./run.sh [--quick] [impl ...]
#   --quick   fewer and shorter samples, for checking that everything works
#   impl      only run these directories under impls/ (default: all)
set -euo pipefail
cd "$(dirname "$0")"

quick=()
only=()
for arg in "$@"; do
    case "$arg" in
    --quick) quick=(--quick) ;;
    *) only+=("${arg%/}") ;;
    esac
done

arch=$(uname -m)
case "$arch" in
arm64) arch=aarch64 ;;
amd64) arch=x86_64 ;;
esac

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
BENCH_GIT_COMMIT=${BENCH_GIT_COMMIT:-$(git rev-parse --verify -q HEAD || echo unknown)}
export BENCH_GIT_COMMIT
out_dir=results/$arch
mkdir -p "$out_dir"

warn() {
    echo "warning: $*" >&2
    if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; fi
}

built=()
skipped=()
for dir in impls/*/; do
    name=$(basename "$dir")
    if [ ${#only[@]} -gt 0 ] && [[ " ${only[*]} " != *" $name "* ]]; then
        continue
    fi

    # Optional `arches = [...]` under `[package.metadata.bench]`.
    arches=$(sed -n 's/^arches *= *\[\(.*\)\]/\1/p' "$dir/Cargo.toml")
    if [ -n "$arches" ] && [[ "$arches" != *"\"$arch\""* ]]; then
        echo "Skipping $name: not supported on $arch"
        skipped+=("$name (unsupported on $arch)")
        continue
    fi

    echo "Building $name"
    if cargo build --release --locked --manifest-path "$dir/Cargo.toml"; then
        built+=("$name")
    else
        warn "$name failed to build on $arch, skipping it"
        skipped+=("$name (build failed)")
    fi
done

# Benchmark only after everything is built, so no compiler competes for the CPU.
failed=()
for name in ${built[@]+"${built[@]}"}; do
    echo "Running $name"
    if ! "$CARGO_TARGET_DIR/release/${name//./_}" ${quick[@]+"${quick[@]}"} --out "$out_dir/$name.json"; then
        failed+=("$name")
    fi
done

echo
echo "Results in $out_dir/: ${built[*]:-none}"
if [ ${#skipped[@]} -gt 0 ]; then
    printf 'Skipped: %s\n' "${skipped[@]}"
fi
if [ ${#failed[@]} -gt 0 ]; then
    printf 'FAILED (test vectors or benchmark): %s\n' "${failed[@]}" >&2
    exit 1
fi
