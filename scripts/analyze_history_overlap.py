#!/usr/bin/env python3
"""Add exact synchronized coverage evidence and a human-readable audit summary."""
from __future__ import annotations
import argparse, csv, json
from datetime import datetime, timezone
from pathlib import Path

UTC_FORMAT = "%Y-%m-%d %H:%M:%S"
GROUPS = {
    "all_ten": ["1HZ100V", "1HZ10V", "1HZ25V", "1HZ50V", "1HZ75V", "R_10", "R_25", "R_50", "R_75", "R_100"],
    "one_hz_family": ["1HZ100V", "1HZ10V", "1HZ25V", "1HZ50V", "1HZ75V"],
    "r_family": ["R_10", "R_25", "R_50", "R_75", "R_100"],
}

def utc(epoch: int | None) -> str | None:
    return datetime.fromtimestamp(epoch, timezone.utc).strftime(UTC_FORMAT) if epoch is not None else None

def epochs(path: Path) -> set[int]:
    with path.open(newline="") as handle:
        return {int(row[1]) for row in csv.reader(handle)}

def group_evidence(root: Path, symbols: list[str], cadence: int) -> dict:
    common = None
    bounds_start = 0
    bounds_end = 2**63 - 1
    for symbol in symbols:
        values = epochs(root / f"{symbol}.csv")
        common = values if common is None else common & values
        bounds_start = max(bounds_start, min(values))
        bounds_end = min(bounds_end, max(values))
    ordered = sorted(common or ())
    runs = []
    if ordered:
        start = previous = ordered[0]
        for value in ordered[1:]:
            if value - previous != cadence:
                runs.append((start, previous, ((previous - start) // cadence) + 1))
                start = value
            previous = value
        runs.append((start, previous, ((previous - start) // cadence) + 1))
    longest = max(runs, key=lambda item: item[2]) if runs else None
    return {
        "symbols": symbols,
        "expected_common_cadence_seconds": cadence,
        "bounding_range_overlap_exists": bounds_start <= bounds_end,
        "bounding_range_overlap_start_utc": utc(bounds_start) if bounds_start <= bounds_end else None,
        "bounding_range_overlap_end_utc": utc(bounds_end) if bounds_start <= bounds_end else None,
        "exact_common_timestamp_count": len(ordered),
        "exact_common_first_utc": utc(ordered[0]) if ordered else None,
        "exact_common_last_utc": utc(ordered[-1]) if ordered else None,
        "contiguous_common_run_count": len(runs),
        "longest_contiguous_common_run": None if longest is None else {
            "start_utc": utc(longest[0]), "end_utc": utc(longest[1]),
            "timestamp_count": longest[2], "duration_seconds": longest[1] - longest[0],
        },
    }

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--normalized", type=Path, required=True)
    parser.add_argument("--audit", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--markdown", type=Path, required=True)
    args = parser.parse_args()
    audit = json.loads(args.audit.read_text())
    evidence = {
        "all_ten": group_evidence(args.normalized, GROUPS["all_ten"], 2),
        "one_hz_family": group_evidence(args.normalized, GROUPS["one_hz_family"], 1),
        "r_family": group_evidence(args.normalized, GROUPS["r_family"], 2),
    }
    audit["synchronized_coverage"] = evidence
    audit["common_overlap"]["warning"] = "Bounding-range overlap is not synchronized coverage; use synchronized_coverage exact intersections."
    args.audit.write_text(json.dumps(audit, indent=2) + "\n")
    manifest = json.loads(args.manifest.read_text())
    manifest["synchronized_coverage"] = evidence
    manifest["qualification_restriction"] = "No combined cross-market/rotation holdout claim is permitted when all_ten exact_common_timestamp_count is zero. Per-symbol work remains conditional on licence and chronological split validation."
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n")

    lines = [
        "# Historical Dataset Audit", "",
        f"Generated: {audit['generated_at_utc']}", "",
        "Raw originals were opened read-only and were not modified. Normalized copies contain valid finite rows sorted ascending; duplicate timestamps are retained and reported.", "",
        "| Symbol | Rows | Raw order | UTC/unit | Malformed | Non-finite | Duplicates | Cadence | Gaps | Start UTC | End UTC |",
        "|---|---:|---|---|---:|---:|---:|---:|---:|---|---|",
    ]
    for item in audit["files"]:
        units = ",".join(f"{k}:{v}" for k,v in item["timestamp_units"].items())
        lines.append(f"| {item['symbol']} | {item['rows_valid_finite']} | {item['source_order']} | mismatches={item['utc_mismatch_count']}; {units} | {item['malformed_count']} | {item['non_finite_price_count']} | {item['duplicate_timestamp_rows']} | {item['expected_interval_seconds']}s | {item['normalized_gap_count']} | {item['normalized_start_utc']} | {item['normalized_end_utc']} |")
    lines += ["", "## Synchronized coverage", ""]
    for name, item in evidence.items():
        longest = item["longest_contiguous_common_run"]
        lines.append(f"- **{name}**: exact common timestamps={item['exact_common_timestamp_count']}; bounding range={item['bounding_range_overlap_start_utc']} to {item['bounding_range_overlap_end_utc']}; longest contiguous exact run={longest}.")
    lines += [
        "", "## Ordering investigation", "",
        "All files are mixed-order concatenations of ascending batches. Each file has 99 backward batch transitions and 100 normalized coverage segments. The 1HZ files contain 5,000-row ascending batches arranged newest-to-oldest, explaining why the raw first row is in July 2026 while the raw last row is in April 2026. The R files are also batch-mixed and include a large discontinuity spanning the older and newer acquisitions.", "",
        "## Qualification restriction", "",
        "The ten-symbol exact synchronized timestamp intersection is empty. Combined cross-market or rotation holdout success must not be claimed. Qualification may only proceed per symbol with chronological train/validation/untouched-holdout splits, subject to licence confirmation. Additional synchronized history covering the same UTC windows for all intended rotation symbols must be downloaded before any combined-market claim.", "",
        "## Licensing", "",
        "Supplier, licence version, acquisition dates, permitted purposes, retention, redistribution, transfer rights and territory remain unverified. Raw datasets must not be distributed.", "",
    ]
    args.markdown.write_text("\n".join(lines))

if __name__ == "__main__":
    main()
