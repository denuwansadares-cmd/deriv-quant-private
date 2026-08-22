static AUTHORITATIVE_RISK: std::sync::OnceLock<Arc<RwLock<RiskManager>>> = std::sync::OnceLock::new();
const STOP_STATE_NAME: &str = "stop-state.json";
const MANUAL_STOP_NAME: &str = "emergency.stop";
const HEARTBEAT_NAME: &str = "runtime-heartbeat";
const LOCK_NAME: &str = "runtime.lock";
const EXPOSURE_STATE_NAME: &str = "open-exposure.json";

#[derive(Debug, Serialize, Deserialize)]
struct PersistedExposureState {
    review_locked: bool,
    updated_at_ms: i64,
    contracts: BTreeMap<String, i64>,
}

fn risk_state_dir() -> PathBuf {
    env::var("DERIV_RISK_STATE_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".risk"))
}
fn risk_path(name: &str) -> PathBuf { risk_state_dir().join(name) }

#[cfg(unix)]
fn secure_options() -> fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
}
#[cfg(not(unix))]
fn secure_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    options
}

fn atomic_secure_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(directory)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let nonce = OsRng.next_u64();
    let temporary = directory.join(format!(".{}.tmp-{}-{nonce}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    let result = (|| {
        let mut file = secure_options().open(&temporary)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeIdentity {
    pid: u32,
    process_start_ticks: u64,
    boot_id: String,
}

fn process_start_ticks(pid: u32) -> std::io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let tail = stat.rsplit_once(')').map(|(_, tail)| tail.trim()).ok_or_else(|| std::io::Error::other("invalid proc stat"))?;
    tail.split_whitespace().nth(19).ok_or_else(|| std::io::Error::other("missing proc start time"))?.parse().map_err(|_| std::io::Error::other("invalid proc start time"))
}
fn current_runtime_identity() -> std::io::Result<RuntimeIdentity> {
    let pid = std::process::id();
    Ok(RuntimeIdentity {
        pid,
        process_start_ticks: process_start_ticks(pid)?,
        boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim().to_string(),
    })
}
fn runtime_identity_is_active(identity: &RuntimeIdentity) -> bool {
    fs::read_to_string("/proc/sys/kernel/random/boot_id").ok().is_some_and(|value| value.trim() == identity.boot_id)
        && process_start_ticks(identity.pid).ok() == Some(identity.process_start_ticks)
}

struct RuntimeLockGuard {
    path: PathBuf,
    identity: RuntimeIdentity,
    _file: fs::File,
}
impl Drop for RuntimeLockGuard {
    fn drop(&mut self) {
        let owned = fs::read_to_string(&self.path).ok()
            .and_then(|value| serde_json::from_str::<RuntimeIdentity>(&value).ok())
            .is_some_and(|identity| identity == self.identity);
        if owned { let _ = fs::remove_file(&self.path); }
    }
}

fn acquire_runtime_lock() -> Result<RuntimeLockGuard> {
    let path = risk_path(LOCK_NAME);
    fs::create_dir_all(risk_state_dir())?;
    let identity = current_runtime_identity()?;
    let open = || {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)
    };
    let mut file = match open() {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let active = fs::read_to_string(&path).ok()
                .and_then(|value| serde_json::from_str::<RuntimeIdentity>(&value).ok())
                .is_some_and(|existing| runtime_identity_is_active(&existing));
            anyhow::ensure!(!active, "another trading runtime owns {}", path.display());
            fs::remove_file(&path)?;
            open()?
        }
        Err(error) => return Err(error.into()),
    };
    serde_json::to_writer(&mut file, &identity)?;
    file.sync_all()?;
    Ok(RuntimeLockGuard { path, identity, _file: file })
}

