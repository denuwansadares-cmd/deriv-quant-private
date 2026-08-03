// ═══════════════════════════════════════════════════════════════════════════
// HISTORY LOADER
// ═══════════════════════════════════════════════════════════════════════════

struct HistoryLoader {
    buffers: HashMap<String, RingBuffer<Tick>>,
    #[cfg_attr(not(test), allow(dead_code))]
    loaded_count: usize,
}

impl HistoryLoader {
    fn load(history_path: &str, symbols: &[String], capacity: usize) -> Result<Self> {
        let base = expand_tilde(history_path);
        if !base.exists() {
            log::warn!(
                "History path '{}' not found – starting empty",
                base.display()
            );
            let buffers: HashMap<_, _> = symbols
                .iter()
                .map(|s| (s.clone(), RingBuffer::new(capacity)))
                .collect();
            return Ok(Self {
                buffers,
                loaded_count: 0,
            });
        }

        let mut buffers = HashMap::new();
        let mut total = 0usize;

        for sym in symbols {
            let mut buffer = RingBuffer::new(capacity);
            let csv_path = base.join(format!("{}.csv", sym));
            if csv_path.exists() {
                if let Ok(count) = load_csv_file(&csv_path, sym, &mut buffer, capacity) {
                    total += count;
                }
            } else {
                if let Ok(entries) = fs::read_dir(&base) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(sym.as_str()) && name.ends_with(".csv") {
                            if let Ok(count) =
                                load_csv_file(&entry.path(), sym, &mut buffer, capacity)
                            {
                                total += count;
                            }
                            break;
                        }
                    }
                }
            }
            buffers.insert(sym.clone(), buffer);
        }
        log::info!("Loaded {} ticks across {} symbols", total, buffers.len());
        Ok(Self {
            buffers,
            loaded_count: total,
        })
    }

    fn get_buffer(&self, symbol: &str) -> Option<&RingBuffer<Tick>> {
        self.buffers.get(symbol)
    }

    fn push_tick(&mut self, tick: &Tick) {
        self.buffers
            .entry(tick.symbol.clone())
            .or_insert_with(|| RingBuffer::new(TICK_BUFFER_CAP))
            .push(tick.clone());
    }
}

