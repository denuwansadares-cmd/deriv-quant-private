// ═══════════════════════════════════════════════════════════════════════════
// CONFIGURATION CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════

const DERIV_API_BASE: &str = "https://api.derivws.com/trading/v1/options";

// ── Profit / Loss Parameters ───────────────────────────────
const PROFIT_TARGET: f64 = 0.04;
const ESTIMATED_SLIPPAGE: f64 = 0.01;
const ESTIMATED_FEES: f64 = 0.005;
const MIN_EFFECTIVE_PROFIT: f64 = 0.02;
// Random mode is intentionally short-lived: digit contracts settle after one
// tick, and ACCU contracts are sold on the first tick after entry.
const MAX_ACCUMULATOR_HOLD_TICKS: u64 = 1;

// ── Stop-Loss ──────────────────────────────────────────────
const STOP_LOSS_PCT: f64 = 0.10;

// ── Pattern Filter ─────────────────────────────────────────

// ── Session Quality ────────────────────────────────────────
const BEST_HOURS: &[(u32, u32)] = &[(1, 9), (14, 22)];
const WORST_HOURS: &[(u32, u32)] = &[(10, 12), (23, 24), (0, 0)];

// ── Risk Management ────────────────────────────────────────
// Stake policy. Monetary limits are held in minor currency units and converted
// to an API number only at the websocket boundary.
const MIN_STAKE_MINOR: i64 = 100; // 1.00
const HARD_MAX_STAKE_MINOR: i64 = 10_000; // 100.00 absolute ceiling; balance and exposure caps may reduce it
// A $50 bankroll can use up to $3 on the strongest qualified signal. Signal
// risk continuously scales this cap down toward the $1 provider minimum.
const BALANCE_RISK_BPS: i64 = 600; // 6% of verified balance
const SESSION_LOSS_LIMIT_BPS: i64 = 2_000; // 20% of verified balance
// Once a session has built a meaningful profit cushion, retain at least 90%
// of the high-water-mark profit (subject to a 1% balance noise allowance).
// This never blocks a winning run; it only stops new entries after giveback.
const PROFIT_LOCK_ACTIVATION_BPS: i64 = 200; // activate at +2% of balance
const PROFIT_LOCK_MIN_GIVEBACK_BPS: i64 = 100; // tolerate at least 1% of balance
const PROFIT_LOCK_PEAK_GIVEBACK_BPS: i64 = 1_000; // or 10% of peak profit
const SESSION_PROFIT_TARGET_MINOR: i64 = 300;
#[cfg(test)]
const STAKE_AMOUNT: f64 = HARD_MAX_STAKE_MINOR as f64 / 100.0;
const UNCERTAIN_TRADE_STAKE: f64 = 1.00;
const WIN_STREAK_SAFE_STAKE_AFTER: u32 = 3;
const WIN_STREAK_SAFE_TRADES_MIN: u32 = 2;
const WIN_STREAK_SAFE_TRADES_MAX: u32 = 5;
// Fixed stake for the three-second best-entry mode. Risk circuit breakers and
// the one-open-contract guard still apply.
#[cfg(test)]
const MIN_RECOVERY_STAKE: f64 = UNCERTAIN_TRADE_STAKE;
const MATCH_RISK_CAUTION: f64 = 0.30;
const MATCH_RISK_SKIP_INITIAL: f64 = 0.45;
const MATCH_CALIBRATION_WINDOW: usize = 100;
// Permit a proposal once the strategy estimated win probability reaches 65%.
// The provider payout ratio is still logged for operator visibility.
const MIN_ENTRY_WIN_RATE: f64 = 0.65;
const MAX_CONSECUTIVE_SAME_TYPE: u32 = 3;
const TYPE_WEIGHT_ACCU: f64 = 0.25;
const TYPE_WEIGHT_OU: f64 = 0.25;
const TYPE_WEIGHT_MD: f64 = 0.25;
const TYPE_WEIGHT_RF: f64 = 0.25;
const LOSS_WEIGHT_REDUCTION: f64 = 0.40;
const LOSS_PENALTY_DURATION: u32 = 4;
const ABSTENTION_PROBABILITY: f64 = 0.15;
// Internal tick-to-proposal budget. Provider/network round-trip time is
// external to this budget and cannot be reduced by the strategy process.
const MAX_ENTRY_LATENCY_MS: u64 = 50;
// Rotate across ACCU, digit, and Rise/Fall contract families.
const ENABLE_RANDOM_CONTRACT_FAMILIES: bool = true;

