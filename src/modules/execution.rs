// ═══════════════════════════════════════════════════════════════════════════
// EXECUTION PACER
// ═══════════════════════════════════════════════════════════════════════════

struct ExecutionPacer {
    last_trade_at: Option<Instant>,
    recent_trades: VecDeque<Instant>,
    total_signals_seen: u64,
    total_signals_skipped: u64,
}

impl ExecutionPacer {
    fn new() -> Self {
        Self {
            last_trade_at: None,
            recent_trades: VecDeque::with_capacity(32),
            total_signals_seen: 0,
            total_signals_skipped: 0,
        }
    }

    fn should_skip_signal(&mut self) -> bool {
        self.total_signals_seen += 1;

        let elapsed = self.last_trade_at.map(|at| at.elapsed());
        if elapsed.is_some_and(|duration| duration < Duration::from_millis(TRADE_COOLDOWN_MS)) {
            let elapsed_ms = elapsed.map_or(0, |duration| duration.as_millis());
            log::debug!(
                "[Pacer] Cooldown — {}ms < {}ms",
                elapsed_ms,
                TRADE_COOLDOWN_MS
            );
            self.total_signals_skipped += 1;
            return true;
        }

        false
    }

    fn record_trade_opened(&mut self) {
        let now = Instant::now();
        self.last_trade_at = Some(now);
        if self.recent_trades.len() == self.recent_trades.capacity() {
            self.recent_trades.pop_front();
        }
        self.recent_trades.push_back(now);
    }

    fn stats(&self) -> String {
        let skip_rate = if self.total_signals_seen > 0 {
            self.total_signals_skipped as f64 / self.total_signals_seen as f64 * 100.0
        } else {
            0.0
        };
        format!(
            "signals={} skipped={} ({:.0}%)",
            self.total_signals_seen, self.total_signals_skipped, skip_rate
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// API TYPES (NEW FLOW)
