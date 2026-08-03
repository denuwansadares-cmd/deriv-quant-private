#[derive(Debug, Clone, Serialize)]
struct BacktestConfig {
    proposal_ttl_ms: i64,
    latency_ms: i64,
    stale_quote_ms: i64,
    payout_bps: i64,
    fee_bps: i64,
    slippage_bps: i64,
    rate_limit_per_sec: u32,
    disconnect_ppm: u32,
    reject_ppm: u32,
    lost_buy_response_ppm: u32,
    sell_failure_ppm: u32,
    missing_tick_ppm: u32,
    out_of_order_ppm: u32,
    max_sell_retries: u32,
    accu_growth_bps: i64,
    contract_duration_ticks: u32,
    sell_bid_discount_bps: i64,
}

impl BacktestConfig {
    fn from_env() -> Self {
        let get = |name: &str, default: i64| {
            env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
        };
        Self {
            proposal_ttl_ms: get("SIM_PROPOSAL_TTL_MS", 3_000).max(1),
            latency_ms: get("SIM_LATENCY_MS", 50).max(0),
            stale_quote_ms: get("SIM_STALE_QUOTE_MS", 500).max(1),
            payout_bps: get("SIM_PAYOUT_BPS", 10_200).clamp(0, 100_000),
            fee_bps: get("SIM_FEE_BPS", 5).clamp(0, 10_000),
            slippage_bps: get("SIM_SLIPPAGE_BPS", 10).clamp(0, 10_000),
            rate_limit_per_sec: get("SIM_RATE_LIMIT_PER_SEC", MAX_REQ_PER_SEC as i64).max(1) as u32,
            disconnect_ppm: get("SIM_DISCONNECT_PPM", 0).clamp(0, 1_000_000) as u32,
            reject_ppm: get("SIM_REJECT_PPM", 0).clamp(0, 1_000_000) as u32,
            lost_buy_response_ppm: get("SIM_LOST_BUY_RESPONSE_PPM", 0).clamp(0, 1_000_000) as u32,
            sell_failure_ppm: get("SIM_SELL_FAILURE_PPM", 0).clamp(0, 1_000_000) as u32,
            missing_tick_ppm: get("SIM_MISSING_TICK_PPM", 0).clamp(0, 1_000_000) as u32,
            out_of_order_ppm: get("SIM_OUT_OF_ORDER_PPM", 0).clamp(0, 1_000_000) as u32,
            max_sell_retries: get("SIM_MAX_SELL_RETRIES", 3).max(0) as u32,
            accu_growth_bps: get("SIM_ACCU_GROWTH_BPS", 100).clamp(1, 10_000),
            contract_duration_ticks: get("SIM_CONTRACT_DURATION_TICKS", 5).clamp(1, 10_000) as u32,
            sell_bid_discount_bps: get("SIM_SELL_BID_DISCOUNT_BPS", 25).clamp(0, 10_000),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct BacktestReport {
    result_classification: &'static str,
    contract_pricing_fidelity: &'static str,
    pricing_error: PricingApproximationError,
    symbol: String,
    decisions: u64,
    accepted: u64,
    rejected: u64,
    trades: u64,
    pnl_minor: i64,
    metrics: BacktestMetrics,
    trace_hash: String,
    trace: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
struct PricingApproximationError {
    reference_snapshot_count: u64,
    mean_absolute_ask_error_minor: Option<f64>,
    mean_absolute_payout_error_minor: Option<f64>,
    limitation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct BacktestMetrics {
    sample_size: u64,
    rejected_trade_count: u64,
    expected_value_minor: Option<f64>,
    expected_value_ci95_minor: Option<[f64; 2]>,
    profit_factor: Option<f64>,
    maximum_drawdown_minor: i64,
    risk_of_ruin: Option<f64>,
    win_rate: Option<f64>,
    win_rate_ci95: Option<[f64; 2]>,
    average_win_minor: Option<f64>,
    median_win_minor: Option<f64>,
    average_loss_minor: Option<f64>,
    median_loss_minor: Option<f64>,
    tail_loss_cvar95_minor: Option<f64>,
    sharpe_per_trade: Option<f64>,
    sortino_per_trade: Option<f64>,
    calmar: Option<f64>,
    exposure_fraction: Option<f64>,
    trades_per_hour: Option<f64>,
    worst_consecutive_loss_run: u64,
}

fn average(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() { return None; }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    })
}

fn calculate_backtest_metrics(
    pnls_minor: &[i64],
    rejected: u64,
    exposure_ticks: u64,
    delivered_ticks: u64,
    duration_secs: u64,
    starting_balance_minor: i64,
) -> BacktestMetrics {
    let values = pnls_minor.iter().map(|v| *v as f64).collect::<Vec<_>>();
    let wins = values.iter().copied().filter(|v| *v > 0.0).collect::<Vec<_>>();
    let losses = values.iter().copied().filter(|v| *v < 0.0).collect::<Vec<_>>();
    let n = values.len();
    let mean = average(&values);
    let variance = mean.and_then(|m| {
        (n > 1).then(|| values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1) as f64)
    });
    let ev_ci = mean.zip(variance).map(|(m, v)| {
        let margin = 1.96 * (v / n as f64).sqrt();
        [m - margin, m + margin]
    });
    let win_rate = (n > 0).then(|| wins.len() as f64 / n as f64);
    let win_ci = win_rate.map(|p| {
        let z2 = 1.96_f64.powi(2);
        let nf = n as f64;
        let center = (p + z2 / (2.0 * nf)) / (1.0 + z2 / nf);
        let margin = 1.96 * ((p * (1.0 - p) / nf + z2 / (4.0 * nf * nf)).sqrt()) / (1.0 + z2 / nf);
        [(center - margin).max(0.0), (center + margin).min(1.0)]
    });
    let gross_win = wins.iter().sum::<f64>();
    let gross_loss = losses.iter().map(|v| v.abs()).sum::<f64>();
    let mut equity = 0i64;
    let mut peak = 0i64;
    let mut max_drawdown = 0i64;
    let mut current_loss_run = 0u64;
    let mut worst_loss_run = 0u64;
    for pnl in pnls_minor {
        equity += pnl;
        peak = peak.max(equity);
        max_drawdown = max_drawdown.max(peak - equity);
        if *pnl < 0 {
            current_loss_run += 1;
            worst_loss_run = worst_loss_run.max(current_loss_run);
        } else {
            current_loss_run = 0;
        }
    }
    let mut sorted_losses = losses.iter().map(|v| v.abs()).collect::<Vec<_>>();
    sorted_losses.sort_by(|a, b| b.total_cmp(a));
    let tail_count = ((sorted_losses.len() as f64) * 0.05).ceil().max(1.0) as usize;
    let tail_loss = average(&sorted_losses[..tail_count.min(sorted_losses.len())]);
    let downside_deviation = if n >= 30 {
        Some((values.iter().filter(|v| **v < 0.0).map(|v| v.powi(2)).sum::<f64>() / n as f64).sqrt())
    } else { None };
    let risk_of_ruin = mean.zip(variance).map(|(m, v)| {
        if m <= 0.0 { 1.0 } else if v <= f64::EPSILON { 0.0 }
        else { (-2.0 * starting_balance_minor as f64 * m / v).exp().clamp(0.0, 1.0) }
    });
    BacktestMetrics {
        sample_size: n as u64,
        rejected_trade_count: rejected,
        expected_value_minor: mean,
        expected_value_ci95_minor: ev_ci,
        profit_factor: (gross_loss > 0.0).then_some(gross_win / gross_loss),
        maximum_drawdown_minor: max_drawdown,
        risk_of_ruin,
        win_rate,
        win_rate_ci95: win_ci,
        average_win_minor: average(&wins),
        median_win_minor: median(&wins),
        average_loss_minor: average(&losses),
        median_loss_minor: median(&losses),
        tail_loss_cvar95_minor: tail_loss,
        sharpe_per_trade: if n >= 30 { mean.zip(variance).and_then(|(m,v)| (v > 0.0).then_some(m / v.sqrt())) } else { None },
        sortino_per_trade: mean.zip(downside_deviation).and_then(|(m,d)| (d > 0.0).then_some(m / d)),
        calmar: (n >= 30 && max_drawdown > 0).then_some(equity as f64 / max_drawdown as f64),
        exposure_fraction: (delivered_ticks > 0).then_some(exposure_ticks as f64 / delivered_ticks as f64),
        trades_per_hour: (duration_secs > 0).then_some(n as f64 * 3600.0 / duration_secs as f64),
        worst_consecutive_loss_run: worst_loss_run,
    }
}

struct SimPosition {
    entry_mid: f64,
    ask_minor: i64,
    payout_minor: i64,
    sell_attempts: u32,
    ticks_open: u32,
}

fn sim_event(trace: &mut Vec<serde_json::Value>, at_ms: i64, kind: &str, details: serde_json::Value) {
    trace.push(serde_json::json!({"at_ms":at_ms,"kind":kind,"details":details}));
}

fn sim_occurs(rng: &mut SeededRng, ppm: u32) -> bool {
    rng.next_u64() % 1_000_000 < u64::from(ppm.min(1_000_000))
}

fn backtest_symbol(symbol: String, ticks: Vec<Tick>, config: BacktestConfig, seed: u64) -> BacktestReport {
    suppress_journal_for_current_thread(true);
    let symbol_seed = seed ^ u64::from_str_radix(&stable_hash_bytes(symbol.as_bytes()), 16).unwrap_or(0);
    let mut rng = SeededRng::new(symbol_seed);
    let mut risk = RiskManager::with_mode(Money(1_000_000), TradingMode::Demo);
    let mut state = TradeState::CollectingTicks;
    let mut window = Vec::new();
    let mut trace = Vec::new();
    let (mut decisions, mut accepted, mut rejected, mut trades, mut pnl_minor) = (0, 0, 0, 0, 0);
    let mut last_epoch = 0;
    let mut rate_epoch = u64::MAX;
    let mut requests = 0;
    let mut position: Option<SimPosition> = None;
    let mut trade_pnls = Vec::new();
    let mut delivered_ticks = 0u64;
    let mut exposure_ticks = 0u64;
    let mut first_epoch = None;

    let mut clock = VirtualClock::new(0);
    for tick in ticks {
        clock.advance_to((tick.epoch as i64).saturating_mul(1_000));
        let at_ms = clock.now_ms;
        if sim_occurs(&mut rng, config.missing_tick_ppm) {
            sim_event(&mut trace, at_ms, "missing_tick", serde_json::json!({}));
            continue;
        }
        let delivered_epoch = if sim_occurs(&mut rng, config.out_of_order_ppm) { tick.epoch.saturating_sub(2) } else { tick.epoch };
        if delivered_epoch <= last_epoch {
            rejected += 1;
            sim_event(&mut trace, at_ms, "tick_rejected", serde_json::json!({"reason":"out_of_order"}));
            continue;
        }
        last_epoch = delivered_epoch;
        first_epoch.get_or_insert(delivered_epoch);
        delivered_ticks += 1;
        if position.is_some() { exposure_ticks += 1; }

        // A position may only observe this tick after it was opened on an earlier tick.
        if let Some(mut open) = position.take() {
            let survived = (tick.mid_price() - open.entry_mid).abs() / open.entry_mid.max(1e-12) < ACCU_GROWTH_RATE;
            open.ticks_open += 1;
            if survived && open.ticks_open < config.contract_duration_ticks {
                open.payout_minor += open.payout_minor.saturating_mul(config.accu_growth_bps) / 10_000;
                position = Some(open);
                continue;
            }
            let model_bid_minor = open.payout_minor.saturating_mul(10_000 - config.sell_bid_discount_bps) / 10_000;
            let pnl = if survived { model_bid_minor - open.ask_minor } else { -open.ask_minor };
            let mut sold = false;
            while open.sell_attempts <= config.max_sell_retries {
                let failed = sim_occurs(&mut rng, config.sell_failure_ppm);
                sim_event(&mut trace, at_ms, "sell_result", serde_json::json!({"attempt":open.sell_attempts,"accepted":!failed}));
                open.sell_attempts += 1;
                if !failed { sold = true; break; }
            }
            if sold {
                let _ = state.can_transition(TradeState::Settling);
                risk.record_trade(&TradeResult { profit_loss: pnl as f64 / 100.0 });
                pnl_minor += pnl;
                trade_pnls.push(pnl);
                trades += 1;
                state = TradeState::CollectingTicks;
            } else {
                sim_event(&mut trace, at_ms, "sell_retry_exhausted", serde_json::json!({"recovery":"carry_open_contract"}));
                position = Some(open);
                continue;
            }
        }

        window.push(tick.clone());
        if window.len() < MIN_ANALYSIS_TICKS { continue; }
        decisions += 1;
        let scores = individual_model_scores(&window);
        let (consensus, confidence) = compute_consensus(&window, &STRATEGY_WEIGHTS);
        let (_, _, agreement) = theory_agreement(&scores);
        if consensus < CONSENSUS_THRESHOLD || confidence < CONFIDENCE_THRESHOLD || agreement < MIN_THEORIES_AGREE {
            rejected += 1;
            sim_event(&mut trace, at_ms, "decision", serde_json::json!({"decision":"reject","reason":"thresholds_not_met","scores":scores}));
            continue;
        }
        if sim_occurs(&mut rng, config.disconnect_ppm) {
            rejected += 1;
            sim_event(&mut trace, at_ms, "disconnect", serde_json::json!({"recovery":"reconnect"}));
            continue;
        }
        if delivered_epoch != rate_epoch { rate_epoch = delivered_epoch; requests = 0; }
        if requests >= config.rate_limit_per_sec {
            rejected += 1;
            sim_event(&mut trace, at_ms, "decision", serde_json::json!({"decision":"reject","reason":"rate_limit"}));
            continue;
        }
        requests += 1;
        if sim_occurs(&mut rng, config.reject_ppm) {
            rejected += 1;
            sim_event(&mut trace, at_ms, "proposal_rejected", serde_json::json!({"reason":"injected"}));
            continue;
        }
        if config.latency_ms > config.proposal_ttl_ms {
            rejected += 1;
            sim_event(&mut trace, at_ms + config.latency_ms, "proposal_expired", serde_json::json!({}));
            continue;
        }
        if config.latency_ms > config.stale_quote_ms {
            rejected += 1;
            sim_event(&mut trace, at_ms + config.latency_ms, "stale_quote", serde_json::json!({}));
            continue;
        }
        let stake_minor = Money::from_api(risk.stake_for_risk(1.0 - confidence)).map_or(MIN_STAKE_MINOR, |m| m.0);
        let ask_minor = stake_minor + stake_minor.saturating_mul(config.fee_bps + config.slippage_bps) / 10_000;
        let payout_minor = stake_minor.saturating_mul(config.payout_bps) / 10_000;
        accepted += 1;
        state = TradeState::ContractOpen;
        sim_event(&mut trace, at_ms + config.latency_ms, "proposal_accepted", serde_json::json!({
            "ask_minor":ask_minor,"payout_minor":payout_minor,"scores":scores,
            "latency_ms":config.latency_ms
        }));
        if sim_occurs(&mut rng, config.lost_buy_response_ppm) {
            sim_event(&mut trace, at_ms + config.latency_ms, "buy_response_lost", serde_json::json!({"recovery":"portfolio_reconciliation","found":true}));
        }
        position = Some(SimPosition { entry_mid: tick.mid_price(), ask_minor, payout_minor, sell_attempts: 0, ticks_open: 0 });
    }
    if position.is_some() {
        sim_event(&mut trace, last_epoch as i64 * 1_000, "open_at_dataset_end", serde_json::json!({"no_lookahead":true}));
    }
    let trace_hash = stable_hash_bytes(&serde_json::to_vec(&trace).unwrap_or_default());
    let duration_secs = first_epoch.map_or(0, |first| last_epoch.saturating_sub(first));
    let metrics = calculate_backtest_metrics(
        &trade_pnls, rejected, exposure_ticks, delivered_ticks, duration_secs, 1_000_000,
    );
    let report = BacktestReport {
        result_classification: "hypothetical_backtest_not_live",
        contract_pricing_fidelity: "approximate_contract_pricing",
        pricing_error: PricingApproximationError {
            reference_snapshot_count: 0,
            mean_absolute_ask_error_minor: None,
            mean_absolute_payout_error_minor: None,
            limitation: "No licensed Deriv proposal/ask/payout snapshots supplied; configured pricing mechanics are not broker-exact.",
        },
        symbol, decisions, accepted, rejected, trades, pnl_minor, metrics, trace_hash, trace
    };
    suppress_journal_for_current_thread(false);
    report
}

fn run_backtester(symbols: &[String], history_path: &str, seed: u64) -> Result<()> {
    let history = HistoryLoader::load(history_path, symbols, usize::MAX)?;
    let config = BacktestConfig::from_env();
    journal_event("backtest_config", serde_json::to_value(&config)?);
    let jobs = symbols.iter().map(|symbol| {
        let ticks = history.get_buffer(symbol).map(|b| b.iter().cloned().collect()).unwrap_or_default();
        (symbol.clone(), ticks)
    }).collect::<Vec<_>>();
    let mut reports = std::thread::scope(|scope| {
        let handles = jobs.into_iter().map(|(symbol, ticks)| {
            let config = config.clone();
            scope.spawn(move || backtest_symbol(symbol, ticks, config, seed))
        }).collect::<Vec<_>>();
        handles.into_iter().map(|h| h.join().expect("backtest worker panicked")).collect::<Vec<_>>()
    });
    reports.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    for report in &reports {
        for event in &report.trace {
            activate_virtual_time(event["at_ms"].as_i64().unwrap_or(0));
            journal_event("backtest_event", serde_json::json!({"symbol":report.symbol,"event":event}));
        }
        journal_event("backtest_symbol_complete", serde_json::to_value(report)?);
    }
    let decisions = reports.iter().map(|r| r.decisions).sum::<u64>();
    let trades = reports.iter().map(|r| r.trades).sum::<u64>();
    let pnl_minor = reports.iter().map(|r| r.pnl_minor).sum::<i64>();
    let replay_hash = stable_hash_bytes(&serde_json::to_vec(&reports)?);
    if let Ok(expected) = env::var("SIM_EXPECT_REPLAY_HASH") {
        anyhow::ensure!(
            expected.trim() == replay_hash,
            "Replay hash mismatch: expected {}, produced {}",
            expected.trim(),
            replay_hash
        );
    }
    journal_event("simulation_complete", serde_json::json!({
        "result_classification":"hypothetical_backtest_not_live",
        "contract_pricing_fidelity":"approximate_contract_pricing",
        "warning":"Backtested results may not match actual trading results.",
        "decisions":decisions,"trades":trades,"pnl_minor":pnl_minor,"seed":seed,"replay_hash":replay_hash
    }));
    println!("HYPOTHETICAL BACKTEST — NOT LIVE RESULTS");
    println!("Deterministic backtest: decisions={decisions} trades={trades} pnl_minor={pnl_minor} seed={seed} replay={replay_hash}");
    VIRTUAL_TIME_MS.store(-1, AtomicOrdering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod backtest_tests {
    use super::*;

    fn ticks(count: u64) -> Vec<Tick> {
        (1..=count).map(|epoch| Tick {
            symbol: "R_100".to_string(),
            quote: 100.0 + epoch as f64 * 0.001,
            ask: 100.01 + epoch as f64 * 0.001,
            bid: 99.99 + epoch as f64 * 0.001,
            epoch,
            pip_size: 2,
        }).collect()
    }

    #[test]
    fn identical_inputs_seed_and_faults_produce_identical_trace() {
        let config = BacktestConfig::from_env();
        let first = backtest_symbol("R_100".to_string(), ticks(80), config.clone(), 42);
        let second = backtest_symbol("R_100".to_string(), ticks(80), config, 42);
        assert_eq!(first.trace_hash, second.trace_hash);
        assert_eq!(first.pnl_minor, second.pnl_minor);
        assert_eq!(first.decisions, second.decisions);
    }

    #[test]
    fn missing_tick_injection_is_deterministic_and_prevents_lookahead() {
        let mut config = BacktestConfig::from_env();
        config.missing_tick_ppm = 1_000_000;
        let report = backtest_symbol("R_100".to_string(), ticks(40), config, 7);
        assert_eq!(report.decisions, 0);
        assert_eq!(report.trades, 0);
        assert_eq!(report.trace.len(), 40);
        assert!(report.trace.iter().all(|event| event["kind"] == "missing_tick"));
    }

    #[test]
    fn performance_metrics_include_confidence_drawdown_tail_and_loss_run() {
        let metrics = calculate_backtest_metrics(
            &[100, -50, -75, 125, -200],
            9,
            3,
            10,
            3_600,
            1_000_000,
        );
        assert_eq!(metrics.sample_size, 5);
        assert_eq!(metrics.rejected_trade_count, 9);
        assert_eq!(metrics.maximum_drawdown_minor, 200);
        assert_eq!(metrics.worst_consecutive_loss_run, 2);
        assert_eq!(metrics.exposure_fraction, Some(0.3));
        assert_eq!(metrics.trades_per_hour, Some(5.0));
        assert!(metrics.win_rate_ci95.is_some());
        assert!(metrics.expected_value_ci95_minor.is_some());
        assert_eq!(metrics.sharpe_per_trade, None);
        assert_eq!(metrics.sortino_per_trade, None);
        assert_eq!(metrics.calmar, None);
    }
}
