// ═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════
// RISK MANAGER
// ═══════════════════════════════════════════════════════════════════════════

struct RiskManager {
    verified_balance: Money,
    base_stake: Money,
    session_pnl: f64,
    gross_profit: f64,
    gross_loss: f64,
    peak_pnl: f64,
    max_drawdown: f64,
    session_started_at: chrono::DateTime<chrono::Utc>,
    total_trades: u32,
    winning_trades: u32,
    losing_trades: u32,
    consecutive_losses: u32,
    consecutive_wins: u32,
    win_streak_safe_trades_remaining: u32,
    consecutive_loss_amount: f64,
    current_stake: Money,
    recovery_wins: u32,
    strike_count: u32,
    mode: TradingMode,
    resume_mode: TradingMode,
    profit_cooldown_until: Option<Instant>,
    stopped: Arc<AtomicBool>,
    per_type_stats: HashMap<ContractType, ContractTypeStats>,
    safety_open_contracts: HashMap<String, Money>,
    safety_confirmed_proposals: HashMap<String, (Money, i64)>,
    safety_last_tick_ms: i64,
    safety_api_events: u64,
    safety_api_errors: u64,
    safety_loss_throttle_until_ms: i64,
    stop_reason: Option<String>,
    persistence_healthy: bool,
    runtime_config: RuntimeRiskConfig,
}

