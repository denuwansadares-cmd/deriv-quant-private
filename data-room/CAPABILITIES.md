# Honest Capabilities Matrix

| Capability | Status | Evidence |
|---|---|---|
| Deterministic shared simulator/strategy/risk code | Implemented, tested | source + tests |
| Virtual clock and seeded RNG | Implemented, tested | deterministic replay |
| Broker-independent boundary | Implemented; one production adapter | interface tests |
| Fixed-point money/risk limits | Implemented, tested | risk tests |
| Explicit Qualified/NotQualified signals | Implemented; unqualified models excluded | qualification tests |
| Locked holdout results | MISSING | retained Deriv dataset is available, but a locked evaluation has not been completed |
| Normalized append-only audit journal | Separate metadata store | journal source/tests |
| Raw API storage | Disabled by default; TTL directory separate | retention source |
| Simulator contract pricing | Approximate, explicitly labelled | report schema |
| Synthetic buyer fixture | Implemented, checksum/replay locked | buyer evaluation |
| Supervisor-controlled demo/live lifecycle | Implemented; one child, persisted config, graceful stop | source + deployment checks |
| Tradier broker adapter | NOT IMPLEMENTED | Deriv is the only implemented broker integration |
| Watchdog persistent stop/process termination | Implemented; external chaos certification pending | source + tests |
| Docker persistent risk/audit volumes | Implemented and exercised in deployment | Compose + HTTPS smoke checks |
| Dashboard control plane | TLS Basic auth, server-injected operator/CSRF, expiry, role, audit, bounded risk and demo/live controls | source + deployment checks |
| Guarded live feature | Compiled in deployment; disabled until explicit risk save, acknowledgement, typed LIVE and runtime gates | source + tests |
| Profitability/live performance | Not established | prohibited claim |
| Signed release/external audit/72h soak | MISSING | release gates |
