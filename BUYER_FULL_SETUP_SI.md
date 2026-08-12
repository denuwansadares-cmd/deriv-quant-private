# Buyer Full Setup Guide (සිංහල)

මෙය අලුත් buyer/operator කෙනෙකුට project එක **simulation සහ Deriv demo mode** වලින් setup කර verify කිරීමටයි. Seller credentials, OTP URLs හෝ private datasets භාවිතා නොකරන්න.

> මෙය experimental trading infrastructure එකකි; profit guarantee එකක් නොවේ. Simulation සහ controlled demo tests අවසන් නොකර live-money mode භාවිතා නොකරන්න.

## 1. Requirements සහ delivery check

- 64-bit Linux (Ubuntu/Debian recommended)
- Git, build tools, `pkg-config`, OpenSSL development files
- Stable Rust/Cargo
- Buyer-owned Deriv app ID, bearer token සහ Options demo account
- Buyer-owned/licensed CSV history

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev ca-certificates curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Delivery එකේ `.env`, seller tokens, `.risk/`, `data/api-records/` හෝ private history files නොතිබිය යුතුය.

## 2. Build සහ credentials නැති safe simulation

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

Simulation එක broker එකට connect නොවේ. Fixture එක mechanics test කිරීම සඳහා පමණයි. Hash mismatch වුණොත් revision, lockfile, seed සහ input inspect කරන්න.

## 3. Buyerගේ Deriv access සහ `.env`

Buyer තමන්ගේම app registration/token භාවිතා කළ යුතුය. Runtime එක active demo account select කර one-time WebSocket OTP ලබාගනී; reconnect එකකට fresh OTP ගනී. අවම scopes පමණක් දෙන්න. OAuth callback/token refresh service එක repo එක තුළ නැත.

Official docs: [authentication](https://developers.deriv.com/docs/intro/authentication/), [OAuth](https://developers.deriv.com/docs/intro/oauth/), [Options accounts](https://developers.deriv.com/docs/options/get-accounts/), [WebSocket](https://developers.deriv.com/docs/options/websocket/).

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

`.env.example` risk values review කරන්න. USD `DERIV_MAX_STAKE_MINOR=100` යනු $1.00; basis points `100` යනු 1%. `DERIV_MAX_OPEN_CONTRACTS=1` අනිවාර්යයි. `.env` commit නොකරන්න; token/OTP logs, screenshots හෝ messages වලට දාන්න එපා.

## 4. පළමු demo run

විනාඩි 15ක, single-symbol test එකකින් ආරම්භ කරන්න:

```bash
DEMO_DURATION_SECS=900 \
cargo run --locked --release --bin deriv_bot -- \
  --mode demo --symbols R_100 --history /absolute/path/to/licensed/history
```

Account selection, ticks, risk-approved demo actions සහ audit metadata verify කරන්න. `Ctrl+C` graceful stop සඳහායි. Logs/audits තුළ secrets නැති බව බලන්න. Continuous demo සඳහා `DEMO_DURATION_SECS=0`, නමුත් මුලින් finite soak tests කරන්න.

## 5. Dashboard, watchdog සහ emergency stop

```bash
cargo build --locked --release --bin deriv_platform --bin deriv_watchdog
export DERIV_DASHBOARD_TOKEN='random-32-plus-character-token'
export DERIV_DASHBOARD_CSRF_TOKEN='different-random-32-plus-token'
export DERIV_DASHBOARD_TOKEN_EXPIRES_UNIX='future-unix-time'
cargo run --locked --release --bin deriv_platform
```

Dashboard default එක `127.0.0.1:8080`. Public network එකකට direct bind නොකරන්න; remote access සඳහා TLS reverse proxy/firewall අවශ්‍යයි.

Bot එක run වන අතර same `DERIV_RISK_STATE_DIR` සමඟ වෙනත් terminal එකක:

```bash
cargo run --locked --release --bin deriv_watchdog
cargo run --locked --release --bin deriv_watchdog -- --check
```

Manual stop: `touch .risk/emergency.stop`. Open contract, provider portfolio, audit සහ exposure reconcile නොකර persistent risk files delete නොකරන්න.

## 6. Docker Compose

`docker-compose.yml` හි seller-machine absolute history paths සහ stricter risk overrides තිබේ. Buyer paths වලට ඒවා වෙනස් කර `.env`, permissions සහ effective risk config review කරන්න.

```bash
openssl rand -hex 32
openssl rand -hex 32
date -d '+4 hours' +%s
docker compose config
docker compose build
docker compose up
docker compose logs -f --tail=200 bot watchdog dashboard
```

Stop: `docker compose down`. `docker compose down -v` persistent risk/audit volumes delete කරන නිසා routine shutdown සඳහා භාවිතා නොකරන්න.

## 7. Troubleshooting

| Error | Check කරන්න |
|---|---|
| Missing app ID/token | `.env`, buyer credentials, token expiry |
| Missing `HISTORY_PATH` | absolute writable directory හෝ `--history` |
| No active Options accounts | active demo/virtual Options account |
| Risk variable missing | `.env.example` risk variables සියල්ල |
| Dashboard unauthorized | distinct 32+ tokens සහ future expiry |
| Watchdog failure | bot, same risk dir, heartbeat, stop file |
| Replay hash mismatch | revision, fixture, seed, lockfile |

`HISTORY_PATH` සහ `DERIV_API_RECORDS_DIR` එකම directory එක නොවිය යුතුය. Permission documented data පමණක් භාවිතා කරන්න. More details: `BACKTESTING.md`, `PRODUCT_API.md`, `SECURITY_EVIDENCE.md`.

## 8. Live mode සහ buyer acceptance

Live path එක default build එකේ නැත. Compile feature සහ exact acknowledgement දෙකම අවශ්‍යයි:

```bash
DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY \
cargo run --locked --release --features live-trading --bin deriv_bot -- \
  --mode live --history /absolute/path/to/licensed/history
```

මෙය production readiness, profitability, legality හෝ safety prove නොකරයි. Independent written approval අවසන් වනතුරු run නොකරන්න.

- [ ] Seller secrets/private records නැත.
- [ ] Buyer-owned app ID, token සහ demo account භාවිතා වේ.
- [ ] Locked build/tests සහ deterministic simulation pass වේ.
- [ ] History provenance/licence සහ risk sign-off documented වේ.
- [ ] Finite demo run, log-redaction check සහ emergency stop test pass වේ.
- [ ] Dashboard, watchdog, backup, retention සහ incident owner verify වේ.
- [ ] Live mode separate written approval ලැබෙනතුරු disabled වේ.

Final acceptance record එකට revision, archive SHA-256, OS/toolchain, tests, replay hash, dataset licences, risk profile, limitations සහ support dates ඇතුළත් කරන්න. `legal/drafts/` qualified legal counsel review කළ යුතු drafts පමණි.
