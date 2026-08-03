// ═══════════════════════════════════════════════════════════════════════════
// DATA TYPES
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tick {
    symbol: String,
    #[serde(deserialize_with = "num_serde::flex_f64")]
    quote: f64,
    #[serde(deserialize_with = "num_serde::flex_f64")]
    ask: f64,
    #[serde(deserialize_with = "num_serde::flex_f64")]
    bid: f64,
    #[serde(deserialize_with = "num_serde::flex_u64")]
    epoch: u64,
    #[serde(deserialize_with = "num_serde::flex_u32")]
    pip_size: u32,
}

impl Tick {
    fn mid_price(&self) -> f64 {
        if self.ask > 0.0 && self.bid > 0.0 { (self.ask + self.bid) / 2.0 } else { self.quote }
    }
    fn spread_frac(&self) -> f64 {
        let midpoint = self.mid_price();
        if midpoint > 0.0 { (self.ask - self.bid).abs() / midpoint } else { 0.0 }
    }
    fn last_digit(&self) -> u8 {
        let scale = 10_u64.saturating_pow(self.pip_size.min(12));
        ((self.quote.abs() * scale as f64).round() as u64 % 10) as u8
    }
}


fn digit_bias(ticks: &[Tick]) -> f64 {
    if ticks.is_empty() {
        return 0.5;
    }
    ticks
        .iter()
        .map(|tick| tick.last_digit() as f64 / 9.0)
        .sum::<f64>()
        / ticks.len() as f64
}
#[cfg_attr(not(test), allow(dead_code))]
fn least_likely_digit(ticks: &[Tick]) -> u8 {
    let mut counts = [1u32; 10];
    for tick in ticks {
        counts[tick.last_digit() as usize] += 1;
    }
    counts
        .iter()
        .enumerate()
        .min_by_key(|(_, count)| *count)
        .map(|(digit, _)| digit as u8)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, Default)]
struct MatchRisk {
    score: f64,
    momentum: f64,
    velocity: f64,
    spread: f64,
    kalman_match: f64,
}

#[derive(Debug)]
struct MatchPredictor {
    digits: VecDeque<u8>,
    outcomes: VecDeque<(f64, bool)>,
    pending_prediction: Option<(u8, f64)>,
    skip_threshold: f64,
}

impl MatchPredictor {
    fn new() -> Self {
        Self {
            digits: VecDeque::with_capacity(100),
            outcomes: VecDeque::with_capacity(MATCH_CALIBRATION_WINDOW),
            pending_prediction: None,
            skip_threshold: MATCH_RISK_SKIP_INITIAL,
        }
    }

    fn observe_and_compute(&mut self, tick: &Tick, ticks: &[Tick]) -> MatchRisk {
        let digit = tick.last_digit();
        if let Some((previous, predicted)) = self.pending_prediction.take() {
            self.outcomes.push_back((predicted, digit == previous));
            if self.outcomes.len() > MATCH_CALIBRATION_WINDOW {
                self.outcomes.pop_front();
            }
            self.calibrate();
        }
        self.digits.push_back(digit);
        if self.digits.len() > 100 {
            self.digits.pop_front();
        }
        let risk = self.compute_match_risk(ticks);
        self.pending_prediction = Some((digit, risk.score));
        risk
    }

    fn calibrate(&mut self) {
        if self.outcomes.len() < 20 {
            return;
        }
        let n = self.outcomes.len() as f64;
        let predicted = self.outcomes.iter().map(|(p, _)| p).sum::<f64>() / n;
        let actual = self.outcomes.iter().filter(|(_, matched)| *matched).count() as f64 / n;
        if actual > predicted + 0.02 {
            self.skip_threshold = (self.skip_threshold - 0.02).clamp(0.25, 0.65);
        } else if actual + 0.02 < predicted {
            self.skip_threshold = (self.skip_threshold + 0.02).clamp(0.25, 0.65);
        }
    }

    fn compute_match_risk(&self, ticks: &[Tick]) -> MatchRisk {
        let Some(&current) = self.digits.back() else {
            return MatchRisk::default();
        };
        let last_twenty: Vec<u8> = self.digits.iter().rev().take(20).copied().collect();
        let repetitions = last_twenty.iter().filter(|&&d| d == current).count();
        let mut momentum = (repetitions as f64 / 20.0 * 2.0).clamp(0.0, 1.0);
        if repetitions >= 3 {
            momentum = momentum.max(0.65);
        }
        if self.digits.len() >= 3 && self.digits[self.digits.len() - 3] == current {
            momentum = momentum.max(0.75);
        }
        let raw_velocity = recent_digit_velocity(ticks);
        let velocity: f64 = if raw_velocity < 0.0002 {
            0.8
        } else if raw_velocity > 0.001 {
            0.4
        } else {
            0.5
        };
        let raw_spread = ticks.last().map(Tick::spread_frac).unwrap_or(0.0);
        let spread: f64 = if raw_spread < 0.0003 {
            0.8
        } else if raw_spread > 0.001 {
            0.4
        } else {
            0.5
        };
        let predicted = kalman_digit_prediction(&self.digits);
        let kalman_match = if (predicted - current as f64).abs() <= 0.5 {
            1.0
        } else {
            0.0
        };
        MatchRisk {
            score: (momentum * 0.25 + velocity * 0.20 + spread * 0.15 + kalman_match * 0.40)
                .clamp(0.0, 1.0),
            momentum,
            velocity,
            spread,
            kalman_match,
        }
    }
}

