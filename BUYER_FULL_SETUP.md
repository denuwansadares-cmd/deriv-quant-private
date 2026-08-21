# Buyer Full Setup Guide

This is the end-to-end setup and operating checklist for the current Rust-only executable platform. Complete simulation first, then demo. Live mode is optional, feature-gated, and never implied by a successful demo.

## 1. Requirements

- Linux host or VM; Docker Engine with Compose v2 for the recommended deployment.
- Rust stable, Cargo, Git, `sha256sum`, `tar`, and `openssl` for native development and secret generation.
- Optional release tools: Docker and `cargo-cyclonedx`.
- Buyer-owned Deriv app ID and token for demo/live only.
- History that the buyer is permitted to store and use.
- For public dashboard TLS: DNS/IP routing, ports 80/443, and deployment-specific Caddy configuration.

```bash
git status --short
cargo --version
docker compose version
cargo run --locked --bin deriv_ops -- release-readiness
```

A clean evaluation must pass formatting, all-target compilation, strict Clippy, 104+ tests, live-feature compilation without execution, deterministic fixture verification, and Compose validation. Test totals can legitimately increase.

## 2. Files and binaries

| Binary | Role |
|---|---|
| `deriv_bot` | simulation/demo/live runtime |
| `deriv_supervisor` | owns one runtime child and consumes dashboard control files |
| `deriv_platform` | Rust-rendered dashboard and API |
| `deriv_watchdog` | heartbeat/process safety monitor |
| `deriv_history` | history normalization and synchronized-coverage evidence |
| `deriv_ops` | verification, reproducibility, buyer evaluation, packaging |
| `generate_synthetic_fixture` | deterministic fixture generator |

Secrets belong in ignored `.env` and `.dashboard.env`; durable runtime safety state belongs in `.risk` or the `deriv-risk` volume. Never commit either.

## 3. First safe simulation

Simulation does not need Deriv credentials and does not send orders.

```bash
cargo build --locked --no-default-features --bins
cargo test --locked --all-targets

DERIV_RANDOM_SEED=20260803 \
DERIV_AUDIT_JOURNAL_DIR=/tmp/deriv-simulation-audit \
cargo run --locked --no-default-features --bin deriv_bot -- \
  --mode simulation \
  --symbols R_100 \
  --history fixtures/synthetic
```

Repeat with the same seed/input to verify deterministic behavior. Optional simulation controls include proposal/stale/latency timing, payout/fee/slippage, rate limits, disconnect/reject/lost-response/sell-failure/missing/out-of-order fault rates, retry count, ACCU growth, duration, sell discount, and `SIM_EXPECT_REPLAY_HASH`; see README’s canonical environment table.

## 4. Audit licensed history

The audit expects exactly the ten configured symbol CSV files. Originals are read; normalized copies and evidence are written separately.

```bash
cargo run --locked --bin deriv_history -- audit \
  --raw /absolute/path/to/raw-history \
  --normalized "$PWD/history-normalized" \
  --report "$PWD/reports/private/history/dataset-audit.json" \
  --manifest "$PWD/reports/private/history/provenance-manifest.json"

cargo run --locked --bin deriv_history -- overlap \
  --normalized "$PWD/history-normalized" \
  --audit "$PWD/reports/private/history/dataset-audit.json" \
  --manifest "$PWD/reports/private/history/provenance-manifest.json" \
  --markdown "$PWD/reports/private/history/DATASET_AUDIT.md"
```

Review malformed/non-finite rows, UTC mismatches, ordering, duplicates, gaps, hashes, licence/transfer restrictions, and exact synchronized intersections. Bounding-range overlap is not synchronized evidence. Never package raw history without explicit transfer permission.

## 5. Configure native demo mode

```bash
cp .env.example .env
chmod 600 .env
```

Edit `.env` with a buyer-owned app ID/token, `DERIV_MODE=demo`, an absolute `HISTORY_PATH`, and all six explicit network-risk variables:

```dotenv
DERIV_APP_ID=BUYER_OWN_APP_ID
DERIV_ACCESS_TOKEN=BUYER_OWN_ACCESS_TOKEN
DERIV_MODE=demo
DERIV_SYMBOLS=R_10,1HZ10V,R_25,1HZ25V,R_50,1HZ50V,R_75,1HZ75V,R_100,1HZ100V
HISTORY_PATH=/absolute/path/to/history-normalized
DERIV_MAX_STAKE_MINOR=100
DERIV_SESSION_LOSS_LIMIT_BPS=100
DERIV_MAX_OPEN_EXPOSURE_BPS=100
DERIV_MAX_OPEN_CONTRACTS=1
DERIV_MAX_CONSECUTIVE_LOSSES=3
DERIV_LOSS_COOLDOWN_BASE_SECS=60
DERIV_RISK_STATE_DIR=.risk
DERIV_AUDIT_JOURNAL_DIR=data/audit
DERIV_API_RECORDS_DIR=data/api-records
DEMO_DURATION_SECS=0
RUST_LOG=info
```