fn init_risk_state(risk: Arc<RwLock<RiskManager>>) -> Result<RuntimeLockGuard> {
    fs::create_dir_all(risk_state_dir())?;
    let runtime_lock = acquire_runtime_lock()?;
    AUTHORITATIVE_RISK.set(risk.clone()).map_err(|_| anyhow::anyhow!("authoritative risk state already initialized"))?;
    let stop_path = risk_path(STOP_STATE_NAME);
    let manual = risk_path(MANUAL_STOP_NAME).exists();
    let mut stop_reason = None;
    let exposure_path = risk_path(EXPOSURE_STATE_NAME);
    if exposure_path.exists() {
        match fs::read_to_string(&exposure_path).ok()
            .and_then(|value| serde_json::from_str::<PersistedExposureState>(&value).ok())
        {
            Some(state) if !state.contracts.is_empty() => {
                risk.write().safety_open_contracts.extend(
                    state.contracts.into_iter().map(|(id, minor)| (id, Money(minor))),
                );
                stop_reason = Some("persisted exposure requires portfolio reconciliation".to_string());
            }
            Some(_) => { let _ = fs::remove_file(&exposure_path); }
            None => stop_reason = Some("corrupt exposure state recovered fail-closed".to_string()),
        }
    }
    if stop_path.exists() {
        let valid = fs::read_to_string(&stop_path).ok()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
            .is_some_and(|value| value["stopped"].as_bool() == Some(true));
        stop_reason = Some(if valid { "persistent stop state exists" } else { "corrupt stop state recovered fail-closed" }.to_string());
    }
    if manual { stop_reason = Some("manual emergency stop exists".to_string()); }
    if let Some(reason) = stop_reason {
        risk.write().engage_stop(reason.clone());
        persist_risk_stop(&reason);
    }
    Ok(runtime_lock)
}

fn persist_open_exposure() {
    let Some(risk) = AUTHORITATIVE_RISK.get() else { return };
    let contracts = risk.read().safety_open_contracts.iter()
        .map(|(id, money)| (id.clone(), money.0))
        .collect::<BTreeMap<_, _>>();
    let path = risk_path(EXPOSURE_STATE_NAME);
    if contracts.is_empty() {
        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                mark_persistence_failure("exposure-state removal", &error);
            }
        }
        return;
    }
    let body = serde_json::to_vec(&PersistedExposureState {
        review_locked: true,
        updated_at_ms: Utc::now().timestamp_millis(),
        contracts,
    }).expect("exposure state serialization is infallible");
    if let Err(error) = atomic_secure_write(&path, &body) {
        mark_persistence_failure("exposure-state write", &error);
    }
}

fn risk_v2_open_contract_ids() -> Vec<String> {
    AUTHORITATIVE_RISK.get().map_or_else(Vec::new, |risk| {
        risk.read().safety_open_contracts.keys().cloned().collect()
    })
}

fn risk_manager_cost(contract_id: &str) -> Option<f64> {
    AUTHORITATIVE_RISK.get()
        .and_then(|risk| risk.read().safety_open_contracts.get(contract_id).copied())
        .map(Money::api_amount)
}

fn mark_persistence_failure(context: &str, error: &std::io::Error) {
    if let Some(risk) = AUTHORITATIVE_RISK.get() {
        let mut risk = risk.write();
        risk.persistence_healthy = false;
        risk.engage_stop(format!("risk persistence failure: {context}"));
    }
    log::error!("Risk persistence failure during {}: {}", context, error);
}

fn persist_risk_stop(reason: &str) {
    let body = serde_json::json!({"stopped":true,"reason":reason,"at_ms":Utc::now().timestamp_millis()}).to_string();
    if let Err(error) = atomic_secure_write(&risk_path(STOP_STATE_NAME), body.as_bytes()) {
        mark_persistence_failure("stop-state write", &error);
    }
    journal_event("risk_stop", serde_json::json!({"reason":reason}));
}

fn risk_v2_stop(reason: impl Into<String>) {
    let reason = reason.into();
    if let Some(risk) = AUTHORITATIVE_RISK.get() { risk.write().engage_stop(reason.clone()); }
    persist_risk_stop(&reason);
}

