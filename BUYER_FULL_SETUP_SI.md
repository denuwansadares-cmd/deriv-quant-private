# Buyer Full Setup Guide — සිංහල

මෙය current Rust-only executable platform එක සඳහා සම්පූර්ණ setup, operation සහ recovery guide එකයි. මුලින් simulation, පසුව demo complete කරන්න. Live mode optional, feature-gated සහ real-money risk සහිතයි.

## 1. අවශ්‍ය දේ

- Recommended deployment සඳහා Linux/VM, Docker Engine සහ Compose v2.
- Native development සහ secret generation සඳහා stable Rust/Cargo, Git, `sha256sum`, `tar`, `openssl`.
- Release packaging සඳහා optional Docker සහ `cargo-cyclonedx`.
- Demo/live සඳහා buyerට අයිති Deriv app ID සහ token.
- Store/use කිරීමට documented permission ඇති history.
- Public TLS dashboard සඳහා DNS/IP routing, ports 80/443 සහ deployment එකට ගැළපෙන Caddy config.

```bash
git status --short
cargo --version
docker compose version
cargo run --locked --bin deriv_ops -- release-readiness
```

Formatting, all-target build, warnings-denied Clippy, සියලු tests, live-feature compile (execution නොකර), deterministic fixture සහ Compose validation pass විය යුතුය. Tests අලුතින් එකතු වන විට count වැඩි විය හැක.

## 2. Rust binaries

| Binary | කාර්යය |
|---|---|
| `deriv_bot` | simulation/demo/live runtime |
| `deriv_supervisor` | එක runtime child එකක් සහ dashboard control files manage කිරීම |
| `deriv_platform` | Rust-rendered dashboard සහ API |
| `deriv_watchdog` | heartbeat/process safety monitor |
| `deriv_history` | history normalize කිරීම සහ synchronized evidence |
| `deriv_ops` | verification, reproducibility, evaluation, packaging |
| `generate_synthetic_fixture` | deterministic test fixture |

Executable logic 100% Rust. HTML/CSS presentation formats ලෙසත් JSON/CSV/Markdown/YAML/TOML data, evidence, docs සහ config formats ලෙසත් තබා ඇත. Secrets ignored `.env`/`.dashboard.env` තුළ; durable safety state `.risk` හෝ `deriv-risk` volume තුළ තබන්න.

## 3. Credentials නැති safe simulation

```bash
cargo build --locked --no-default-features --bins
cargo test --locked --all-targets
DERIV_RANDOM_SEED=20260803 \
DERIV_AUDIT_JOURNAL_DIR=/tmp/deriv-simulation-audit \
cargo run --locked --no-default-features --bin deriv_bot -- \
  --mode simulation --symbols R_100 --history fixtures/synthetic
```

එකම seed/input නැවත run කර determinism verify කරන්න. Simulation knobs ලෙස proposal/stale/latency, payout/fee/slippage, rate-limit, disconnect/reject/lost-buy/sell-failure/missing/out-of-order fault rates, retry count, growth/duration/sell discount සහ `SIM_EXPECT_REPLAY_HASH` භාවිතා කළ හැක; exact names README canonical table එකේ ඇත.

## 4. Licensed history audit කිරීම

Audit command එක configured symbols 10ට අදාළ CSV files 10ම බලාපොරොත්තු වේ. Raw originals වෙනස් නොකර separate normalized copies/evidence සාදයි.

```bash
cargo run --locked --bin deriv_history -- audit \
  --raw /absolute/path/to/raw-history \
  --normalized "$PWD/history-normalized" \
  --report "$PWD/reports/history/dataset-audit.json" \
  --manifest "$PWD/reports/history/provenance-manifest.json"

cargo run --locked --bin deriv_history -- overlap \
  --normalized "$PWD/history-normalized" \
  --audit "$PWD/reports/history/dataset-audit.json" \
  --manifest "$PWD/reports/history/provenance-manifest.json" \
  --markdown "$PWD/reports/history/DATASET_AUDIT.md"
```

