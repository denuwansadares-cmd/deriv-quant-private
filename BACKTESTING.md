# Deterministic simulator/backtester

> **HYPOTHETICAL BACKTEST — NOT LIVE RESULTS.** Backtested or optimised results
> may not match actual trading results and must never be marketed as live
> performance, guaranteed performance, or evidence of future returns.

Run an offline multi-symbol replay with the same model scoring, contract-family
policy, trade-state transition rules, and risk sizing used by the broker modes:

```bash
DERIV_RANDOM_SEED=42 cargo run -- --mode simulation --history /path/to/csv-directory
```

The final line contains a replay hash. Require an exact replay later with:

```bash
SIM_EXPECT_REPLAY_HASH=<hash> DERIV_RANDOM_SEED=42 \
  cargo run -- --mode simulation --history /path/to/csv-directory
```

Simulation controls:

| Variable | Default | Meaning |
|---|---:|---|
| `SIM_PROPOSAL_TTL_MS` | 3000 | Proposal expiry |
| `SIM_LATENCY_MS` | 50 | Proposal/buy latency |
| `SIM_STALE_QUOTE_MS` | 500 | Maximum quote age |
| `SIM_PAYOUT_BPS` | 10200 | Gross payout in basis points of stake |
| `SIM_FEE_BPS` | 5 | Fee model |
| `SIM_SLIPPAGE_BPS` | 10 | Entry slippage model |
| `SIM_RATE_LIMIT_PER_SEC` | production limit | Injected request limit |
| `SIM_DISCONNECT_PPM` | 0 | Disconnect probability |
| `SIM_REJECT_PPM` | 0 | Broker reject probability |
| `SIM_LOST_BUY_RESPONSE_PPM` | 0 | Lost buy response with portfolio recovery |
| `SIM_SELL_FAILURE_PPM` | 0 | Sell failure probability per attempt |
| `SIM_MISSING_TICK_PPM` | 0 | Missing tick probability |
| `SIM_OUT_OF_ORDER_PPM` | 0 | Out-of-order tick probability |
| `SIM_MAX_SELL_RETRIES` | 3 | Sell retry ceiling |

All probability values are parts per million. Workers build isolated traces and
the coordinator merges them by symbol, so thread scheduling does not affect the
record or replay hash. A contract opened on one tick can settle only after a
later delivered tick; an open contract at dataset end remains open rather than
reading beyond the dataset.

Each per-symbol report includes sample size, rejected-trade count, expected
value with a 95% interval, profit factor, maximum drawdown, estimated risk of
ruin, win rate with a Wilson 95% interval, average/median wins and losses,
worst-5% tail-loss average, exposure, trades per hour, and worst consecutive
loss run. Per-trade Sharpe, Sortino, and Calmar are emitted only when the sample
size and required denominators make them meaningful; otherwise they are
`null`. Risk of ruin is a model estimate based on observed mean/variance and
the configured starting balance, not a guarantee.

Pinned Deriv API contract metadata lives in
`schemas/deriv/pinned-contracts.json`. CI runs the local contract tests. The
official documentation says each production release publishes `schemas.zip`,
but a stable archive URL was not exposed when this pin was created; therefore
the repository does not pretend to pin an unverifiable download URL.

## Production model qualification

Backtests are candidate research, not automatic production enablement.
`config/strategy-qualifications.json` is fail-closed and initially contains no
qualified models. A model receives production consensus weight only when an
independently reviewed evidence record passes every gate: minimum independent
sample size, positive holdout expectancy, a 95% lower bound above break-even,
drawdown and risk-of-ruin ceilings, stable neighbouring parameters, positive
95% lower-bound lift over both simple and random baselines, multiple independent
markets and regimes without profit concentration, calibration/Brier limits, and
failure-stress tests. The evidence must identify its code version, holdout
dataset, checksum, and evidence version. Passing one criterion never compensates
for failing another.
