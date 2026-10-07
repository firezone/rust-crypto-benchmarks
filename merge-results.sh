#!/usr/bin/env bash
# Merges results/<arch>/<impl>.json into the single file the site loads.
# Usage: ./merge-results.sh [results-dir] [output]
set -euo pipefail
cd "$(dirname "$0")"

results=${1:-results}
output=${2:-site/results.json}

shopt -s nullglob
files=("$results"/*/*.json)
if [ ${#files[@]} -eq 0 ]; then
    echo "no results in $results/" >&2
    exit 1
fi
jq -s '{ generated: (now | todate), runs: . }' "${files[@]}" >"$output"
echo "Wrote ${#files[@]} runs to $output"
