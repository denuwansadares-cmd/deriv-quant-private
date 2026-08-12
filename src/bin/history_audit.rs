use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    io::{BufWriter, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const EXPECTED: [&str; 10] = [
    "1HZ100V", "1HZ10V", "1HZ25V", "1HZ50V", "1HZ75V", "R_10", "R_25", "R_50", "R_75", "R_100",
];
fn utc(epoch: i64) -> String {
    DateTime::<Utc>::from_timestamp(epoch, 0)
        .map(|v| v.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}
fn iso_now() -> String {
    Utc::now().to_rfc3339()
}
fn hash(path: &Path) -> Result<String> {
    let output = Command::new("sha256sum").arg(path).output()?;
    if !output.status.success() {
        return Err("sha256sum failed".into());
    }
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .ok_or("missing digest")?
        .to_owned())
}
fn arg(args: &[String], name: &str) -> Result<PathBuf> {
    args.windows(2)
        .find(|v| v[0] == name)
        .map(|v| PathBuf::from(&v[1]))
        .ok_or_else(|| format!("missing {name}").into())
}
fn epoch(raw: i64) -> (&'static str, i64, bool) {
    let magnitude = raw.unsigned_abs();
    let (unit, divisor) = if magnitude >= 100_000_000_000_000_000 {
        ("nanoseconds", 1_000_000_000)
    } else if magnitude >= 100_000_000_000_000 {
        ("microseconds", 1_000_000)
    } else if magnitude >= 100_000_000_000 {
        ("milliseconds", 1_000)
    } else {
        ("seconds", 1)
    };
    (unit, raw / divisor, raw % divisor == 0)
}
fn atomic_csv(path: &Path, rows: &[(i64, String, usize)]) -> Result {
    let parent = path.parent().ok_or("missing parent")?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("csv.tmp-{}", std::process::id()));
    let mut out = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?,
    );
    for (epoch, price, _) in rows {
        writeln!(out, "{},{epoch},{price}", utc(*epoch))?;
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    fs::rename(temp, path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
fn audit_file(source: &Path, destination: &Path) -> Result<Value> {
    let mut rows = Vec::new();
    let mut malformed = Vec::new();
    let mut non_finite = Vec::new();
    let mut mismatches = Vec::new();
    let mut mismatch_total = 0_u64;
    let mut units: BTreeMap<&str, u64> = BTreeMap::new();
    let mut counts = HashMap::new();
    let mut deltas = HashMap::new();
    let (mut positive, mut negative, mut zero) = (0_u64, 0_u64, 0_u64);
    let mut previous = None;
    let mut backward = Vec::new();
    let mut first = None;
    let mut last = None;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(source)?;
    for (index, record) in reader.records().enumerate() {
        let line = index + 1;
        let record = match record {
            Ok(v) => v,
            Err(_) => {
                malformed.push(json!({"line":line,"reason":"csv_parse"}));
                continue;
            }
        };
        if record.len() != 3 {
            malformed.push(json!({"line":line,"reason":"column_count","columns":record.len()}));
            continue;
        }
        let raw = match record[1].trim().parse::<i64>() {
            Ok(v) => v,
            Err(_) => {
                malformed.push(json!({"line":line,"reason":"invalid_epoch"}));
                continue;
            }
        };
        let (unit, seconds, integral) = epoch(raw);
        *units.entry(unit).or_default() += 1;
        if !integral {
            malformed.push(json!({"line":line,"reason":"non_integral_epoch_seconds","unit":unit}));
            continue;
        }
        let timestamp = record[0].trim();
        if NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S").is_err() {
            malformed.push(json!({"line":line,"reason":"invalid_utc_timestamp"}));
            continue;
        }
        let expected = utc(seconds);
        if timestamp != expected {
            mismatch_total += 1;
            if mismatches.len() < 20 {
                mismatches.push(json!({"line":line,"text":timestamp,"epoch_utc":expected}));
            }
        }
        let price_text = record[2].trim();
        let price = match price_text.parse::<f64>() {
            Ok(v) => v,
            Err(_) => {
                malformed.push(json!({"line":line,"reason":"invalid_price"}));
                continue;
            }
        };
        if !price.is_finite() {
            non_finite.push(json!({"line":line,"kind":price_text}));
            continue;
        }
        let item = json!({"line":line,"utc":timestamp,"epoch":seconds});
        if first.is_none() {
            first = Some(item.clone())
        }
        last = Some(item);
        if let Some(old) = previous {
            let delta = seconds - old;
            if delta > 0 {
                positive += 1;
                *deltas.entry(delta).or_insert(0_u64) += 1;
            } else if delta < 0 {
                negative += 1;
                if backward.len() < 20 {
                    backward.push(
                        json!({"line":line,"previous_epoch":old,"epoch":seconds,"delta":delta}),
                    );
                }
            } else {
                zero += 1;
            }
        }
        previous = Some(seconds);
        *counts.entry(seconds).or_insert(0_u64) += 1;
        rows.push((seconds, price_text.to_owned(), line));
    }
    if rows.is_empty() {
        return Err(format!("no valid finite rows in {}", source.display()).into());
    }
    let source_order = if negative == 0 {
        if positive > 0 {
            "ascending"
        } else {
            "constant"
        }
    } else if positive == 0 {
        "descending"
    } else {
        "mixed"
    };
    let interval = deltas
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(delta, _)| delta)
        .unwrap_or(0);
    let duplicate_keys = counts.values().filter(|v| **v > 1).count();
    let duplicate_rows: u64 = counts.values().filter(|v| **v > 1).map(|v| v - 1).sum();
    rows.sort_by_key(|v| (v.0, v.2));
    let (mut gaps, mut inconsistent, mut missing, mut largest) = (0_u64, 0_u64, 0_i64, 0_i64);
    let mut prior = None;
    for (value, _, _) in &rows {
        if let Some(old) = prior {
            let delta = value - old;
            largest = largest.max(delta);
            if delta > 0 && interval > 0 && delta != interval {
                inconsistent += 1;
                if delta > interval {
                    gaps += 1;
                    missing += (delta / interval - 1).max(0);
                }
            }
        }
        prior = Some(*value);
    }
    atomic_csv(destination, &rows)?;
    let metadata = fs::metadata(source)?;
    let modified: DateTime<Utc> = metadata.modified()?.into();
    Ok(
        json!({"symbol":source.file_stem().and_then(|v|v.to_str()).unwrap_or(""),"source_path":source,"normalized_path":destination,"raw_sha256":hash(source)?,"normalized_sha256":hash(destination)?,"raw_size_bytes":metadata.len(),"normalized_size_bytes":fs::metadata(destination)?.len(),"rows_total":rows.len()+malformed.len()+non_finite.len(),"rows_valid_finite":rows.len(),"malformed_count":malformed.len(),"malformed_examples":malformed.into_iter().take(20).collect::<Vec<_>>(),"non_finite_price_count":non_finite.len(),"non_finite_examples":non_finite.into_iter().take(20).collect::<Vec<_>>(),"timestamp_units":units,"utc_mismatch_count":mismatch_total,"raw_mtime_utc":modified.to_rfc3339(),"raw_mode":format!("0o{:o}",metadata.permissions().mode()&0o777),"utc_mismatch_examples":mismatches,"source_order":source_order,"ascending_transitions":positive,"descending_transitions":negative,"duplicate_transitions":zero,"backward_jump_examples":backward,"duplicate_timestamp_keys":duplicate_keys,"duplicate_timestamp_rows":duplicate_rows,"expected_interval_seconds":interval,"normalized_gap_count":gaps,"normalized_inconsistent_interval_count":inconsistent,"estimated_missing_intervals":missing,"largest_normalized_gap_seconds":largest,"raw_first_valid":first,"raw_last_valid":last,"normalized_start_utc":utc(rows[0].0),"normalized_start_epoch":rows[0].0,"normalized_end_utc":utc(rows.last().unwrap().0),"normalized_end_epoch":rows.last().unwrap().0}),
    )
}
fn write_json(path: &Path, value: &Value) -> Result {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}
fn audit(args: &[String]) -> Result {
    let raw = arg(args, "--raw")?;
    let normalized = arg(args, "--normalized")?;
    let report_path = arg(args, "--report")?;
    let manifest_path = arg(args, "--manifest")?;
    let mut actual = fs::read_dir(&raw)?
        .filter_map(|v| v.ok())
        .filter(|v| v.path().extension().is_some_and(|e| e == "csv"))
        .filter_map(|v| v.path().file_stem()?.to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    actual.sort();
    let mut expected = EXPECTED.map(str::to_owned).to_vec();
    expected.sort();
    if actual != expected {
        return Err(format!("expected exactly {expected:?}, found {actual:?}").into());
    }
    fs::create_dir_all(&normalized)?;
    fs::set_permissions(&normalized, fs::Permissions::from_mode(0o700))?;
    let files = EXPECTED
        .iter()
        .map(|symbol| {
            audit_file(
                &raw.join(format!("{symbol}.csv")),
                &normalized.join(format!("{symbol}.csv")),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let start = files
        .iter()
        .filter_map(|v| v["normalized_start_epoch"].as_i64())
        .max()
        .unwrap();
    let end = files
        .iter()
        .filter_map(|v| v["normalized_end_epoch"].as_i64())
        .min()
        .unwrap();
    let overlap = json!({"exists":start<=end,"start_epoch":start,"end_epoch":end,"start_utc":utc(start),"end_utc":utc(end),"duration_seconds":0.max(end-start)});
    let generated = iso_now();
    let report = json!({"schema_version":1,"generated_at_utc":generated,"raw_directory":raw,"normalized_directory":normalized,"raw_files_modified":false,"files":files,"common_overlap":overlap});
    write_json(&report_path, &report)?;
    let entries=report["files"].as_array().unwrap().iter().map(|v|json!({"symbol":v["symbol"],"raw_sha256":v["raw_sha256"],"normalized_sha256":v["normalized_sha256"],"rows_valid_finite":v["rows_valid_finite"],"normalized_start_utc":v["normalized_start_utc"],"normalized_end_utc":v["normalized_end_utc"]})).collect::<Vec<_>>();
    write_json(
        &manifest_path,
        &json!({"schema_version":1,"generated_at_utc":generated,"provider_or_supplier":"Deriv (operator-reported support confirmation, 2026-08-06)","source_directory":raw,"licence_status":"NO_SEPARATE_LICENCE_REQUIRED_FOR_LONG_TERM_RETENTION","ownership_status":"UNVERIFIED","permitted_purpose":"local storage, audit, research, and backtesting","redistribution_permitted":false,"transfer_permitted":false,"retention_permission":"CONFIRMED_LONG_TERM","territory":"UNVERIFIED","acquisition_dates":"UNVERIFIED","required_owner_action":"Preserve the original Deriv support confirmation as transfer evidence.","derivation":"Valid finite rows canonicalized to UTC and sorted ascending; originals not modified; duplicates retained and reported.","files":entries,"common_overlap":overlap}),
    )
}
fn epoch_set(path: &Path) -> Result<BTreeSet<i64>> {
    let mut set = BTreeSet::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)?;
    for row in reader.records() {
        set.insert(row?[1].parse()?);
    }
    Ok(set)
}
fn evidence(root: &Path, symbols: &[&str], cadence: i64) -> Result<Value> {
    let mut common: Option<BTreeSet<i64>> = None;
    let (mut bound_start, mut bound_end) = (i64::MIN, i64::MAX);
    for symbol in symbols {
        let values = epoch_set(&root.join(format!("{symbol}.csv")))?;
        bound_start = bound_start.max(*values.first().ok_or("empty dataset")?);
        bound_end = bound_end.min(*values.last().ok_or("empty dataset")?);
        common = Some(match common {
            None => values,
            Some(old) => old.intersection(&values).copied().collect(),
        });
    }
    let ordered = common.unwrap_or_default().into_iter().collect::<Vec<_>>();
    let mut runs = Vec::new();
    if let Some(&first) = ordered.first() {
        let (mut start, mut previous) = (first, first);
        for &value in &ordered[1..] {
            if value - previous != cadence {
                runs.push((start, previous, (previous - start) / cadence + 1));
                start = value;
            }
            previous = value;
        }
        runs.push((start, previous, (previous - start) / cadence + 1));
    }
    let longest = runs.iter().max_by_key(|v| v.2);
    Ok(
        json!({"symbols":symbols,"expected_common_cadence_seconds":cadence,"bounding_range_overlap_exists":bound_start<=bound_end,"bounding_range_overlap_start_utc":(bound_start<=bound_end).then(||utc(bound_start)),"bounding_range_overlap_end_utc":(bound_start<=bound_end).then(||utc(bound_end)),"exact_common_timestamp_count":ordered.len(),"exact_common_first_utc":ordered.first().map(|v|utc(*v)),"exact_common_last_utc":ordered.last().map(|v|utc(*v)),"contiguous_common_run_count":runs.len(),"longest_contiguous_common_run":longest.map(|v|json!({"start_utc":utc(v.0),"end_utc":utc(v.1),"timestamp_count":v.2,"duration_seconds":v.1-v.0}))}),
    )
}
fn overlap(args: &[String]) -> Result {
    let normalized = arg(args, "--normalized")?;
    let audit_path = arg(args, "--audit")?;
    let manifest_path = arg(args, "--manifest")?;
    let markdown = arg(args, "--markdown")?;
    let one = ["1HZ100V", "1HZ10V", "1HZ25V", "1HZ50V", "1HZ75V"];
    let r = ["R_10", "R_25", "R_50", "R_75", "R_100"];
    let coverage = json!({"all_ten":evidence(&normalized,&EXPECTED,2)?,"one_hz_family":evidence(&normalized,&one,1)?,"r_family":evidence(&normalized,&r,2)?});
    let mut audit: Value = serde_json::from_str(&fs::read_to_string(&audit_path)?)?;
    audit["synchronized_coverage"] = coverage.clone();
    audit["common_overlap"]["warning"]=json!("Bounding-range overlap is not synchronized coverage; use synchronized_coverage exact intersections.");
    write_json(&audit_path, &audit)?;
    let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    manifest["synchronized_coverage"] = coverage.clone();
    manifest["qualification_restriction"]=json!("No combined cross-market/rotation holdout claim is permitted when all_ten exact_common_timestamp_count is zero.");
    write_json(&manifest_path, &manifest)?;
    let mut text=format!("# Historical Dataset Audit\n\nGenerated: {}\n\nRaw originals were not modified. Normalized copies contain valid finite rows sorted ascending.\n\n## Synchronized coverage\n\n",audit["generated_at_utc"].as_str().unwrap_or("unknown"));
    for name in ["all_ten", "one_hz_family", "r_family"] {
        let item = &coverage[name];
        text.push_str(&format!("- **{name}**: exact common timestamps={}; bounding range={} to {}; longest contiguous exact run={}.\n",item["exact_common_timestamp_count"],item["bounding_range_overlap_start_utc"],item["bounding_range_overlap_end_utc"],item["longest_contiguous_common_run"]));
    }
    text.push_str("\n## Qualification restriction\n\nCombined-market claims require exact synchronized holdout coverage. Per-symbol work remains conditional on licence and chronological split validation.\n");
    fs::write(markdown, text)?;
    Ok(())
}
fn main() -> Result {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("audit") => audit(&args[2..]),
        Some("overlap") => overlap(&args[2..]),
        _ => Err("usage: deriv_history <audit|overlap> [options]".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_units_are_exact() {
        assert_eq!(epoch(1_700_000_000), ("seconds", 1_700_000_000, true));
        assert_eq!(
            epoch(1_700_000_000_000),
            ("milliseconds", 1_700_000_000, true)
        );
        assert!(!epoch(1_700_000_000_001).2);
    }
    #[test]
    fn utc_is_stable() {
        assert_eq!(utc(0), "1970-01-01 00:00:00");
    }
}
