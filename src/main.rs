// ===========================================================================
// DERIV ACCUMULATE AUTO-TRADING BOT v2.0
// New Deriv REST + OTP WebSocket API Flow
// Deterministic ACCU production strategy; see CLAIMS.md
// ===========================================================================
//
// Flow:
//   1. Load .env (DERIV_APP_ID, DERIV_PAT)
//   2. Fetch Options accounts via REST API
//   3. Select demo account (DOTxxxx)
//   4. Request OTP
//   5. Connect to OTP WebSocket
//   6. Send ACCU proposals → buy → monitor → sell
//
// Usage:
//   cargo run -- --mode demo
//   cargo run -- --mode simulation
//   DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY cargo run --features live-trading -- --mode live
// ===========================================================================

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use rand::rngs::OsRng;
use rand::seq::SliceRandom;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, sleep, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[cfg(test)]
#[path = "modules/characterization_tests.rs"]
mod characterization_tests;
mod num_serde;
include!("modules/config.rs");

include!("modules/models.rs");
include!("modules/math.rs");
include!("modules/market.rs");
include!("modules/risk.rs");
include!("modules/strategy.rs");
include!("modules/qualification.rs");
include!("modules/execution.rs");
include!("modules/api.rs");
include!("modules/journal.rs");
include!("modules/deterministic.rs");
include!("modules/backtest.rs");
include!("modules/risk_v2.rs");
include!("modules/interfaces.rs");
include!("modules/schema_contract.rs");
include!("modules/retention.rs");
include!("modules/runtime.rs");
#[cfg(test)]
include!("modules/reliability_tests.rs");

// ═══════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════