Malformed/non-finite rows, UTC mismatch, order, duplicate, gap, hash, licence/transfer restriction සහ exact synchronized intersection review කරන්න. Bounding-range overlap එක synchronized evidence නොවේ. Transfer permission නැති raw history buyer package එකට දාන්න එපා.

## 5. Native demo `.env`

```bash
cp .env.example .env
chmod 600 .env
```

Buyer-owned values සහ network risk variables හයම explicitව සකසන්න:

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

USD minor units 100 = USD 1.00; basis points 100 = 1%. Reviewed ranges: stake 100–10,000; session loss 1–2,000 bps; exposure 1–1,000 bps; open contracts exactly 1; consecutive losses 1–10; cooldown 10–3,600 seconds.

```bash
cargo run --locked --release --bin deriv_bot -- \
  --mode demo --history "$HISTORY_PATH"
```

Demo account type, intended symbols, heartbeat/journal creation, secret redaction සහ shutdown exposure drain/reconciliation verify කරන්න.

## 6. Native supervisor, dashboard සහ watchdog

Supervisor එක `deriv_bot` PATH එකෙන් හොයන නිසා binaries එකට build/install කරන්න:

```bash
cargo build --locked --release --features live-trading --bins
export PATH="$PWD/target/release:$PATH"
export DERIV_RISK_STATE_DIR="$PWD/.risk"
export DERIV_AUDIT_JOURNAL_DIR="$PWD/data/audit"
export HISTORY_PATH="$PWD/history-normalized"
deriv_supervisor
```

Dashboard terminal:

```bash
export DERIV_RISK_STATE_DIR="$PWD/.risk"
export DERIV_AUDIT_JOURNAL_DIR="$PWD/data/audit"
export DERIV_DASHBOARD_BIND=127.0.0.1:8080
export DERIV_DASHBOARD_TOKEN='at-least-32-random-characters-operator'
export DERIV_DASHBOARD_CSRF_TOKEN='different-32-random-characters-csrf'
export DERIV_DASHBOARD_TOKEN_EXPIRES_UNIX="$(($(date +%s)+86400))"
deriv_platform
```

Watchdog terminal:

```bash
export DERIV_RISK_STATE_DIR="$PWD/.risk"
export DERIV_WATCHDOG_STALE_SECS=15
export DERIV_WATCHDOG_STARTUP_GRACE_SECS=30
deriv_watchdog
# one-shot check
deriv_watchdog --check
```

`http://127.0.0.1:8080` open කරන්න. Dashboard HTML Rust server එකෙන් render වන අතර තත්පර 5කට වරක් refresh වේ; JavaScript නැහැ. Direct POST API සඳහා bearer, separate CSRF, operator role සහ future expiry අවශ්‍යයි. Risk configuration සඳහා `X-Stake-Minor`/`X-Loss-Limit-Bps`; live start සඳහා `X-Live-Risk-Acknowledged: true` සහ `X-Live-Confirmation: LIVE` අවශ්‍යයි.

## 7. Recommended Docker/Caddy deployment

`Caddyfile` public IP/domain deployment එකට update කරන්න. Ignored secret files සාදන්න:

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

Shared machine එකක real password shell historyට දාන්න එපා; production secret manager/interactively protected method එකක් භාවිතා කරන්න. Caddy upstream token/CSRF values dashboard values සමඟ exact match විය යුතුය. `docker-compose.yml` host history bind paths deployment host එකට update කරන්න.

```bash
docker compose config --quiet
docker compose build
docker compose up -d
docker compose ps
docker compose logs -f --tail=200 bot watchdog dashboard caddy
```

Caddy පමණක් 80/443 expose කරයි; dashboard internal. Bot/supervisor/watchdog durable risk volume share කරයි. Dashboard audit journal read කර control state write කරයි. Containers read-only root FS, dropped capabilities, limits සහ no-new-privileges භාවිතා කරයි.

Routine stop:

```bash
docker compose down
```

`docker compose down -v` routine use කරන්න එපා; risk/audit/Caddy volumes delete වේ.

## 8. Operator/viewer access

Current Caddy config එකේ `Denu` operator; `Buyer` read-only. Buyer POST controls වලට 403 ලැබීම expected. Usernames වෙනස් කරනවා නම් Basic Auth සහ match expressions දෙකම consistentව update කරන්න.