// ── API ────────────────────────────────────────────────────
const MAX_REQ_PER_SEC: usize = 8;
const COMMAND_CHANNEL_CAPACITY: usize = 256;
const PROPOSAL_TIMEOUT_SECS: u64 = 3;
const BUY_TIMEOUT_SECS: u64 = 12;
const CONTRACT_UPDATE_TIMEOUT_SECS: u64 = 30;
// Shutdown covers the longest contract this binary can open plus time for a
// final provider update/reconnect; it is independent of the per-update timeout.
const SHUTDOWN_DRAIN_SAFETY_BUFFER_SECS: u64 = 60;
const PROPOSAL_MAX_AGE_SECS: u64 = 3;
const WS_HEARTBEAT_SECS: u64 = 20;
const WS_STALE_TIMEOUT_SECS: u64 = 45;

// ── Strategy Thresholds ────────────────────────────────────
const CONSENSUS_THRESHOLD: f64 = 0.72;
const CONFIDENCE_THRESHOLD: f64 = 0.68;
const MIN_THEORIES_AGREE: usize = 4;

// Model-driven mode: all eight implemented models feed a research ensemble in demo.
// Live trading still requires independently qualified model evidence.
const BASELINE_RANDOM_MODE: bool = false;
// Matches/Diff and Rise/Fall stay disabled during startup. Rise/Fall still
// requires its own signal validation after the shared activation boundary.
const ENABLE_RISE_FALL: bool = true;
const DEFAULT_ADVANCED_FAMILIES_ACTIVATION_DELAY_SECS: u64 = 3 * 60 * 60;

fn rise_fall_is_active(elapsed: Duration, delay: Duration) -> bool {
    ENABLE_RISE_FALL && elapsed >= delay
}

fn advanced_families_activation_delay() -> Duration {
    Duration::from_secs(
        env::var("DERIV_ADVANCED_FAMILIES_ACTIVATION_DELAY_SECS")
            .or_else(|_| env::var("DERIV_RISE_FALL_ACTIVATION_DELAY_SECS"))
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_ADVANCED_FAMILIES_ACTIVATION_DELAY_SECS),
    )
}
// Rise/Fall is only allowed when the directional case is materially stronger
// than the normal accumulator entry. These are abstention thresholds, not a
// promise that a qualifying contract will win.
const RISE_FALL_CONSENSUS_THRESHOLD: f64 = 0.82;
const RISE_FALL_CONFIDENCE_THRESHOLD: f64 = 0.78;
const RISE_FALL_MIN_THEORIES_AGREE: usize = 6;
const DEFAULT_RISE_FALL_DURATION_MINUTES: u16 = 1;
const MAX_RISE_FALL_DURATION_MINUTES: u16 = 60;
fn shutdown_drain_timeout() -> Duration {
    Duration::from_secs(
        rise_fall_duration_minutes() as u64 * 60 + SHUTDOWN_DRAIN_SAFETY_BUFFER_SECS,
    )
}

fn rise_fall_duration_minutes() -> u16 {
    env::var("DERIV_RISE_FALL_DURATION_MINUTES")
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_RISE_FALL_DURATION_MINUTES)
        .clamp(1, MAX_RISE_FALL_DURATION_MINUTES)
}
// A Rise/Fall proposal is permitted only after the same causal rule has met
// the target on earlier, non-overlapping outcomes from this market. This is a
// precision target, never a guarantee about the next contract.
const RISE_FALL_TARGET_WIN_RATE: f64 = 0.98;
const RISE_FALL_MIN_BACKTEST_TRADES: usize = 40;
const RISE_FALL_BACKTEST_TICKS: usize = 1_500;
const RISE_FALL_MODEL_WINDOW: usize = 120;
const RISE_FALL_MIN_WILSON_RATE: f64 = 0.90;
// Cadence fallback: still requires directional evidence, while allowing mostly-neutral scores.
const RELAXED_CONSENSUS: f64 = 0.64;
const RELAXED_CONFIDENCE: f64 = 0.62;
const RELAXED_THEORIES_AGREE: usize = 3;