Minor units: USD 100 = USD 1.00. Basis points: 100 = 1%. Reviewed ranges are stake 100–10,000; session loss 1–2,000 bps; exposure 1–1,000 bps; one open contract; consecutive losses 1–10; cooldown base 10–3,600 seconds. These controls limit risk but cannot prevent every loss.

Run directly:

```bash
cargo run --locked --release --bin deriv_bot -- \
  --mode demo --history "$HISTORY_PATH"
```

Verify account type is demo, selected symbols are intended, heartbeat/journal files appear, no secrets appear in logs, and shutdown drains/reconciles exposure.

## 6. Native supervisor, dashboard, and watchdog

Build installed-together binaries. The supervisor resolves `deriv_bot` through `PATH`.

```bash
cargo build --locked --release --features live-trading --bins
export PATH="$PWD/target/release:$PATH"
export DERIV_RISK_STATE_DIR="$PWD/.risk"
export DERIV_AUDIT_JOURNAL_DIR="$PWD/data/audit"
export HISTORY_PATH="$PWD/history-normalized"
deriv_supervisor
```

In separate terminals:

```bash
export DERIV_RISK_STATE_DIR="$PWD/.risk"
export DERIV_AUDIT_JOURNAL_DIR="$PWD/data/audit"
export DERIV_DASHBOARD_BIND=127.0.0.1:8080
export DERIV_DASHBOARD_TOKEN='at-least-32-random-characters-operator'
export DERIV_DASHBOARD_CSRF_TOKEN='different-32-random-characters-csrf'
export DERIV_DASHBOARD_TOKEN_EXPIRES_UNIX="$(($(date +%s)+86400))"
deriv_platform
```

```bash
export DERIV_RISK_STATE_DIR="$PWD/.risk"
export DERIV_WATCHDOG_STALE_SECS=15
export DERIV_WATCHDOG_STARTUP_GRACE_SECS=30
deriv_watchdog
# one-shot probe:
deriv_watchdog --check
```

Open `http://127.0.0.1:8080`. The HTML is rendered by Rust, refreshes every five seconds, and uses no JavaScript. Direct write API requests require bearer, CSRF, role, and expiry. Risk configuration additionally uses `X-Stake-Minor` and `X-Loss-Limit-Bps`; live start uses `X-Live-Risk-Acknowledged: true` and `X-Live-Confirmation: LIVE`.

## 7. Recommended Docker/Caddy deployment

First replace the deployment-specific public address in `Caddyfile`. Create ignored secret files:

```bash
cp .env.example .env
chmod 600 .env
operator_token="$(openssl rand -hex 32)"
csrf_token="$(openssl rand -hex 32)"
expiry="$(( $(date +%s) + 86400 ))"
printf 'DERIV_DASHBOARD_TOKEN=%s\nDERIV_DASHBOARD_CSRF_TOKEN=%s\nDERIV_DASHBOARD_TOKEN_EXPIRES_UNIX=%s\n' \
  "$operator_token" "$csrf_token" "$expiry" >> .env

operator_hash="$(docker run --rm caddy:2.11-alpine caddy hash-password --plaintext 'CHANGE_OPERATOR_PASSWORD')"
buyer_hash="$(docker run --rm caddy:2.11-alpine caddy hash-password --plaintext 'CHANGE_VIEWER_PASSWORD')"
printf 'DASHBOARD_ADMIN_HASH=%s\nDASHBOARD_BUYER_HASH=%s\nDASHBOARD_OPERATOR_TOKEN=%s\nDASHBOARD_OPERATOR_CSRF=%s\n' \
  "$operator_hash" "$buyer_hash" "$operator_token" "$csrf_token" > .dashboard.env
chmod 600 .dashboard.env
```

Do not paste real passwords into shell history on a shared machine; use a protected secret manager or interactive procedure in production. Ensure the two Caddy upstream secrets exactly equal the dashboard secrets. Update the two host history bind paths in `docker-compose.yml` for the deployment host.

```bash
docker compose config --quiet
docker compose build
docker compose up -d
docker compose ps
docker compose logs -f --tail=200 bot watchdog dashboard caddy
```