fn load_csv_file(
    path: &Path,
    symbol: &str,
    buffer: &mut RingBuffer<Tick>,
    capacity: usize,
) -> Result<usize> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(path)
        .with_context(|| format!("Failed to open CSV: {}", path.display()))?;

    let mut records = reader.records();
    let Some(first) = records.next().transpose()? else {
        return Ok(0);
    };
    let headerless = first.get(1).and_then(|v| v.parse::<u64>().ok()).is_some()
        && first.get(2).and_then(|v| v.parse::<f64>().ok()).is_some();

    let find_column = |names: &[&str]| {
        first.iter().position(|value| {
            let value = value.trim().to_ascii_lowercase();
            names.iter().any(|name| value == *name)
        })
    };
    let (col_epoch, col_quote, col_ask, col_bid, col_pip) = if headerless {
        (Some(1), Some(2), None, None, None)
    } else {
        (
            find_column(&["epoch", "timestamp", "time", "ts"]),
            find_column(&["quote", "price", "close", "value"]),
            find_column(&["ask", "offer"]),
            find_column(&["bid"]),
            find_column(&["pip_size", "pip"]),
        )
    };
    if col_epoch.is_none() || col_quote.is_none() {
        anyhow::bail!("CSV {} has no epoch/quote columns", path.display());
    }

    let parse_record = |record: &csv::StringRecord| -> Option<(u64, f64, f64, f64, u32)> {
        let epoch = col_epoch.and_then(|i| record.get(i))?.parse().ok()?;
        let quote: f64 = col_quote.and_then(|i| record.get(i))?.parse().ok()?;
        if !quote.is_finite() || quote <= 0.0 {
            return None;
        }
        let ask = col_ask
            .and_then(|i| record.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(quote);
        let bid = col_bid
            .and_then(|i| record.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(quote);
        let pip_size = col_pip
            .and_then(|i| record.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        Some((epoch, quote, ask, bid, pip_size))
    };

    // Keep only the newest ticks while scanning. The downloader files can be
    // hundreds of MB in total; collecting and sorting every historical row
    // delayed startup long enough for the one-shot Connected event to be lost.
    let mut parsed = BTreeMap::new();
    let mut retain_tick = |tick: (u64, f64, f64, f64, u32)| {
        parsed.insert(tick.0, tick);
        while parsed.len() > capacity {
            parsed.pop_first();
        }
    };
    if headerless {
        if let Some(tick) = parse_record(&first) {
            retain_tick(tick);
        }
    }
    for result in records {
        if let Some(tick) = parse_record(&result?) {
            retain_tick(tick);
        }
    }
    for (_, (epoch, quote, ask, bid, pip_size)) in parsed {
        buffer.push(Tick {
            symbol: symbol.to_string(),
            quote,
            ask,
            bid,
            epoch,
            pip_size,
        });
    }
    Ok(buffer.len())
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        let home = env::var("USERPROFILE")
            .or_else(|_| env::var("HOME"))
            .unwrap_or_default();
        if path == "~" {
            PathBuf::from(home)
        } else {
            PathBuf::from(home).join(&path[2..])
        }
    } else {
        PathBuf::from(path)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// INDEX ROTATOR
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct HistoricalMarketScore {
    symbol: String,
    score: f64,
    observations: usize,
    recent_rms: f64,
    baseline_rms: f64,
    tail_rate: f64,
    average_spread: f64,
    chaos_edge: f64,
    theory_confidence: f64,
}

fn historical_market_score(
    symbol: &str,
    ticks: &[Tick],
    now_epoch: u64,
) -> Option<HistoricalMarketScore> {
    const MIN_SCORE_TICKS: usize = 120;
    const RECENT_RETURNS: usize = 60;
    const MAX_SCORE_TICKS: usize = 600;
    if ticks.len() < MIN_SCORE_TICKS {
        return None;
    }
    let latest_epoch = ticks.last()?.epoch;
    if latest_epoch == 0 || now_epoch.saturating_sub(latest_epoch) > 10 {
        return None;
    }
    let sample = &ticks[ticks.len().saturating_sub(MAX_SCORE_TICKS)..];
    let returns: Vec<f64> = sample
        .windows(2)
        .filter_map(|pair| {
            let previous = pair[0].mid_price();
            let current = pair[1].mid_price();
            (previous > 0.0 && current > 0.0).then_some((current / previous).ln())
        })
        .filter(|value| value.is_finite())
        .collect();
    if returns.len() < MIN_SCORE_TICKS - 1 || returns.len() <= RECENT_RETURNS {
        return None;
    }
    let split = returns.len() - RECENT_RETURNS;
    let baseline = &returns[..split];
    let recent = &returns[split..];
    let rms =
        |values: &[f64]| (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt();
    let baseline_rms = rms(baseline).max(1e-10);
    let recent_rms = rms(recent);
    let abs_mean = baseline.iter().map(|v| v.abs()).sum::<f64>() / baseline.len() as f64;
    let abs_sd = (baseline
        .iter()
        .map(|v| (v.abs() - abs_mean).powi(2))
        .sum::<f64>()
        / baseline.len() as f64)
        .sqrt();
    let jump_threshold = abs_mean + 3.0 * abs_sd.max(1e-10);
    let tail_rate =
        recent.iter().filter(|v| v.abs() > jump_threshold).count() as f64 / recent.len() as f64;
    let regime_ratio = recent_rms / baseline_rms;
    let regime_score = 1.0 / (1.0 + (regime_ratio - 1.0).max(0.0));
    let stability_score = 1.0 / (1.0 + recent_rms * 100_000.0);
    let tail_score = (1.0 - tail_rate * 4.0).clamp(0.0, 1.0);
    let recent_ticks = &sample[sample.len().saturating_sub(RECENT_RETURNS)..];
    let average_spread =
        recent_ticks.iter().map(Tick::spread_frac).sum::<f64>() / recent_ticks.len() as f64;
    let spread_score = (1.0 - average_spread / MAX_SPREAD_FRACTION).clamp(0.0, 1.0);
    let chaos_edge = ((chaos_theory(sample) - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    let (_, theory_confidence) = compute_consensus(sample, &STRATEGY_WEIGHTS);
    let theory_scores = [
        fluid_dynamics(sample),
        chaos_theory(sample),
        mean_reversion(sample),
        trend_analysis(sample),
        multi_timeframe(sample),
        nash_equilibrium(sample),
        tick_velocity(sample),
        volume_surge(sample),
    ];
    let theory_consensus = compute_consensus(sample, &STRATEGY_WEIGHTS).0;
    let theory_agree = directional_agreement(&theory_scores, theory_consensus >= 0.5);
    let agreement_score = theory_agree as f64 / theory_scores.len() as f64;
    let score = 0.25 * stability_score
        + 0.20 * regime_score
        + 0.15 * tail_score
        + 0.10 * spread_score
        + 0.20 * chaos_edge
        + 0.10 * theory_confidence * agreement_score;
    Some(HistoricalMarketScore {
        symbol: symbol.to_string(),
        score,
        observations: returns.len(),
        recent_rms,
        baseline_rms,
        tail_rate,
        average_spread,
        chaos_edge,
        theory_confidence,
    })
}

struct MarketPerformance {
    wins: u32,
    losses: u32,
    consecutive_losses: u32,
    consecutive_wins: u32,
    cooldown_until_ms: i64,
}

struct IndexRotator {
    indices: Vec<String>,
    history: VecDeque<String>,
    performance: HashMap<String, MarketPerformance>,
    selection_counts: HashMap<String, u64>,
}

impl IndexRotator {
    fn new(indices: Vec<String>) -> Self {
        let performance = indices
            .iter()
            .cloned()
            .map(|symbol| {
                (
                    symbol,
                    MarketPerformance {
                        wins: 0,
                        losses: 0,
                        consecutive_losses: 0,
                        consecutive_wins: 0,
                        cooldown_until_ms: 0,
                    },
                )
            })
            .collect();
        Self {
            indices,
            history: VecDeque::with_capacity(16),
            performance,
            selection_counts: HashMap::new(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn best_historical_market(
        &self,
        loader: &HistoryLoader,
        now_epoch: u64,
    ) -> Option<HistoricalMarketScore> {
        self.indices
            .iter()
            .filter(|symbol| self.market_available(symbol))
            .filter_map(|symbol| {
                let ticks: Vec<Tick> = loader.get_buffer(symbol)?.iter().cloned().collect();
                historical_market_score(symbol, &ticks, now_epoch)
            })
            .max_by(|left, right| {
                left.score
                    .partial_cmp(&right.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    fn ranked_historical_markets(
        &self,
        loader: &HistoryLoader,
        now_epoch: u64,
    ) -> Vec<HistoricalMarketScore> {
        let mut ranked: Vec<_> = self
            .indices
            .iter()
            .filter(|symbol| self.market_available(symbol))
            .filter_map(|symbol| {
                let ticks: Vec<Tick> = loader.get_buffer(symbol)?.iter().cloned().collect();
                historical_market_score(symbol, &ticks, now_epoch)
            })
            .collect();
        ranked.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.symbol.cmp(&right.symbol))
        });
        ranked
    }

    fn market_available(&self, symbol: &str) -> bool {
        let now_ms = Utc::now().timestamp_millis();
        self.performance
            .get(symbol)
            .is_none_or(|stats| stats.cooldown_until_ms <= now_ms)
    }

    fn next_index(&mut self) -> String {
        let now_ms = Utc::now().timestamp_millis();
        let mut candidates: Vec<String> = self
            .indices
            .iter()
            .filter(|symbol| {
                self.performance
                    .get(*symbol)
                    .is_none_or(|stats| stats.cooldown_until_ms <= now_ms)
            })
            .cloned()
            .collect();

        // Rotate after every completed trade when another eligible market exists.
        // Avoid an immediate re-selection of the market just traded while every
        // other eligible configured market keeps an equal random chance.
        if candidates.len() > 1 {
            if let Some(previous) = self.history.back() {
                candidates.retain(|symbol| symbol != previous);
            }
        }

        if candidates.is_empty() {
            candidates = self
                .indices
                .iter()
                .filter(|symbol| {
                    self.performance
                        .get(*symbol)
                        .is_none_or(|stats| stats.cooldown_until_ms <= now_ms)
                })
                .cloned()
                .collect();
        }

        if candidates.is_empty() {
            let chosen = self
                .indices
                .iter()
                .min_by_key(|symbol| {
                    self.performance
                        .get(*symbol)
                        .map_or(0, |stats| stats.cooldown_until_ms)
                })
                .expect("at least one market must be configured")
                .clone();
            log::warn!("[Market] All markets cooling down; waiting for {}", chosen);
            return chosen;
        }

        // Choose among the least-used eligible markets. Randomness breaks ties,
        // while the usage count guarantees balanced coverage of every configured
        // market instead of repeatedly sampling a lucky subset.
        let minimum_selections = candidates
            .iter()
            .map(|symbol| self.selection_counts.get(symbol).copied().unwrap_or(0))
            .min()
            .unwrap_or(0);
        candidates.retain(|symbol| {
            self.selection_counts.get(symbol).copied().unwrap_or(0) == minimum_selections
        });
        let chosen = candidates
            .choose(&mut OsRng)
            .expect("eligible market list cannot be empty")
            .clone();

        if self.history.len() >= 16 {
            self.history.pop_front();
        }
        self.history.push_back(chosen.clone());
        *self.selection_counts.entry(chosen.clone()).or_insert(0) += 1;
        chosen
    }

    fn record_result(&mut self, symbol: &str, profitable: bool) {
        let stats = self
            .performance
            .entry(symbol.to_string())
            .or_insert(MarketPerformance {
                wins: 0,
                losses: 0,
                consecutive_losses: 0,
                consecutive_wins: 0,
                cooldown_until_ms: 0,
            });
        if profitable {
            stats.wins += 1;
            stats.consecutive_wins += 1;
            stats.consecutive_losses = 0;
            if stats.consecutive_wins >= 2 {
                stats.cooldown_until_ms = Utc::now().timestamp_millis() + 30_000;
                log::info!(
                    "[Market] {} reached two consecutive wins; diversifying for 30s",
                    symbol
                );
            } else {
                stats.cooldown_until_ms = 0;
            }
        } else {
            stats.losses += 1;
            stats.consecutive_losses += 1;
            stats.consecutive_wins = 0;
            let cooldown_secs = if stats.consecutive_losses >= 2 {
                MARKET_REPEAT_LOSS_COOLDOWN_SECS
            } else {
                MARKET_LOSS_COOLDOWN_SECS
            };
            stats.cooldown_until_ms = Utc::now().timestamp_millis() + cooldown_secs * 1000;
            if cooldown_secs > 0 {
                log::warn!(
                    "[Market] {} loss: cooling down for {}s (consecutive losses={})",
                    symbol,
                    cooldown_secs,
                    stats.consecutive_losses
                );
            }
        }
    }
}

