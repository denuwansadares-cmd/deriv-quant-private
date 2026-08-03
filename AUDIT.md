# Data, API, IP, and transfer audit

Audit date: 2026-08-03. This is an engineering due-diligence record, not legal advice. “Conditional” and “blocker” findings must not be represented as cleared.

## Executive result

| Area | Status | Evidence and required action |
|---|---|---|
| Historical CSV ownership/licence | **BLOCKER** | The code can load external CSV files, but no dataset provenance, licence, consent, retention permission, or transfer right is present. Exclude all CSV/history data from a sale unless documented rights are supplied. |
| Deriv API terms compliance | **CONDITIONAL** | The code uses REST account discovery followed by an OTP-authenticated WebSocket. Demo is the default; live requires an explicit acknowledgement gate. Application-managed API records expire after 24 hours. Written confirmation is still required for storage, redistribution, and transaction-specific use. |
| Third-party dependency licences | **CONDITIONAL** | The locked Rust graph contains 217 registry packages and no missing Cargo licence expression. See `THIRD_PARTY_LICENSES.md` and the machine-readable TSV. Preserve notices and have counsel review compound/copyleft expressions and distribution obligations. |
| Code ownership/copyright | **DRAFT — SIGNATURE REQUIRED** | `LICENSE`, `COPYRIGHT`, and `AUTHORSHIP_DECLARATION.md` now record the proposed ownership and licence structure. The agreement has blank buyer, delivery, address, and signature fields and grants no effective buyer rights until completed and signed by both Parties. Independent chain-of-title and legal review remain required. |
| Secrets | **ROTATION REQUIRED** | Local `.env` contains non-placeholder `DERIV_APP_ID` and `DERIV_PAT` values. Values were not copied into this audit. Git history is unavailable in this workspace, so prior commitment cannot be ruled out. `.gitignore` now excludes secret files; rotate/revoke before delivery. |
| Transferable assets | **CONDITIONAL** | Source and original documentation may transfer only after chain-of-title clearance. Dependencies remain under their own licences. Credentials, Deriv registrations/access, OTPs, accounts, API-derived records, and unlicensed data are excluded. |
| 24-hour retention | **IMPLEMENTED** | Startup removes regular files at least 24 hours old from `DERIV_API_RECORDS_DIR` (default `data/api-records`). It does not recurse, follow symlinks, or delete `HISTORY_PATH`. |
| Buyer-owned onboarding | **IMPLEMENTED** | `.env.example` and `BUYER_ONBOARDING.md` require the buyer's App ID and OAuth/PAT access token. Legacy PAT variable names remain compatibility aliases. |

## Data inventory and ownership

- In-memory WebSocket ticks, proposals, buys, and portfolio updates are processed during execution. The inspected production code does not write raw WebSocket traffic or trading records to disk.
- `HistoryLoader` reads CSV files supplied through `HISTORY_PATH`. Loader capability is not evidence of ownership or a licence.
- Tests create temporary synthetic CSV fixtures. These are generated test data, not a market dataset.
- `data/api-records` is reserved for application-managed API-derived cache files and is excluded from source control.
- `main.rs.orig` and `main.rs.codex-backup` are development snapshots. Review provenance and exclude them from a clean buyer package unless they are intentionally part of the source transfer.
- Three zero-byte artefacts (`{`, `src/main.rs:69:7`, and `src/main.rs:81:7`) are not product assets and should be excluded from delivery.

For every external dataset, obtain a record containing supplier, licence text/version, acquisition date, permitted purpose, retention limit, redistribution/transfer right, territory, deletion procedure, and evidence file. Without that record the dataset is non-transferable.

## Deriv API compliance controls

- REST base used by the code: `https://api.derivws.com/trading/v1/options`.
- Authorization is supplied at runtime through environment variables and not hard-coded.
- The bot lists options accounts, selects a virtual/demo account, requests an OTP URL, and connects through WebSocket.
- The official WebSocket documentation says the OTP is single-use and valid for 120 seconds. The reconnect path requests a new OTP.
- Live mode is explicitly gated in code. The gate does not establish operational safety, regulatory approval, or contractual compliance.
- Secrets and OTP-bearing fields are redacted by the JSON logging sanitizer, with unit-test coverage.
- API usage limits can change; operational limits must be checked under the buyer's application before deployment.

Official references: [API overview](https://developers.deriv.com/docs/intro/api-overview/), [authentication](https://developers.deriv.com/docs/intro/authentication/), [OAuth](https://developers.deriv.com/docs/intro/oauth/), [Options accounts](https://developers.deriv.com/docs/options/get-accounts/), and [OTP WebSocket](https://developers.deriv.com/docs/options/websocket/).

## Retention policy

The maximum engineering retention for application-managed API-derived records is 24 hours. On every startup `enforce_api_record_retention` scans only the configured top-level cache directory and deletes expired regular files. Symlinks and directories are ignored. A deletion failure stops startup, preventing silent non-compliance.

This mechanism is not a licence grant and does not make otherwise prohibited storage lawful. It also cannot clean arbitrary exports, backups, logs, object storage, database copies, or user-selected `HISTORY_PATH` content. Those systems require separate controls and evidence.

## Secrets audit limitations and response

The scan found credentials present in local `.env` and found no private-key or obvious embedded bearer-token pattern in the other inspected source files. A `.git` history was not available and the `git` executable is absent, so “never committed” cannot be certified.

Required response: rotate/revoke the current token, create buyer-owned credentials, remove `.env` from every delivery, scan the authoritative Git history and CI/CD stores, and document completion. Never transfer the seller's API registration by assumption.

## Transfer schedule

Potentially transferable after ownership clearance:

- original Rust source code and configuration templates;
- original project documentation and tests;
- build instructions and non-secret operational know-how;
- the seller's copyright interest, only to the extent the seller can prove it owns that interest.

Excluded unless separately licensed/assigned in writing:

- Deriv App IDs, OAuth registrations, PATs/access tokens, OTPs, accounts, balances, permissions, and rate-limit allocations;
- historical CSV datasets, raw WebSocket recordings, API-derived caches, logs, and backups containing provider content;
- third-party Rust crates or other third-party IP as owned assets (they remain usable only under their licences);
- personal data, customer/account data, and any asset without documented chain of title.

## Closing actions before buyer delivery

1. Complete authoritative Git-history and CI secret scanning; rotate all discovered credentials.
2. Complete the ownership declaration and delivery schedule, obtain legal review, and have both Parties sign the proprietary licence.
3. Produce a signed dataset schedule with licence evidence or deliver no historical/API-derived data.
4. Review current Deriv terms and obtain written permissions needed for the transaction and deployment.
5. Regenerate dependency notices from the final `Cargo.lock` and preserve required licence texts.
6. Deliver a source-only archive using `.gitignore` exclusions, then verify the archive contains no `.env`, CSV, API records, logs, or credentials.
