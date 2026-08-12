// ═══════════════════════════════════════════════════════════════════════════
// 8 MATHEMATICAL STRATEGIES
// ═══════════════════════════════════════════════════════════════════════════

fn fluid_dynamics(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 3 {
        return 0.5;
    }
    let (mut up, mut dn) = (0.0f64, 0.0f64);
    for i in 1..n {
        let prev = ticks[i - 1].mid_price();
        let curr = ticks[i].mid_price();
        if prev <= 0.0 {
            continue;
        }
        let delta = curr - prev;
        let vel = delta / prev;
        let turb = ticks[i].spread_frac().min(0.1);
        let p = vel.abs() * (1.0 + turb * 20.0);
        if delta > 0.0 {
            up += p;
        } else {
            dn += p;
        }
    }
    let tot = up + dn;
    if tot < 1e-12 {
        0.5
    } else {
        sigmoid((up - dn) / tot * 4.0)
    }
}

fn chaos_theory(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 10 {
        return 0.5;
    }
    let mut divs = Vec::with_capacity(n - 2);
    for i in 1..n - 1 {
        let p0 = ticks[i - 1].mid_price().max(1e-10);
        let p1 = ticks[i].mid_price().max(1e-10);
        let p2 = ticks[i + 1].mid_price().max(1e-10);
        let d0 = ((p1 / p0).ln()).abs();
        let d1 = ((p2 / p1).ln()).abs();
        if d0 > 1e-10 && d1 > 1e-10 {
            divs.push((d1 / d0).ln());
        }
    }
    if divs.is_empty() {
        return 0.5;
    }
    let lyap = divs.iter().sum::<f64>() / divs.len() as f64;
    let first = ticks[0].mid_price();
    let last = ticks[n - 1].mid_price();
    let trend = if first > 0.0 {
        (last - first) / first
    } else {
        0.0
    };
    if lyap > 0.01 {
        sigmoid(trend * 15.0)
    } else if lyap < -0.01 {
        sigmoid(-trend * 15.0)
    } else {
        0.5
    }
}

fn mean_reversion(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 5 {
        return 0.5;
    }
    let w = 20.min(n);
    let recent: Vec<f64> = ticks[n - w..].iter().map(|t| t.mid_price()).collect();
    let avg = mean(&recent);
    let sd = std_dev(&recent, avg).max(1e-10);
    let cur = recent.last().copied().unwrap_or(avg);
    let z = (cur - avg) / sd;
    sigmoid(-z * 0.7)
}

fn trend_analysis(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 30 {
        return 0.5;
    }
    let prices: Vec<f64> = ticks.iter().map(|t| t.mid_price()).collect();
    let fast = ema(&prices, 12);
    let slow = ema(&prices, 26);
    let macd = fast - slow;
    let cur = prices.last().copied().unwrap_or(1.0);
    let nm = if cur > 0.0 { macd / cur } else { 0.0 };
    sigmoid(nm * 200.0)
}

fn multi_timeframe(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 5 {
        return 0.5;
    }
    let windows = [5usize, 10, 20, 40];
    let mut scores = Vec::new();
    for &w in &windows {
        if n >= w {
            let s = &ticks[n - w..];
            let fp = s[0].mid_price();
            let lp = s.last().map(|t| t.mid_price()).unwrap_or(fp);
            if fp > 0.0 {
                scores.push(sigmoid((lp - fp) / fp * 25.0));
            }
        }
    }
    if scores.is_empty() {
        0.5
    } else {
        scores.iter().sum::<f64>() / scores.len() as f64
    }
}

fn nash_equilibrium(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 5 {
        return 0.5;
    }
    let (mut up, mut dn) = (0usize, 0usize);
    for i in 1..n {
        let p = ticks[i - 1].mid_price();
        let c = ticks[i].mid_price();
        if c > p {
            up += 1;
        } else if c < p {
            dn += 1;
        }
    }
    let tot = (up + dn).max(1);
    let pu = up as f64 / tot as f64;
    let pd = dn as f64 / tot as f64;
    let call_ev = pu - pd;
    let put_ev = pd - pu;
    if call_ev + put_ev < 1e-12 {
        return 0.5;
    }
    call_ev.max(0.0) / (call_ev.max(0.0) + put_ev.max(0.0) + 1e-12)
}