fn risk_v2_observe_tick() {
    if let Some(risk) = AUTHORITATIVE_RISK.get() {
        let mut risk = risk.write();
        risk.safety_last_tick_ms = Utc::now().timestamp_millis();
        risk.safety_api_events = risk.safety_api_events.saturating_add(1);
    }
}
fn risk_v2_observe_api_error() {
    let should_stop = if let Some(risk) = AUTHORITATIVE_RISK.get() {
        let mut risk = risk.write();
        risk.safety_api_events = risk.safety_api_events.saturating_add(1);
        risk.safety_api_errors = risk.safety_api_errors.saturating_add(1);
        risk.safety_api_events >= 20 && risk.safety_api_errors as f64 / risk.safety_api_events as f64 >= 0.20
    } else { true };
    if should_stop { risk_v2_stop("API error-rate kill switch"); }
}
fn risk_v2_confirm_proposal(id: &str, price: f64) {
    if let (Some(risk), Some(price)) = (AUTHORITATIVE_RISK.get(), Money::from_api(price)) {
        risk.write().safety_confirmed_proposals.insert(id.to_string(), (price, Utc::now().timestamp_millis()));
    }
}
fn risk_v2_pre_trade(command: &ApiCommand) -> std::result::Result<(), String> {
    let Some(shared) = AUTHORITATIVE_RISK.get() else { return Err("authoritative risk state unavailable".into()) };
    if risk_path(MANUAL_STOP_NAME).exists() { risk_v2_stop("manual emergency stop file detected"); }
    let mut risk = shared.write();
    let stopped = risk.is_stopped();
    match command {
        ApiCommand::SellContract { contract_id } => {
            if contract_id.trim().is_empty() || !risk.safety_open_contracts.contains_key(contract_id) {
                return Err("sell rejected: contract is not an existing tracked exposure".into());
            }
            return Ok(());
        }
        ApiCommand::SubscribeContract { contract_id } => {
            if contract_id.trim().is_empty() || !risk.safety_open_contracts.contains_key(contract_id) {
                return Err("subscription rejected: unknown contract".into());
            }
            return Ok(());
        }
        ApiCommand::RequestPortfolio => return Ok(()),
        _ if stopped => return Err(risk.stop_reason.clone().unwrap_or_else(|| "stopped".into())),
        _ => {}
    }
    let now = Utc::now().timestamp_millis();
    if now - risk.safety_last_tick_ms > WS_STALE_TIMEOUT_SECS as i64 * 1000 {
        drop(risk);
        risk_v2_stop("stale-data kill switch");
        return Err("stale market data".into());
    }
    match command {
        ApiCommand::RequestProposal { amount, .. }
        | ApiCommand::RequestOverUnder { amount, .. }
        | ApiCommand::RequestMatchesDiff { amount, .. }
        | ApiCommand::RequestRiseFall { amount, .. } => {
            let stake = Money::from_api(*amount).ok_or_else(|| "invalid stake".to_string())?;
            let market_budget = (risk.verified_balance.0 * risk.runtime_config.max_open_exposure_bps / 10_000).max(MIN_STAKE_MINOR);
            let expected_loss_limit = market_budget;
            let family_budget = market_budget;
            let maximum_exposure = market_budget;
            if stake.0 > risk.runtime_config.max_stake_minor { return Err("hard stake ceiling".into()); }
            if stake.0 > expected_loss_limit { return Err("expected-loss ceiling".into()); }
            if stake.0 > market_budget || stake.0 > family_budget { return Err("market/family budget".into()); }
            let open_exposure = risk.safety_open_contracts.values().map(|money| money.0).sum::<i64>();
            if open_exposure.saturating_add(stake.0) > maximum_exposure { return Err("maximum open exposure".into()); }
            if now < risk.safety_loss_throttle_until_ms { return Err("consecutive-loss cooldown".into()); }
        }
        ApiCommand::BuyContract { proposal_id, price } => {
            let final_price = Money::from_api(*price).ok_or_else(|| "invalid final buy price".to_string())?;
            let Some((confirmed_price, confirmed_at)) = risk.safety_confirmed_proposals.remove(proposal_id) else {
                return Err("no confirmed proposal ID = no buy".into());
            };
            if final_price != confirmed_price { return Err("final buy price differs from confirmed proposal".into()); }
            if now.saturating_sub(confirmed_at) > PROPOSAL_MAX_AGE_SECS as i64 * 1000 { return Err("proposal expired before buy".into()); }
            let maximum_exposure = (risk.verified_balance.0 * risk.runtime_config.max_open_exposure_bps / 10_000).max(MIN_STAKE_MINOR);
            let open_exposure = risk.safety_open_contracts.values().map(|money| money.0).sum::<i64>();
            if risk.safety_open_contracts.len() >= risk.runtime_config.max_open_contracts {
                return Err("one-open-contract limit".into());
            }
            if final_price.0 > risk.runtime_config.max_stake_minor || open_exposure.saturating_add(final_price.0) > maximum_exposure {
                return Err("buy-time price/exposure ceiling".into());
            }
        }
        _ => {}
    }
    Ok(())
}
fn risk_v2_contract_opened(contract_id: &str, cost: f64) {
    if let (Some(risk), Some(cost)) = (AUTHORITATIVE_RISK.get(), Money::from_api(cost)) {
        let mut risk = risk.write();
        let duplicate = risk.safety_open_contracts.contains_key(contract_id);
        if !duplicate && risk.safety_open_contracts.len() >= risk.runtime_config.max_open_contracts {
            risk.engage_stop("open-contract cardinality invariant failed");
        }
        risk.safety_open_contracts.insert(contract_id.to_string(), cost);
        drop(risk);
        persist_open_exposure();
    }
}
fn risk_v2_contract_closed(contract_id: &str, pnl: f64) {
    if let Some(risk) = AUTHORITATIVE_RISK.get() {
        let mut risk = risk.write();
        risk.safety_open_contracts.remove(contract_id);
        if pnl < 0.0 {
            let exponent = risk.consecutive_losses.saturating_sub(1).min(5);
            let cooldown_secs = risk
                .runtime_config
                .loss_cooldown_base_secs
                .saturating_mul(1u64 << exponent)
                .min(3_600);
            risk.safety_loss_throttle_until_ms = Utc::now().timestamp_millis()
                + i64::try_from(cooldown_secs).unwrap_or(3_600) * 1_000;
        }
        drop(risk);
        persist_open_exposure();
    }
}
fn unknown_settlement_details(contract_id: &str) -> serde_json::Value {
    serde_json::json!({
        "normalized_contract_id": normalized_id(contract_id),
        "trade_pnl_minor": serde_json::Value::Null,
        "reason": "authoritative portfolio no longer contains tracked contract",
    })
}
fn risk_v2_contract_closed_unknown(contract_id: &str) {
    if let Some(risk) = AUTHORITATIVE_RISK.get() {
        risk.write().safety_open_contracts.remove(contract_id);
        persist_open_exposure();
    }
    journal_event("settlement_pnl_unknown", unknown_settlement_details(contract_id));
}
fn risk_v2_post_trade_invariants() {
    if let Some(risk) = AUTHORITATIVE_RISK.get() {
        let risk = risk.read();
        let exposure = risk.safety_open_contracts.values().map(|money| money.0).sum::<i64>();
        let maximum = (risk.verified_balance.0 * risk.runtime_config.max_open_exposure_bps / 10_000).max(MIN_STAKE_MINOR);
        if exposure < 0 || exposure > maximum { drop(risk); risk_v2_stop("post-trade exposure invariant failed"); }
    }
}
fn write_runtime_heartbeat() {
    if let Err(error) = atomic_secure_write(&risk_path(HEARTBEAT_NAME), Utc::now().timestamp_millis().to_string().as_bytes()) {
        mark_persistence_failure("heartbeat write", &error);
    }
}

