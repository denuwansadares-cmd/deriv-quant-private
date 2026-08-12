# Buyer Full Setup Guide

## What this system gives you

This package provides a structured way to build, simulate, evaluate, and operate the trading system using your own Deriv account and licensed market data. It includes deterministic simulation, demo-mode testing, configurable risk limits, audit records, a local dashboard, a watchdog, and an emergency-stop mechanism. These controls are designed to help a buyer evaluate the software methodically before deciding whether any real-money use is appropriate.

The seller reports having used the system in real-money trading and having achieved profitable results during that period. That statement describes the seller's personal historical experience; it is not an independently audited performance record, a promise, or a guarantee that another user will obtain the same result. Market conditions, configuration, execution, account settings, and user decisions can materially change outcomes.

> **Important risk notice:** Trading involves risk and can result in partial or total loss of the money committed. Past performance does not guarantee future results. The software is a technical tool, not financial advice or a guaranteed-income product. Use simulation and a controlled demo account first, choose conservative limits, and obtain independent financial, technical, and legal advice where appropriate. The buyer remains responsible for credentials, configuration, operation, trading decisions, and compliance with applicable rules. Nothing in this guide excludes any responsibility that cannot lawfully be excluded.

## 1. Requirements and delivery check

- 64-bit Linux (Ubuntu or Debian recommended)
- Git, build tools, `pkg-config`, and OpenSSL development files
- Stable Rust and Cargo
- A buyer-owned Deriv app ID, bearer token, and Options demo account
- Buyer-owned or properly licensed CSV history

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev ca-certificates curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The delivery must not contain `.env`, seller tokens, `.risk/`, `data/api-records/`, or private history files.

## 2. Safe simulation without credentials

```bash
cd /absolute/path/to/deriv_bot
test ! -f .env
test ! -d data/api-records
cargo build --locked
cargo test --locked --no-default-features

DERIV_RANDOM_SEED=20260803 \
SIM_EXPECT_REPLAY_HASH=44effca3edc5f553 \
cargo run --locked --no-default-features --bin deriv_bot -- \
  --mode simulation --symbols R_100 --history fixtures/synthetic
```

Simulation does not connect to the broker. The included fixture tests system mechanics only; it does not demonstrate profitability. If the replay hash does not match, inspect the revision, lockfile, random seed, and input data before continuing.

## 3. Configure the buyer's Deriv access and `.env`

The buyer must use their own app registration and token. At runtime, the application selects an active demo account and obtains a one-time WebSocket OTP. It obtains a fresh OTP after reconnecting. Grant only the minimum required scopes. An OAuth callback or token-refresh service is not included in this repository.