fn tick_velocity(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 20 {
        return 0.5;
    }
    let vel = |s: &[Tick]| -> f64 {
        let mut tot = 0.0;
        for i in 1..s.len() {
            tot += (s[i].mid_price() - s[i - 1].mid_price()).abs();
        }
        if s.len() > 1 {
            tot / s.len() as f64
        } else {
            0.0
        }
    };
    let sw = 10.min(n);
    let lw = 50.min(n);
    let sv = vel(&ticks[n - sw..]);
    let lv = vel(&ticks[n - lw..]);
    if lv < 1e-12 {
        return 0.5;
    }
    let ratio = sv / lv;
    let ss = &ticks[n - sw..];
    let first = ss[0].mid_price();
    let last = ss[ss.len() - 1].mid_price();
    let trend = if first > 0.0 {
        (last - first) / first
    } else {
        0.0
    };
    if ratio > 1.5 {
        sigmoid(trend * 30.0)
    } else {
        sigmoid(trend * 10.0)
    }
}

fn volume_surge(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 15 {
        return 0.5;
    }
    let proxy = |t0: &Tick, t1: &Tick| -> f64 {
        let delta = (t1.mid_price() - t0.mid_price()).abs();
        let spread = t1.spread_frac().max(1e-8);
        delta / spread
    };
    let ln = n.min(50);
    let mut sum = 0.0;
    for i in (n - ln + 1)..n {
        sum += proxy(&ticks[i - 1], &ticks[i]);
    }
    let avg = sum / (ln.saturating_sub(1)) as f64;
    if avg < 1e-12 {
        return 0.5;
    }
    let cur = proxy(&ticks[n - 2], &ticks[n - 1]);
    let ratio = cur / avg;
    let recent_delta = ticks[n - 1].mid_price() - ticks[n - 2].mid_price();
    let bias = if recent_delta > 0.0 {
        0.15
    } else if recent_delta < 0.0 {
        -0.15
    } else {
        0.0
    };
    if ratio > 2.0 {
        (0.5 + bias * ratio.min(5.0) / 2.5).clamp(0.0, 1.0)
    } else if ratio > 1.2 {
        (0.5 + bias * 0.5).clamp(0.0, 1.0)
    } else {
        0.5
    }
}

fn individual_model_scores(ticks: &[Tick]) -> [f64; 8] {
    [
        fluid_dynamics(ticks),
        chaos_theory(ticks),
        mean_reversion(ticks),
        trend_analysis(ticks),
        multi_timeframe(ticks),
        nash_equilibrium(ticks),
        tick_velocity(ticks),
        volume_surge(ticks),
    ]
}

