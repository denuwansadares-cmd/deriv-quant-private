# Product claims register

This file freezes the claims that may be made about this repository. A feature must not be described as active, profitable, production-ready, or validated unless it is listed under **Active and tested** with current evidence.

## Legal and data-licensing gate

Resolve this before distributing or selling the product or its data.

The current Deriv API terms must be reviewed for every release and transaction. In particular, do not assume that API/feed content may be retained indefinitely, that historical CSV files or raw WebSocket recordings may be transferred to a buyer, or that API access transfers automatically with an acquisition. The terms may restrict storage or caching, describe the API licence as revocable and non-transferable, and allow usage limits to change. Obtain written permission or an appropriate data licence where required, or use a legally licensed external data source.

No historical market dataset, raw WebSocket archive, Deriv credential, API entitlement, or permission to redistribute Deriv content is included merely because this source code is transferred. A buyer must complete independent legal and licensing due diligence against the current [Deriv terms and conditions](https://deriv.com/terms-and-conditions/) and applicable law.

This repository does not provide legal advice.

## Active and tested

- **Deterministic production default:** `BASELINE_RANDOM_MODE`, `ENABLE_RANDOM_CONTRACT_FAMILIES`, and `ENABLE_RISE_FALL` are disabled. The active contract family is therefore ACCU; disabled families cannot be selected during initialization or rotation.
- **Explicit account-mode flow:** demo remains the default. Live mode requires an exact real-money acknowledgement, selects only a non-virtual account, and validates a real WebSocket endpoint. Simulation does not connect to the broker.
- **Trading state and execution controls:** the code implements a one-open-contract state guard, request timeouts, heartbeat/stale-connection handling, entry pacing, sell retries, and session-loss controls.
- **Deterministic simulation:** simulation uses historical ticks, a virtual clock, a supplied seed, and the shared model-scoring and risk-sizing code. This is reproducibility infrastructure, not evidence of trading performance.
- **Hypothetical reporting only:** every serialized backtest report is labelled `hypothetical_backtest_not_live`; reports and console output warn that backtested results may not match actual trading results.
- **Fail-closed model qualification:** none of the eight model scores receives production consensus weight without independently attributable holdout evidence passing every qualification gate. The default registry enables no models.
- **Decision journal:** metadata-only JSON Lines records cover version/config/dataset/seed context, normalized identifiers, model decisions, state transitions, order dispatch/results, latency, recovery action, and P&L calculations. The journal is conservatively purged after 24 hours pending legal review.
- **Balance-derived stake policy:** the risk engine derives its base stake from 1% of the verified demo balance, clamps it to an absolute $10 maximum and $1 minimum, and reduces it monotonically with signal risk and drawdown. API stake values are produced from integer minor units.
- **Local CSV ingestion and market ranking:** a CSV loader and ranking logic are implemented and unit-tested with synthetic fixtures. The repository does not establish that any external dataset is licensed, current, complete, or transferable.
- **Verification baseline:** `cargo check`, formatting validation, and the non-ignored unit tests pass as of 2026-08-03. One test requiring an external historical dataset is ignored and is not evidence that such a dataset is available or licensed.

These statements describe code paths and automated tests only. They do not establish profitability, production safety, regulatory compliance, or suitability for real-money trading.

## Implemented but disabled

- Over/Under and Matches/Differs proposal, selection, and result-handling paths exist, but `ENABLE_RANDOM_CONTRACT_FAMILIES` is `false`.
- Rise/Fall analysis and proposal paths exist, but `ENABLE_RISE_FALL` is `false`.
- `RISE_FALL_TARGET_WIN_RATE = 0.98` is a historical-filter threshold in a disabled path. It is **not** a measured or promised 98% live win rate.
- Multi-family random rotation exists in code but is excluded from the current live selection configuration.

Implemented-but-disabled code must not be advertised as active or tested in live trading.

## Planned

- Independent, reproducible out-of-sample performance evaluation with fees, latency, slippage, confidence intervals, drawdown, and complete methodology.
- A documented data-retention and deletion policy aligned with written permissions and the current provider terms.
- A licensed historical-data pipeline with provenance, permitted uses, retention limits, and buyer-transfer rights recorded for every dataset.
- Transaction-specific confirmation of whether API credentials, application registrations, rate limits, and licences can be assigned or must be reissued.
- Production security, operational, regulatory, and real-money risk review.

## Prohibited claims

Unless supported by new independent evidence and moved to **Active and tested**, do not claim or imply:

- a 98% win rate or any guaranteed win/profit rate;
- guaranteed profit, low/no risk, or “the most powerful bot”;
- that stake sizing is mathematically optimal or proven to be risk-balanced;
- that all contract families are active;
- that backtests predict live results;
- that bundled or referenced market data is licensed for storage, redistribution, sale, or buyer transfer;
- that a source-code acquisition automatically transfers Deriv API access or contractual permissions.
