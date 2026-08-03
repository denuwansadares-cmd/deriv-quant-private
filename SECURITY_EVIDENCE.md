# Reliability and Security Evidence

Evidence date: 2026-08-03

## Verified locally

- 10,000 seeded lifecycle disconnect scenarios completed with zero duplicate-buy decisions in the proof model.
- 10,000 malformed/hostile JSON cases completed without panic or a fabricated contract-open event.
- 10,000 seeded state-transition properties matched the allow-list; rejected transitions did not mutate state.
- 10,000 deterministic RNG steps matched for identical seeds.
- The full suite passed: 75 passed, 0 failed, 1 ignored.
- Two isolated locked release builds were bit-for-bit identical (SHA-256 b28592d78b8d9ba0867a2479ea0fc4859e273651633fba66f5b41c3af322005f).
- RustSec scanned 218 locked dependencies against advisory database commit d91a8fc9492378f23cba86b81770c6d16de6ebba: 0 known vulnerabilities.

These are engineering test results, not profitability or live-trading results. The duplicate-buy test is a lifecycle proof model plus existing runtime characterization tests; it is not a claim that 10,000 real broker sessions were executed.

## Not yet verified

- 72-hour wall-clock soak and bounded-memory claim.
- External penetration/security review.
- Independent license counsel review.
- Signed release binaries and third-party signature verification.
- Cross-machine bit-for-bit reproducible release.
- OS keychain/Vault integration.
- OAuth2 PKCE, RBAC, and multi-user security.
- Full process-level network chaos against a broker-compatible proxy.

These remain release gates. They must not appear in buyer material as completed work.
