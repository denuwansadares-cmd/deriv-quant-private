#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../source"
test ! -e .env
test ! -d data/api-records
cargo fmt --all -- --check
cargo test --locked --no-default-features
fixture_dir="${TMPDIR:-/tmp}/deriv-buyer-fixture-$$"
mkdir -p "$fixture_dir"
cargo run --locked --no-default-features --bin generate_synthetic_fixture -- "$fixture_dir"
cmp fixtures/synthetic/R_100.csv "$fixture_dir/R_100.csv"
sha256sum -c fixtures/synthetic/R_100.csv.sha256
DERIV_AUDIT_JOURNAL_DIR="$fixture_dir/audit" DERIV_RANDOM_SEED=20260803 SIM_EXPECT_REPLAY_HASH=44effca3edc5f553 cargo run --locked --no-default-features --bin deriv_bot -- --mode simulation --symbols R_100 --history fixtures/synthetic
cargo check --locked --features live-trading --bin deriv_bot
if command -v cargo-audit >/dev/null 2>&1; then cargo audit; else echo "SKIP cargo-audit: install cargo-audit"; fi
../scripts/reproducible-build.sh
sha256sum Cargo.lock
echo "PASS: engineering evaluation complete; no profitability/live approval inferred"
