# Buyer onboarding

The buyer must use its own Deriv application registration and credentials. Seller credentials, OTPs, account identifiers, rate-limit allocations, and contractual permissions are not transferable assets.

## Supported authentication boundary

This build uses the current Options API sequence:

1. Use the buyer-owned application ID and OAuth or PAT bearer access token with the REST Options accounts endpoint.
2. Select a demo options account. This build rejects live mode.
3. Request a one-time WebSocket URL for that account through the REST OTP endpoint.
4. Open the returned WebSocket URL immediately. Deriv documents the OTP as single-use and valid for 120 seconds.
5. On reconnect, request a fresh OTP; never persist or reuse the previous OTP URL.

Official references:

- [API overview](https://developers.deriv.com/docs/intro/api-overview/)
- [Authentication overview](https://developers.deriv.com/docs/intro/authentication/)
- [OAuth applications](https://developers.deriv.com/docs/intro/oauth/)
- [Options accounts](https://developers.deriv.com/docs/options/get-accounts/)
- [OTP-authenticated Options WebSocket](https://developers.deriv.com/docs/options/websocket/)

## Setup

1. Create the buyer's own Deriv application using the authentication method appropriate to the deployment. Configure redirect URLs and scopes in the buyer's account; do not reuse the seller's registration.
2. Obtain a buyer-owned OAuth access token or PAT with only the scopes required for the demo options workflow. Token acquisition and refresh happen outside this CLI; this repository does not contain an OAuth callback server.
3. Copy `.env.example` to `.env` locally and set `DERIV_APP_ID` and `DERIV_ACCESS_TOKEN`. The legacy `DERIV_PAT`/`DERIV_PAT_TOKEN` names remain accepted for compatibility.
4. Keep `DERIV_MODE=demo`. Do not put credentials in command-line arguments, source files, logs, support messages, or buyer deliverables.
5. Leave `HISTORY_PATH` empty unless the buyer has a documented licence for that dataset and its intended use.
6. Keep application-managed API records under `DERIV_API_RECORDS_DIR`. Files there are automatically expired after 24 hours on each startup.
7. Run `cargo test`, then `cargo run -- --mode demo`.

## Environment separation

Deriv documents REST and WebSocket APIs as distinct parts of the API flow and distinguishes public, demo, and real access. This repository contains an explicitly gated real-account path, but that path has not been established as production-ready, profitable, compliant, or suitable for any user. Security, compliance, endpoint, and risk review remain mandatory before real-money use.

## Credential handover checklist

- Remove the seller's `.env` and all secrets from the delivery archive.
- Revoke/rotate any token that may have appeared in a working copy, log, shell history, CI variable, or commit.
- Have the buyer create its own application ID and credential.
- Confirm OAuth redirect URLs, scopes, account selection, and rate limits under the buyer's registration.
- Verify logs redact OTPs, tokens, authorization headers, and WebSocket URLs containing credentials.
# Runtime modes and audit records

The default mode is `demo`. Deterministic historical replay is available with
`--mode simulation` and `DERIV_RANDOM_SEED=<u64>`. Live trading requires both
`--mode live` and the exact acknowledgement
`DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY`; it is never selected as a
fallback.

Decision records are appended as JSON Lines under
`data/api-records/decision-journal.jsonl` (or `DERIV_API_RECORDS_DIR`). They
contain normalized identifiers and decision metadata, not credentials or raw
API payloads. The journal currently uses the same conservative 24-hour purge
policy as other API-derived records. Longer retention or expanded metadata must
not be enabled until a legal review defines what is permitted.
