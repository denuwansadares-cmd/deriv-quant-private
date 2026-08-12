# Security Threat Model

## Assets

Broker credentials, account authorization, funds, strategy/config integrity, decision records, source/IP, and licensed datasets.

## Primary threats and controls

| Threat | Current controls | Residual risk |
|---|---|---|
| Secret leakage | env-only inputs, recursive redaction tests, delivery exclusion | local .env must be rotated; authoritative git/CI scan required |
| Duplicate buy after disconnect | uncertain-buy reconciliation, one-open guard, stress/characterization tests | external broker chaos audit still required |
| Stale/corrupt market data | stale kill switch, parser validation, out-of-order tests | provider/schema changes |
| Excess exposure/loss chasing | USD 1-100 absolute selection, balance/exposure scaling, one-contract guard, session loss, drawdown reduction, persistent stop | warning does not remove trading risk; independent quant/risk review absent |
| Strategy enabled without evidence | fail-closed qualification registry | registry governance/signing absent |
| Dashboard abuse | Caddy Basic auth, TLS, private backend port, server-injected short-lived operator/CSRF credentials, role/expiry checks, rate-limited emergency stop, action audit | single operator role; external penetration test and managed identity integration pending |
| Supply-chain compromise | locked dependencies, RustSec CI, SBOM/licence inventory | signed releases/provenance attestation absent |
| Data/API licence breach | 24h retention, metadata-only journal, delivery exclusions | written legal/provider review required |

## Explicit non-claims

No penetration test, external audit, regulatory approval, keychain/Vault deployment, OAuth/RBAC security, or production real-money certification has been completed.