fn recent_digit_velocity(ticks: &[Tick]) -> f64 {
    let sample = &ticks[ticks.len().saturating_sub(20)..];
    if sample.len() < 2 {
        return 0.0;
    }
    sample
        .windows(2)
        .map(|pair| {
            let previous = pair[0].mid_price().abs().max(1e-12);
            (pair[1].mid_price() - pair[0].mid_price()).abs() / previous
        })
        .sum::<f64>()
        / (sample.len() - 1) as f64
}

fn kalman_digit_prediction(digits: &VecDeque<u8>) -> f64 {
    let mut estimate = digits.front().copied().unwrap_or(0) as f64;
    let mut uncertainty = 1.0;
    for &digit in digits.iter().skip(1) {
        uncertainty += 0.05;
        let gain = uncertainty / (uncertainty + 1.0);
        estimate += gain * (digit as f64 - estimate);
        uncertainty *= 1.0 - gain;
    }
    estimate.clamp(0.0, 9.0)
}

fn payout_allows_trade(ask_price: f64, payout: f64, estimated_win_rate: f64) -> Option<f64> {
    if ask_price <= 0.0 || payout <= ask_price || !estimated_win_rate.is_finite() {
        return None;
    }
    let required = ask_price / payout;
    (estimated_win_rate >= required + MIN_PAYOUT_EDGE).then_some(required)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ContractType {
    Accumulator,
    RiseFall,
    OverUnder,
    MatchesDiff,
}

impl fmt::Display for ContractType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Accumulator => "ACCU",
            Self::RiseFall => "RISE_FALL",
            Self::OverUnder => "OVER_UNDER",
            Self::MatchesDiff => "MATCHES_DIFF",
        })
    }
}

#[derive(Debug, Clone, Default)]
struct ContractTypeStats {
    trades: u32,
    wins: u32,
    losses: u32,
    profit: f64,
}

impl ContractTypeStats {
    fn summary(&self, label: &str) -> String {
        let rate = if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64 * 100.0
        };
        format!(
            "{label}:{}t/{}W/{}L({rate:.0}%)",
            self.trades, self.wins, self.losses
        )
    }
}

struct ContractTypeRotator {
    last_type: Option<ContractType>,
    consecutive_same: u32,
    penalty: HashMap<ContractType, u32>,
}
impl ContractTypeRotator {
    fn new() -> Self {
        Self {
            last_type: None,
            consecutive_same: 0,
            penalty: HashMap::new(),
        }
    }
    fn weights(&self) -> [f64; 3] {
        let w = |t, base| {
            if self.penalty.get(&t).copied().unwrap_or(0) > 0 {
                base * (1.0 - LOSS_WEIGHT_REDUCTION)
            } else {
                base
            }
        };
        [
            w(ContractType::Accumulator, TYPE_WEIGHT_ACCU),
            w(ContractType::OverUnder, TYPE_WEIGHT_OU),
            w(ContractType::MatchesDiff, TYPE_WEIGHT_MD),
        ]
    }
    fn next_type(&mut self) -> (ContractType, &'static str) {
        let mut weights = self.weights();
        let exploration = self.consecutive_same >= MAX_CONSECUTIVE_SAME_TYPE;
        if exploration {
            if let Some(last) = self.last_type {
                weights[match last {
                    ContractType::Accumulator => 0,
                    ContractType::RiseFall => 0,
                    ContractType::OverUnder => 1,
                    ContractType::MatchesDiff => 2,
                }] = 0.10;
            }
        }
        let roll = random_unit() * weights.iter().sum::<f64>();
        let selected = if roll < weights[0] {
            ContractType::Accumulator
        } else if roll < weights[0] + weights[1] {
            ContractType::OverUnder
        } else {
            ContractType::MatchesDiff
        };
        if self.last_type == Some(selected) {
            self.consecutive_same += 1;
        } else {
            self.last_type = Some(selected);
            self.consecutive_same = 1;
        }
        for value in self.penalty.values_mut() {
            *value = value.saturating_sub(1);
        }
        (
            selected,
            if exploration {
                "exploration_constraint"
            } else {
                "weighted_random"
            },
        )
    }
    fn record_result(&mut self, t: ContractType, win: bool) {
        if !win {
            self.penalty.insert(t, LOSS_PENALTY_DURATION);
        }
    }
}