Official documentation: [authentication](https://developers.deriv.com/docs/intro/authentication/), [OAuth](https://developers.deriv.com/docs/intro/oauth/), [Options accounts](https://developers.deriv.com/docs/options/get-accounts/), and [WebSocket](https://developers.deriv.com/docs/options/websocket/).

```bash
cp .env.example .env
chmod 600 .env
mkdir -p .risk data/api-records data/audit /absolute/path/to/licensed/history
chmod 700 .risk data/api-records data/audit
```

```dotenv
DERIV_APP_ID=BUYER_OWN_APP_ID
DERIV_ACCESS_TOKEN=BUYER_OWN_ACCESS_TOKEN
DERIV_MODE=demo
HISTORY_PATH=/absolute/path/to/licensed/history
DERIV_RISK_STATE_DIR=.risk
DERIV_API_RECORDS_DIR=data/api-records
DERIV_AUDIT_JOURNAL_DIR=data/audit
RUST_LOG=info
```

Review every risk value in `.env.example`. For USD, `DERIV_MAX_STAKE_MINOR=100` means USD 1.00, while 100 basis points means 1%. `DERIV_MAX_OPEN_CONTRACTS=1` is mandatory. Never commit `.env`, and never place a token or OTP in logs, screenshots, or messages.

## 4. Run the first demo test

Start with a 15-minute, single-symbol test:

```bash
DEMO_DURATION_SECS=900 \
cargo run --locked --release --bin deriv_bot -- \
  --mode demo --symbols R_100 --history /absolute/path/to/licensed/history
```

Verify account selection, incoming ticks, risk-approved demo actions, and audit metadata. Use `Ctrl+C` for a graceful stop. Confirm that logs and audit records contain no secrets. `DEMO_DURATION_SECS=0` enables continuous demo operation, but complete finite soak tests first.

## 5. Dashboard, watchdog, and emergency stop

```bash
cargo build --locked --release --bin deriv_platform --bin deriv_watchdog
export DERIV_DASHBOARD_TOKEN='random-32-plus-character-token'
export DERIV_DASHBOARD_CSRF_TOKEN='different-random-32-plus-token'
export DERIV_DASHBOARD_TOKEN_EXPIRES_UNIX='future-unix-time'
cargo run --locked --release --bin deriv_platform
```

The dashboard listens on `127.0.0.1:8080` by default. Do not bind it directly to a public network. Use a TLS reverse proxy and firewall controls for remote access.

In the packaged deployment, Caddy is the only public entry point. It terminates HTTPS, requires Basic authentication, and injects separate short-lived operator and CSRF credentials only into matched control requests. The dashboard remains private and has no broker credentials. From the authenticated UI, an operator can save the idle-session stake/loss profile, start demo, explicitly acknowledge and start guarded live mode, request a graceful stop, or engage the persistent emergency stop. Live mode still requires all compile-time, runtime, state and risk gates; a warning checkbox alone cannot bypass them.

While the bot is running, open another terminal and use the same `DERIV_RISK_STATE_DIR`:

```bash
cargo run --locked --release --bin deriv_watchdog
cargo run --locked --release --bin deriv_watchdog -- --check
```

Manual emergency stop:

```bash
touch .risk/emergency.stop
```

Do not delete persistent risk files until you have reconciled any open contract, the provider portfolio, the audit records, and the recorded exposure.

## 6. Docker Compose

`docker-compose.yml` may contain seller-machine absolute history paths and stricter risk overrides. Replace those paths with buyer-controlled paths, then review `.env`, filesystem permissions, and the effective risk configuration.

```bash
openssl rand -hex 32
openssl rand -hex 32
date -d '+4 hours' +%s
docker compose config
docker compose build
docker compose up
docker compose logs -f --tail=200 bot watchdog dashboard
```

Use `docker compose down` to stop the services. Do not use `docker compose down -v` for routine shutdown because it deletes persistent risk and audit volumes.

## 7. Troubleshooting

| Error | What to check |
|---|---|
| Missing app ID or token | `.env`, buyer credentials, and token expiry |
| Missing `HISTORY_PATH` | An absolute, writable directory or `--history` |
| No active Options accounts | An active demo/virtual Options account |
| Missing risk variable | Every risk variable listed in `.env.example` |
| Dashboard unauthorized | Two distinct 32+ character tokens and a future expiry time |
| Watchdog failure | Bot status, matching risk directory, heartbeat, and stop file |
| Replay hash mismatch | Revision, fixture, seed, and lockfile |

`HISTORY_PATH` and `DERIV_API_RECORDS_DIR` must not point to the same directory. Use only data for which you have documented permission. See `BACKTESTING.md`, `PRODUCT_API.md`, and `SECURITY_EVIDENCE.md` for more detail.

## 8. Live mode and buyer acceptance

Live mode is excluded from the default build. Enabling it requires both the compile-time feature and the exact acknowledgement shown below:

```bash
DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY \
cargo run --locked --release --features live-trading --bin deriv_bot -- \
  --mode live --history /absolute/path/to/licensed/history
```

Enabling this path does not prove production readiness, profitability, legality, or safety. Do not use it until the buyer has completed independent evaluation and provided separate written approval.

For ownership transfer, replace the seller cloud account, static IP/domain, DNS, TLS state, Basic-auth hash, dashboard operator token, CSRF token, expiry, Deriv app ID and Deriv access token with buyer-controlled values. Keep port 8080 private, expose only ports 80/443 through the proxy, revoke all seller access, and record the effective Compose configuration and credential-rotation date in the acceptance evidence. Start with demo after transfer; live activation remains a separate buyer decision.

- [ ] The delivery contains no seller secrets or private records.
- [ ] Buyer-owned app ID, token, and demo account are in use.
- [ ] Locked build/tests and deterministic simulation pass.
- [ ] Dataset provenance/licensing and risk approval are documented.
- [ ] Finite demo run, log-redaction check, and emergency-stop test pass.
- [ ] Dashboard, watchdog, backup, retention, and incident ownership are verified.
- [ ] Live mode remains disabled until separate written approval is provided.
- [ ] The buyer has read and accepted the risk notice and product limitations.

The final acceptance record should include the revision, archive SHA-256, operating system and toolchain, test results, replay hash, dataset licences, selected risk profile, known limitations, and support dates. Documents in `legal/drafts/` are drafts only and should be reviewed by qualified legal counsel before use.

## A responsible way to evaluate the product

The strongest evidence of value is a process the buyer can reproduce. Build the locked revision, run its tests, reproduce the simulation hash, operate it in demo mode under conservative limits, inspect the audit trail, and test the emergency stop. Only then should the buyer decide whether the system fits their technical capability, risk tolerance, and intended use.
