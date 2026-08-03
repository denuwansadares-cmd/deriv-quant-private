#!/usr/bin/env bash
set -euo pipefail
export CARGO_INCREMENTAL=0
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"
first="${TMPDIR:-/tmp}/deriv-repro-a"
second="${TMPDIR:-/tmp}/deriv-repro-b"
mkdir -p "$first" "$second"
CARGO_TARGET_DIR="$first" cargo build --locked --release --no-default-features --bin deriv_platform
CARGO_TARGET_DIR="$second" cargo build --locked --release --no-default-features --bin deriv_platform
cmp "$first/release/deriv_platform" "$second/release/deriv_platform"
sha256sum "$first/release/deriv_platform"
