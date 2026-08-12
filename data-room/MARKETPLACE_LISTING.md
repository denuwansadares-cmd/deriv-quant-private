# Marketplace Listing Kit

## Listing title

Deriv Quant — Auditable Rust Multi-Strategy Trading Infrastructure

## One-line summary

A source-available Rust/Tokio platform for Deriv research, deterministic replay, guarded demo/live execution, operational recovery, and buyer-verifiable risk controls.

## Buyer-facing description

Deriv Quant is a documented trading-infrastructure foundation, not a profit promise or an opaque executable. It combines Deriv account discovery and OTP WebSocket connectivity, multi-market history and live-tick handling, eight fail-closed research-model slots, four contract command families, deterministic replay, fixed-point risk controls, reconnect recovery, an authenticated operator dashboard, an independent watchdog, Docker deployment, automated tests, and a structured diligence package.

The product is intended for a technical buyer who wants to evaluate, extend, rebrand, or integrate a substantial Deriv-specific engineering base. Demo mode is the default. Live execution requires both a compile-time feature and an exact runtime acknowledgement. No strategy receives qualified production weight without independently attributable holdout evidence.

## Reproducible facts for the listing

- Rust/Tokio asynchronous runtime with REST and WebSocket broker boundaries.
- Accumulator, Over/Under, Matches/Differs, and Rise/Fall command paths.
- Ten configured synthetic markets with history loading, ranking, and rotation.
- Fixed-point stake calculations, one-open-contract enforcement, exposure limits, cooldowns, persistent stops, and reduce-only shutdown behavior.
- Reconnect, uncertain-buy portfolio reconciliation, sell retry, heartbeat, and watchdog paths.
- Deterministic simulation with seeded replay, virtual time, replay hashes, fault injection, and hypothetical metrics.
- Authenticated local command center with viewer mode, CSRF controls, rate limiting, action audit, telemetry, and emergency stop.
- 98 automated tests passing with zero failures and zero ignored tests as verified on 2026-08-10.
- Formatting, all-target compilation, strict Clippy, live-feature compilation, dashboard syntax, and deterministic-fixture checks pass through `cargo run --locked --bin deriv_ops -- release-readiness`.
- Docker, Caddy, English and Sinhala setup guides, API documentation, SBOM, dependency notices, threat model, claims register, and buyer evaluation workflow included.

## What the buyer receives

The exact delivery is controlled by the signed schedule. A normal source delivery can include the Rust source, dashboard and watchdog source, configuration templates, tests, deterministic fixture, Docker/Caddy deployment files, documentation, SBOM and notices, evidence manifest, reproducible evaluation scripts, and the agreed support period.

Buyer credentials, Deriv accounts, balances, App IDs, OAuth/PAT tokens, OTPs, rate-limit allocations, and any dataset not explicitly licensed in the signed delivery schedule are excluded.

## Required listing disclaimer

Experimental software infrastructure only. No profitability, win rate, future return, uninterrupted service, regulatory approval, or protection from trading loss is represented or guaranteed. Backtests are hypothetical and can differ materially from live results. Real-money operation requires the buyer's own credentials, supervision, validation, and independent legal, security, quantitative, regulatory, and operational review.

## Suggested media sequence

1. Command-center overview in read-only buyer mode.
2. Risk configuration and live-readiness gates, with credentials excluded.
3. Decision-provenance feed and settled-session chart.
4. Deterministic simulation report and replay hash.
5. Test run showing 98 passing tests.
6. Architecture and buyer data-room overview.
7. Docker deployment and health-check demonstration.

Never show account identifiers, credentials, tokens, OTP URLs, personal information, or unsupported performance claims in marketplace media.

## Demo acceptance checklist

- Run `cargo run --locked --bin deriv_ops -- release-readiness` from the exact release snapshot.
- Start the dashboard with a fresh buyer/demo configuration.
- Demonstrate that viewer mode cannot access write controls.
- Demonstrate server-side stake bounds and live-start acknowledgement gates.
- Run the deterministic fixture twice and compare replay hashes.
- Show a graceful stop and persistent emergency stop without abandoning tracked exposure.
- Provide the archive checksum and evidence manifest.
- Record every limitation and excluded asset before price negotiation.
