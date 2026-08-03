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
const HARD_MAX_STAKE_MINOR: i64 = 1_000; // 10.00, never exceeded
const BALANCE_RISK_BPS: i64 = 100; // 1% of verified balance
const SESSION_LOSS_LIMIT_BPS: i64 = 2_000; // 20% of verified balance
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
const MIN_PAYOUT_EDGE: f64 = 0.005;
const MAX_CONSECUTIVE_SAME_TYPE: u32 = 3;
const TYPE_WEIGHT_ACCU: f64 = 0.35;
const TYPE_WEIGHT_OU: f64 = 0.30;
const TYPE_WEIGHT_MD: f64 = 0.35;
const LOSS_WEIGHT_REDUCTION: f64 = 0.40;
const LOSS_PENALTY_DURATION: u32 = 4;
const ABSTENTION_PROBABILITY: f64 = 0.15;
const MAX_ENTRY_LATENCY_MS: u64 = 200;
const SIGNAL_ENTRY_DELAY_MS_MIN: u64 = 5;
const SIGNAL_ENTRY_DELAY_MS_MAX: u64 = 20;
// Trade ACCU continuously. The digit and Rise/Fall implementations remain
// available in the codebase, but are excluded from live contract selection.
const ENABLE_RANDOM_CONTRACT_FAMILIES: bool = false;

// ── API ────────────────────────────────────────────────────
const MAX_REQ_PER_SEC: usize = 8;
const COMMAND_CHANNEL_CAPACITY: usize = 256;
const PROPOSAL_TIMEOUT_SECS: u64 = 3;
const BUY_TIMEOUT_SECS: u64 = 12;
const CONTRACT_UPDATE_TIMEOUT_SECS: u64 = 30;
const PROPOSAL_MAX_AGE_SECS: u64 = 3;
const WS_HEARTBEAT_SECS: u64 = 20;
const WS_STALE_TIMEOUT_SECS: u64 = 45;

// ── Strategy Thresholds ────────────────────────────────────
const CONSENSUS_THRESHOLD: f64 = 0.72;
const CONFIDENCE_THRESHOLD: f64 = 0.68;
const MIN_THEORIES_AGREE: usize = 4;

// Continuous entry mode: request the next supported contract as soon as the
// previous contract settles and the selected market produces another tick.
// Account-level stops and the one-open-contract state guard remain.
const BASELINE_RANDOM_MODE: bool = false;
// Keep live entries limited to ACCU and digit contracts for now.
// Rise/Fall remains implemented, but cannot be selected while this is false.
const ENABLE_RISE_FALL: bool = false;
// Rise/Fall is only allowed when the directional case is materially stronger
// than the normal accumulator entry. These are abstention thresholds, not a
// promise that a qualifying contract will win.
const RISE_FALL_CONSENSUS_THRESHOLD: f64 = 0.82;
const RISE_FALL_CONFIDENCE_THRESHOLD: f64 = 0.78;
const RISE_FALL_MIN_THEORIES_AGREE: usize = 6;
const RISE_FALL_DURATION_TICKS: u8 = 5;
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
// Start at most one risk-sized trade per three-minute window. The cooldown is
// measured from the confirmed contract opening, and the one-open-contract state
// guard prevents overlapping trades.
const TRADE_COOLDOWN_MS: u64 = 180_000;
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

// ── Spread/Volatility Filters ──────────────────────────────
const MAX_SPREAD_FRACTION: f64 = 0.0015;
const MAX_TICK_VELOCITY: f64 = 0.03;

// ── Data ───────────────────────────────────────────────────
const TICK_BUFFER_CAP: usize = 2000;
// Hurst validation and trend analysis are not ready before this point.
const MIN_ANALYSIS_TICKS: usize = 30;
const MARKET_RANK_REFRESH_SECS: u64 = 3;
// Do not select the least-bad member of a poor market pool. A candidate must
// have acceptable absolute quality, and a new market must beat the current
// market by enough to avoid noisy tick-by-tick switching.
const MIN_MARKET_QUALITY_SCORE: f64 = 0.48;
const MARKET_SWITCH_SCORE_MARGIN: f64 = 0.025;

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
