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

        // Accept either the documented CSV directory or a single CSV file. A
        // file is loaded only for the matching symbol, which makes the bundled
        // synthetic fixture useful directly from the CLI without silently
        // producing an empty simulation.
        if base.is_file() {
            let requested_symbol = base
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            for sym in symbols {
                let mut buffer = RingBuffer::new(capacity);
                if sym == requested_symbol {
                    total += load_csv_file(&base, sym, &mut buffer, capacity)?;
                }
                buffers.insert(sym.clone(), buffer);
            }
            log::info!("Loaded {} ticks across {} symbols", total, buffers.len());
            return Ok(Self { buffers, loaded_count: total });
        }

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

/// Append a live tick to the operator-owned CSV history without affecting strategy state.
fn append_history_tick(history_path: &str, tick: &Tick) -> std::io::Result<()> {
    let base = expand_tilde(history_path);
    std::fs::create_dir_all(&base)?;
    let path = base.join(format!("{}.csv", tick.symbol));
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{},{:.17},{:.17},{:.17},{}", tick.epoch, tick.quote, tick.ask, tick.bid, tick.pip_size)?;
    file.flush()
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
    next_cursor: usize,
    performance: HashMap<String, MarketPerformance>,
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
            next_cursor: 0,
            performance,
        }
    }

    fn ranked_historical_markets(
        &self,
        loader: &HistoryLoader,
        now_epoch: u64,
    ) -> Vec<HistoricalMarketScore> {
        let mut ranked: Vec<_> = self.indices
            .iter()
            .filter(|symbol| self.market_available(symbol))
            .filter_map(|symbol| {
                let ticks: Vec<Tick> = loader.get_buffer(symbol)?.iter().cloned().collect();
                historical_market_score(symbol, &ticks, now_epoch)
            })
            .collect();
        ranked.sort_by(|left, right| {
            right.score.partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.symbol.cmp(&right.symbol))
        });
        ranked
    }

    #[cfg(test)]
    fn best_historical_market(&self, loader: &HistoryLoader, now_epoch: u64) -> Option<HistoricalMarketScore> {
        self.ranked_historical_markets(loader, now_epoch).into_iter().next()
    }

    fn market_available(&self, symbol: &str) -> bool {
        let now_ms = Utc::now().timestamp_millis();
        self.performance
            .get(symbol)
            .is_none_or(|stats| stats.cooldown_until_ms <= now_ms)
    }

    fn next_index(&mut self) -> String {
        let now_ms = Utc::now().timestamp_millis();
        assert!(
            !self.indices.is_empty(),
            "at least one market must be configured"
        );

        // Strict configured-order round robin. Each call advances the cursor,
        // so after the last index the next scan starts again at index one.
        // Markets in a risk cooldown are skipped without disturbing the order.
        for offset in 0..self.indices.len() {
            let index = (self.next_cursor + offset) % self.indices.len();
            let symbol = &self.indices[index];
            let available = self
                .performance
                .get(symbol)
                .is_none_or(|stats| stats.cooldown_until_ms <= now_ms);
            if available {
                self.next_cursor = (index + 1) % self.indices.len();
                return symbol.clone();
            }
        }

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
