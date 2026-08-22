# Private Demonstration Script

Target duration: 5–10 minutes. Use only the tracked synthetic fixture and offline simulation unless the owner separately authorizes a buyer-owned Deriv demo account. Never use real credentials or real money during this demonstration.

## 1. Scope and safety (one minute)

State that Deriv is the only implemented broker adapter, Tradier is not implemented, models are fail-closed until qualified evidence is supplied, and simulation/demo results do not predict live profitability. Show `CLAIMS.md` and `BUYER_EVIDENCE_INDEX.md`.

## 2. Build and health (one minute)

```bash
git rev-parse HEAD
git status --short
cargo run --locked --bin deriv_ops -- release-readiness
```

Explain that the preflight checks formatting, strict lint, tests, live-path compilation without trading, fixture regeneration, and Compose configuration. A skipped tool must be described as skipped.

## 3. Architecture and lifecycle (one minute)

Use `data-room/ARCHITECTURE.md` to trace market input → qualification → authoritative risk → proposal/buy state machine → exposure → settlement → journal → dashboard → supervised shutdown. Show `data-room/CAPABILITIES.md` for implementation boundaries.

## 4. Deterministic synthetic replay (two minutes)

```bash
DERIV_AUDIT_JOURNAL_DIR=target/buyer-evidence/demo-audit \
DERIV_RANDOM_SEED=20260803 \
SIM_EXPECT_REPLAY_HASH=44effca3edc5f553 \
cargo run --locked --no-default-features --bin deriv_bot -- \
  --mode simulation --symbols R_100 --history fixtures/synthetic
```

Expected current output includes the explicit `HYPOTHETICAL BACKTEST — NOT LIVE RESULTS` label and replay hash `44effca3edc5f553`. The current fixture produces 1,971 decisions and zero trades; do not describe zero trades as success or performance.

## 5. Safety and recovery evidence (two minutes)

Run focused deterministic tests:

```bash
cargo test --locked characterization_tests::disconnect_during_buy_preserves_uncertainty_for_portfolio_recovery
cargo test --locked reliability_proof_tests::ten_thousand_disconnect_scenarios_never_issue_duplicate_buy
cargo test --locked tests::ctrl_c_three_seconds_after_contract_opened_keeps_drain_alive_until_settlement
cargo test --locked authoritative_risk_tests::stop_rejects_entry_but_allows_only_tracked_reduce_only_sell
```

Explain uncertain-buy reconciliation, duplicate-buy suppression, persistent stop behavior, one-open-contract enforcement, and graceful exposure draining. These are automated engineering scenarios, not real broker sessions.

## 6. Dashboard boundary (one minute)

Show the server-rendered files and Caddy boundary. Explain that browser read routes are protected by Caddy in the supplied remote topology and all write routes require operator bearer, separate CSRF token, role, and expiry. Do not claim an external penetration test. A connected dashboard/demo exercise is `OWNER ATTESTATION REQUIRED` until a sanitized exact-commit artifact exists.

## 7. Close and limitations (one minute)

Show `RELEASE_READINESS.md`, `THIRD_PARTY_LICENSES.md`, and `SUBMISSION_SCOPE.md`. Reiterate exclusions: credentials, accounts, private keys, raw broker records, private operational data, historical datasets without transfer rights, third-party ownership rights, and unsupported future integrations. Stop after displaying genuine evidence; do not manufacture screenshots or logs.
