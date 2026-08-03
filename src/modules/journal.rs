use std::fs::OpenOptions;
use std::io::{BufWriter, Read, Write};
use std::sync::{Mutex, OnceLock};

thread_local! {
    static JOURNAL_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static DECISION_JOURNAL: OnceLock<DecisionJournal> = OnceLock::new();

struct DecisionJournal {
    writer: Mutex<BufWriter<fs::File>>,
    code_version: String,
    config_hash: String,
    dataset_id: String,
    random_seed: u64,
}

fn stable_hash_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn normalized_id(value: &str) -> String {
    stable_hash_bytes(value.as_bytes())
}

fn dataset_checksum(path: &str) -> Result<String> {
    let path = expand_tilde(path);
    if !path.exists() {
        return Ok("missing".to_string());
    }
    let mut files = Vec::new();
    if path.is_file() {
        files.push(path);
    } else {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                files.push(entry.path());
            }
        }
        files.sort();
    }
    let mut combined = Vec::new();
    for file in files {
        combined.extend_from_slice(file.to_string_lossy().as_bytes());
        let mut reader = fs::File::open(file)?;
        let mut buffer = [0u8; 8192];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 { break; }
            combined.extend_from_slice(&buffer[..count]);
        }
    }
    Ok(stable_hash_bytes(&combined))
}

fn audit_journal_dir() -> std::path::PathBuf {
    env::var("DERIV_AUDIT_JOURNAL_DIR").ok().filter(|value| !value.trim().is_empty())
        .map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("data/audit"))
}

fn init_decision_journal(
    mode: &str,
    symbols: &[String],
    history_path: &str,
    seed: u64,
) -> Result<()> {
    let directory = audit_journal_dir();
    fs::create_dir_all(&directory)?;
    let path = directory.join("decision-journal.jsonl");
    let file = {
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; OpenOptions::new().create(true).append(true).mode(0o600).open(path)? }
        #[cfg(not(unix))] { OpenOptions::new().create(true).append(true).open(path)? }
    };
    let config = format!(
        "mode={mode};symbols={:?};risk_bps={BALANCE_RISK_BPS};max={HARD_MAX_STAKE_MINOR};random={BASELINE_RANDOM_MODE};digits={ENABLE_RANDOM_CONTRACT_FAMILIES};rise_fall={ENABLE_RISE_FALL};qualification_hash={}",
        symbols,
        dataset_checksum(
            &env::var("STRATEGY_QUALIFICATIONS_PATH")
                .unwrap_or_else(|_| "config/strategy-qualifications.json".to_string())
        )?
    );
    let journal = DecisionJournal {
        writer: Mutex::new(BufWriter::new(file)),
        code_version: option_env!("GIT_COMMIT")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_string(),
        config_hash: stable_hash_bytes(config.as_bytes()),
        dataset_id: dataset_checksum(history_path)?,
        random_seed: seed,
    };
    DECISION_JOURNAL
        .set(journal)
        .map_err(|_| anyhow::anyhow!("decision journal already initialized"))?;
    journal_event("run_started", serde_json::json!({"mode": mode}));
    Ok(())
}

fn journal_event(kind: &str, details: serde_json::Value) {
    if JOURNAL_SUPPRESSED.with(std::cell::Cell::get) {
        return;
    }
    let Some(journal) = DECISION_JOURNAL.get() else { return; };
    let record = serde_json::json!({
        "schema": 1,
        "recorded_at_ms": journal_timestamp_ms(),
        "kind": kind,
        "code_version": journal.code_version,
        "config_hash": journal.config_hash,
        "strategy_version": "contract-family-v1",
        "dataset_id": journal.dataset_id,
        "random_seed": journal.random_seed,
        "details": details,
    });
    if let Ok(mut writer) = journal.writer.lock() {
        if serde_json::to_writer(&mut *writer, &record).is_ok() {
            let _ = writer.write_all(b"\n");
            let _ = writer.flush();
            let _ = writer.get_ref().sync_data();
        }
    }
}

fn journal_flush() -> Result<()> {
    if let Some(journal) = DECISION_JOURNAL.get() {
        let mut writer = journal.writer.lock().map_err(|_| anyhow::anyhow!("decision journal lock poisoned"))?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
    }
    Ok(())
}

fn suppress_journal_for_current_thread(suppressed: bool) {
    JOURNAL_SUPPRESSED.with(|value| value.set(suppressed));
}

fn journal_state_transition(from: TradeState, to: TradeState, accepted: bool) {
    journal_event("state_transition", serde_json::json!({
        "from": format!("{from:?}"),
        "to": format!("{to:?}"),
        "accepted": accepted,
    }));
}

#[cfg(test)]
mod journal_tests {
    use super::*;

    #[test]
    fn normalized_identifiers_are_stable_and_do_not_expose_source_ids() {
        let first = normalized_id("contract-secret-123");
        assert_eq!(first, normalized_id("contract-secret-123"));
        assert_ne!(first, normalized_id("contract-secret-124"));
        assert!(!first.contains("contract-secret"));
    }
}
