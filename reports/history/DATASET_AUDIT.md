# Historical Dataset Audit

Generated: 2026-08-03T16:18:44.642915+00:00

Raw originals were opened read-only and were not modified. Normalized copies contain valid finite rows sorted ascending; duplicate timestamps are retained and reported.

| Symbol | Rows | Raw order | UTC/unit | Malformed | Non-finite | Duplicates | Cadence | Gaps | Start UTC | End UTC |
|---|---:|---|---|---:|---:|---:|---:|---:|---|---|
| 1HZ100V | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 1s | 99 | 2026-04-18 02:15:54 | 2026-07-26 03:39:13 |
| 1HZ10V | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 1s | 99 | 2026-04-18 01:43:21 | 2026-07-26 03:06:40 |
| 1HZ25V | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 1s | 99 | 2026-04-18 01:52:07 | 2026-07-26 03:15:26 |
| 1HZ50V | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 1s | 99 | 2026-04-18 02:00:15 | 2026-07-26 03:23:34 |
| 1HZ75V | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 1s | 99 | 2026-04-18 02:08:17 | 2026-07-26 03:31:36 |
| R_10 | 500017 | mixed | mismatches=0; seconds:500017 | 0 | 0 | 0 | 2s | 99 | 2025-07-25 21:12:46 | 2026-07-25 14:29:20 |
| R_25 | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 2s | 99 | 2025-07-25 21:13:20 | 2026-07-25 14:37:44 |
| R_50 | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 2s | 99 | 2025-07-25 21:13:20 | 2026-07-25 14:45:32 |
| R_75 | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 2s | 99 | 2025-07-25 21:13:20 | 2026-07-25 14:52:30 |
| R_100 | 500000 | mixed | mismatches=0; seconds:500000 | 0 | 0 | 0 | 2s | 99 | 2025-07-25 21:13:20 | 2026-07-25 14:59:04 |

## Synchronized coverage

- **all_ten**: exact common timestamps=0; bounding range=2026-04-18 02:15:54 to 2026-07-25 14:29:20; longest contiguous exact run=None.
- **one_hz_family**: exact common timestamps=304700; bounding range=2026-04-18 02:15:54 to 2026-07-26 03:06:40; longest contiguous exact run={'start_utc': '2026-04-18 02:15:54', 'end_utc': '2026-04-18 03:06:40', 'timestamp_count': 3047, 'duration_seconds': 3046}.
- **r_family**: exact common timestamps=411692; bounding range=2025-07-25 21:13:20 to 2026-07-25 14:29:20; longest contiguous exact run={'start_utc': '2025-07-25 21:13:20', 'end_utc': '2025-07-25 23:59:58', 'timestamp_count': 5000, 'duration_seconds': 9998}.

## Ordering investigation

All files are mixed-order concatenations of ascending batches. Each file has 99 backward batch transitions and 100 normalized coverage segments. The 1HZ files contain 5,000-row ascending batches arranged newest-to-oldest, explaining why the raw first row is in July 2026 while the raw last row is in April 2026. The R files are also batch-mixed and include a large discontinuity spanning the older and newer acquisitions.

## Qualification restriction

The ten-symbol exact synchronized timestamp intersection is empty. Combined cross-market or rotation holdout success must not be claimed. Qualification may only proceed per symbol with chronological train/validation/untouched-holdout splits, subject to licence confirmation. Additional synchronized history covering the same UTC windows for all intended rotation symbols must be downloaded before any combined-market claim.

## Licensing

Supplier, licence version, acquisition dates, permitted purposes, retention, redistribution, transfer rights and territory remain unverified. Raw datasets must not be distributed.