#[derive(Debug, Clone)]
struct PendingBuyInfo {
    symbol: String,
    proposed_price: f64,
    entry_tick_epoch: u64,
    contract_type: ContractType,
}
fn random_unit() -> f64 {
    OsRng.next_u64() as f64 / u64::MAX as f64
}
fn random_contract_type() -> ContractType {
    match OsRng.next_u64() % 3 {
        0 => ContractType::Accumulator,
        1 => ContractType::OverUnder,
        _ => ContractType::MatchesDiff,
    }
}
#[cfg(test)]
fn varied_stake(base: f64, variation: f64) -> f64 {
    ((base * (1.0 + (random_unit() * 2.0 - 1.0) * variation)) * 100.0).round() / 100.0
}
/// Deriv rejects proposal stakes with more than two decimal places.
/// Normalize at the API boundary so every proposal path obeys that rule,
/// including continuously calculated signal-risk stake amounts.
fn deriv_stake(amount: f64) -> f64 {
    (amount * 100.0).round() / 100.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Money(i64);

impl Money {
    fn from_api(value: f64) -> Option<Self> {
        if !value.is_finite() || value < 0.0 { return None; }
        Some(Self((value * 100.0).round() as i64))
    }
    fn api_amount(self) -> f64 { self.0 as f64 / 100.0 }
}

/// Use the configured maximum stake when signal risk is at most 60%.
/// A qualified signal above 60% risk is limited to one dollar.
fn stake_for_signal_risk(session_cap: f64, signal_risk: f64) -> f64 {
    let cap = Money::from_api(session_cap).unwrap_or(Money(MIN_STAKE_MINOR));
    let risk = signal_risk.clamp(0.0, 1.0);
    let scaled = ((cap.0 - MIN_STAKE_MINOR).max(0) as f64 * (1.0 - risk)).round() as i64;
    Money((MIN_STAKE_MINOR + scaled).clamp(MIN_STAKE_MINOR, HARD_MAX_STAKE_MINOR)).api_amount()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TradeState {
    Idle,
    CollectingTicks,
    Analysing,
    ProposalPending,
    ProposalReady,
    BuyPending,
    ContractOpen,
    Settling,
    CooldownOrReanalyse,
}

impl TradeState {
    fn can_transition(self, next: Self) -> bool {
        use TradeState::*;
        matches!((self, next),
            (Idle, CollectingTicks) | (CollectingTicks, Analysing)
                | (Analysing, CollectingTicks) | (Analysing, ProposalPending)
                | (ProposalPending, ProposalReady) | (ProposalPending, CollectingTicks)
                | (ProposalReady, BuyPending) | (ProposalReady, CollectingTicks)
                | (BuyPending, ContractOpen) | (BuyPending, CollectingTicks)
                | (ContractOpen, Settling) | (Settling, CooldownOrReanalyse)
                | (CooldownOrReanalyse, CollectingTicks))
    }
    fn transition(&mut self, next: Self) -> bool {
        let from = *self;
        let accepted = self.can_transition(next);
        journal_state_transition(from, next, accepted);
        if accepted {
            log::debug!("Trade state {:?} -> {:?}", self, next);
            *self = next;
            true
        } else {
            log::warn!("Rejected invalid trade-state transition {:?} -> {:?}", self, next);
            false
        }
    }
    fn execution_pending(self) -> bool {
        matches!(self, Self::ProposalPending | Self::ProposalReady | Self::BuyPending | Self::ContractOpen | Self::Settling)
    }
}

#[derive(Debug, Clone)]
struct TradeResult { profit_loss: f64 }
impl TradeResult { fn is_profitable(&self) -> bool { self.profit_loss > 0.0 } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TradingMode { Demo, Live, Cooldown, Stopped }
impl fmt::Display for TradingMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self { Self::Demo => "DEMO", Self::Live => "LIVE", Self::Cooldown => "COOLDOWN", Self::Stopped => "STOPPED" })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OptionsAccount {
    #[serde(alias = "accountId", alias = "account_id")]
    account_id: String,
    #[serde(default, alias = "accountType", alias = "account_type")]
    account_type: Option<String>,
    #[serde(default, deserialize_with = "num_serde::flex_f64_opt")]
    balance: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default, alias = "isVirtual", alias = "is_virtual")]
    is_virtual: Option<bool>,
    #[serde(default, alias = "isDisabled", alias = "is_disabled")]
    is_disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct OptionsAccountsResponse {
    #[serde(default)]
    data: Option<Vec<OptionsAccount>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OtpResponse {
    // Top-level wss field
    #[serde(default, alias = "wss", alias = "ws_url")]
    wss: Option<String>,
    // Nested data.wss or data.url (some API versions nest the URL)
    #[serde(default)]
    data: Option<OtpData>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct OtpData {
    #[serde(default, alias = "wss", alias = "websocket_url")]
    wss: Option<String>,
    #[serde(default, alias = "url")]
    url: Option<String>,
}
