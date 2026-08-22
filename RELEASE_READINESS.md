# Release Readiness

This document defines reproducible owner-review gates. It is not a claim of live-trading approval or profitability.

## Clean-checkout prerequisites

- Git, a Rust toolchain compatible with edition 2021, Cargo, rustfmt, and Clippy.
- Docker with Compose for container configuration/build/runtime checks.
- `cargo-audit`, `cargo-deny`, `cargo-cyclonedx`, and `cargo-llvm-cov` only for the corresponding gates. If unavailable, record the gate as unavailable; never invent a result.
- No broker credential is required for build, tests, schema checks, fixture regeneration, or simulation. Do not provide one.

## Supported feature matrix

| Configuration | Purpose | Required gate |
|---|---|---|
| `--no-default-features` / default | simulation and guarded demo build | check, strict Clippy, all-target tests, docs |
| `--features live-trading` | compile-time live path | check, strict Clippy, all-target tests only; never submit an order during release verification |

## Exact local verification sequence

```bash
git status --short
git diff --check
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo build --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo check --locked --workspace --all-targets --features live-trading
cargo clippy --locked --workspace --all-targets --features live-trading -- -D warnings
cargo test --locked --workspace --all-targets --features live-trading
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps
cargo run --locked --bin deriv_ops -- release-readiness
```

The 2026-08-22 local baseline was 111 passed, 0 failed, 0 ignored in each supported feature configuration. Counts may change after meaningful tests are added. The deterministic fixture must regenerate byte-for-byte and validate against `fixtures/synthetic/R_100.csv.sha256`.

For deterministic replay, run the command in `BUYER_EVIDENCE_INDEX.md`; expected hash is `44effca3edc5f553`. It is an offline hypothetical result.

## Conditional gates

```bash
cargo audit --deny warnings
cargo deny check
cargo cyclonedx --target all --format json
cargo llvm-cov --locked --workspace --all-targets --json --output-path target/buyer-evidence/coverage.json
docker compose config --quiet
docker build -t deriv-platform:owner-review .
```

Pinned local tools used on 2026-08-22: `cargo-llvm-cov 0.9.0`, `cargo-deny 0.18.6`, `cargo-about 0.9.0`, `cargo-audit 0.22.2`, `cargo-cyclonedx 0.5.9`, and Gitleaks `8.30.1`. Against dirty worktree changes layered on HEAD `296985c266890227fa29b39919dfd9e8bf1ce0bd`, default coverage was 49.458% lines, 47.393% regions, and 54.296% functions; live-feature coverage was 49.475% lines, 47.403% regions, and 54.296% functions. Raw JSON belongs under ignored `target/buyer-evidence/`. Coverage excludes real broker execution and is not a safety or profitability certification.

## Evidence and packaging

Store local logs and summaries only under ignored `target/buyer-evidence/`. Record commit SHA, dirty state, tool versions, command, exit code, sanitized output, limitations, and SHA-256 checksums. Packaging requires an independently reviewed final buyer licence and its explicit acknowledgement variables. `deriv_ops package-data-room` copies only explicitly allowed Git-tracked regular source files; validate the archive from a clean temporary extraction with `deriv_ops buyer-evaluate` before delivery.

## Manual blockers

Remote CI must not be called green without a genuine successful run. The dependency identity inventory, all-target SBOM, and deterministic notice bundle reconcile at exactly 217 components. The bundle preserves declared alternatives and third-party attribution; `defmt-parser 1.0.0` supplied no packaged licence file and remains marked for owner/legal review. Obtain legal approval of distribution obligations before distribution. Credential rotation, dataset/API transfer rights, chain of title, final licence terms, external audits, long-duration soak, connected-demo evidence, and acceptance of the 30-day support outline remain owner/external actions.
