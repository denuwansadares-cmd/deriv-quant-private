# Atrium submission engineering audit

Updated: 2026-08-22. This is an engineering due-diligence record, not legal advice. Conditional findings and manual actions must not be represented as cleared.

## Current result

| Area | Status | Evidence and required action |
|---|---|---|
| Repository history | **PASS** | Git history is available. The reachable `main` history contains 16 commits spanning 2026-08-03 through 2026-08-22. No history was rewritten. |
| Secrets scan | **PASS — FULL REACHABLE HISTORY** | Gitleaks 8.30.1 scanned all 16 commits reachable from all 2 refs at HEAD `296985c266890227fa29b39919dfd9e8bf1ce0bd`, with redaction enabled, a read-only repository mount, and container networking disabled. No leaks were found. This does not prove ignored local credentials have been rotated. |
| Local credentials | **MANUAL ACTION** | Ignored local credential files were not read and are not part of the tracked submission. Credential rotation or revocation remains a manual seller action and must not be marked complete without seller confirmation. |
| Verification | **PASS** | On 2026-08-22, formatting, the offline locked all-target build, strict Clippy with warnings denied, and the complete offline suite passed: 111 passed, 0 failed, 0 ignored. |
| Third-party inventory | **RECONCILED; LEGAL REVIEW REQUIRED** | Cargo metadata, the headerless inventory TSV, the all-target SBOM, and the generated notice bundle reconcile at exactly 217/217/217 identities/components/sections. `defmt-parser 1.0.0` contains no packaged licence text and remains explicitly marked for manual review; licence alternatives are preserved without selecting one. |
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