#[cfg(test)]
mod authoritative_risk_tests {
    use super::*;

    #[test]
    fn stop_rejects_entry_but_allows_only_tracked_reduce_only_sell() {
        let directory = std::env::temp_dir().join(format!(
            "deriv-risk-authoritative-{}-{}",
            std::process::id(),
            OsRng.next_u64()
        ));
        fs::create_dir_all(&directory).unwrap();
        std::env::set_var("DERIV_RISK_STATE_DIR", &directory);
        let risk = Arc::new(RwLock::new(RiskManager::with_mode(
            Money(100_000),
            TradingMode::Demo,
        )));
        assert!(AUTHORITATIVE_RISK.set(risk).is_ok());

        let proposal = ApiCommand::RequestProposal {
            underlying_symbol: "R_100".into(),
            currency: "USD".into(),
            amount: 1.0,
            growth_rate: 0.01,
        };
        assert!(risk_v2_pre_trade(&proposal).is_ok());
        risk_v2_confirm_proposal("proposal-1", 1.0);
        assert!(risk_v2_pre_trade(&ApiCommand::BuyContract {
            proposal_id: "proposal-1".into(),
            price: 1.0,
        })
        .is_ok());
        risk_v2_contract_opened("contract-1", 1.0);
        risk_v2_stop("test emergency stop");

        assert!(risk_v2_pre_trade(&proposal).is_err());
        assert!(risk_v2_pre_trade(&ApiCommand::BuyContract {
            proposal_id: "proposal-2".into(),
            price: 1.0,
        })
        .is_err());
        assert!(risk_v2_pre_trade(&ApiCommand::SellContract {
            contract_id: "contract-1".into(),
        })
        .is_ok());
        assert!(risk_v2_pre_trade(&ApiCommand::SellContract {
            contract_id: "unknown-contract".into(),
        })
        .is_err());
        assert!(directory.join(STOP_STATE_NAME).is_file());
        fs::remove_dir_all(directory).unwrap();
    }
}
