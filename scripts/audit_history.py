#!/usr/bin/env python3
"""Audit headerless UTC,epoch,price CSV files and create protected ascending copies."""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import statistics
from collections import Counter
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

EXPECTED = ["1HZ100V", "1HZ10V", "1HZ25V", "1HZ50V", "1HZ75V", "R_10", "R_25", "R_50", "R_75", "R_100"]
UTC_FORMAT = "%Y-%m-%d %H:%M:%S"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def epoch_unit(value: int) -> tuple[str, float]:
    magnitude = abs(value)
    if magnitude >= 10**17:
        return "nanoseconds", value / 1_000_000_000
    if magnitude >= 10**14:
        return "microseconds", value / 1_000_000
    if magnitude >= 10**11:
        return "milliseconds", value / 1_000
    return "seconds", float(value)


def audit_one(source: Path, destination: Path) -> dict[str, Any]:
    rows: list[tuple[int, str, int]] = []
    malformed: list[dict[str, Any]] = []
    non_finite: list[dict[str, Any]] = []
    utc_mismatches: list[dict[str, Any]] = []
    utc_mismatch_total = 0
    units: Counter[str] = Counter()
    epoch_counts: Counter[int] = Counter()
    positive_deltas: Counter[int] = Counter()
    positive = negative = zero = 0
    previous_epoch: int | None = None
    backward_examples: list[dict[str, Any]] = []
    first_valid: dict[str, Any] | None = None
    last_valid: dict[str, Any] | None = None

    with source.open("r", encoding="utf-8", newline="") as handle:
        for line_number, fields in enumerate(csv.reader(handle), 1):
            if len(fields) != 3:
                malformed.append({"line": line_number, "reason": "column_count", "columns": len(fields)})
                continue
            timestamp_text, epoch_text, price_text = (field.strip() for field in fields)
            try:
                epoch_raw = int(epoch_text)
            except ValueError:
                malformed.append({"line": line_number, "reason": "invalid_epoch"})
                continue
            unit, epoch_seconds_float = epoch_unit(epoch_raw)
            units[unit] += 1
            if not math.isfinite(epoch_seconds_float) or epoch_seconds_float != math.trunc(epoch_seconds_float):
                malformed.append({"line": line_number, "reason": "non_integral_epoch_seconds", "unit": unit})
                continue
            epoch_seconds = int(epoch_seconds_float)
            try:
                parsed = datetime.strptime(timestamp_text, UTC_FORMAT).replace(tzinfo=timezone.utc)
            except ValueError:
                malformed.append({"line": line_number, "reason": "invalid_utc_timestamp"})
                continue
            expected_text = datetime.fromtimestamp(epoch_seconds, timezone.utc).strftime(UTC_FORMAT)
            if parsed.strftime(UTC_FORMAT) != expected_text:
                utc_mismatch_total += 1
                if len(utc_mismatches) < 20:
                    utc_mismatches.append({"line": line_number, "text": timestamp_text, "epoch_utc": expected_text})
            try:
                price = Decimal(price_text)
            except InvalidOperation:
                malformed.append({"line": line_number, "reason": "invalid_price"})
                continue
            if not price.is_finite():
                non_finite.append({"line": line_number, "kind": str(price)})
                continue

            record = {"line": line_number, "utc": timestamp_text, "epoch": epoch_seconds}
            first_valid = first_valid or record
            last_valid = record
            if previous_epoch is not None:
                delta = epoch_seconds - previous_epoch
                if delta > 0:
                    positive += 1
                    positive_deltas[delta] += 1
                elif delta < 0:
                    negative += 1
                    if len(backward_examples) < 20:
                        backward_examples.append({"line": line_number, "previous_epoch": previous_epoch, "epoch": epoch_seconds, "delta": delta})
                else:
                    zero += 1
            previous_epoch = epoch_seconds
            epoch_counts[epoch_seconds] += 1
            rows.append((epoch_seconds, price_text, line_number))

    if not rows:
        raise RuntimeError(f"No valid finite rows in {source}")
    if negative == 0:
        source_order = "ascending" if positive else "constant"
    elif positive == 0:
        source_order = "descending"
    else:
        source_order = "mixed"
    expected_interval = positive_deltas.most_common(1)[0][0] if positive_deltas else 0
    duplicate_timestamps = sum(count - 1 for count in epoch_counts.values() if count > 1)
    duplicate_keys = sum(1 for count in epoch_counts.values() if count > 1)

    rows.sort(key=lambda row: (row[0], row[2]))
    normalized_gap_count = inconsistent_interval_count = estimated_missing = 0
    largest_gap = 0
    prior: int | None = None
    for epoch, _, _ in rows:
        if prior is not None:
            delta = epoch - prior
            if delta > largest_gap:
                largest_gap = delta
            if delta > 0 and expected_interval and delta != expected_interval:
                inconsistent_interval_count += 1
                if delta > expected_interval:
                    normalized_gap_count += 1
                    estimated_missing += max(0, (delta // expected_interval) - 1)
        prior = epoch

    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + f".tmp-{os.getpid()}")
    with temporary.open("w", encoding="utf-8", newline="") as handle:
        os.chmod(temporary, 0o600)
        writer = csv.writer(handle, lineterminator="\n")
        for epoch, price_text, _ in rows:
            writer.writerow([datetime.fromtimestamp(epoch, timezone.utc).strftime(UTC_FORMAT), epoch, price_text])
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, destination)
    os.chmod(destination, 0o600)

    return {
        "symbol": source.stem,
        "source_path": str(source),
        "normalized_path": str(destination),
        "raw_sha256": sha256(source),
        "normalized_sha256": sha256(destination),
        "raw_size_bytes": source.stat().st_size,
        "normalized_size_bytes": destination.stat().st_size,
        "rows_total": len(rows) + len(malformed) + len(non_finite),
        "rows_valid_finite": len(rows),
        "malformed_count": len(malformed),
        "malformed_examples": malformed[:20],
        "non_finite_price_count": len(non_finite),
        "non_finite_examples": non_finite[:20],
        "timestamp_units": dict(units),
        "utc_mismatch_count": utc_mismatch_total,
        "raw_mtime_utc": datetime.fromtimestamp(source.stat().st_mtime, timezone.utc).isoformat(),
        "raw_mode": oct(source.stat().st_mode & 0o777),
        "utc_mismatch_examples": utc_mismatches,
        "source_order": source_order,
        "ascending_transitions": positive,
        "descending_transitions": negative,
        "duplicate_transitions": zero,
        "backward_jump_examples": backward_examples,
        "duplicate_timestamp_keys": duplicate_keys,
        "duplicate_timestamp_rows": duplicate_timestamps,
        "expected_interval_seconds": expected_interval,
        "normalized_gap_count": normalized_gap_count,
        "normalized_inconsistent_interval_count": inconsistent_interval_count,
        "estimated_missing_intervals": estimated_missing,
        "largest_normalized_gap_seconds": largest_gap,
        "raw_first_valid": first_valid,
        "raw_last_valid": last_valid,
        "normalized_start_utc": datetime.fromtimestamp(rows[0][0], timezone.utc).strftime(UTC_FORMAT),
        "normalized_start_epoch": rows[0][0],
        "normalized_end_utc": datetime.fromtimestamp(rows[-1][0], timezone.utc).strftime(UTC_FORMAT),
        "normalized_end_epoch": rows[-1][0],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--normalized", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()

    actual = sorted(path.stem for path in args.raw.glob("*.csv"))
    if actual != sorted(EXPECTED):
        raise SystemExit(f"Expected exactly {sorted(EXPECTED)}, found {actual}")
    args.normalized.mkdir(parents=True, exist_ok=True)
    os.chmod(args.normalized, 0o700)

    results = [audit_one(args.raw / f"{symbol}.csv", args.normalized / f"{symbol}.csv") for symbol in EXPECTED]
    overlap_start = max(item["normalized_start_epoch"] for item in results)
    overlap_end = min(item["normalized_end_epoch"] for item in results)
    overlap = {
        "exists": overlap_start <= overlap_end,
        "start_epoch": overlap_start,
        "end_epoch": overlap_end,
        "start_utc": datetime.fromtimestamp(overlap_start, timezone.utc).strftime(UTC_FORMAT),
        "end_utc": datetime.fromtimestamp(overlap_end, timezone.utc).strftime(UTC_FORMAT),
        "duration_seconds": max(0, overlap_end - overlap_start),
    }
    report = {
        "schema_version": 1,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "raw_directory": str(args.raw),
        "normalized_directory": str(args.normalized),
        "raw_files_modified": False,
        "files": results,
        "common_overlap": overlap,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    manifest = {
        "schema_version": 1,
        "generated_at_utc": report["generated_at_utc"],
        "provider_or_supplier": "owner-supplied external directory; exact supplier unverified",
        "source_directory": str(args.raw),
        "licence_status": "UNVERIFIED",
        "ownership_status": "UNVERIFIED",
        "permitted_purpose": "local audit and qualification only pending owner/legal confirmation",
        "redistribution_permitted": False,
        "transfer_permitted": False,
        "retention_permission": "UNVERIFIED",
        "territory": "UNVERIFIED",
        "acquisition_dates": "UNVERIFIED",
        "required_owner_action": "Document supplier, licence text/version, acquisition dates, permitted uses, retention, deletion, redistribution, transfer and territory before distribution.",
        "derivation": "Valid finite rows canonicalized to UTC and sorted ascending; originals not modified; duplicates retained and reported.",
        "files": [{key: item[key] for key in ("symbol", "raw_sha256", "normalized_sha256", "rows_valid_finite", "normalized_start_utc", "normalized_end_utc")} for item in results],
        "common_overlap": overlap,
    }
    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