// ── Execution pacing ───────────────────────────────────────
// Start at most one risk-sized trade per thirty-second window. The cooldown is
// measured from the confirmed contract opening, and the one-open-contract state
// guard prevents overlapping trades.
const TRADE_COOLDOWN_MS: u64 = 30_000;
// Do not let optional market-quality filters leave an otherwise healthy bot
// idle indefinitely. Once this deadline is reached, the next subscribed tick
// uses the ACCU fallback while all account/state circuit breakers remain active.
const MAX_ENTRY_WAIT_SECS: u64 = 180;
const ONE_DOLLAR_LOSS_THRESHOLD: f64 = 1.00;
const ONE_DOLLAR_LOSS_PAUSE_SECS: u64 = 30;
const POST_LOSS_TRADE_PAUSE_SECS: u64 = 10;
const TRADES_PER_REST: u32 = 100;
const TRADE_REST_SECS: u64 = 150;
const SELL_RETRY_MS: u64 = 250;
// If no post-entry tick arrives, make the first ACCU sell attempt promptly.
const INITIAL_SELL_FALLBACK_MS: u64 = 3_000;
// Never jump straight back into a losing market merely to fill an entry slot.
const MARKET_LOSS_COOLDOWN_SECS: i64 = 300;
const MARKET_REPEAT_LOSS_COOLDOWN_SECS: i64 = 600;
// Re-rank only while idle. A small switch margin prevents noisy scores from
// making the bot bounce between otherwise equivalent synthetic indices.
const MARKET_RANK_REFRESH_SECS: u64 = 15;
const MIN_MARKET_QUALITY_SCORE: f64 = 0.45;
const MARKET_SWITCH_SCORE_MARGIN: f64 = 0.03;

// ── Spread/Volatility Filters ──────────────────────────────
const MAX_SPREAD_FRACTION: f64 = 0.0015;
const MAX_TICK_VELOCITY: f64 = 0.03;

// ── Data ───────────────────────────────────────────────────
const TICK_BUFFER_CAP: usize = 2000;
// Hurst validation and trend analysis are not ready before this point.
const MIN_ANALYSIS_TICKS: usize = 30;

// ── Indices ────────────────────────────────────────────────
const AVAILABLE_INDICES: &[&str] = &[
    "R_10", "1HZ10V", "R_25", "1HZ25V", "R_50", "1HZ50V", "R_75", "1HZ75V", "R_100", "1HZ100V",
];

// ── ACCU Proposal Parameters ───────────────────────────────
const ACCU_GROWTH_RATE: f64 = 0.01;

const STRATEGY_WEIGHTS: [f64; 8] = [0.15, 0.10, 0.15, 0.15, 0.10, 0.10, 0.10, 0.15];

const BANNER: &str = r#"
██████╗ ███████╗██████╗ ██╗██╗   ██╗    █████╗  ██████╗ ██████╗██╗   ██╗███╗   ███╗██╗   ██╗██╗      █████╗ ████████╗███████╗
██╔══██╗██╔════╝██╔══██╗██║██║   ██║   ██╔══██╗██╔════╝██╔════╝██║   ██║████╗ ████║██║   ██║██║     ██╔══██╗╚══██╔══╝██╔════╝
██║  ██║█████╗  ██████╔╝██║██║   ██║   ███████║██║     ██║     ██║   ██║██╔████╔██║██║   ██║██║     ███████║   ██║   █████╗
██║  ██║██╔══╝  ██╔══██╗██║╚██╗ ██╔╝   ██╔══██║██║     ██║     ██║   ██║██║╚██╔╝██║██║   ██║██║     ██╔══██║   ██║   ██╔══╝
██████╔╝███████╗██║  ██║██║ ╚████╔╝    ██║  ██║╚██████╗╚██████╗╚██████╔╝██║ ╚═╝ ██║╚██████╔╝███████╗██║  ██║   ██║   ███████╗
╚═════╝ ╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝     ╚═╝  ╚═╝ ╚═════╝ ╚═════╝ ╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═╝   ╚═╝   ╚══════╝

██████╗  ██████╗ ████████╗    24/7 AUTO TRADING BOT v2.0    ████████╗██╗  ██╗███████╗    ██╗   ██╗██╗  ████████╗██╗███╗   ███╗ █████╗ ████████╗███████╗
"#;