impl RiskManager {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_balance(Money(1_000_000))
    }

    fn with_balance(verified_balance: Money) -> Self {
        Self::with_runtime_config(verified_balance, TradingMode::Demo, RuntimeRiskConfig::compiled_defaults())
    }

    fn with_runtime_config(verified_balance: Money, mode: TradingMode, runtime_config: RuntimeRiskConfig) -> Self {
        let balance_risk = verified_balance.0.saturating_mul(BALANCE_RISK_BPS) / 10_000;
        let base_stake = Money(balance_risk.clamp(MIN_STAKE_MINOR, runtime_config.max_stake_minor));
        Self {
            verified_balance,
            base_stake,
            session_pnl: 0.0,
            gross_profit: 0.0,
            gross_loss: 0.0,
            peak_pnl: 0.0,
            max_drawdown: 0.0,
            session_started_at: Utc::now(),
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
            consecutive_losses: 0,
            consecutive_wins: 0,
            win_streak_safe_trades_remaining: 0,
            consecutive_loss_amount: 0.0,
            current_stake: base_stake,
            recovery_wins: 0,
            strike_count: 0,
            mode,
            resume_mode: mode,
            profit_cooldown_until: None,
            stopped: Arc::new(AtomicBool::new(false)),
            per_type_stats: HashMap::new(),
            safety_open_contracts: HashMap::new(),
            safety_confirmed_proposals: HashMap::new(),
            safety_last_tick_ms: Utc::now().timestamp_millis(),
            safety_api_events: 0,
            safety_api_errors: 0,
            safety_loss_throttle_until_ms: 0,
            stop_reason: None,
            persistence_healthy: true,
            runtime_config,
        }
    }

    fn with_mode(verified_balance: Money, mode: TradingMode) -> Self {
        let mut manager = Self::with_balance(verified_balance);
        manager.mode = mode;
        manager.resume_mode = mode;
        manager
    }

    fn record_trade_typed(
        &mut self,
        result: &TradeResult,
        contract_type: ContractType,
    ) -> TradingMode {
        let stats = self.per_type_stats.entry(contract_type).or_default();
        stats.trades += 1;
        stats.profit += result.profit_loss;
        if result.is_profitable() {
            stats.wins += 1;
        } else {
            stats.losses += 1;
        }
        self.record_trade(result)
    }

    fn record_trade(&mut self, result: &TradeResult) -> TradingMode {
        self.total_trades += 1;
        self.session_pnl += result.profit_loss;
        let was_win_streak_safe_trade = self.win_streak_safe_trades_remaining > 0;
        if was_win_streak_safe_trade {
            self.win_streak_safe_trades_remaining -= 1;
        }
        if result.is_profitable() {
            self.gross_profit += result.profit_loss;
            self.winning_trades += 1;
            if was_win_streak_safe_trade {
                self.consecutive_wins = 0;
            } else {
                self.consecutive_wins += 1;
            }
            self.consecutive_losses = 0;
            self.consecutive_loss_amount = 0.0;
            if self.current_stake < self.base_stake {
                self.recovery_wins += 1;
                if self.recovery_wins >= 2 {
                    let drawdown = (self.peak_pnl - self.session_pnl).max(0.0);
                    let risk = (drawdown / self.session_loss_limit().api_amount()).clamp(0.0, 1.0);
                    let available = self.base_stake.0 - MIN_STAKE_MINOR;
                    self.current_stake = Money(
                        MIN_STAKE_MINOR + ((available as f64) * (1.0 - risk)).round() as i64
                    );
                    self.recovery_wins = 0;
                    log::info!(
                        "[DrawdownRisk] Drawdown reduced; risk={:.0}% stake={:.2}",
                        risk * 100.0,
                        self.current_stake.api_amount()
                    );
                } else {
                    log::info!(
                        "[DrawdownRisk] De-risked stake remains at {:.2}",
                        self.current_stake.api_amount()
                    );
                }
            }
            if !was_win_streak_safe_trade && self.consecutive_wins >= WIN_STREAK_SAFE_STAKE_AFTER {
                let span = WIN_STREAK_SAFE_TRADES_MAX - WIN_STREAK_SAFE_TRADES_MIN + 1;
                self.win_streak_safe_trades_remaining =
                    WIN_STREAK_SAFE_TRADES_MIN + (OsRng.next_u64() % span as u64) as u32;
                self.consecutive_wins = 0;
                log::info!(
                    "[WinStreakProtection] {} wins: next {} trade(s) capped at stake {:.2}",
                    WIN_STREAK_SAFE_STAKE_AFTER,
                    self.win_streak_safe_trades_remaining,
                    UNCERTAIN_TRADE_STAKE
                );
            }
        } else {
            self.gross_loss += result.profit_loss.abs();
            self.losing_trades += 1;
            self.consecutive_wins = 0;
            self.win_streak_safe_trades_remaining = self.win_streak_safe_trades_remaining.max(1);
            self.consecutive_losses += 1;
            self.consecutive_loss_amount += result.profit_loss.abs();
            self.recovery_wins = 0;
            // Go straight to the minimum stake after any loss. Contract
            // selection and entry signals remain unchanged during recovery.
            self.current_stake = Money(MIN_STAKE_MINOR);
            log::warn!(
                "[StakeProtection] Loss {}: next stake reduced to {:.2}",
                self.consecutive_losses,
                self.current_stake.api_amount()
            );
        }
        self.peak_pnl = self.peak_pnl.max(self.session_pnl);
        self.max_drawdown = self.max_drawdown.max(self.peak_pnl - self.session_pnl);
        journal_event("pnl_calculation", serde_json::json!({
            "trade_pnl_minor": Money::from_api(result.profit_loss.abs()).map(|money| {
                if result.profit_loss < 0.0 { -money.0 } else { money.0 }
            }),
            "session_pnl_minor": Money::from_api(self.session_pnl.abs()).map(|money| {
                if self.session_pnl < 0.0 { -money.0 } else { money.0 }
            }),
            "next_stake_minor": self.current_stake.0,
            "drawdown_minor": Money::from_api(self.max_drawdown).map(|money| money.0),
            "recovery_action": if result.is_profitable() {
                "drawdown_scaled"
            } else {
                "de_risk_to_minimum"
            },
        }));

        let old = self.mode;
        self.evaluate();
        if self.mode != old {
            log::warn!(
                "RiskManager: {} → {} | P&L=${:.4} | strikes={}",
                old,
                self.mode,
                self.session_pnl,
                self.strike_count
            );
        }
        self.mode
    }

    fn evaluate(&mut self) {
        let session_loss_reached = self.session_pnl <= -self.session_loss_limit().api_amount();
        let session_profit_reached = Money::from_api(self.session_pnl)
            .is_some_and(|profit| profit.0 >= SESSION_PROFIT_TARGET_MINOR);
        if self.mode != TradingMode::Stopped && session_profit_reached {
            log::info!(
                "Session profit target reached: P&L=${:.4}. Stopping new {} entries.",
                self.session_pnl,
                self.mode
            );
            journal_event("session_profit_target", serde_json::json!({
                "session_pnl_minor": Money::from_api(self.session_pnl).map(|money| money.0),
                "target_minor": SESSION_PROFIT_TARGET_MINOR,
            }));
            self.engage_stop("session profit target reached");
        }
        let profit_lock_activation = self.verified_balance.0
            .saturating_mul(PROFIT_LOCK_ACTIVATION_BPS) / 10_000;
        let profit_lock_giveback = (self.verified_balance.0
            .saturating_mul(PROFIT_LOCK_MIN_GIVEBACK_BPS) / 10_000)
            .max(
                Money::from_api(self.peak_pnl)
                    .map(|peak| peak.0.saturating_mul(PROFIT_LOCK_PEAK_GIVEBACK_BPS) / 10_000)
                    .unwrap_or_default(),
            );
        let peak_profit_minor = Money::from_api(self.peak_pnl)
            .map(|peak| peak.0)
            .unwrap_or_default();
        let drawdown_minor = Money::from_api((self.peak_pnl - self.session_pnl).max(0.0))
            .map(|drawdown| drawdown.0)
            .unwrap_or_default();
        let profit_lock_reached = peak_profit_minor >= profit_lock_activation
            && drawdown_minor >= profit_lock_giveback;
        if self.mode != TradingMode::Stopped && profit_lock_reached {
            log::error!(
                "Profit-lock circuit breaker reached: peak=${:.4}, current=${:.4}, giveback=${:.4}. Stopping {} trading.",
                self.peak_pnl,
                self.session_pnl,
                drawdown_minor as f64 / 100.0,
                self.mode
            );
            journal_event("profit_lock", serde_json::json!({
                "peak_profit_minor": peak_profit_minor,
                "session_pnl_minor": Money::from_api(self.session_pnl.abs()).map(|money| {
                    if self.session_pnl < 0.0 { -money.0 } else { money.0 }
                }),
                "giveback_minor": drawdown_minor,
                "allowed_giveback_minor": profit_lock_giveback,
            }));
            self.engage_stop("trailing profit-lock circuit breaker");
        }
        let consecutive_loss_limit_reached =
            self.consecutive_losses >= self.runtime_config.max_consecutive_losses;
        if self.mode != TradingMode::Stopped && consecutive_loss_limit_reached {
            log::error!(
                "Consecutive-loss circuit breaker reached: {} losses, P&L=${:.4}. Stopping {} trading.",
                self.consecutive_losses,
                self.session_pnl,
                self.mode
            );
            self.engage_stop("consecutive-loss circuit breaker");
        }
        if self.mode != TradingMode::Stopped && session_loss_reached {
            log::error!(
                "Hard session loss circuit breaker reached: P&L=${:.4}, consecutive losses=${:.4} across {} trade(s). Stopping {} trading.",
                self.session_pnl,
                self.consecutive_loss_amount,
                self.total_trades,
                self.mode
            );
            self.engage_stop("hard session loss circuit breaker");
        }

        // Ordinary losses do not pause sequential trading. Only unrecovered
        // net session loss can trip the $2 circuit breaker, so prior profits
        // offset losses before a stop is considered.
    }

    fn refresh_timed_state(&mut self) {
        self.evaluate();
    }

    fn current_mode(&self) -> TradingMode {
        self.mode
    }
    #[cfg(test)]
    fn current_stake(&self) -> f64 {
        if self.win_streak_safe_trades_remaining > 0 {
            UNCERTAIN_TRADE_STAKE
        } else {
            self.current_stake.api_amount()
        }
    }
    fn session_loss_limit(&self) -> Money {
        Money(self.verified_balance.0.saturating_mul(self.runtime_config.session_loss_limit_bps) / 10_000)
    }
    fn stake_for_risk(&self, signal_risk: f64) -> f64 {
        let cap = if self.win_streak_safe_trades_remaining > 0 {
            Money(MIN_STAKE_MINOR)
        } else {
            self.current_stake.min(self.base_stake)
        };
        stake_for_signal_risk(cap.api_amount(), signal_risk)
    }
    #[cfg(test)]
    fn accumulator_stake(&self, configured_base: f64) -> f64 {
        self.stake_for_risk(0.0).min(deriv_stake(configured_base))
    }
    fn engage_stop(&mut self, reason: impl Into<String>) {
        self.stop_reason = Some(reason.into());
        self.mode = TradingMode::Stopped;
        self.stopped.store(true, Ordering::SeqCst);
        self.profit_cooldown_until = None;
    }
    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst) || self.mode == TradingMode::Stopped
    }
    fn type_summary(&self) -> String {
        [
            (ContractType::RiseFall, "RF"),
            (ContractType::OverUnder, "OU"),
            (ContractType::MatchesDiff, "MD"),
            (ContractType::Accumulator, "ACCU"),
        ]
        .iter()
        .map(|(kind, label)| {
            self.per_type_stats
                .get(kind)
                .cloned()
                .unwrap_or_default()
                .summary(label)
        })
        .collect::<Vec<_>>()
        .join(" | ")
    }
    fn summary(&self) -> String {
        let total = self.total_trades as f64;
        let win_rate = if total > 0.0 {
            self.winning_trades as f64 / total
        } else {
            0.0
        };
        let loss_rate = if total > 0.0 {
            self.losing_trades as f64 / total
        } else {
            0.0
        };
        let average_profit = if self.winning_trades > 0 {
            self.gross_profit / self.winning_trades as f64
        } else {
            0.0
        };
        let average_loss = if self.losing_trades > 0 {
            self.gross_loss / self.losing_trades as f64
        } else {
            0.0
        };
        let expected_value = win_rate * average_profit - loss_rate * average_loss;
        let profit_factor = if self.gross_loss > 0.0 {
            self.gross_profit / self.gross_loss
        } else {
            0.0
        };
        let minutes = (Utc::now() - self.session_started_at)
            .num_milliseconds()
            .max(1) as f64
            / 60_000.0;
        format!(
            "P&L=${:.4} | T:{} W:{:.1}% L:{:.1}% | EV=${:.4} avgW=${:.4} avgL=${:.4} PF={:.2} DD=${:.4} TPM={:.2} | mode={} strikes={}",
            self.session_pnl, self.total_trades, win_rate * 100.0, loss_rate * 100.0,
            expected_value, average_profit, average_loss, profit_factor, self.max_drawdown,
            total / minutes, self.mode, self.strike_count
        )
    }
}