async fn join_runtime_tasks(
    tasks: Vec<tokio::task::JoinHandle<()>>,
    timeout_duration: Duration,
) -> Result<()> {
    tokio::time::timeout(timeout_duration, async {
        for task in tasks {
            task.await.context("runtime task join failed")?;
        }
        Result::<()>::Ok(())
    })
    .await
    .context("runtime tasks did not join before shutdown timeout")??;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    load_env();
    let expired_records = enforce_api_record_retention()?;
    if expired_records > 0 {
        log::info!(
            "Expired {} API-derived record(s) after 24 hours",
            expired_records
        );
    }

    println!("{}", BANNER);
    println!("        DERIV ACCUMULATE BOT v2.0 — NEW API FLOW\n");
    println!("        8 Theories + Kalman + Hurst + StopLoss + Pattern + Session\n");
    println!(
        "        SL: {}% | MaxHold: {}t | SessionFilter: skip worst hours",
        (STOP_LOSS_PCT * 100.0),
        MAX_ACCUMULATOR_HOLD_TICKS
    );
    println!(
        "        NORMAL: cs≥{}% cf≥{}% agree≥{}/8",
        (CONSENSUS_THRESHOLD * 100.0) as u32,
        (CONFIDENCE_THRESHOLD * 100.0) as u32,
        MIN_THEORIES_AGREE
    );
    println!(
        "        RELAXED: cs≥{}% cf≥{}% agree≥{}/8",
        (RELAXED_CONSENSUS * 100.0) as u32,
        (RELAXED_CONFIDENCE * 100.0) as u32,
        RELAXED_THEORIES_AGREE
    );
    println!();

    let (mode_str, symbols, history_path) = parse_args();
    let mode_str = mode_str.to_ascii_lowercase();
    let demo_duration = configured_demo_duration(&mode_str)?;
    let runtime_risk_config = RuntimeRiskConfig::load(&mode_str)?;
    let random_seed = std::env::var("DERIV_RANDOM_SEED")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0xD3A1_5001);
    if mode_str == "simulation" {
        activate_virtual_time(0);
    }
    anyhow::ensure!(
        !history_path.trim().is_empty(),
        "HISTORY_PATH must be set explicitly or supplied with --history"
    );
    init_decision_journal(&mode_str, &symbols, &history_path, random_seed)?;
    if !matches!(mode_str.as_str(), "demo" | "live" | "simulation") {
        anyhow::bail!("Unsupported mode; use demo, live, or simulation");
    }
    if mode_str == "live"
        && std::env::var("DERIV_ENABLE_LIVE_TRADING").as_deref() != Ok("I_UNDERSTAND_REAL_MONEY")
    {
        anyhow::bail!("Live mode requires DERIV_ENABLE_LIVE_TRADING=I_UNDERSTAND_REAL_MONEY");
    }
    #[cfg(not(feature = "live-trading"))]
    if mode_str == "live" {
        anyhow::bail!(
            "Live mode is not compiled in; rebuild with --features live-trading after safety review"
        );
    }
    if mode_str == "simulation" {
        return run_deterministic_simulation(&symbols, &history_path, random_seed);
    }
    log::info!(
        "Mode: {} | Symbols: {:?} | History: {}",
        mode_str,
        symbols,
        history_path
    );

    let app_id = get_app_id().context("Missing DERIV_APP_ID — set it in .env")?;
    let access_token = get_access_token().context(
        "Missing DERIV_ACCESS_TOKEN — set a buyer-owned OAuth or PAT access token in .env",
    )?;

    // ── Step 1: Fetch Options Accounts ───────────────────────
    log::info!("Fetching options accounts...");
    let accounts = fetch_options_accounts(&app_id, &access_token).await?;
    log::info!(
        "Found {} options account(s); identifiers and balances redacted",
        accounts.len()
    );

    // ── Step 2: Select demo account ──────────────────────────
    // Filter out disabled accounts, prefer virtual/demo, then DOT prefix
    let active_accounts: Vec<&OptionsAccount> = accounts
        .iter()
        .filter(|a| !a.is_disabled.unwrap_or(false))
        .collect();

    if active_accounts.is_empty() {
        anyhow::bail!("No active options accounts found");
    }

    let is_demo_account = |a: &&OptionsAccount| {
        a.account_id.starts_with("DOT")
            || a.is_virtual.unwrap_or(false)
            || a.account_type
                .as_deref()
                .map(|kind| kind.eq_ignore_ascii_case("demo"))
                .unwrap_or(false)
    };
    let selected_account = active_accounts
        .iter()
        .find(|account| {
            if mode_str == "live" {
                !is_demo_account(account)
            } else {
                is_demo_account(account)
            }
        })
        .copied()
        .with_context(|| format!("No active {} account found", mode_str))?;

    log::info!(
        "Selected {} account (identifier and balance redacted)",
        mode_str
    );
    let account_currency = selected_account
        .currency
        .clone()
        .unwrap_or_else(|| "USD".to_string());
    let verified_balance = selected_account
        .balance
        .and_then(Money::from_api)
        .filter(|balance| balance.0 >= MIN_STAKE_MINOR)
        .with_context(|| {
            format!(
                "Selected {} account has no valid verified balance",
                mode_str
            )
        })?;
    let trading_mode = if mode_str == "live" {
        TradingMode::Live
    } else {
        TradingMode::Demo
    };
    let risk_manager = Arc::new(RwLock::new(RiskManager::with_runtime_config(
        verified_balance,
        trading_mode,
        runtime_risk_config,
    )));
    let _runtime_lock = init_risk_state(risk_manager.clone())?;
    anyhow::ensure!(
        !risk_manager.read().is_stopped(),
        "persistent risk stop is engaged; operator review is required"
    );
    log::info!("Trading currency: {}", account_currency);

    // ── Step 3: Request OTP ──────────────────────────────────
    log::info!("Requesting OTP...");
    let ws_url = request_otp(&app_id, &access_token, &selected_account.account_id).await?;
    let expected_endpoint = if mode_str == "live" {
        "/ws/real"
    } else {
        "/ws/demo"
    };
    if !ws_url.contains(expected_endpoint) {
        anyhow::bail!("OTP URL does not match requested {} mode", mode_str);
    }
    log::info!("OTP obtained — connecting...");

    // ── Step 4: Launch API + Connect ─────────────────────────
    let mut api = ApiHandle::launch(
        ws_url,
        symbols.clone(),
        app_id.clone(),
        access_token.clone(),
        selected_account.account_id.clone(),
    )
    .await?;
    // Subscribe before loading history so the broadcast Connected event cannot
    // race past us while large CSV files are being scanned.
    let mut conn_rx = api.subscribe_events();

    // ── Shared State ─────────────────────────────────────────
    let history_loader = Arc::new(RwLock::new(HistoryLoader::load(
        &history_path,
        &symbols,
        TICK_BUFFER_CAP,
    )?));
    let qualification_gate = Arc::new(QualificationGate::load()?);
    for (index, name) in MODEL_NAMES.iter().enumerate() {
        log::info!(
            "[Qualification] {} enabled={} reason={}",
            name,
            qualification_gate.qualified[index],
            qualification_gate.reasons[index]
        );
    }
    let index_rotator = Arc::new(RwLock::new(IndexRotator::new(symbols.clone())));
    let tick_counter = Arc::new(AtomicU64::new(0));
    let total_latency_us = Arc::new(AtomicU64::new(0));
    let latency_samples = Arc::new(AtomicU64::new(0));
    let (runtime_shutdown, _) = broadcast::channel::<()>(8);
    let mut heartbeat_shutdown = runtime_shutdown.subscribe();
    let heartbeat_task = tokio::spawn(async move {
        let mut heartbeat = interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    write_runtime_heartbeat();
                    if risk_path(MANUAL_STOP_NAME).exists() {
                        risk_v2_stop("manual emergency stop file detected");
                    }
                }
                _ = heartbeat_shutdown.recv() => break,
            }
        }
    });

    // Set initial mode
    {
        let mut rm = risk_manager.write();
        rm.mode = trading_mode;
        rm.resume_mode = trading_mode;
        log::info!("Starting in {} mode", trading_mode);
    }

    // Wait for connection
    {
        loop {
            match conn_rx.recv().await {
                Ok(ApiEvent::Connected) => {
                    log::info!("WebSocket connected!");
                    break;
                }
                Ok(ApiEvent::Error { code, message, .. }) => {
                    log::error!("Connection error [{}]: {}", code, message);
                }
                Err(_) => anyhow::bail!("Connection event channel closed"),
                _ => {}
            }
        }
    }

    // ── Shared pending buy info (Strategy → Executor) ────────
    // Strategy sets (symbol, price) before BuyContract;
    // Executor takes it on ContractOpened to populate open_contracts.
    // symbol, proposed price, epoch of the tick on which the buy was queued.
    let pending_buy_info: Arc<RwLock<Option<PendingBuyInfo>>> = Arc::new(RwLock::new(None));

    // ── Spawn: Strategy Analyzer + Trader ────────────────────
    let hl1 = history_loader.clone();
    let rm1 = risk_manager.clone();
    let tc1 = tick_counter.clone();
    let lat1 = total_latency_us.clone();
    let lats1 = latency_samples.clone();
    let rot1 = index_rotator.clone();
    let cmd1 = api.command_sender();
    let mut evt_rx1 = api.subscribe_events();
    let pbi1 = pending_buy_info.clone();
    let proposal_currency = account_currency.clone();
    let qualifications1 = qualification_gate.clone();
    let digit_contracts_enabled = ENABLE_RANDOM_CONTRACT_FAMILIES;

    let mut strategy_shutdown = runtime_shutdown.subscribe();
    let strategy_task = tokio::spawn(async move {
        log::info!("[Strategy] Started");
        let mut pacer = ExecutionPacer::new();
        let mut state = TradeState::Idle;
        state.transition(TradeState::CollectingTicks);
        let mut buy_reconciliation_pending = false;
        let mut pending_symbol: Option<String> = None;
        let mut pending_since: Option<Instant> = None;
        let mut pending_proposal: Option<(String, f64)> = None;
        let mut pending_contract_type = ContractType::Accumulator;
        let mut pending_entry_tick_epoch = 0u64;
        let mut pending_buy_cost = 0.0f64;
        let mut pending_estimated_win_rate: Option<f64> = None;
        let mut match_predictors: HashMap<String, MatchPredictor> = HashMap::new();
        let mut latest_match_risk: HashMap<String, (MatchRisk, f64)> = HashMap::new();
        let mut active_buy_cost = 0.0f64;
        let mut active_contract_type = ContractType::Accumulator;
        let mut contract_strategy =
            EnabledContractStrategy::new(digit_contracts_enabled, BASELINE_RANDOM_MODE);
        let initial_decision = contract_strategy.initial_decision();
        let mut selected_contract_type = initial_decision.contract_type;
        let initial_type_reason = initial_decision.reason;
        log::info!(
            "[ROTATE] Initial contract type: {} | reason: {}",
            selected_contract_type,
            initial_type_reason
        );
        let mut next_entry_after = Instant::now();
        let mut entry_wait_started_at = Instant::now();
        let mut trades_since_rest = 0u32;
        let mut waiting_for_post_loss_trade = false;
        let mut active_contract: Option<String> = None;
        let mut selected_market = rot1.write().next_index();
        let mut next_market_rank_at = Instant::now();
        log::info!("[Strategy] Initial market: {}", selected_market);
        log::info!(
            "[Strategy] Contract families: ACCU{}",
            if digit_contracts_enabled {
                " + OVER/UNDER + MATCHES/DIFF + RISE/FALL (random, market-adjusted)"
            } else {
                " (digit contracts disabled in live mode)"
            }
        );

        loop {
            let event = tokio::select! {
                _ = strategy_shutdown.recv() => break,
                event = evt_rx1.recv() => event,
            };
            match event {
                Ok(ApiEvent::Connected) => {
                    let mode = rm1.read().current_mode();
                    log::info!("[Strategy] Connected! Mode={}", mode);
                    if let Some(contract_id) = active_contract.clone() {
                        if let Err(error) =
                            cmd1.try_send(ApiCommand::SubscribeContract { contract_id })
                        {
                            log::error!(
                                "[Strategy] Failed to restore contract subscription: {}",
                                error
                            );
                        }
                    }
                    if state == TradeState::BuyPending {
                        if let Err(error) = cmd1.try_send(ApiCommand::RequestPortfolio) {
                            log::error!(
                                "[Strategy] Portfolio reconciliation request failed: {}",
                                error
                            );
                        }
                    }
                }
                Ok(ApiEvent::Tick(tick)) => {
                    risk_v2_observe_tick();
                    let recv_ts = Utc::now();
                    let tick_received_at = Instant::now();
                    tc1.fetch_add(1, Ordering::Relaxed);

                    if tick.epoch > 0 {
                        let tick_ms = (tick.epoch as i64).saturating_mul(1000);
                        let recv_ms = recv_ts.timestamp_millis();
                        let lat_us = ((recv_ms.saturating_sub(tick_ms)) * 1000).max(0) as u64;
                        lat1.fetch_add(lat_us, Ordering::Relaxed);
                        lats1.fetch_add(1, Ordering::Relaxed);
                    }

                    hl1.write().push_tick(&tick);
                    let predictor_ticks: Vec<Tick> = {
                        let history = hl1.read();
                        history
                            .get_buffer(&tick.symbol)
                            .map(|buf| {
                                let all: Vec<Tick> = buf.iter().cloned().collect();
                                all[all.len().saturating_sub(100)..].to_vec()
                            })
                            .unwrap_or_default()
                    };
                    let predictor = match_predictors
                        .entry(tick.symbol.clone())
                        .or_insert_with(MatchPredictor::new);
                    let match_risk = predictor.observe_and_compute(&tick, &predictor_ticks);
                    latest_match_risk
                        .insert(tick.symbol.clone(), (match_risk, predictor.skip_threshold));
                    {
                        // A timed cooldown cannot finish itself while entries are paused,
                        // because no trade result arrives to call `evaluate`.
                        rm1.write().refresh_timed_state();
                    }
                    let mode = rm1.read().current_mode();
                    if mode == TradingMode::Stopped {
                        log::info!("[Strategy] Stopped");
                        break;
                    }
                    if mode == TradingMode::Cooldown {
                        continue;
                    }
                    if !rot1.read().market_available(&selected_market) {
                        selected_market = rot1.write().next_index();
                        if !rot1.read().market_available(&selected_market) {
                            continue;
                        }
                        log::info!("[Strategy] Cooldown reroute to: {}", selected_market);
                    }
                    if state == TradeState::ProposalReady {
                        let symbol_matches =
                            pending_symbol.as_deref() == Some(tick.symbol.as_str());
                        let proposal_is_fresh = pending_since
                            .map(|at| at.elapsed() <= Duration::from_secs(PROPOSAL_MAX_AGE_SECS))
                            .unwrap_or(false);
                        if symbol_matches && proposal_is_fresh {
                            if let Some((proposal_id, ask_price)) = pending_proposal.take() {
                                *pbi1.write() = Some(PendingBuyInfo {
                                    symbol: tick.symbol.clone(),
                                    proposed_price: ask_price,
                                    entry_tick_epoch: tick.epoch,
                                    contract_type: pending_contract_type,
                                });
                                pending_buy_cost = ask_price;
                                if state.transition(TradeState::BuyPending) {
                                    pending_since = Some(Instant::now());
                                    if let Err(error) = cmd1.try_send(ApiCommand::BuyContract {
                                        proposal_id,
                                        price: ask_price,
                                    }) {
                                        log::error!("Next-tick buy delivery failed: {}", error);
                                        *pbi1.write() = None;
                                        pending_symbol = None;
                                        pending_since = None;
                                        state.transition(TradeState::CollectingTicks);
                                    } else {
                                        log::info!(
                                            "[TickBoundary] Buy queued on next {} tick epoch={}",
                                            tick.symbol,
                                            tick.epoch
                                        );
                                    }
                                }
                            }
                            continue;
                        }
                    }
                    if state.execution_pending() {
                        let timeout = match state {
                            TradeState::ProposalPending | TradeState::ProposalReady => {
                                PROPOSAL_TIMEOUT_SECS
                            }
                            TradeState::BuyPending => BUY_TIMEOUT_SECS,
                            _ => CONTRACT_UPDATE_TIMEOUT_SECS,
                        };
                        let expired = pending_since
                            .map(|at| at.elapsed() > Duration::from_secs(timeout))
                            .unwrap_or(false);
                        if expired && state == TradeState::BuyPending && buy_reconciliation_pending
                        {
                            let _ = cmd1.try_send(ApiCommand::RequestPortfolio);
                            pending_since = Some(Instant::now());
                            log::warn!(
                                "[Strategy] Retrying uncertain-buy portfolio reconciliation"
                            );
                            continue;
                        }
                        if expired && state == TradeState::ContractOpen {
                            if let Some(contract_id) = active_contract.clone() {
                                match cmd1.try_send(ApiCommand::SubscribeContract {
                                    contract_id: contract_id.clone(),
                                }) {
                                    Ok(()) => log::warn!(
                                        "[Strategy] Contract update timed out; refreshing redacted subscription"
                                    ),
                                    Err(error) => log::error!(
                                        "[Strategy] Contract refresh delivery failed (identifier redacted): {}",
                                        error
                                    ),
                                }
                            }
                            pending_since = Some(Instant::now());
                            continue;
                        }
                        if expired
                            && matches!(
                                state,
                                TradeState::ProposalPending
                                    | TradeState::ProposalReady
                                    | TradeState::BuyPending
                            )
                        {
                            log::warn!("Pending {:?} timed out; clearing request state", state);
                            *pbi1.write() = None;
                            pending_proposal = None;
                            pending_symbol = None;
                            pending_since = None;
                            state.transition(TradeState::CollectingTicks);
                        } else {
                            continue;
                        }
                    }
                    if !BASELINE_RANDOM_MODE
                        && state == TradeState::CollectingTicks
                        && Instant::now() >= next_market_rank_at
                    {
                        let ranked_choice = {
                            let history = hl1.read();
                            let rotator = rot1.read();
                            let ranked = rotator.ranked_historical_markets(&history, tick.epoch);
                            ranked.first().cloned().map(|best| (best, ranked))
                        };
                        next_market_rank_at =
                            Instant::now() + Duration::from_secs(MARKET_RANK_REFRESH_SECS);
                        let Some((candidate, pool)) = ranked_choice else {
                            log::debug!("[MarketRank] No fresh market has enough history");
                            continue;
                        };
                        if candidate.score < MIN_MARKET_QUALITY_SCORE {
                            log::debug!(
                                "[MarketRank] Abstaining: best {} score={:.3} below quality floor {:.3}",
                                candidate.symbol,
                                candidate.score,
                                MIN_MARKET_QUALITY_SCORE
                            );
                            continue;
                        }
                        let current_score = pool
                            .iter()
                            .find(|market| market.symbol == selected_market)
                            .map(|market| market.score);
                        let should_switch = selected_market != candidate.symbol
                            && current_score.is_none_or(|score| {
                                candidate.score >= score + MARKET_SWITCH_SCORE_MARGIN
                            });
                        if should_switch {
                            let pool_summary = pool
                                .iter()
                                .map(|market| format!("{}:{:.3}", market.symbol, market.score))
                                .collect::<Vec<_>>()
                                .join(", ");
                            log::info!(
                                "[MarketRank] All-{} [{}] -> deterministic best {} score={:.3} chaos={:.3} conf={:.3} n={} rms={:.6}/{:.6} tail={:.1}% spread={:.6}",
                                pool.len(), pool_summary, candidate.symbol, candidate.score, candidate.chaos_edge, candidate.theory_confidence, candidate.observations,
                                candidate.recent_rms, candidate.baseline_rms,
                                candidate.tail_rate * 100.0, candidate.average_spread
                            );
                            selected_market = candidate.symbol;
                        }
                    }
                    if tick.symbol != selected_market {
                        continue;
                    }
                    macro_rules! reject_tick {
                        ($($arg:tt)*) => {{
                            let reason = format!($($arg)*);
                            log::debug!("{}", reason);
                            journal_event("strategy_decision", serde_json::json!({
                                "tick_id": normalized_id(&format!("{}:{}", tick.symbol, tick.epoch)),
                                "decision": "reject",
                                "reason": reason,
                                "latency_us": tick_received_at.elapsed().as_micros(),
                            }));
                            state.transition(TradeState::CollectingTicks);
                            continue;
                        }};
                    }
                    if !state.transition(TradeState::Analysing) {
                        log::warn!(
                            "[Strategy][REJECT] {} cannot start analysis from {:?}",
                            tick.symbol,
                            state
                        );
                        continue;
                    }
                    if Instant::now() < next_entry_after {
                        state.transition(TradeState::CollectingTicks);
                        continue;
                    }
                    if BASELINE_RANDOM_MODE {
                        if pacer.should_skip_signal() {
                            state.transition(TradeState::CollectingTicks);
                            continue;
                        }
                        let sym = tick.symbol.clone();
                        let force_liveness_entry = entry_wait_started_at.elapsed()
                            >= Duration::from_secs(MAX_ENTRY_WAIT_SECS);
                        let market_ticks: Vec<Tick> = {
                            let history = hl1.read();
                            history
                                .get_buffer(&sym)
                                .map(|buffer| buffer.iter().cloned().collect())
                                .unwrap_or_default()
                        };
                        if !force_liveness_entry {
                            if let Some(reason) = random_market_risk_reason(&market_ticks) {
                                let previous_market = selected_market.clone();
                                selected_market = rot1.write().next_index();
                                log::warn!(
                                    "[RandomEntry][RISK] {} skipped: {}; rotating to {}",
                                    previous_market,
                                    reason,
                                    selected_market
                                );
                                state.transition(TradeState::CollectingTicks);
                                continue;
                            }
                        } else {
                            log::warn!(
                                "[EntryLiveness] No entry for {}s; requesting ACCU fallback on {}",
                                MAX_ENTRY_WAIT_SECS,
                                sym
                            );
                            selected_contract_type = ContractType::Accumulator;
                        }
                        let (risk, threshold) =
                            latest_match_risk.get(&sym).copied().unwrap_or_default();
                        let accumulator_stake = rm1.read().stake_for_risk(0.0);
                        pending_contract_type = selected_contract_type;
                        pending_estimated_win_rate = None;
                        let digit_stake = rm1.read().stake_for_risk(0.25);
                        let command = match selected_contract_type {
                            ContractType::Accumulator => ApiCommand::RequestProposal {
                                underlying_symbol: sym.clone(),
                                currency: proposal_currency.clone(),
                                amount: accumulator_stake,
                                growth_rate: ACCU_GROWTH_RATE,
                            },
                            ContractType::OverUnder => {
                                let current = tick.last_digit();
                                let use_over = if risk.score > MATCH_RISK_CAUTION {
                                    current >= 5
                                } else {
                                    OsRng.next_u64().is_multiple_of(2)
                                };
                                let (contract_type, barrier) = if use_over {
                                    ("DIGITOVER".to_string(), 0)
                                } else {
                                    ("DIGITUNDER".to_string(), 9)
                                };
                                let amount = rm1.read().stake_for_risk(risk.score);
                                log::info!(
                                    "[OU] {} | match_risk={:.2} | threshold={:.2} | stake={:.2}",
                                    sym,
                                    risk.score,
                                    threshold,
                                    amount
                                );
                                ApiCommand::RequestOverUnder {
                                    underlying_symbol: sym.clone(),
                                    currency: proposal_currency.clone(),
                                    amount,
                                    contract_type,
                                    barrier,
                                }
                            }
                            ContractType::MatchesDiff => {
                                if risk.score > threshold {
                                    // Keep the minute eligible by falling back to ACCU
                                    // when the randomly selected digit trade is unsafe.
                                    pending_contract_type = ContractType::Accumulator;
                                    pending_estimated_win_rate = None;
                                    log::info!(
                                        "[DIFF] {} | match_risk={:.2} | threshold={:.2} -> ACCU fallback",
                                        sym,
                                        risk.score,
                                        threshold
                                    );
                                    ApiCommand::RequestProposal {
                                        underlying_symbol: sym.clone(),
                                        currency: proposal_currency.clone(),
                                        amount: accumulator_stake,
                                        growth_rate: ACCU_GROWTH_RATE,
                                    }
                                } else {
                                    pending_estimated_win_rate = Some(1.0 - risk.score);
                                    log::info!("[DIFF] {} | match_risk={:.2} | threshold={:.2} | m={:.2} v={:.2} s={:.2} k={:.0} | stake={:.2} -> TRADE", sym, risk.score, threshold, risk.momentum, risk.velocity, risk.spread, risk.kalman_match, digit_stake);
                                    ApiCommand::RequestMatchesDiff {
                                        underlying_symbol: sym.clone(),
                                        currency: proposal_currency.clone(),
                                        amount: digit_stake,
                                        contract_type: "DIGITDIFF".to_string(),
                                        barrier: tick.last_digit(),
                                    }
                                }
                            }
                            ContractType::RiseFall => {
                                let rise_fall_ticks: Vec<Tick> = {
                                    let history = hl1.read();
                                    history
                                        .get_buffer(&sym)
                                        .map(|buffer| buffer.iter().cloned().collect())
                                        .unwrap_or_default()
                                };
                                if let Some(signal) = validated_rise_fall_signal(&rise_fall_ticks) {
                                    let contract_type = if signal.bullish { "CALL" } else { "PUT" };
                                    let rise_fall_stake = rm1.read().stake_for_risk(0.50);
                                    pending_estimated_win_rate = Some(signal.historical_win_rate);
                                    log::info!(
                                        "[RISE/FALL] {} | direction={} | cs={:.3} cf={:.3} agree={}/8 H={:.3} | walk-forward={}/{} ({:.2}%, lower={:.2}%) | stake={:.2}",
                                        sym, contract_type, signal.consensus, signal.confidence,
                                        signal.agreement, signal.hurst, signal.historical_wins,
                                        signal.historical_trades, signal.historical_win_rate * 100.0,
                                        signal.wilson_lower_rate * 100.0, rise_fall_stake
                                    );
                                    ApiCommand::RequestRiseFall {
                                        underlying_symbol: sym.clone(),
                                        currency: proposal_currency.clone(),
                                        amount: rise_fall_stake,
                                        contract_type: contract_type.to_string(),
                                        duration_ticks: RISE_FALL_DURATION_TICKS,
                                    }
                                } else {
                                    pending_contract_type = ContractType::Accumulator;
                                    pending_estimated_win_rate = None;
                                    log::info!("[RISE/FALL] {} | historical validation failed -> ACCU fallback", sym);
                                    ApiCommand::RequestProposal {
                                        underlying_symbol: sym.clone(),
                                        currency: proposal_currency.clone(),
                                        amount: accumulator_stake,
                                        growth_rate: ACCU_GROWTH_RATE,
                                    }
                                }
                            }
                        };
                        pending_entry_tick_epoch = tick.epoch;
                        pending_symbol = Some(sym.clone());
                        pending_since = Some(Instant::now());
                        pending_proposal = None;
                        if !state.transition(TradeState::ProposalPending) {
                            pending_symbol = None;
                            pending_since = None;
                            state.transition(TradeState::CollectingTicks);
                            continue;
                        }
                        if let Err(error) = cmd1.try_send(command) {
                            log::error!("Random proposal delivery failed: {}", error);
                            pending_symbol = None;
                            pending_since = None;
                            state.transition(TradeState::CollectingTicks);
                        } else {
                            log::info!(
                                "[RandomEntry] Proposal queued: {} type={} epoch={}",
                                sym,
                                selected_contract_type,
                                tick.epoch
                            );
                        }
                        continue;
                    }
                    let spread = tick.spread_frac();
                    if spread > MAX_SPREAD_FRACTION {
                        reject_tick!(
                            "[Strategy][FILTER] {} spread {:.6} > {:.6}",
                            tick.symbol,
                            spread,
                            MAX_SPREAD_FRACTION
                        );
                    }

                    let sq = session_quality();
                    if !session_allows_entry(sq) {
                        reject_tick!(
                            "[Strategy][FILTER] {} invalid session quality {:.2}",
                            tick.symbol,
                            sq
                        );
                    }

                    let all_ticks: Vec<Tick> = {
                        let hl = hl1.read();
                        if let Some(buf) = hl.get_buffer(&tick.symbol) {
                            if buf.len() < MIN_ANALYSIS_TICKS {
                                reject_tick!(
                                    "[Strategy][WARMUP] {} ticks={}/{} (Hurst/trend not ready)",
                                    tick.symbol,
                                    buf.len(),
                                    MIN_ANALYSIS_TICKS
                                );
                            }
                            buf.iter().cloned().collect()
                        } else {
                            reject_tick!(
                                "[Strategy][WARMUP] {} has no history buffer",
                                tick.symbol
                            );
                        }
                    };

                    if digit_contracts_enabled
                        && selected_contract_type != ContractType::Accumulator
                    {
                        let recent_start = all_ticks.len().saturating_sub(120);
                        let recent = &all_ticks[recent_start..];
                        let (match_risk, match_threshold) = latest_match_risk
                            .get(&tick.symbol)
                            .copied()
                            .unwrap_or_default();
                        pending_estimated_win_rate = None;
                        if random_unit() < ABSTENTION_PROBABILITY {
                            reject_tick!("[FastPath] {} deliberate abstention", tick.symbol);
                        }
                        let consensus = digit_bias(recent);
                        let last_digit = tick.last_digit();
                        let stake = rm1.read().stake_for_risk(match_risk.score);
                        let delay_span = SIGNAL_ENTRY_DELAY_MS_MAX - SIGNAL_ENTRY_DELAY_MS_MIN + 1;
                        let signal_delay_ms =
                            SIGNAL_ENTRY_DELAY_MS_MIN + OsRng.next_u64() % delay_span;
                        sleep(Duration::from_millis(signal_delay_ms)).await;
                        let latency_ms = tick_received_at.elapsed().as_millis() as u64;
                        if latency_ms > MAX_ENTRY_LATENCY_MS {
                            reject_tick!(
                                "[FastPath] {} proposal latency {}ms exceeded",
                                tick.symbol,
                                latency_ms
                            );
                        }
                        let sym = tick.symbol.clone();
                        let command = match selected_contract_type {
                            ContractType::OverUnder => {
                                let (api_type, barrier, tag) = if consensus > 0.55 {
                                    (
                                        "DIGITOVER".to_string(),
                                        (OsRng.next_u64() % 2) as u8,
                                        "OVER",
                                    )
                                } else if consensus < 0.45 {
                                    (
                                        "DIGITUNDER".to_string(),
                                        8 + (OsRng.next_u64() % 2) as u8,
                                        "UNDER",
                                    )
                                } else {
                                    reject_tick!("[OU] {} neutral consensus {:.3}", sym, consensus);
                                };
                                log::info!("[{}] {} | barrier={} | last_digit={} | entry_lat={}ms | signal_delay={}ms | stake=${:.2}", tag, sym, barrier, last_digit, latency_ms, signal_delay_ms, stake);
                                ApiCommand::RequestOverUnder {
                                    underlying_symbol: sym.clone(),
                                    currency: proposal_currency.clone(),
                                    amount: stake,
                                    contract_type: api_type,
                                    barrier,
                                }
                            }
                            ContractType::MatchesDiff => {
                                if match_risk.score > match_threshold {
                                    reject_tick!(
                                        "[DIFF] {} | match_risk={:.2} | threshold={:.2} -> SKIP",
                                        sym,
                                        match_risk.score,
                                        match_threshold
                                    );
                                }
                                let adjusted_stake = if match_risk.score > MATCH_RISK_CAUTION {
                                    stake * 0.5
                                } else {
                                    stake
                                };
                                pending_estimated_win_rate = Some(1.0 - match_risk.score);
                                log::info!("[DIFF] {} | match_risk={:.2} | threshold={:.2} | barrier={} | stake={:.2} -> TRADE", sym, match_risk.score, match_threshold, last_digit, adjusted_stake);
                                ApiCommand::RequestMatchesDiff {
                                    underlying_symbol: sym.clone(),
                                    currency: proposal_currency.clone(),
                                    amount: adjusted_stake,
                                    contract_type: "DIGITDIFF".to_string(),
                                    barrier: last_digit,
                                }
                            }
                            ContractType::Accumulator => unreachable!(),
                            ContractType::RiseFall => unreachable!(),
                        };
                        pending_symbol = Some(sym);
                        pending_since = Some(Instant::now());
                        pending_proposal = None;
                        pending_contract_type = selected_contract_type;
                        pending_entry_tick_epoch = tick.epoch;
                        if !state.transition(TradeState::ProposalPending) {
                            pending_symbol = None;
                            pending_since = None;
                            continue;
                        }
                        if let Err(error) = cmd1.try_send(command) {
                            log::error!("Fast proposal delivery failed: {}", error);
                            pending_symbol = None;
                            pending_since = None;
                            state.transition(TradeState::CollectingTicks);
                        }
                        continue;
                    }

                    let tick_vel = tick_volatility(&all_ticks);
                    if tick_vel > MAX_TICK_VELOCITY {
                        reject_tick!(
                            "[Strategy][FILTER] {} volatility {:.6} > {:.6}",
                            tick.symbol,
                            tick_vel,
                            MAX_TICK_VELOCITY
                        );
                    }
                    if is_reversal_pattern(&all_ticks) {
                        reject_tick!(
                            "[Strategy][FILTER] {} reversal pattern detected",
                            tick.symbol
                        );
                    }

                    let scores = individual_model_scores(&all_ticks);
                    if qualifications1.qualified_count() < MIN_THEORIES_AGREE {
                        reject_tick!(
                            "[Strategy][QUALIFICATION] only {}/{} models qualified",
                            qualifications1.qualified_count(),
                            MIN_THEORIES_AGREE
                        );
                    }
                    let Some(qualified_consensus) = qualifications1.consensus(&scores) else {
                        reject_tick!("[Strategy][QUALIFICATION] no qualified model weight");
                    };
                    let cs = qualified_consensus.score;
                    let conf = qualified_consensus.confidence;
                    let bullish_cnt = qualified_consensus.bullish_count();
                    let bearish_cnt = qualified_consensus.bearish_count();

                    let kalman_price = kalman_predict(&all_ticks);
                    let current_price = tick.mid_price();
                    let kalman_delta = if current_price > 0.0 {
                        (kalman_price - current_price) / current_price
                    } else {
                        0.0
                    };

                    let prices: Vec<f64> = all_ticks.iter().map(|t| t.mid_price()).collect();
                    let hurst = hurst_exponent(&prices);

                    let is_bullish = cs > 0.5;
                    // Count only theories aligned with the weighted direction.
                    let agree_cnt = qualified_consensus.directional_agreement(is_bullish);
                    let opposing_cnt = if is_bullish { bearish_cnt } else { bullish_cnt };
                    let adjust_threshold = |threshold: f64| {
                        if hurst > 0.6 {
                            threshold - 0.02
                        } else if hurst < 0.4 {
                            threshold + 0.02
                        } else {
                            threshold
                        }
                    };
                    let strong_rejection = consensus_rejection_reason(
                        ConsensusPath::Strong,
                        agree_cnt,
                        cs,
                        conf,
                        MIN_THEORIES_AGREE,
                        adjust_threshold(CONSENSUS_THRESHOLD),
                        adjust_threshold(CONFIDENCE_THRESHOLD),
                    );
                    // Directional models only decide whether Rise/Fall is justified.
                    // Their disagreement must not veto a direction-neutral ACCU entry.
                    let consensus_path = if let Some(reason) = strong_rejection {
                        log::debug!(
                            "[Strategy] {} directional path not strong: {}; using ACCU (aligned={} opposing={} bull={} bear={}) scores={:?}",
                            tick.symbol, reason, agree_cnt, opposing_cnt, bullish_cnt, bearish_cnt, scores
                        );
                        ConsensusPath::Relaxed
                    } else {
                        ConsensusPath::Strong
                    };

                    let effective_target = PROFIT_TARGET - ESTIMATED_SLIPPAGE - ESTIMATED_FEES;
                    if effective_target < MIN_EFFECTIVE_PROFIT {
                        reject_tick!(
                            "[Strategy][FILTER] {} effective profit {:.4} < {:.4}",
                            tick.symbol,
                            effective_target,
                            MIN_EFFECTIVE_PROFIT
                        );
                    }
                    if pacer.should_skip_signal() {
                        reject_tick!(
                            "[Strategy][FILTER] {} qualified signal rejected by pacing guard",
                            tick.symbol
                        );
                    }

                    let rise_fall_signal = validated_rise_fall_signal(&all_ticks);
                    let direction_is_exceptional = ENABLE_RISE_FALL
                        && consensus_path == ConsensusPath::Strong
                        && rise_fall_signal.is_some();
                    let stake = rm1.read().stake_for_risk(if direction_is_exceptional {
                        0.50
                    } else {
                        0.0
                    });
                    let mode_tag = if consensus_path == ConsensusPath::Relaxed {
                        "RELAXED"
                    } else {
                        "NORMAL"
                    };

                    log::info!("SIGNAL[{}]: {} | cs={:.4} cf={:.4} | agree={}/8 | kd={:+.4}% | H={:.3} | ${:.2} | {}",
                        mode_tag, tick.symbol, cs, conf, agree_cnt,
                        kalman_delta * 100.0, hurst, stake, pacer.stats());
                    journal_event(
                        "strategy_decision",
                        serde_json::json!({
                            "tick_id": normalized_id(&format!("{}:{}", tick.symbol, tick.epoch)),
                            "model_scores": scores,
                            "consensus": cs,
                            "confidence": conf,
                            "agreement": agree_cnt,
                            "decision": "accept",
                            "reason": mode_tag,
                            "stake": stake,
                            "latency_us": tick_received_at.elapsed().as_micros(),
                        }),
                    );

                    let sym = tick.symbol.clone();
                    pending_contract_type = if direction_is_exceptional {
                        ContractType::RiseFall
                    } else {
                        ContractType::Accumulator
                    };
                    pending_estimated_win_rate =
                        rise_fall_signal.map(|signal| signal.historical_win_rate);
                    pending_entry_tick_epoch = tick.epoch;
                    pending_symbol = Some(sym.clone());
                    pending_since = Some(Instant::now());
                    pending_proposal = None;
                    if !state.transition(TradeState::ProposalPending) {
                        log::error!("[Strategy][REJECT] {} qualified signal could not enter ProposalPending", sym);
                        pending_symbol = None;
                        pending_since = None;
                        state.transition(TradeState::CollectingTicks);
                        continue;
                    }
                    let proposal_command = if direction_is_exceptional {
                        let signal = rise_fall_signal.expect("validated above");
                        let api_type = if signal.bullish { "CALL" } else { "PUT" };
                        log::info!(
                            "[RISE/FALL] {} direction={} cs={:.3} cf={:.3} agree={}/8 H={:.3} walk-forward={}/{} ({:.2}%, lower={:.2}%)",
                            sym, api_type, signal.consensus, signal.confidence,
                            signal.agreement, signal.hurst, signal.historical_wins,
                            signal.historical_trades, signal.historical_win_rate * 100.0,
                            signal.wilson_lower_rate * 100.0
                        );
                        ApiCommand::RequestRiseFall {
                            underlying_symbol: sym.clone(),
                            currency: proposal_currency.clone(),
                            amount: stake,
                            contract_type: api_type.to_string(),
                            duration_ticks: RISE_FALL_DURATION_TICKS,
                        }
                    } else {
                        ApiCommand::RequestProposal {
                            underlying_symbol: sym.clone(),
                            currency: proposal_currency.clone(),
                            amount: stake,
                            growth_rate: ACCU_GROWTH_RATE,
                        }
                    };
                    if let Err(error) = cmd1.try_send(proposal_command) {
                        log::error!("Proposal command delivery failed: {}", error);
                        pending_symbol = None;
                        pending_since = None;
                        state.transition(TradeState::CollectingTicks);
                    } else {
                        log::info!(
                            "[Strategy] Proposal request queued: {} type={} amount={:.2}",
                            sym,
                            pending_contract_type,
                            stake
                        );
                    }
                }
                Ok(ApiEvent::Proposal {
                    id,
                    ask_price,
                    payout,
                    underlying_symbol,
                }) => {
                    risk_v2_confirm_proposal(&id, ask_price);
                    journal_event(
                        "order_result",
                        serde_json::json!({
                            "result_kind": "proposal",
                            "normalized_proposal_id": normalized_id(&id),
                            "normalized_symbol": normalized_id(&underlying_symbol),
                            "ask_price_minor": Money::from_api(ask_price).map(|money| money.0),
                            "payout_minor": Money::from_api(payout).map(|money| money.0),
                        }),
                    );
                    log::info!(
                        "Proposal received: {} @ ${:.4} (identifier redacted)",
                        underlying_symbol,
                        ask_price
                    );
                    let symbol_matches =
                        pending_symbol.as_deref() == Some(underlying_symbol.as_str());
                    let proposal_age = pending_since.map(|at| at.elapsed());
                    let proposal_is_fresh = proposal_age
                        .map(|age| age <= Duration::from_secs(PROPOSAL_MAX_AGE_SECS))
                        .unwrap_or(false);
                    let proposal_mode = rm1.read().current_mode();
                    if state == TradeState::ProposalPending
                        && symbol_matches
                        && proposal_is_fresh
                        && proposal_mode != TradingMode::Stopped
                    {
                        if let Some(estimated) = pending_estimated_win_rate {
                            if let Some(required) =
                                payout_allows_trade(ask_price, payout, estimated)
                            {
                                log::info!(
                                    "PAYOUT CHECK: required={:.1}% vs estimated={:.1}% -> TRADE",
                                    required * 100.0,
                                    estimated * 100.0
                                );
                            } else {
                                let required = if payout > 0.0 {
                                    ask_price / payout
                                } else {
                                    1.0
                                };
                                log::warn!(
                                    "PAYOUT CHECK: required={:.1}% vs estimated={:.1}% -> ABORT",
                                    required * 100.0,
                                    estimated * 100.0
                                );
                                pending_symbol = None;
                                pending_since = None;
                                pending_estimated_win_rate = None;
                                if BASELINE_RANDOM_MODE {
                                    let rejected_type = selected_contract_type;
                                    selected_contract_type = if digit_contracts_enabled {
                                        random_contract_type()
                                    } else {
                                        ContractType::Accumulator
                                    };
                                    next_market_rank_at = Instant::now();
                                    log::info!(
                                        "[ROTATE] {} -> {} | reason: payout_rejected",
                                        rejected_type,
                                        selected_contract_type
                                    );
                                }
                                state.transition(TradeState::CollectingTicks);
                                continue;
                            }
                        }
                        state.transition(TradeState::ProposalReady);
                        {
                            *pbi1.write() = Some(PendingBuyInfo {
                                symbol: underlying_symbol.clone(),
                                proposed_price: ask_price,
                                entry_tick_epoch: pending_entry_tick_epoch,
                                contract_type: pending_contract_type,
                            });
                            pending_buy_cost = ask_price;
                            if state.transition(TradeState::BuyPending) {
                                pending_since = Some(Instant::now());
                                if let Err(error) = cmd1.try_send(ApiCommand::BuyContract {
                                    proposal_id: id,
                                    price: ask_price,
                                }) {
                                    log::error!("Immediate buy delivery failed: {}", error);
                                    *pbi1.write() = None;
                                    pending_symbol = None;
                                    pending_since = None;
                                    state.transition(TradeState::CollectingTicks);
                                } else {
                                    log::info!(
                                        "[FastPath] {} buy queued immediately after proposal",
                                        pending_contract_type
                                    );
                                }
                            }
                        }
                    } else {
                        log::debug!(
                            "[Strategy][FILTER] proposal rejected: state={:?} symbol_match={} age_ms={:?} mode={}",
                            state,
                            symbol_matches,
                            proposal_age.map(|age| age.as_millis()),
                            proposal_mode
                        );
                    }
                }
                Ok(ApiEvent::ContractOpened { contract_id, .. }) => {
                    risk_v2_contract_opened(&contract_id, pending_buy_cost);
                    journal_event(
                        "order_result",
                        serde_json::json!({
                            "result_kind": "contract_opened",
                            "normalized_contract_id": normalized_id(&contract_id),
                        }),
                    );
                    if state != TradeState::BuyPending || active_contract.is_some() {
                        continue;
                    }
                    buy_reconciliation_pending = false;
                    state.transition(TradeState::ContractOpen);
                    active_contract = Some(contract_id.clone());
                    active_contract_type = pending_contract_type;
                    active_buy_cost = pending_buy_cost;
                    pacer.record_trade_opened();
                    pending_since = Some(Instant::now());
                    entry_wait_started_at = Instant::now();
                    log::info!("Contract opened (identifier redacted)");
                }
                Ok(ApiEvent::ContractSold {
                    contract_id,
                    sold_for,
                }) => {
                    journal_event(
                        "order_result",
                        serde_json::json!({
                            "result_kind": "contract_sold",
                            "normalized_contract_id": normalized_id(&contract_id),
                            "sold_for_minor": Money::from_api(sold_for).map(|money| money.0),
                        }),
                    );
                    if active_contract.as_deref() != Some(contract_id.as_str()) {
                        continue;
                    }
                    state.transition(TradeState::Settling);
                    log::info!(
                        "Contract settled (identifier redacted) value={:.4}",
                        sold_for
                    );
                    let was_win = sold_for > active_buy_cost;
                    let settled_result = TradeResult {
                        profit_loss: sold_for - active_buy_cost,
                    };
                    rm1.write()
                        .record_trade_typed(&settled_result, active_contract_type);
                    risk_v2_contract_closed(&contract_id, settled_result.profit_loss);
                    risk_v2_post_trade_invariants();
                    let previous_type = selected_contract_type;
                    let decision = contract_strategy.after_outcome(ContractOutcome {
                        contract_type: active_contract_type,
                        won: was_win,
                    });
                    selected_contract_type = decision.contract_type;
                    let rotate_reason = decision.reason;
                    log::info!(
                        "[ROTATE] {} -> {} | reason: {}",
                        previous_type,
                        selected_contract_type,
                        rotate_reason
                    );
                    // The next qualified tick may start the next proposal. The
                    // state machine still guarantees exactly one open trade.
                    trades_since_rest += 1;
                    let one_dollar_loss = settled_result.profit_loss <= -ONE_DOLLAR_LOSS_THRESHOLD;
                    if one_dollar_loss {
                        waiting_for_post_loss_trade = true;
                        next_entry_after =
                            Instant::now() + Duration::from_secs(ONE_DOLLAR_LOSS_PAUSE_SECS);
                        log::warn!(
                            "[Strategy] ${:.2} loss — waiting {} seconds before one recovery trade",
                            settled_result.profit_loss.abs(),
                            ONE_DOLLAR_LOSS_PAUSE_SECS
                        );
                    } else if waiting_for_post_loss_trade {
                        waiting_for_post_loss_trade = false;
                        next_entry_after =
                            Instant::now() + Duration::from_secs(POST_LOSS_TRADE_PAUSE_SECS);
                        log::info!(
                            "[Strategy] Post-loss trade completed — waiting {} seconds before continuing",
                            POST_LOSS_TRADE_PAUSE_SECS
                        );
                    } else {
                        next_entry_after = Instant::now();
                    }
                    if trades_since_rest >= TRADES_PER_REST {
                        trades_since_rest = 0;
                        next_entry_after = Instant::now() + Duration::from_secs(TRADE_REST_SECS);
                        log::info!(
                            "[Strategy] {} completed trades — resting for {} seconds",
                            TRADES_PER_REST,
                            TRADE_REST_SECS
                        );
                    }
                    entry_wait_started_at = Instant::now();
                    active_contract = None;
                    pending_symbol = None;
                    pending_since = None;
                    pending_proposal = None;
                    next_market_rank_at = Instant::now();
                    if BASELINE_RANDOM_MODE {
                        selected_market = rot1.write().next_index();
                        log::info!("[Strategy] Continuous next market: {}", selected_market);
                    } else {
                        log::info!("[Strategy] Re-ranking all markets for the next minute slot");
                    }
                    state.transition(TradeState::CooldownOrReanalyse);
                    state.transition(TradeState::CollectingTicks);
                }
                Ok(ApiEvent::ContractUpdate {
                    contract_id: _,
                    current_spot,
                    bid_price,
                }) => {
                    log::debug!(
                        "Contract update (identifier redacted): spot={:.6} bid={:.4}",
                        current_spot,
                        bid_price
                    );
                }
                Ok(ApiEvent::Portfolio(contracts)) => {
                    if state == TradeState::BuyPending {
                        buy_reconciliation_pending = false;
                        let candidates = pending_symbol
                            .as_deref()
                            .map(|symbol| {
                                contracts
                                    .iter()
                                    .filter(|contract| {
                                        let price_matches =
                                            (contract.buy_price - pending_buy_cost).abs() <= 0.01;
                                        let time_matches = contract.purchase_time
                                            >= pending_entry_tick_epoch
                                            && contract.purchase_time
                                                <= pending_entry_tick_epoch
                                                    .saturating_add(BUY_TIMEOUT_SECS * 2);
                                        let proposal_matches = pending_proposal
                                            .as_ref()
                                            .is_none_or(|(proposal_id, _)| {
                                                contract
                                                    .proposal_id
                                                    .as_ref()
                                                    .is_none_or(|value| value == proposal_id)
                                            });
                                        let family_matches =
                                            contract.contract_type.as_ref().is_none_or(|value| {
                                                value.eq_ignore_ascii_case(
                                                    &pending_contract_type.to_string(),
                                                )
                                            });
                                        contract.symbol == symbol
                                            && price_matches
                                            && time_matches
                                            && proposal_matches
                                            && family_matches
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        if candidates.len() > 1 {
                            risk_v2_stop("ambiguous portfolio recovery candidates");
                            buy_reconciliation_pending = true;
                            log::error!("[Strategy] Ambiguous portfolio recovery; persistent safe-stop engaged");
                            continue;
                        }
                        let recovered = candidates.into_iter().next();
                        if let Some(contract) = recovered {
                            state.transition(TradeState::ContractOpen);
                            active_contract = Some(contract.contract_id.clone());
                            active_contract_type = pending_contract_type;
                            active_buy_cost = contract.buy_price;
                            pending_since = Some(Instant::now());
                            journal_event(
                                "buy_recovery_correlated",
                                serde_json::json!({
                                    "contract_id": normalized_id(&contract.contract_id),
                                    "transaction_id": contract.transaction_id.as_deref().map(normalized_id),
                                    "proposal_id": contract.proposal_id.as_deref().map(normalized_id),
                                    "family": contract.contract_type,
                                }),
                            );
                            log::warn!("[Strategy] Recovered uncertain buy as contract (identifier redacted)");
                        } else {
                            *pbi1.write() = None;
                            pending_proposal = None;
                            pending_symbol = None;
                            pending_since = None;
                            state.transition(TradeState::CollectingTicks);
                            log::info!("[Strategy] Portfolio confirms uncertain buy did not open");
                        }
                    }
                }
                Ok(ApiEvent::Error {
                    code,
                    message,
                    request_kind,
                }) => {
                    risk_v2_observe_api_error();
                    log::error!(
                        "Deriv Error [{}]: {} (request={:?})",
                        code,
                        message,
                        request_kind
                    );
                    let confirmed_rejection =
                        state != TradeState::BuyPending || request_kind.as_deref() == Some("buy");
                    if confirmed_rejection
                        && matches!(
                            state,
                            TradeState::ProposalPending
                                | TradeState::ProposalReady
                                | TradeState::BuyPending
                        )
                    {
                        *pbi1.write() = None;
                        pending_proposal = None;
                        pending_symbol = None;
                        pending_since = None;
                        if BASELINE_RANDOM_MODE {
                            let rejected_type = selected_contract_type;
                            let decision = contract_strategy.initial_decision();
                            selected_contract_type = decision.contract_type;
                            let reason = decision.reason;
                            next_market_rank_at = Instant::now();
                            log::warn!(
                                "[BestMarketEntry] Rejected {} proposal; re-ranking all markets and retrying with {} ({})",
                                rejected_type, selected_contract_type, reason
                            );
                        }
                        state.transition(TradeState::CollectingTicks);
                    }
                }
                Ok(ApiEvent::Disconnected) => {
                    log::warn!("[Strategy] Disconnected — unsafe pending state cleared");
                    if state == TradeState::BuyPending {
                        buy_reconciliation_pending = true;
                    }
                    if active_contract.is_none() && state != TradeState::BuyPending {
                        *pbi1.write() = None;
                        pending_proposal = None;
                        pending_symbol = None;
                        pending_since = None;
                        if matches!(
                            state,
                            TradeState::ProposalPending
                                | TradeState::ProposalReady
                                | TradeState::BuyPending
                        ) {
                            state.transition(TradeState::CollectingTicks);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    log::info!("[Strategy] Event channel closed");
                    break;
                }

                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("[Strategy] Lagged {} events", n);
                }
            }
        }
        log::info!("[Strategy] Ended — {}", pacer.stats());
    });

    // ── Spawn: Trade Executor ────────────────────────────────
    let rot2 = index_rotator.clone();
    let cmd2 = api.command_sender();
    let mut evt_rx2 = api.subscribe_events();
    let pbi2 = pending_buy_info.clone();

    let mut executor_shutdown = runtime_shutdown.subscribe();
    let executor_task = tokio::spawn(async move {
        log::info!("[Executor] Started");
        let mut open_contracts: HashMap<String, (String, f64, u64, u64, Instant)> = HashMap::new();
        let mut auto_contracts: HashMap<String, (String, f64, ContractType)> = HashMap::new();
        // Timestamped attempts allow rejected or lost sells to retry instead of sticking forever.
        let mut closing_contracts: HashMap<String, Instant> = HashMap::new();
        // contract_id → (symbol, buy_price, entry_tick_epoch, ticks_held, opened_at)
        let mut exit_timer = interval(Duration::from_millis(25));

        loop {
            tokio::select! {
            _ = executor_shutdown.recv() => { break; }
            _ = exit_timer.tick() => {
                for (contract_id, (symbol, _, _, ticks_held, opened_at)) in open_contracts.iter() {
                    let initial_fallback_due = !closing_contracts.contains_key(contract_id)
                        && opened_at.elapsed() >= Duration::from_millis(INITIAL_SELL_FALLBACK_MS);
                    let retry_due = closing_contracts.get(contract_id).is_some_and(|last_attempt| {
                        last_attempt.elapsed() >= Duration::from_millis(SELL_RETRY_MS)
                    });
                    if initial_fallback_due || retry_due {
                        log::info!("FAST EXIT RETRY: {} after {} tick(s) (identifier redacted)", symbol, ticks_held);
                        if cmd2.try_send(ApiCommand::SellContract { contract_id: contract_id.clone() }).is_ok() {
                            closing_contracts.insert(contract_id.clone(), Instant::now());
                        }
                    }
                }
            }
            event = evt_rx2.recv() => match event {
                Ok(ApiEvent::ContractOpened { contract_id, buy_price }) => {
                    // Take pending buy info from Strategy
                    if let Some(info) = pbi2.write().take() {
                        let entry_price = buy_price.unwrap_or(info.proposed_price);
                        if info.contract_type == ContractType::Accumulator {
                            open_contracts.insert(contract_id.clone(), (info.symbol, entry_price, info.entry_tick_epoch, 0u64, Instant::now()));
                            log::info!("[Executor] Tracking ACCU (identifier redacted, entry=${:.4})", entry_price);
                        } else {
                            auto_contracts.insert(contract_id.clone(), (info.symbol, entry_price, info.contract_type));
                            log::info!("[Executor] Awaiting auto-settlement (identifier redacted) type={}", info.contract_type);
                        }
                    } else {
                        log::warn!(
                            "[Executor] ContractOpened without pending buy info (identifier redacted)"
                        );
                    }
                    // Subscribe to contract updates for stop-loss/profit monitoring
                    let _ = cmd2.try_send(ApiCommand::SubscribeContract {
                        contract_id: contract_id.clone(),
                    });
                }
                Ok(ApiEvent::Connected) => {
                    for contract_id in open_contracts.keys() {
                        let _ = cmd2.try_send(ApiCommand::SubscribeContract {
                            contract_id: contract_id.clone(),
                        });
                        closing_contracts.remove(contract_id);
                    }
                    for contract_id in auto_contracts.keys() {
                        let _ = cmd2.try_send(ApiCommand::SubscribeContract { contract_id: contract_id.clone() });
                    }
                    if !open_contracts.is_empty() {
                        log::info!("[Executor] Restored {} open contract(s) after reconnect", open_contracts.len());
                    }
                }
                Ok(ApiEvent::Portfolio(contracts)) => {
                    if let Some(pending_info) = pbi2.read().clone() {
                        let pending_symbol = pending_info.symbol.clone();
                        let entry_tick_epoch = pending_info.entry_tick_epoch;
                        let pending_contract_type = pending_info.contract_type;
                        if let Some(contract) = contracts
                            .iter()
                            .filter(|contract| {
                                contract.symbol == pending_symbol
                                    && contract.purchase_time >= entry_tick_epoch
                            })
                            .max_by_key(|contract| contract.purchase_time)
                        {
                            let _ = pbi2.write().take();
                            if pending_contract_type == ContractType::Accumulator {
                                open_contracts.entry(contract.contract_id.clone()).or_insert_with(|| (contract.symbol.clone(), contract.buy_price, entry_tick_epoch, 0, Instant::now()));
                            } else {
                                auto_contracts.entry(contract.contract_id.clone()).or_insert_with(|| (contract.symbol.clone(), contract.buy_price, pending_contract_type));
                            }
                            let _ = cmd2.try_send(ApiCommand::SubscribeContract {
                                contract_id: contract.contract_id.clone(),
                            });
                            log::warn!("[Executor] Recovered and tracking uncertain contract (identifier redacted)");
                        }
                    }
                }
                Ok(ApiEvent::ContractSold {
                    contract_id,
                    sold_for,
                }) => {
                    closing_contracts.remove(&contract_id);
                    if let Some((sym, buy_cost, _, ticks_held, _)) = open_contracts.remove(&contract_id) {
                        let gross_pnl = sold_for - buy_cost;
                        let net_pnl = gross_pnl;
                        let result = TradeResult { profit_loss: net_pnl };
                        let tag = if result.is_profitable() {
                            "PROFIT"
                        } else {
                            "LOSS"
                        };
                        log::info!(
                            "CLOSED[{}]: {} | gross=${:.4} | net=${:.4} | ticks={} (identifier redacted)",
                            tag,
                            sym,
                            gross_pnl,
                            net_pnl,
                            ticks_held
                        );

                        rot2.write().record_result(&sym, result.is_profitable());
                    } else if let Some((sym, buy_cost, contract_type)) = auto_contracts.remove(&contract_id) {
                        let net_pnl = sold_for - buy_cost;
                        let result = TradeResult { profit_loss: net_pnl };
                        let return_pct = if buy_cost > 0.0 { net_pnl / buy_cost * 100.0 } else { 0.0 };
                        log::info!("[CLOSE] {} | {} | PnL={:+.2} | {:+.0}% (identifier redacted)", contract_type, sym, net_pnl, return_pct);
                        rot2.write().record_result(&sym, result.is_profitable());
                    } else {
                        log::warn!("[Executor] Sold unknown contract (identifier redacted)");
                    }
                }
                Ok(ApiEvent::Tick(tick)) => {
                    for (contract_id, (symbol, _, entry_tick_epoch, ticks_held, _)) in open_contracts.iter_mut() {
                        if symbol == &tick.symbol
                            && tick.epoch > *entry_tick_epoch
                            && !closing_contracts.contains_key(contract_id)
                        {
                            *ticks_held += 1;
                            if *ticks_held >= MAX_ACCUMULATOR_HOLD_TICKS {
                                log::info!(
                                    "MAX-HOLD EXIT: {} after {} ticks (identifier redacted)",
                                    symbol, ticks_held
                                );
                                if cmd2.try_send(ApiCommand::SellContract {
                                    contract_id: contract_id.clone(),
                                }).is_ok() {
                                    closing_contracts.insert(contract_id.clone(), Instant::now());
                                }
                            }
                        }
                    }
                }
                Ok(ApiEvent::ContractUpdate {
                    contract_id,
                    current_spot,
                    bid_price,
                }) => {
                    log::debug!(
                        "[Executor] Contract update (identifier redacted): spot={:.6} bid={:.4}",
                        current_spot, bid_price
                    );
                    if let Some((symbol, buy_cost, _, ticks_held, _)) =
                        open_contracts.get(&contract_id)
                    {
                        if bid_price.is_finite() && bid_price > 0.0 && *buy_cost > 0.0 {
                            let return_pct = (bid_price - *buy_cost) / *buy_cost;
                            let take_profit = return_pct >= PROFIT_TARGET;
                            let stop_loss = return_pct <= -STOP_LOSS_PCT;
                            if (take_profit || stop_loss)
                                && !closing_contracts.contains_key(&contract_id)
                            {
                                let reason = if take_profit { "TAKE-PROFIT" } else { "STOP-LOSS" };
                                log::info!(
                                    "{}: {} return={:+.2}% bid=USD {:.4} ticks={} (identifier redacted)",
                                    reason, symbol, return_pct * 100.0, bid_price,
                                    ticks_held
                                );
                                if cmd2.try_send(ApiCommand::SellContract {
                                    contract_id: contract_id.clone(),
                                }).is_ok() {
                                    closing_contracts.insert(contract_id, Instant::now());
                                }
                            }
                        }
                    }
                }
                Ok(ApiEvent::Disconnected) => {
                    log::warn!("[Executor] Disconnected; retaining {} open contract(s)", open_contracts.len());
                }
                Ok(ApiEvent::Error { code, message, .. }) => {
                    log::error!("[Executor] Deriv Error [{}]: {}", code, message);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    log::info!("[Executor] Channel closed");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("[Executor] Lagged {} events", n);
                }
                _ => {}
            }}
        }
        log::info!("[Executor] Ended");
    });

    // ── Spawn: Risk Monitor ────────────────────
    let rm3 = risk_manager.clone();
    let shutdown3 = api.shutdown_sender();
    let mut evt_rx3 = api.subscribe_events();

    let mut risk_shutdown = runtime_shutdown.subscribe();
    let risk_task = tokio::spawn(async move {
        log::info!("[RiskMonitor] Started");
        let mut ticker = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = risk_shutdown.recv() => { break; }
                _ = ticker.tick() => {
                    let mode = { let mut risk = rm3.write(); risk.evaluate(); risk.current_mode() };
                    if mode == TradingMode::Stopped && rm3.read().safety_open_contracts.is_empty() {
                        log::info!("[RiskMonitor] Stop engaged and exposure drained; stopping WebSocket");
                        let _ = shutdown3.send(());
                        break;
                    }
                }
                Ok(event) = evt_rx3.recv() => {
                    if let ApiEvent::ContractSold { contract_id: _, sold_for } = event {
                        log::debug!("[Risk] Sold contract (identifier redacted) for ${:.4}", sold_for);
                    }
                }
                else => { log::info!("[RiskMonitor] Channel closed"); break; }
            }
        }
        log::info!("[RiskMonitor] Ended");
    });

    // ── Spawn: Status Reporter ───────────────────────────────
    let rm_status = risk_manager.clone();
    let tc_status = tick_counter.clone();
    let lat_status = total_latency_us.clone();
    let lats_status = latency_samples.clone();
    let mut status_shutdown = runtime_shutdown.subscribe();
    let status_task = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = status_shutdown.recv() => break,
                _ = ticker.tick() => {}
            }
            let ticks = tc_status.load(Ordering::Relaxed);
            let rm = rm_status.read();
            let total_lat = lat_status.load(Ordering::Relaxed);
            let samples = lats_status.load(Ordering::Relaxed);
            let avg_lat_ms = if samples > 0 {
                total_lat as f64 / samples as f64 / 1000.0
            } else {
                0.0
            };
            log::info!(
                "STATUS | ticks={} | {} | avg_lat={:.1}ms | {}",
                ticks,
                rm.type_summary(),
                avg_lat_ms,
                rm.summary()
            );
            if rm.is_stopped() {
                break;
            }
        }
    });

    // ── Wait for Ctrl+C or the loss circuit breaker ───────────
    log::info!(
        "Bot RUNNING with a {}-second trade cadence. Press Ctrl+C to stop.",
        TRADE_COOLDOWN_MS / 1000
    );
    let auto_stop = async {
        let mut ticker = interval(Duration::from_millis(100));
        loop {
            ticker.tick().await;
            let risk = risk_manager.read();
            if risk.is_stopped() && risk.safety_open_contracts.is_empty() {
                break;
            }
        }
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result?;
            log::info!("Ctrl+C received. Engaging persistent stop and draining exposure...");
            risk_v2_stop("operator interrupt");
        }
        _ = auto_stop => {
            log::info!("Automatic risk stop reached. Shutting down...");
        }
        _ = wait_for_demo_deadline(demo_duration) => {
            log::info!("Configured demo duration reached; engaging persistent stop...");
            risk_v2_stop("configured demo duration completed");
        }
    }

    if risk_manager.read().is_stopped() {
        let drain_deadline = Instant::now() + Duration::from_secs(CONTRACT_UPDATE_TIMEOUT_SECS);
        while !risk_manager.read().safety_open_contracts.is_empty()
            && Instant::now() < drain_deadline
        {
            sleep(Duration::from_millis(100)).await;
        }
        anyhow::ensure!(
            risk_manager.read().safety_open_contracts.is_empty(),
            "open exposure did not drain before shutdown deadline"
        );
    }
    let _ = runtime_shutdown.send(());
    let _ = api.shutdown_tx.send(());
    join_runtime_tasks(
        vec![
            heartbeat_task,
            strategy_task,
            executor_task,
            risk_task,
            status_task,
        ],
        Duration::from_secs(10),
    )
    .await?;
    tokio::time::timeout(Duration::from_secs(10), api.join())
        .await
        .context("WebSocket task did not join before shutdown timeout")??;
    sleep(Duration::from_millis(100)).await;
    journal_flush()?;

    let rm = risk_manager.read();
    log::info!("FINAL: {}", rm.summary());
    log::info!("Goodbye.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn cooperative_runtime_tasks_are_all_joined() {
        let (shutdown, _) = broadcast::channel::<()>(2);
        let mut first_rx = shutdown.subscribe();
        let mut second_rx = shutdown.subscribe();
        let first = tokio::spawn(async move {
            let _ = first_rx.recv().await;
        });
        let second = tokio::spawn(async move {
            let _ = second_rx.recv().await;
        });
        let _ = shutdown.send(());
        join_runtime_tasks(vec![first, second], Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[test]
    fn digit_helpers_and_stakes_stay_in_valid_ranges() {
        let tick = Tick {
            symbol: "R_10".into(),
            quote: 123.45,
            ask: 123.45,
            bid: 123.45,
            epoch: 1,
            pip_size: 2,
        };
        assert_eq!(tick.last_digit(), 5);
        let digit_ticks = (0..40)
            .map(|i| Tick {
                quote: (1000 + (i % 10)) as f64 / 100.0,
                ..tick.clone()
            })
            .collect::<Vec<_>>();
        assert!((digit_bias(&digit_ticks) - 0.5).abs() < 0.01);
        assert!(least_likely_digit(&digit_ticks) <= 9);
        for _ in 0..100 {
            assert!((0.92..=1.08).contains(&varied_stake(1.0, 0.08)));
        }
    }

    #[test]
    fn match_predictor_scores_and_calibrates_conservatively() {
        let mut predictor = MatchPredictor::new();
        let base = Tick {
            symbol: "R_10".into(),
            quote: 100.11,
            ask: 100.11,
            bid: 100.11,
            epoch: 1,
            pip_size: 2,
        };
        let mut ticks = Vec::new();
        for i in 0..30 {
            let tick = Tick {
                epoch: i + 1,
                ..base.clone()
            };
            ticks.push(tick.clone());
            let risk = predictor.observe_and_compute(&tick, &ticks);
            assert!((0.0..=1.0).contains(&risk.score));
        }
        assert!(predictor.skip_threshold < MATCH_RISK_SKIP_INITIAL);
    }

    #[test]
    fn payout_gate_uses_price_over_total_payout() {
        let required = payout_allows_trade(1.0, 1.10, 0.92).unwrap();
        assert!((required - (1.0 / 1.10)).abs() < 1e-9);
        assert!(payout_allows_trade(1.0, 1.10, 0.90).is_none());
        assert!(payout_allows_trade(1.0, 0.0, 0.99).is_none());
    }

    #[test]
    fn contract_type_rotator_penalizes_losses_and_returns_valid_types() {
        let mut rotator = ContractTypeRotator::new();
        let before = rotator.weights();
        rotator.record_result(ContractType::MatchesDiff, false);
        let after = rotator.weights();
        assert!(after[2] < before[2]);
        for _ in 0..100 {
            assert!(matches!(
                rotator.next_type().0,
                ContractType::Accumulator | ContractType::OverUnder | ContractType::MatchesDiff
            ));
        }
    }

    #[test]
    fn parses_numeric_strings_and_validates_states() {
        assert_eq!(json_f64(&serde_json::json!("10.25")), 10.25);
        assert_eq!(json_u64(&serde_json::json!("42")), 42);
        let mut state = TradeState::Idle;
        assert!(state.transition(TradeState::CollectingTicks));
        assert!(state.transition(TradeState::Analysing));
        assert!(state.transition(TradeState::ProposalPending));
        assert!(state.transition(TradeState::ProposalReady));
        assert!(state.transition(TradeState::BuyPending));
        assert!(state.transition(TradeState::ContractOpen));
        assert!(!state.transition(TradeState::ContractOpen));
    }

    #[tokio::test]
    async fn proposal_symbol_is_correlated_from_echo_request() {
        let (tx, mut rx) = broadcast::channel(4);
        handle_ws_message(r#"{"msg_type":"proposal","proposal":{"id":"p-1","ask_price":"5.25"},"echo_req":{"underlying_symbol":"R_50"}}"#, &tx).await.unwrap();
        assert!(
            matches!(rx.recv().await.unwrap(), ApiEvent::Proposal { underlying_symbol, ask_price, .. } if underlying_symbol == "R_50" && ask_price == 5.25)
        );
    }

    #[tokio::test]
    async fn tick_and_settlement_numeric_strings_parse() {
        let (tx, mut rx) = broadcast::channel(4);
        handle_ws_message(r#"{"msg_type":"tick","tick":{"symbol":"1HZ100V","quote":"123.45","epoch":"1700000000"}}"#, &tx).await.unwrap();
        assert!(matches!(rx.recv().await.unwrap(), ApiEvent::Tick(t) if t.quote == 123.45));
        handle_ws_message(r#"{"msg_type":"proposal_open_contract","proposal_open_contract":{"contract_id":"42","status":"won","sell_price":"5.75"}}"#, &tx).await.unwrap();
        assert!(
            matches!(rx.recv().await.unwrap(), ApiEvent::ContractSold { contract_id, sold_for } if contract_id == "42" && sold_for == 5.75)
        );
    }

    #[test]
    fn ordinary_losses_keep_trading_and_logs_redact_secrets() {
        let mut risk = RiskManager::new();
        risk.mode = TradingMode::Demo;
        risk.session_pnl = -1.99;
        risk.evaluate();
        assert_eq!(risk.current_mode(), TradingMode::Demo);
        assert!(!risk.is_stopped());
        let logged = sanitized_json(&serde_json::json!({"otp":"secret", "token":"secret"}));
        assert!(!logged.contains("secret"));
    }

    #[test]
    fn profitable_session_is_not_capped_or_paused() {
        let mut risk = RiskManager::new();
        risk.mode = TradingMode::Demo;
        risk.resume_mode = TradingMode::Demo;
        risk.session_pnl = 100.0;
        risk.evaluate();
        assert_eq!(risk.current_mode(), TradingMode::Demo);
        assert!(risk.profit_cooldown_until.is_none());
    }

    #[test]
    fn accumulator_stake_is_owned_by_the_risk_engine() {
        let risk = RiskManager::with_balance(Money(50_000));
        assert_eq!(risk.current_stake(), 5.00);
        assert_eq!(risk.accumulator_stake(750.00), 5.00);
    }

    #[test]
    fn losses_reduce_stake_and_recovery_remains_drawdown_scaled() {
        let mut risk = RiskManager::new();
        risk.record_trade(&TradeResult { profit_loss: -0.10 });
        assert_eq!(risk.current_stake(), MIN_RECOVERY_STAKE);

        risk.record_trade(&TradeResult { profit_loss: -0.10 });
        assert_eq!(risk.current_stake(), MIN_RECOVERY_STAKE);

        risk.record_trade(&TradeResult { profit_loss: 0.05 });
        assert_eq!(risk.current_stake(), MIN_RECOVERY_STAKE);
        assert_eq!(risk.recovery_wins, 1);

        risk.record_trade(&TradeResult { profit_loss: 0.05 });
        assert!((MIN_RECOVERY_STAKE..=risk.base_stake.api_amount()).contains(&risk.current_stake()));
        assert_eq!(risk.recovery_wins, 0);
    }

    #[test]
    fn three_win_streak_caps_the_next_stake_at_one_dollar() {
        let mut risk = RiskManager::new();
        for _ in 0..WIN_STREAK_SAFE_STAKE_AFTER {
            risk.record_trade(&TradeResult { profit_loss: 0.05 });
        }
        assert_eq!(risk.current_stake(), UNCERTAIN_TRADE_STAKE);
        assert!((WIN_STREAK_SAFE_TRADES_MIN..=WIN_STREAK_SAFE_TRADES_MAX)
            .contains(&risk.win_streak_safe_trades_remaining));

        risk.record_trade(&TradeResult { profit_loss: -0.05 });
        assert_eq!(risk.consecutive_wins, 0);
        assert_eq!(risk.current_stake(), MIN_RECOVERY_STAKE);
    }

    #[test]
    fn accumulator_base_stake_and_hard_session_loss_limit_are_enforced() {
        let mut risk = RiskManager::new();
        risk.mode = TradingMode::Demo;
        risk.session_pnl = -risk.session_loss_limit().api_amount();
        risk.evaluate();
        assert_eq!(risk.current_mode(), TradingMode::Stopped);
    }

    #[test]
    fn profits_offset_losses_before_the_twenty_percent_stop() {
        let mut covered = RiskManager::new();
        covered.mode = TradingMode::Demo;
        covered.record_trade(&TradeResult { profit_loss: 1.01 });
        covered.record_trade(&TradeResult { profit_loss: -1.00 });
        assert_eq!(covered.current_mode(), TradingMode::Demo);
        assert!((covered.session_pnl - 0.01).abs() < 1e-9);

        let mut unrecovered = RiskManager::new();
        unrecovered.mode = TradingMode::Demo;
        let loss_limit = unrecovered.session_loss_limit().api_amount();
        unrecovered.record_trade(&TradeResult {
            profit_loss: -loss_limit,
        });
        assert_eq!(unrecovered.current_mode(), TradingMode::Stopped);
    }

    #[test]
    fn signal_risk_monotonically_decreases_stake() {
        assert_eq!(stake_for_signal_risk(STAKE_AMOUNT, 0.0), STAKE_AMOUNT);
        assert_eq!(stake_for_signal_risk(STAKE_AMOUNT, 0.5), 5.5);
        assert_eq!(stake_for_signal_risk(STAKE_AMOUNT, 1.0), MIN_RECOVERY_STAKE);
    }

    #[test]
    fn completed_trade_count_does_not_stop_continuous_cadence() {
        let mut risk = RiskManager::new();
        risk.mode = TradingMode::Demo;
        for _ in 0..100 {
            risk.record_trade(&TradeResult { profit_loss: 0.01 });
            assert!(!risk.is_stopped());
        }
        assert_eq!(risk.current_mode(), TradingMode::Demo);
    }

    #[test]
    fn balance_sizing_and_absolute_cap_are_enforced() {
        assert_eq!(
            RiskManager::with_balance(Money(50_000)).current_stake(),
            5.0
        );
        assert_eq!(
            RiskManager::with_balance(Money(100_000_000)).current_stake(),
            10.0
        );
        assert_eq!(RiskManager::with_balance(Money(100)).current_stake(), 1.0);
    }

    #[test]
    fn proposal_stakes_are_normalized_to_deriv_currency_precision() {
        assert_eq!(deriv_stake(0.347_891), 0.35);
        assert_eq!(deriv_stake(1.004), 1.0);
        assert_eq!(
            serde_json::json!({"amount": deriv_stake(0.347_891)}).to_string(),
            r#"{"amount":0.35}"#
        );
    }

    #[test]
    fn continuous_markets_are_not_blocked_by_utc_hour() {
        for hour in [0, 10, 11, 12, 23] {
            let quality = session_quality_for_hour(hour);
            assert_eq!(quality, 0.30);
            assert!(session_allows_entry(quality));
        }
    }

    #[test]
    fn agreement_is_counted_only_in_consensus_direction() {
        let scores = [0.90, 0.80, 0.70, 0.40, 0.30, 0.20, 0.10, 0.50];
        assert_eq!(directional_agreement(&scores, true), 3);
        assert_eq!(directional_agreement(&scores, false), 4);
    }

    #[test]
    fn random_market_selection_always_returns_a_configured_market() {
        let markets = vec!["R_10".to_string(), "R_25".to_string(), "R_50".to_string()];
        let mut rotator = IndexRotator::new(markets.clone());
        for _ in 0..100 {
            assert!(markets.contains(&rotator.next_index()));
        }
    }

    #[test]
    fn one_rotation_cycle_covers_every_available_market() {
        let markets: Vec<String> = AVAILABLE_INDICES
            .iter()
            .map(|symbol| (*symbol).into())
            .collect();
        let mut rotator = IndexRotator::new(markets.clone());
        let selected: std::collections::HashSet<String> =
            (0..markets.len()).map(|_| rotator.next_index()).collect();
        assert_eq!(selected.len(), markets.len());
        assert!(markets.iter().all(|market| selected.contains(market)));
    }

    #[test]
    fn rotation_does_not_repeat_the_previous_market_when_an_alternative_is_available() {
        let markets = vec!["R_10".to_string(), "R_25".to_string(), "R_50".to_string()];
        let mut rotator = IndexRotator::new(markets);
        let mut previous = rotator.next_index();

        for _ in 0..100 {
            let current = rotator.next_index();
            assert_ne!(current, previous);
            previous = current;
        }
    }

    #[test]
    fn two_wins_temporarily_diversify_away_from_symbol() {
        let mut rotator = IndexRotator::new(vec!["R_10".into(), "R_25".into()]);
        rotator.record_result("R_10", true);
        assert!(rotator.market_available("R_10"));
        rotator.record_result("R_10", true);
        assert!(!rotator.market_available("R_10"));
        for _ in 0..10 {
            assert_ne!(rotator.next_index(), "R_10");
        }
    }

    #[test]
    fn losing_market_is_cooled_and_skipped_by_rotation() {
        let markets = vec!["R_10".to_string(), "R_25".to_string(), "R_50".to_string()];
        let mut rotator = IndexRotator::new(markets);
        rotator.record_result("R_10", false);
        assert!(!rotator.market_available("R_10"));
        let first_cooldown = rotator.performance.get("R_10").unwrap().cooldown_until_ms
            - Utc::now().timestamp_millis();
        assert!(first_cooldown > 299_000 && first_cooldown <= MARKET_LOSS_COOLDOWN_SECS * 1000);
        rotator.record_result("R_10", false);
        assert!(!rotator.market_available("R_10"));
        for _ in 0..10 {
            assert_ne!(rotator.next_index(), "R_10");
        }
        let remaining = rotator.performance.get("R_10").unwrap().cooldown_until_ms
            - Utc::now().timestamp_millis();
        assert!(remaining > 599_000 && remaining <= MARKET_REPEAT_LOSS_COOLDOWN_SECS * 1000);
    }

    #[test]
    fn random_market_risk_gate_rejects_warmup_and_high_spread() {
        let warmup = vec![Tick {
            symbol: "R_10".into(),
            quote: 100.0,
            bid: 99.99,
            ask: 100.01,
            epoch: 1,
            pip_size: 2,
        }];
        assert!(
            random_market_risk_reason(&warmup).is_some_and(|reason| reason.starts_with("warmup"))
        );

        let high_spread: Vec<Tick> = (0..MIN_ANALYSIS_TICKS)
            .map(|index| Tick {
                symbol: "R_10".into(),
                quote: 100.0,
                bid: 99.0,
                ask: 101.0,
                epoch: index as u64 + 1,
                pip_size: 2,
            })
            .collect();
        assert!(random_market_risk_reason(&high_spread)
            .is_some_and(|reason| reason.starts_with("spread")));
    }

    #[test]
    fn random_market_risk_gate_accepts_stable_market() {
        let stable: Vec<Tick> = (0..MIN_ANALYSIS_TICKS)
            .map(|index| Tick {
                symbol: "R_10".into(),
                quote: 100.0 + index as f64 * 0.001,
                bid: 99.999 + index as f64 * 0.001,
                ask: 100.001 + index as f64 * 0.001,
                epoch: index as u64 + 1,
                pip_size: 2,
            })
            .collect();
        assert_eq!(random_market_risk_reason(&stable), None);
    }

    #[test]
    fn headerless_descending_history_keeps_latest_ticks_in_time_order() {
        let path = std::env::temp_dir().join(format!(
            "deriv-history-{}-{}.csv",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(
            &path,
            "2026-01-01 00:00:03,103,13.0\n2026-01-01 00:00:02,102,12.0\n2026-01-01 00:00:01,101,11.0\n",
        )
        .unwrap();
        let mut buffer = RingBuffer::new(2);
        let loaded = load_csv_file(&path, "R_10", &mut buffer, 2).unwrap();
        let ticks: Vec<_> = buffer.iter().map(|tick| (tick.epoch, tick.quote)).collect();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded, 2);
        assert_eq!(ticks, vec![(102, 12.0), (103, 13.0)]);
    }

    #[test]
    #[ignore = "requires the external downloader dataset"]
    fn external_history_dataset_loads_all_configured_markets() {
        let path = env::var("HISTORY_TEST_PATH").expect("set HISTORY_TEST_PATH");
        let symbols: Vec<String> = AVAILABLE_INDICES.iter().map(|s| s.to_string()).collect();
        let loader = HistoryLoader::load(&path, &symbols, TICK_BUFFER_CAP).unwrap();
        assert_eq!(loader.loaded_count, symbols.len() * TICK_BUFFER_CAP);
        for symbol in symbols {
            let ticks: Vec<_> = loader.get_buffer(&symbol).unwrap().iter().collect();
            assert_eq!(ticks.len(), TICK_BUFFER_CAP);
            assert!(ticks.windows(2).all(|pair| pair[0].epoch <= pair[1].epoch));
        }
    }

    #[test]
    fn exact_half_theory_scores_are_neutral() {
        let scores = [0.5, 0.56, 0.44, 0.5, 0.55, 0.45, 0.8, 0.2];
        assert_eq!(theory_agreement(&scores), (2, 2, 2));
    }

    #[test]
    fn strong_consensus_requires_four_theories_and_strong_thresholds() {
        assert_eq!(
            consensus_rejection_reason(
                ConsensusPath::Strong,
                4,
                0.74,
                0.70,
                MIN_THEORIES_AGREE,
                CONSENSUS_THRESHOLD,
                CONFIDENCE_THRESHOLD,
            ),
            None
        );
        assert!(consensus_rejection_reason(
            ConsensusPath::Strong,
            3,
            0.90,
            0.90,
            MIN_THEORIES_AGREE,
            CONSENSUS_THRESHOLD,
            CONFIDENCE_THRESHOLD,
        )
        .unwrap()
        .contains("agree=3/8 < 4"));
    }

    #[test]
    fn relaxed_consensus_accepts_valid_three_theory_fallback() {
        assert_eq!(
            consensus_rejection_reason(
                ConsensusPath::Relaxed,
                3,
                0.67,
                0.63,
                RELAXED_THEORIES_AGREE,
                RELAXED_CONSENSUS,
                RELAXED_CONFIDENCE,
            ),
            None
        );
    }

    #[tokio::test]
    async fn portfolio_response_recovers_open_contract_details() {
        let (tx, mut rx) = broadcast::channel(4);
        handle_ws_message(
            r#"{"msg_type":"portfolio","portfolio":{"contracts":[{"contract_id":"77","symbol":"R_50","buy_price":"1.00","purchase_time":"123"}]}}"#,
            &tx,
        )
        .await
        .unwrap();
        assert!(matches!(
            rx.recv().await.unwrap(),
            ApiEvent::Portfolio(contracts)
                if contracts.len() == 1
                    && contracts[0].contract_id == "77"
                    && contracts[0].buy_price == 1.0
        ));
    }

    #[tokio::test]
    async fn api_error_preserves_request_context() {
        let (tx, mut rx) = broadcast::channel(4);
        handle_ws_message(
            r#"{"msg_type":"portfolio","error":{"code":"RateLimit","message":"slow down"},"echo_req":{"passthrough":{"kind":"portfolio"}}}"#,
            &tx,
        )
        .await
        .unwrap();
        assert!(matches!(
            rx.recv().await.unwrap(),
            ApiEvent::Error { request_kind: Some(kind), .. } if kind == "portfolio"
        ));
    }

    #[test]
    fn opened_trade_activates_three_minute_pacing_guard() {
        let mut pacer = ExecutionPacer::new();
        pacer.record_trade_opened();
        assert!(pacer.should_skip_signal());
    }

    #[tokio::test]
    async fn buy_response_preserves_confirmed_entry_price() {
        let (tx, mut rx) = broadcast::channel(4);
        handle_ws_message(
            r#"{"msg_type":"buy","buy":{"contract_id":"42","buy_price":"1.25"}}"#,
            &tx,
        )
        .await
        .unwrap();
        assert!(matches!(
            rx.recv().await.unwrap(),
            ApiEvent::ContractOpened { contract_id, buy_price: Some(price) }
                if contract_id == "42" && price == 1.25
        ));
    }

    #[tokio::test]
    async fn incomplete_sell_response_is_not_counted_as_zero_value_loss() {
        let (tx, mut rx) = broadcast::channel(4);
        handle_ws_message(r#"{"msg_type":"sell","sell":{"contract_id":"42"}}"#, &tx)
            .await
            .unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn relaxed_consensus_rejects_weak_or_low_confidence_signals() {
        assert!(consensus_rejection_reason(
            ConsensusPath::Relaxed,
            0,
            0.90,
            0.90,
            RELAXED_THEORIES_AGREE,
            RELAXED_CONSENSUS,
            RELAXED_CONFIDENCE,
        )
        .unwrap()
        .contains("agree=0/8 < 3"));
        assert!(consensus_rejection_reason(
            ConsensusPath::Relaxed,
            3,
            0.66,
            0.49,
            RELAXED_THEORIES_AGREE,
            RELAXED_CONSENSUS,
            RELAXED_CONFIDENCE,
        )
        .unwrap()
        .contains("confidence"));
        assert!(consensus_rejection_reason(
            ConsensusPath::Relaxed,
            3,
            0.50,
            0.90,
            RELAXED_THEORIES_AGREE,
            RELAXED_CONSENSUS,
            RELAXED_CONFIDENCE,
        )
        .unwrap()
        .contains("inside"));
    }

    fn scored_ticks(symbol: &str, recent_jump: f64) -> Vec<Tick> {
        let mut price = 100.0;
        (0..240)
            .map(|index| {
                let movement = if index >= 180 {
                    if index % 2 == 0 {
                        recent_jump
                    } else {
                        -recent_jump * 0.9
                    }
                } else if index % 2 == 0 {
                    0.00001
                } else {
                    -0.000009
                };
                price *= 1.0 + movement;
                Tick {
                    symbol: symbol.into(),
                    quote: price,
                    ask: price * 1.00001,
                    bid: price * 0.99999,
                    epoch: 1_000 + index,
                    pip_size: 5,
                }
            })
            .collect()
    }

    #[test]
    fn rise_fall_confidence_is_conservative_and_needs_history() {
        assert_eq!(wilson_lower_bound(0, 0), 0.0);
        assert!(wilson_lower_bound(49, 50) < RISE_FALL_TARGET_WIN_RATE);
        assert!(wilson_lower_bound(100, 100) > 0.96);

        let ticks = scored_ticks("R_10", 0.00001);
        assert!(validated_rise_fall_signal(&ticks[..RISE_FALL_MODEL_WINDOW - 1]).is_none());
    }

    #[test]
    fn historical_ranker_prefers_stable_live_regime_and_rejects_stale_data() {
        let stable = scored_ticks("R_10", 0.00001);
        let unstable = scored_ticks("R_100", 0.01);
        let stable_score = historical_market_score("R_10", &stable, 1_239).unwrap();
        let unstable_score = historical_market_score("R_100", &unstable, 1_239).unwrap();
        assert!(stable_score.score > unstable_score.score);
        assert!(stable_score.tail_rate < unstable_score.tail_rate);
        assert!((0.0..=1.0).contains(&stable_score.chaos_edge));
        assert!((0.0..=1.0).contains(&stable_score.theory_confidence));
        assert!(historical_market_score("R_10", &stable, 1_300).is_none());
    }

    #[test]
    fn all_market_ranker_selects_best_history_candidate() {
        let symbols = vec!["R_10".to_string(), "R_100".to_string()];
        let mut loader = HistoryLoader {
            buffers: HashMap::new(),
            loaded_count: 0,
        };
        for tick in scored_ticks("R_10", 0.00001) {
            loader.push_tick(&tick);
        }
        for tick in scored_ticks("R_100", 0.01) {
            loader.push_tick(&tick);
        }
        let rotator = IndexRotator::new(symbols);
        let best = rotator.best_historical_market(&loader, 1_239).unwrap();
        assert_eq!(best.symbol, "R_10");
    }
}
