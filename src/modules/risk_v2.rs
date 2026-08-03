static RISK_V2: std::sync::OnceLock<parking_lot::Mutex<RiskEngineV2>> = std::sync::OnceLock::new();
static RUNTIME_LOCK: std::sync::OnceLock<fs::File> = std::sync::OnceLock::new();
const STOP_STATE_NAME: &str = "stop-state.json";
const MANUAL_STOP_NAME: &str = "emergency.stop";
const HEARTBEAT_NAME: &str = "runtime-heartbeat";
const LOCK_NAME: &str = "runtime.lock";

fn risk_state_dir() -> std::path::PathBuf {
    env::var("DERIV_RISK_STATE_DIR").ok().filter(|v| !v.trim().is_empty())
        .map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(".risk"))
}
fn risk_path(name: &str) -> std::path::PathBuf { risk_state_dir().join(name) }

#[cfg(unix)]
fn secure_options() -> fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    options
}
#[cfg(not(unix))]
fn secure_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    options
}

fn atomic_secure_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(directory)?;
    let temp = directory.join(format!(".{}.tmp-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    let mut file = secure_options().open(&temp)?;
    use std::io::Write;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

fn acquire_runtime_lock() -> Result<()> {
    let path = risk_path(LOCK_NAME);
    fs::create_dir_all(risk_state_dir())?;
    let open = || {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)
    };
    let mut file = match open() {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read_to_string(&path).unwrap_or_default();
            let active = existing.trim().parse::<u32>().ok()
                .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists());
            anyhow::ensure!(!active, "another trading runtime owns {}", path.display());
            fs::remove_file(&path)?;
            open()?
        }
        Err(error) => return Err(error.into()),
    };
    use std::io::Write;
    write!(file, "{}", std::process::id())?;
    file.sync_all()?;
    RUNTIME_LOCK.set(file).map_err(|_| anyhow::anyhow!("runtime lock already held"))
}

struct RiskEngineV2 {
    balance_minor: i64,
    max_stake_minor: i64,
    expected_loss_limit_minor: i64,
    market_budget_minor: i64,
    family_budget_minor: i64,
    maximum_open_exposure_minor: i64,
    open_exposure_minor: i64,
    session_pnl_minor: i64,
    consecutive_losses: u32,
    loss_throttle_until_ms: i64,
    last_tick_ms: i64,
    api_events: u64,
    api_errors: u64,
    confirmed_proposals: std::collections::HashSet<String>,
    stopped: bool,
    stop_reason: Option<String>,
}

fn init_risk_v2(balance: Money) -> Result<()> {
    fs::create_dir_all(risk_state_dir())?;
    acquire_runtime_lock()?;
    let stop_path = risk_path(STOP_STATE_NAME);
    let manual = risk_path(MANUAL_STOP_NAME).exists();
    let mut persisted = stop_path.exists();
    let mut reason = persisted.then(|| "persistent stop state exists".to_string());
    if persisted {
        let valid = fs::read_to_string(&stop_path).ok()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
            .is_some_and(|value| value["stopped"].as_bool() == Some(true));
        if !valid {
            let corrupt = risk_state_dir().join(format!("stop-state.corrupt-{}.json", Utc::now().timestamp_millis()));
            let _ = fs::rename(&stop_path, corrupt);
            atomic_secure_write(&stop_path, serde_json::json!({"stopped":true,"reason":"corrupt stop state recovered fail-closed","at_ms":Utc::now().timestamp_millis()}).to_string().as_bytes())?;
            reason = Some("corrupt stop state recovered fail-closed".to_string());
            persisted = true;
        }
    }
    if manual { persisted = true; reason = Some("manual emergency stop exists".to_string()); }
    let engine = RiskEngineV2 {
        balance_minor: balance.0,
        max_stake_minor: HARD_MAX_STAKE_MINOR,
        expected_loss_limit_minor: (balance.0 / 100).max(MIN_STAKE_MINOR),
        market_budget_minor: (balance.0 * 2 / 100).max(MIN_STAKE_MINOR),
        family_budget_minor: (balance.0 * 5 / 100).max(MIN_STAKE_MINOR),
        maximum_open_exposure_minor: (balance.0 * 2 / 100).max(MIN_STAKE_MINOR),
        open_exposure_minor: 0, session_pnl_minor: 0, consecutive_losses: 0,
        loss_throttle_until_ms: 0, last_tick_ms: Utc::now().timestamp_millis(),
        api_events: 0, api_errors: 0, confirmed_proposals: std::collections::HashSet::new(),
        stopped: persisted, stop_reason: reason,
    };
    RISK_V2.set(parking_lot::Mutex::new(engine))
        .map_err(|_| anyhow::anyhow!("Risk Engine v2 already initialized"))
}

