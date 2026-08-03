# Honest Capabilities Matrix

| Capability | Status | Evidence |
|---|---|---|
| Deterministic shared simulator/strategy/risk code | Implemented, tested | source + tests |
| Virtual clock and seeded RNG | Implemented, tested | deterministic replay |
| Broker-independent boundary | Implemented; one production adapter | interface tests |
| Fixed-point money/risk limits | Implemented, tested | risk tests |
| Explicit Qualified/NotQualified signals | Implemented; unqualified models excluded | qualification tests |
| Licensed locked holdout results | MISSING | no licensed dataset |
| Normalized append-only audit journal | Separate metadata store | journal source/tests |
| Raw API storage | Disabled by default; TTL directory separate | retention source |
| Simulator contract pricing | Approximate, explicitly labelled | report schema |
| Synthetic buyer fixture | Implemented, checksum/replay locked | buyer evaluation |
| Watchdog persistent stop/process termination | Implemented; supervisor integration not externally certified | source |
| Docker persistent risk volume | Configured; engine unavailable locally | Docker/Compose files |
| Dashboard operator security | Token expiry, role, CSRF, rate limit, audit | platform source |
| Live feature | Source included, default build disabled | compile/runtime gates |
| Profitability/live performance | Not established | prohibited claim |
| Signed release/external audit/72h soak | MISSING | release gates |
