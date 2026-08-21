# Atrium submission engineering audit

Updated: 2026-08-21. This is an engineering due-diligence record, not legal advice. Conditional findings and manual actions must not be represented as cleared.

## Current result

| Area | Status | Evidence and required action |
|---|---|---|
| Repository history | **PASS** | Git history is available. The reachable `main` history contains 13 commits spanning 2026-08-03 through 2026-08-21. No history was rewritten. |
| Secrets scan | **PASS FOR REACHABLE HISTORY** | Gitleaks scanned all 13 reachable commits with redaction enabled and found no secrets. The scan used a read-only repository mount with container networking disabled. |
| Local credentials | **MANUAL ACTION** | Ignored local credential files were not read and are not part of the tracked submission. Credential rotation or revocation remains a manual seller action and must not be marked complete without seller confirmation. |
| Verification | **PASS** | On 2026-08-21, formatting, the offline locked all-target build, strict Clippy with warnings denied, and the complete offline suite passed: 104 passed, 0 failed, 0 ignored. |
| Third-party inventory | **CONDITIONAL** | `Cargo.lock` contains one local root package and 217 registry packages. The headerless notice TSV contains 217 matching registry package identities. The all-target CycloneDX SBOM contains 217 library components. Preserve notices and obtain appropriate legal review of distribution obligations. |
| Transaction rights | **SEPARATE AGREEMENT REQUIRED** | This appraisal repository does not independently grant rights. Rights, price, and permitted use are governed only by a separately accepted written transaction agreement. See `SUBMISSION_SCOPE.md`. |
| Dataset rights | **NOT INCLUDED OR VERIFIED HERE** | Historical datasets and the removed private audit reports are not part of the submission. Any future dataset transfer requires separate, current evidence of the relevant rights. |

## Submission exclusions

The current submission excludes local credentials, runtime state, historical datasets, generated private dataset reports, logs, backups, build output, buyer bundles, and historical transaction drafts. `.gitignore` covers the corresponding local paths. Third-party packages remain governed by their own licences and provider services remain governed by their applicable terms.

## Manual actions before connection or delivery

1. The seller must independently decide and confirm whether local credentials have been rotated or revoked; this repository does not record that action as complete.
2. Keep the hosting repository private and grant Atrium selected-repository, read-only access only.
3. Handle transaction documents separately and do not represent them as legally reviewed without direct evidence.
4. Re-run the secret scan and engineering verification after any further source or dependency change.
5. Review third-party licence obligations and any proposed dataset/provider rights for the actual transaction.