fn compute_consensus(ticks: &[Tick], weights: &[f64; 8]) -> (f64, f64) {
    let scores = individual_model_scores(ticks);
    let tw: f64 = weights.iter().sum();
    let cs: f64 = scores
        .iter()
        .zip(weights.iter())
        .map(|(s, w)| s * w)
        .sum::<f64>()
        / tw;
    let var: f64 = scores
        .iter()
        .zip(weights.iter())
        .map(|(s, w)| w * (s - cs).powi(2))
        .sum::<f64>()
        / tw;
    let conf = (1.0 - var.sqrt() * 2.4).clamp(0.0, 1.0);
    (cs, conf)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsensusPath {
    Strong,
    Relaxed,
}

fn theory_agreement(scores: &[f64; 8]) -> (usize, usize, usize) {
    // Scores in the neutral band, including exactly 0.5, express no direction.
    let bullish = scores.iter().filter(|&&score| score > 0.55).count();
    let bearish = scores.iter().filter(|&&score| score < 0.45).count();
    (bullish, bearish, bullish.max(bearish))
}

fn directional_agreement(scores: &[f64; 8], bullish_direction: bool) -> usize {
    if bullish_direction {
        scores.iter().filter(|&&score| score > 0.55).count()
    } else {
        scores.iter().filter(|&&score| score < 0.45).count()
    }
}

fn consensus_rejection_reason(
    path: ConsensusPath,
    agree: usize,
    consensus: f64,
    confidence: f64,
    min_agree: usize,
    consensus_threshold: f64,
    confidence_threshold: f64,
) -> Option<String> {
    let path_name = match path {
        ConsensusPath::Strong => "strong",
        ConsensusPath::Relaxed => "fallback",
    };
    if agree < min_agree {
        return Some(format!(
            "{path_name} consensus agree={agree}/8 < {min_agree}"
        ));
    }
    if confidence < confidence_threshold {
        return Some(format!(
            "{path_name} confidence {confidence:.4} < {confidence_threshold:.4}"
        ));
    }
    if !(consensus >= consensus_threshold || consensus <= 1.0 - consensus_threshold) {
        return Some(format!(
            "{path_name} consensus {consensus:.4} inside ({:.4}, {consensus_threshold:.4})",
            1.0 - consensus_threshold
        ));
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct RiseFallSignal {
    bullish: bool,
    consensus: f64,
    confidence: f64,
    agreement: usize,
    hurst: f64,
    historical_wins: usize,
    historical_trades: usize,
    historical_win_rate: f64,
    wilson_lower_rate: f64,
}

// Uses only information available at the decision tick. Hurst measures
// persistence, not direction, so low Hurst is not treated as PUT evidence.
fn raw_rise_fall_signal(ticks: &[Tick]) -> Option<(bool, f64, f64, usize, f64)> {
    if ticks.len() < RISE_FALL_MODEL_WINDOW {
        return None;
    }
    let sample = &ticks[ticks.len() - RISE_FALL_MODEL_WINDOW..];
    if sample.last()?.spread_frac() > MAX_SPREAD_FRACTION
        || tick_volatility(sample) > MAX_TICK_VELOCITY
        || is_reversal_pattern(sample)
    {
        return None;
    }
    let scores = [
        fluid_dynamics(sample),
        chaos_theory(sample),
        mean_reversion(sample),
        trend_analysis(sample),
        multi_timeframe(sample),
        nash_equilibrium(sample),
        tick_velocity(sample),
        volume_surge(sample),
    ];
    let (consensus, confidence) = compute_consensus(sample, &STRATEGY_WEIGHTS);
    let bullish = consensus > 0.5;
    let agreement = directional_agreement(&scores, bullish);
    let current_price = sample.last()?.mid_price();
    let kalman_delta = if current_price > 0.0 {
        (kalman_predict(sample) - current_price) / current_price
    } else {
        0.0
    };
    let prices: Vec<f64> = sample.iter().map(Tick::mid_price).collect();
    let hurst = hurst_exponent(&prices);
    let kalman_agrees = (bullish && kalman_delta > 0.0) || (!bullish && kalman_delta < 0.0);
    let horizon_agrees = [5usize, 10, 20].iter().all(|&window| {
        let first = sample[sample.len() - window].mid_price();
        let last = sample.last().map(Tick::mid_price).unwrap_or(first);
        if bullish {
            last > first
        } else {
            last < first
        }
    });
    let exceptional = agreement >= RISE_FALL_MIN_THEORIES_AGREE
        && confidence >= RISE_FALL_CONFIDENCE_THRESHOLD
        && (consensus >= RISE_FALL_CONSENSUS_THRESHOLD
            || consensus <= 1.0 - RISE_FALL_CONSENSUS_THRESHOLD)
        && kalman_agrees
        && hurst >= 0.56
        && horizon_agrees;
    exceptional.then_some((bullish, consensus, confidence, agreement, hurst))
}

fn wilson_lower_bound(wins: usize, trades: usize) -> f64 {
    if trades == 0 {
        return 0.0;
    }
    let z: f64 = 1.96;
    let n = trades as f64;
    let p = wins as f64 / n;
    let denominator = 1.0 + z * z / n;
    let centre = p + z * z / (2.0 * n);
    let margin = z * ((p * (1.0 - p) + z * z / (4.0 * n)) / n).sqrt();
    ((centre - margin) / denominator).clamp(0.0, 1.0)
}

// Walk-forward validation prevents look-ahead: every historical decision sees
// only its prefix, and its outcome ends before the current decision.
fn validated_rise_fall_signal(ticks: &[Tick]) -> Option<RiseFallSignal> {
    let (bullish, consensus, confidence, agreement, hurst) = raw_rise_fall_signal(ticks)?;
    let horizon_seconds = u64::from(rise_fall_duration_minutes()) * 60;
    let start = ticks
        .len()
        .saturating_sub(RISE_FALL_BACKTEST_TICKS)
        .max(RISE_FALL_MODEL_WINDOW);
    let mut wins = 0usize;
    let mut trades = 0usize;
    for decision_end in start..ticks.len() {
        let prefix_start = decision_end.saturating_sub(RISE_FALL_MODEL_WINDOW);
        let Some((past_bullish, ..)) = raw_rise_fall_signal(&ticks[prefix_start..decision_end])
        else {
            continue;
        };
        let entry = ticks[decision_end - 1].mid_price();
        let expiry_epoch = ticks[decision_end - 1].epoch.saturating_add(horizon_seconds);
        let Some(exit_tick) = ticks[decision_end..]
            .iter()
            .find(|tick| tick.epoch >= expiry_epoch)
        else {
            continue;
        };
        let exit = exit_tick.mid_price();
        wins += usize::from(if past_bullish {
            exit > entry
        } else {
            exit < entry
        });
        trades += 1;
    }
    if trades < RISE_FALL_MIN_BACKTEST_TRADES {
        return None;
    }
    let historical_win_rate = wins as f64 / trades as f64;
    let wilson_lower_rate = wilson_lower_bound(wins, trades);
    if historical_win_rate < RISE_FALL_TARGET_WIN_RATE
        || wilson_lower_rate < RISE_FALL_MIN_WILSON_RATE
    {
        return None;
    }
    Some(RiseFallSignal {
        bullish,
        consensus,
        confidence,
        agreement,
        hurst,
        historical_wins: wins,
        historical_trades: trades,
        historical_win_rate,
        wilson_lower_rate,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// KALMAN + HURST + FILTERS
// ═══════════════════════════════════════════════════════════════════════════

fn kalman_predict(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 5 {
        return ticks.last().map(|t| t.mid_price()).unwrap_or(0.0);
    }
    let prices: Vec<f64> = ticks.iter().map(|t| t.mid_price()).collect();
    let dt = 1.0;
    let mut p_est = prices[0];
    let mut v_est = if n > 1 { prices[1] - prices[0] } else { 0.0 };
    let mut p_cov = 1.0_f64;
    let mut v_cov = 0.1_f64;
    let proc_noise = 0.001;
    let obs_noise = 0.005;

    for price in prices.iter().take(n).skip(1) {
        p_est += v_est * dt;
        p_cov += v_cov * dt * dt + proc_noise;
        v_cov += proc_noise;
        let k = p_cov / (p_cov + obs_noise);
        let residual = *price - p_est;
        p_est += k * residual;
        v_est += k * residual / dt.max(1e-6);
        p_cov *= 1.0 - k;
        v_cov *= 1.0 - k * 0.1;
    }
    p_est + v_est * dt
}

fn hurst_exponent(prices: &[f64]) -> f64 {
    let n = prices.len();
    if n < 30 {
        return 0.5;
    }
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let scales = [5usize, 10, 20, 30];
    let mut log_rs = Vec::new();
    let mut log_n = Vec::new();

    for &scale in &scales {
        if scale >= returns.len() {
            continue;
        }
        let chunks: Vec<&[f64]> = returns.chunks(scale).collect();
        let mut rs_sum = 0.0;
        for chunk in &chunks {
            if chunk.len() < 3 {
                continue;
            }
            let mean = chunk.iter().sum::<f64>() / chunk.len() as f64;
            let mut cum_dev = 0.0;
            let mut max_cum = f64::MIN;
            let mut min_cum = f64::MAX;
            for &r in *chunk {
                cum_dev += r - mean;
                max_cum = max_cum.max(cum_dev);
                min_cum = min_cum.min(cum_dev);
            }
            let variance =
                chunk.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / chunk.len() as f64;
            let std_dev = variance.sqrt().max(1e-12);
            rs_sum += (max_cum - min_cum) / std_dev;
        }
        let avg_rs = rs_sum / chunks.len() as f64;
        log_rs.push(avg_rs.ln());
        log_n.push((scale as f64).ln());
    }

    if log_rs.len() < 2 {
        return 0.5;
    }
    let sx: f64 = log_n.iter().sum();
    let sy: f64 = log_rs.iter().sum();
    let sxy: f64 = log_n.iter().zip(log_rs.iter()).map(|(x, y)| x * y).sum();
    let sx2: f64 = log_n.iter().map(|x| x * x).sum();
    let m = log_n.len() as f64;
    let h = (m * sxy - sx * sy) / (m * sx2 - sx * sx).max(1e-12);
    h.clamp(0.1, 1.0)
}

fn tick_volatility(ticks: &[Tick]) -> f64 {
    let n = ticks.len();
    if n < 5 {
        return 0.0;
    }
    let mut max_delta = 0.0f64;
    for i in 1..n {
        let prev = ticks[i - 1].mid_price();
        let curr = ticks[i].mid_price();
        if prev > 0.0 {
            let delta = ((curr - prev) / prev).abs();
            max_delta = max_delta.max(delta);
        }
    }
    max_delta
}

fn random_market_risk_reason(ticks: &[Tick]) -> Option<String> {
    if ticks.len() < MIN_ANALYSIS_TICKS {
        return Some(format!(
            "warmup ticks={}/{}",
            ticks.len(),
            MIN_ANALYSIS_TICKS
        ));
    }

    let recent = &ticks[ticks.len().saturating_sub(120)..];
    let spread = recent
        .last()
        .map(Tick::spread_frac)
        .unwrap_or(f64::INFINITY);
    if !spread.is_finite() || spread > MAX_SPREAD_FRACTION {
        return Some(format!("spread {:.6} > {:.6}", spread, MAX_SPREAD_FRACTION));
    }

    let volatility = tick_volatility(recent);
    if !volatility.is_finite() || volatility > MAX_TICK_VELOCITY {
        return Some(format!(
            "volatility {:.6} > {:.6}",
            volatility, MAX_TICK_VELOCITY
        ));
    }

    if is_reversal_pattern(recent) {
        return Some("reversal pattern detected".to_string());
    }

    None
}

fn is_reversal_pattern(ticks: &[Tick]) -> bool {
    let n = ticks.len();
    if n < 4 {
        return false;
    }
    // Tick data has no OHLC/wick information. Detect a statistically sharp
    // direction flip instead of manufacturing a candle from two quotes.
    let prices: Vec<f64> = ticks[n.saturating_sub(20)..]
        .iter()
        .map(Tick::mid_price)
        .collect();
    let returns: Vec<f64> = prices
        .windows(2)
        .filter_map(|w| (w[0] > 0.0).then_some((w[1] - w[0]) / w[0]))
        .collect();
    if returns.len() < 3 {
        return false;
    }
    let last = returns[returns.len() - 1];
    let previous = returns[returns.len() - 2];
    let baseline = mean(&returns[..returns.len() - 1]);
    let sigma = std_dev(&returns[..returns.len() - 1], baseline).max(1e-12);
    last.signum() != previous.signum() && last.abs() >= sigma * 1.5
}

fn session_quality_for_hour(hour: u32) -> f64 {
    for &(start, end) in BEST_HOURS {
        if hour >= start && hour <= end {
            return 1.0;
        }
    }
    for &(start, end) in WORST_HOURS {
        if (start <= end && hour >= start && hour <= end)
            || (start > end && (hour >= start || hour <= end))
        {
            return 0.3;
        }
    }
    0.7
}

fn session_quality() -> f64 {
    let hour = Utc::now()
        .format("%H")
        .to_string()
        .parse::<u32>()
        .unwrap_or(12);
    session_quality_for_hour(hour)
}

fn session_allows_entry(quality: f64) -> bool {
    // Synthetic indices trade continuously. Session quality is retained as
    // telemetry, but must not turn a 24/7 strategy into a hard UTC-hours lock.
    quality.is_finite() && quality > 0.0
}