fn persist_risk_stop(reason: &str) {
    let body = serde_json::json!({"stopped":true,"reason":reason,"at_ms":Utc::now().timestamp_millis()}).to_string();
    let _ = atomic_secure_write(&risk_path(STOP_STATE_NAME), body.as_bytes());
    journal_event("risk_v2_stop", serde_json::json!({"reason":reason}));
}
fn risk_v2_stop(reason: impl Into<String>) {
    let reason = reason.into();
    if let Some(engine) = RISK_V2.get() { let mut engine=engine.lock(); engine.stopped=true; engine.stop_reason=Some(reason.clone()); }
    persist_risk_stop(&reason);
}
fn risk_v2_observe_tick() {
    if let Some(engine)=RISK_V2.get(){let mut engine=engine.lock();engine.last_tick_ms=Utc::now().timestamp_millis();engine.api_events+=1;}
}
fn risk_v2_observe_api_error() {
    if let Some(engine)=RISK_V2.get(){let mut engine=engine.lock();engine.api_events+=1;engine.api_errors+=1;
        if engine.api_events>=20 && engine.api_errors as f64/engine.api_events as f64>=0.20{drop(engine);risk_v2_stop("API error-rate kill switch");}}
}
fn risk_v2_confirm_proposal(id:&str){if let Some(engine)=RISK_V2.get(){engine.lock().confirmed_proposals.insert(id.to_string());}}
fn risk_v2_pre_trade(command:&ApiCommand)->std::result::Result<(),String>{
    let Some(engine)=RISK_V2.get() else{return Ok(())};let mut engine=engine.lock();
    if risk_path(MANUAL_STOP_NAME).exists(){drop(engine);risk_v2_stop("manual emergency stop file detected");return Err("manual emergency stop".into())}
    if engine.stopped{return Err(engine.stop_reason.clone().unwrap_or_else(||"stopped".into()))}
    if Utc::now().timestamp_millis()-engine.last_tick_ms>WS_STALE_TIMEOUT_SECS as i64*1000{drop(engine);risk_v2_stop("stale-data kill switch");return Err("stale market data".into())}
    match command {
        ApiCommand::RequestProposal{amount,..}|ApiCommand::RequestOverUnder{amount,..}|ApiCommand::RequestMatchesDiff{amount,..}|ApiCommand::RequestRiseFall{amount,..}=>{
            let stake=Money::from_api(*amount).ok_or_else(||"invalid stake".to_string())?.0;
            if stake>engine.max_stake_minor{return Err("hard stake ceiling".into())}
            if stake>engine.expected_loss_limit_minor{return Err("expected-loss ceiling".into())}
            if stake>engine.market_budget_minor||stake>engine.family_budget_minor{return Err("market/family budget".into())}
            if engine.open_exposure_minor+stake>engine.maximum_open_exposure_minor{return Err("maximum open exposure".into())}
            if Utc::now().timestamp_millis()<engine.loss_throttle_until_ms{return Err("consecutive-loss cooldown".into())}
        }
        ApiCommand::BuyContract{proposal_id,price}=>{
            if proposal_id.trim().is_empty()||!price.is_finite()||*price<=0.0{return Err("no proposal/no confirmed ID = no buy".into())}
            if !engine.confirmed_proposals.remove(proposal_id){return Err("proposal ID was not confirmed".into())}
        }
        _=>{}
    } Ok(())
}
fn risk_v2_contract_opened(cost:f64){if let Some(engine)=RISK_V2.get(){if let Some(cost)=Money::from_api(cost){engine.lock().open_exposure_minor+=cost.0}}}
fn risk_v2_post_trade(pnl:f64){
    if let Some(engine)=RISK_V2.get(){let mut engine=engine.lock();let pnl_minor=Money::from_api(pnl.abs()).map_or(0,|m|if pnl<0.0{-m.0}else{m.0});
        engine.session_pnl_minor+=pnl_minor;engine.open_exposure_minor=0;engine.consecutive_losses=if pnl_minor<0{engine.consecutive_losses+1}else{0};
        if engine.consecutive_losses>=3{engine.loss_throttle_until_ms=Utc::now().timestamp_millis()+60_000;engine.consecutive_losses=0}
        if engine.session_pnl_minor<=-(engine.balance_minor*SESSION_LOSS_LIMIT_BPS/10_000){drop(engine);risk_v2_stop("daily/session loss limit")}
    }
}
fn risk_v2_post_trade_invariants(){if let Some(engine)=RISK_V2.get(){let engine=engine.lock();if engine.open_exposure_minor<0||engine.open_exposure_minor>engine.maximum_open_exposure_minor{drop(engine);risk_v2_stop("post-trade exposure invariant failed")}}}
fn write_runtime_heartbeat(){let _=atomic_secure_write(&risk_path(HEARTBEAT_NAME),Utc::now().timestamp_millis().to_string().as_bytes());}