Topology: Caddy exposes 80/443; dashboard is internal only; bot/supervisor and watchdog share durable risk state; dashboard reads the audit journal and writes control state; all application containers use read-only root filesystems, dropped capabilities, resource limits, and no-new-privileges.

Routine stop:

```bash
docker compose down
```

Never use `docker compose down -v` as a routine stop: it deletes risk, audit, and Caddy volumes.

## 8. Operator and viewer behavior

Caddy Basic Auth identity `Denu` is the operator and may reach POST controls. `Buyer` is read-only and receives 403 for control/emergency routes. Change these deployment-specific usernames if required and update the Caddy matchers consistently.

Before any start: inspect saved stake/loss values, supervisor idle state, emergency/stop latches, broker exposure, token expiry, history mount, and account type. Use graceful stop first. Emergency stop is a persistent safety latch, not a normal pause button.

## 9. Safety-state recovery

If startup reports a stop latch, stale runtime, or exposure pending:

1. Stop supervisor/bot without deleting volumes.
2. Independently inspect the Deriv account and reconcile every open contract.
3. Preserve `.risk`/`deriv-risk` and journal evidence.
4. Determine whether the stop came from risk, watchdog, operator action, disconnect uncertainty, or process identity mismatch.
5. Record the review and authorization.
6. Only then archive/clear the specific latch using the approved operating procedure.
7. Restart in demo and verify heartbeat, portfolio, and control status before considering live mode.

Never delete `.risk`, `runtime.lock`, `open-exposure.json`, `stop-state.json`, or named volumes merely to force a restart.

## 10. Live mode

Live mode requires all of the following: buyer acceptance, legal/provider review, buyer-owned real account/token, reviewed history and qualifications, completed demo soak, reconciled risk state, `live-trading` build feature, exact environment acknowledgement, saved risk limits, idle supervisor, no latch/exposure uncertainty, and explicit dashboard warning/typed confirmation.

Direct runtime example:

```bash
DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY \
cargo run --locked --release --features live-trading --bin deriv_bot -- \
  --mode live --history "$HISTORY_PATH"
```

A successful build, test, backtest, simulation, demo, or dashboard start is not live approval and is not evidence of profitability.

## 11. Verification and release workflows

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo check --locked --features live-trading --bin deriv_bot
cargo run --locked --bin deriv_ops -- release-readiness
cargo run --locked --bin deriv_ops -- reproducible-build
```

Buyer-package evaluation uses `deriv_ops buyer-evaluate` from the packaged layout. Data-room packaging requires `cargo-cyclonedx`, a separate lawyer-reviewed licence, `FINAL_BUYER_LICENCE`, and `FINAL_BUYER_LICENCE_REVIEWED=YES`.

## 12. Troubleshooting

- Missing app ID/token: network modes require buyer-owned credentials; simulation does not.
- Risk variable error: all six network-risk values must be explicit and within reviewed bounds.
- History error: use an existing absolute file/directory; do not point `HISTORY_PATH` at raw API-record storage.
- Dashboard 401: verify token/CSRF length, difference, exact values, role header, and future expiry.
- Viewer 403: expected for POST controls.
- Caddy failure: verify public address/DNS, 80/443 reachability, password-hash quoting, and `.dashboard.env`.
- Supervisor cannot spawn: put the directory containing `deriv_bot` on `PATH`.
- Watchdog stale: verify shared PID namespace/risk volume, startup grace, heartbeat age, and runtime identity before reset.
- Live rejected: verify Cargo feature, exact acknowledgement, real account selection, idle state, saved limits, and no safety latch.
- Reproducible build mismatch: preserve both artifacts and investigate toolchain/environment before release.

## 13. Final acceptance checklist

- [ ] Buyer controls credentials, app registration, infrastructure, history rights, and capital.
- [ ] Simulation and deterministic replay verified.
- [ ] History audit/provenance/overlap reviewed.
- [ ] Demo account identity confirmed before every network run.
- [ ] Six explicit risk settings independently approved.
- [ ] Risk/audit volumes backed up and retention policy documented.
- [ ] Operator/viewer access tested; secrets rotated and expiry tested.
- [ ] Graceful stop, emergency latch, watchdog stale path, and recovery procedure rehearsed.
- [ ] Release-readiness and reproducible-build pass on the exact snapshot.
- [ ] Legal licence, third-party notices, SBOM, provider terms, and data-transfer rights reviewed.
- [ ] No profitability, win-rate, production-readiness, or future-return claim inferred from engineering evidence.