Start කිරීමට පෙර saved stake/loss, idle state, latches, broker exposure, token expiry, history mount සහ account type verify කරන්න. Graceful stop මුලින් භාවිතා කරන්න. Emergency stop normal pause button එකක් නොවේ; persistent safety latch එකකි.

## 9. Safety recovery

Stop latch/stale runtime/exposure pending නම්:

1. Volumes delete නොකර supervisor/bot stop කරන්න.
2. Deriv account independentව inspect කර open contracts reconcile කරන්න.
3. `.risk`/`deriv-risk` සහ journal evidence preserve කරන්න.
4. Risk, watchdog, operator, disconnect uncertainty හෝ PID identity mismatch හේතුව හඳුනාගන්න.
5. Review/authorization record කරන්න.
6. Approved procedure එකෙන් අදාළ latch එක පමණක් archive/clear කරන්න.
7. Demo restart කර heartbeat, portfolio සහ control state verify කරන්න.

Start force කිරීමට `.risk`, `runtime.lock`, `open-exposure.json`, `stop-state.json` හෝ named volumes delete කරන්න එපා.

## 10. Live mode

Live සඳහා buyer acceptance, legal/provider review, buyer-owned real account/token, history/qualification review, demo soak, reconciled state, `live-trading` feature, exact environment acknowledgement, saved risk limits, idle supervisor, no latch සහ explicit warning/typed confirmation සියල්ල අවශ්‍යයි.

```bash
DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY \
cargo run --locked --release --features live-trading --bin deriv_bot -- \
  --mode live --history "$HISTORY_PATH"
```

Build/test/backtest/simulation/demo success එක live approval හෝ profit evidence නොවේ.

## 11. Verification සහ release

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo check --locked --features live-trading --bin deriv_bot
cargo run --locked --bin deriv_ops -- release-readiness
cargo run --locked --bin deriv_ops -- reproducible-build
```

Buyer package evaluation සඳහා `deriv_ops buyer-evaluate`. Packaging සඳහා `cargo-cyclonedx`, separately reviewed licence, `FINAL_BUYER_LICENCE`, සහ `FINAL_BUYER_LICENCE_REVIEWED=YES` අවශ්‍යයි.

## 12. Troubleshooting

- App ID/token missing: demo/liveට buyer credentials අවශ්‍යයි; simulationට නැහැ.
- Risk error: variables හයම explicitව reviewed ranges තුළ තිබිය යුතුයි.
- History error: existing absolute path භාවිතා කරන්න; raw API records path භාවිතා නොකරන්න.
- Dashboard 401: token/CSRF length, difference, exact values, role සහ expiry බලන්න.
- Viewer 403: POST සඳහා expected.
- Caddy fail: address/DNS, 80/443, hash quoting, `.dashboard.env` බලන්න.
- Supervisor spawn fail: `deriv_bot` directory PATHට දාන්න.
- Watchdog stale: shared PID namespace/risk volume, grace, heartbeat සහ identity බලන්න.
- Live reject: feature, acknowledgement, real account, idle/saved limits/latch state බලන්න.
- Repro mismatch: artifacts preserve කර toolchain/environment investigate කරන්න.

## 13. Final acceptance checklist

- [ ] Credentials, app, infrastructure, history rights සහ capital buyer control කරයි.
- [ ] Simulation/deterministic replay verified.
- [ ] History audit/provenance/overlap reviewed.
- [ ] Network run සෑම වරම demo/real account identity confirm කරයි.
- [ ] Risk settings හය independently approve කරයි.
- [ ] Risk/audit volumes backup සහ retention documented.
- [ ] Operator/viewer access, rotation සහ expiry test කරයි.
- [ ] Graceful stop, emergency latch, watchdog සහ recovery rehearse කරයි.
- [ ] Exact snapshot එකේ release-readiness/reproducible build pass.
- [ ] Licence, notices, SBOM, provider terms සහ transfer rights reviewed.
- [ ] Engineering evidence profit/win-rate/future-return claim එකක් ලෙස භාවිතා නොකරයි.
