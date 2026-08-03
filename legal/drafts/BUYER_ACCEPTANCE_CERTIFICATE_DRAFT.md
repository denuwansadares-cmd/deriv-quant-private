# Buyer delivery and acceptance certificate

**DRAFT ONLY — NOT ACCEPTED OR SIGNED**

This certificate relates to the proposed Exclusive Proprietary Usage Licence between **PANAMULLA ARACHCHIGE SADARES DENUWAN** (“Licensor”) and **[BUYER LEGAL NAME]** (“Licensee”). It has no effect unless completed and signed consistently with the final licence agreement.

## 1. Delivery identification

- Product/version: [PRODUCT / VERSION]
- Delivery date and UTC time: [DATE/TIME]
- Git commit: [FULL COMMIT HASH]
- Source archive filename: [FILENAME]
- Source archive SHA-256: [SHA-256]
- Evidence manifest filename and SHA-256: [DETAILS]
- Container image digest, if included: [DIGEST / NOT INCLUDED]
- Secure delivery channel: [CHANNEL]
- Acceptance-period start/end: [DATES]

## 2. Included items

Check only items actually delivered and identify their hashes or paths.

- [ ] Proprietary source-code snapshot
- [ ] `Cargo.toml` and locked dependency graph
- [ ] Bot source
- [ ] Watchdog source
- [ ] Dashboard source and static assets
- [ ] Docker and Compose definitions
- [ ] Automated tests and dated test report
- [ ] Architecture and operator documentation
- [ ] Known limitations and claims document
- [ ] SBOM and third-party licence inventory
- [ ] Public evidence-verification key
- [ ] Signed evidence manifest
- [ ] Secret-free environment-variable example
- [ ] Final signed usage licence
- [ ] Final 30-day support terms
- [ ] Other: [DETAILS]

## 3. Expressly excluded items

The Licensee confirms that the delivery does **not** include:

- raw Deriv historical CSV files;
- normalized, sorted, transformed, or derived copies of those CSV files;
- raw API/WebSocket recordings or provider feed caches;
- any Deriv account, App ID, OAuth registration, PAT, access token, OTP, credential, balance, rate-limit allocation, or transferable API/data permission;
- real-money trading approval, financial advice, a profit guarantee, a win-rate guarantee, customers, revenue, or regulatory approval;
- third-party ownership rights beyond rights independently granted by the applicable third-party licences; or
- future features, hosting, operations, data acquisition, or support beyond the signed scope.

## 4. Buyer-run acceptance checks

Acceptance must be based on the delivered supported environment, not an undisclosed modified environment.

- [ ] Archive SHA-256 matches the signed delivery manifest
- [ ] Evidence signature verifies with the delivered public key
- [ ] Secret scan passes
- [ ] Formatting check passes
- [ ] Locked tests pass; exact results attached
- [ ] Strict Clippy passes
- [ ] Docker image builds without live-trading feature
- [ ] Runtime user is non-root
- [ ] Dashboard is bound to loopback in the supplied Compose configuration
- [ ] Unauthenticated emergency stop is rejected
- [ ] Correctly authenticated emergency stop is accepted
- [ ] Persistent-stop and reduce-only behavior matches documented tests
- [ ] Raw historical data is absent from the archive and image
- [ ] `.env` and seller credentials are absent
- [ ] Known limitations were received and reviewed

Test environment: [OS / ARCH / DOCKER / RUST VERSIONS]  
Commands executed: [COMMANDS]  
Attached result hashes: [HASHES]

## 5. Acceptance decision

Select exactly one:

- [ ] **Accepted.** The identified delivery materially satisfies the signed acceptance criteria.
- [ ] **Rejected for a claimed Material Defect.** Complete Section 6 within the contractual acceptance period.

No checkbox is pre-selected. Acceptance alone does not make the licence effective; every payment, signature, and SHA-256-bound acceptance condition in the final agreement must also be satisfied. Silence, production use, or expiry has only the effect stated in the final lawyer-reviewed agreement.

## 6. Material-defect notice, if applicable

- Failed criterion: [DETAIL]
- Reproduction steps: [DETAIL]
- Expected result: [DETAIL]
- Observed result: [DETAIL]
- Redacted log/evidence hash: [SHA-256]
- Buyer modifications present: [YES/NO; DETAILS]
- Notice date/time and delivery method: [DETAIL]

Trading outcome, provider changes/outages, unsupported modifications, absent future features, and matters disclosed as limitations are not automatically Material Defects.

## 7. Buyer acknowledgements

The Licensee acknowledges that ownership remains with PANAMULLA ARACHCHIGE SADARES DENUWAN; no intellectual-property assignment is made, and only the effective signed exclusive usage licence grants rights. The product is not financial advice and no profitability, win rate, uninterrupted service, provider availability, data licence, or real-money suitability is promised.

Licensee legal name: [BUYER LEGAL NAME]  
Authorized signatory/title: [NAME / TITLE]  
Signature: ______________________________  
Date: __________________________________

Licensor acknowledgement of receipt only, not buyer acceptance:

Name: PANAMULLA ARACHCHIGE SADARES DENUWAN  
Signature: ______________________________  
Date: __________________________________
