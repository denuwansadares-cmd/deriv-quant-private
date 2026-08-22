// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
enum ApiCommand {
    RequestProposal {
        underlying_symbol: String,
        currency: String,
        amount: f64,
        growth_rate: f64,
    },
    RequestOverUnder {
        underlying_symbol: String,
        currency: String,
        amount: f64,
        contract_type: String,
        barrier: u8,
    },
    RequestMatchesDiff {
        underlying_symbol: String,
        currency: String,
        amount: f64,
        contract_type: String,
        barrier: u8,
    },
    RequestRiseFall {
        underlying_symbol: String,
        currency: String,
        amount: f64,
        contract_type: String,
        duration_minutes: u16,
    },
    BuyContract {
        proposal_id: String,
        price: f64,
    },
    SellContract {
        contract_id: String,
    },
    SubscribeContract {
        contract_id: String,
    },
    RequestPortfolio,
}

#[derive(Debug, Clone)]
struct PortfolioContract {
    contract_id: String,
    symbol: String,
    buy_price: f64,
    purchase_time: u64,
    contract_type: Option<String>,
    proposal_id: Option<String>,
    transaction_id: Option<String>,
}

#[derive(Debug, Clone)]
enum ApiEvent {
    Connected,
    Disconnected,
    Tick(Tick),
    Proposal {
        id: String,
        ask_price: f64,
        payout: f64,
        underlying_symbol: String,
    },
    ContractOpened {
        contract_id: String,
        buy_price: Option<f64>,
        proposal_id: Option<String>,
    },
    ContractSold {
        contract_id: String,
        sold_for: f64,
    },
    ContractUpdate {
        contract_id: String,
        current_spot: f64,
        bid_price: f64,
    },
    Portfolio(Vec<PortfolioContract>),
    Error {
        code: String,
        message: String,
        request_kind: Option<String>,
        request_contract_id: Option<String>,
    },
}

struct ApiHandle {
    cmd_tx: mpsc::Sender<ApiCommand>,
    event_tx: broadcast::Sender<ApiEvent>,
    shutdown_tx: broadcast::Sender<()>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl ApiHandle {
    fn new(
        cmd_tx: mpsc::Sender<ApiCommand>,
        event_tx: broadcast::Sender<ApiEvent>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            cmd_tx,
            event_tx,
            shutdown_tx,
            task: None,
        }
    }

    fn event_rx(&self) -> broadcast::Receiver<ApiEvent> {
        self.event_tx.subscribe()
    }

    async fn launch(
        ws_url: String,
        symbols: Vec<String>,
        app_id: String,
        access_token: String,
        account_id: String,
    ) -> Result<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let (event_tx, _) = broadcast::channel(4096);
        let (shutdown_tx, _) = broadcast::channel::<()>(1);

        let mut handle = Self::new(cmd_tx, event_tx.clone(), shutdown_tx.clone());
        let ws_cmd_rx = cmd_rx;
        let ws_event_tx = event_tx;
        let ws_shutdown_rx = shutdown_tx.subscribe();

        let task = tokio::spawn(async move {
            ws_task(
                ws_url,
                symbols,
                WsAuth {
                    app_id,
                    access_token,
                    account_id,
                },
                ws_cmd_rx,
                ws_event_tx,
                ws_shutdown_rx,
            )
            .await;
            log::info!("WebSocket task exited");
        });
        handle.task = Some(task);
        Ok(handle)
    }

    async fn join(&mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.await.context("WebSocket task join failed")?;
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// REST API HELPERS
// ═══════════════════════════════════════════════════════════════════════════

async fn fetch_options_accounts(app_id: &str, token: &str) -> Result<Vec<OptionsAccount>> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .context("Failed to build HTTP client")?;
    let resp = client
        .get(format!("{}/accounts", DERIV_API_BASE))
        .header("Deriv-App-ID", app_id)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .context("Failed to fetch options accounts")?;

    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("Accounts API returned HTTP {} (body redacted)", status);
    }

    let accounts_resp: OptionsAccountsResponse = resp
        .json()
        .await
        .context("Failed to parse accounts response")?;

    Ok(accounts_resp.data.unwrap_or_default())
}

async fn request_otp(app_id: &str, token: &str, account_id: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .context("Failed to build HTTP client")?;
    let resp = client
        .post(format!("{}/accounts/{}/otp", DERIV_API_BASE, account_id))
        .header("Deriv-App-ID", app_id)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .context("Failed to request OTP")?;

    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("OTP API returned HTTP {} (body redacted)", status);
    }

    let otp_resp: OtpResponse = resp.json().await.context("Failed to parse OTP response")?;

    if let Some(err) = otp_resp.error {
        let _ = err;
        anyhow::bail!("OTP API returned a provider error (details redacted)");
    }
    otp_resp
        .wss
        .or_else(|| otp_resp.data.as_ref().and_then(|d| d.wss.clone()))
        .or_else(|| otp_resp.data.as_ref().and_then(|d| d.url.clone()))
        .context("OTP response missing WebSocket URL (expected 'wss', 'data.wss', or 'data.url')")
}

// ═══════════════════════════════════════════════════════════════════════════
// WEBSOCKET TASK
// ═══════════════════════════════════════════════════════════════════════════

struct WsAuth {
    app_id: String,
    access_token: String,
    account_id: String,
}

async fn ws_task(
    ws_url: String,
    symbols: Vec<String>,
    auth: WsAuth,
    mut cmd_rx: mpsc::Receiver<ApiCommand>,
    event_tx: broadcast::Sender<ApiEvent>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let mut req_timestamps: VecDeque<std::time::Instant> = VecDeque::new();

    async fn rate_limit(timestamps: &mut VecDeque<std::time::Instant>, max: usize) {
        let now = std::time::Instant::now();
        while timestamps
            .front()
            .is_some_and(|t| now - *t >= Duration::from_secs(1))
        {
            timestamps.pop_front();
        }
        if timestamps.len() >= max {
            if let Some(oldest) = timestamps.front() {
                let wait = Duration::from_secs(1).saturating_sub(now - *oldest);
                if !wait.is_zero() {
                    sleep(wait).await;
                }
            }
            let now = std::time::Instant::now();
            while timestamps
                .front()
                .is_some_and(|t| now - *t >= Duration::from_secs(1))
            {
                timestamps.pop_front();
            }
        }
        timestamps.push_back(std::time::Instant::now());
    }

    let mut next_ws_url = Some(ws_url);
    'connect: loop {
        let connection_url = if let Some(url) = next_ws_url.take() {
            url
        } else {
            match request_otp(&auth.app_id, &auth.access_token, &auth.account_id).await {
                Ok(url) => url,
                Err(error) => {
                    log::error!("OTP refresh failed: {}. Retrying in 2s...", error);
                    let _ = event_tx.send(ApiEvent::Disconnected);
                    sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
        };
        log::info!("Connecting to OTP WebSocket...");
        let ws = match connect_async(&connection_url).await {
            Ok((stream, _)) => stream,
            Err(e) => {
                log::error!("Connection failed: {}. Retrying in 2s...", e);
                let _ = event_tx.send(ApiEvent::Disconnected);
                sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        log::info!("Connected to Deriv OTP WS");
        let _ = event_tx.send(ApiEvent::Connected);

        let (mut write, mut read) = ws.split();
        for (index, symbol) in symbols.iter().enumerate() {
            let req_id = (index as u64) + 1;
            let payload = serde_json::json!({"ticks": symbol, "subscribe": 1, "req_id": req_id, "passthrough": {"kind": "ticks", "symbol": symbol}});
            log::debug!(
                "WS OUT tick subscription req_id={} symbol={}",
                req_id,
                symbol
            );
            if let Err(error) = write.send(Message::Text(payload.to_string())).await {
                log::error!(
                    "Tick subscription write failed req_id={} symbol={}: {}",
                    req_id,
                    symbol,
                    error
                );
                let _ = event_tx.send(ApiEvent::Disconnected);
                continue 'connect;
            }
        }
        let mut next_req_id = symbols.len() as u64 + 1;
        let mut heartbeat = interval(Duration::from_secs(WS_HEARTBEAT_SECS));
        heartbeat.tick().await;
        let mut last_ws_message = Instant::now();

        loop {
            tokio::select! {
                // WebSocket message
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            last_ws_message = Instant::now();
                            if let Err(e) = handle_ws_message(&text, &event_tx).await {
                                log::error!("WS msg: {}", e);
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            last_ws_message = Instant::now();
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Pong(_))) => {
                            last_ws_message = Instant::now();
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            log::info!("WS closed. Reconnecting...");
                            let _ = event_tx.send(ApiEvent::Disconnected);
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            log::error!("WS error: {}. Reconnecting...", e);
                            let _ = event_tx.send(ApiEvent::Disconnected);
                            break;
                        }
                    }
                }

                _ = heartbeat.tick() => {
                    if last_ws_message.elapsed() >= Duration::from_secs(WS_STALE_TIMEOUT_SECS) {
                        log::warn!("WebSocket received no data for {}s; reconnecting", WS_STALE_TIMEOUT_SECS);
                        let _ = event_tx.send(ApiEvent::Disconnected);
                        break;
                    }
                    if let Err(error) = write.send(Message::Ping(Vec::new())).await {
                        log::error!("WebSocket heartbeat failed: {}; reconnecting", error);
                        let _ = event_tx.send(ApiEvent::Disconnected);
                        break;
                    }
                }

                // Command
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => {
                            let mut payload = match &cmd {
                                ApiCommand::RequestProposal { underlying_symbol, currency, amount, growth_rate } => {
                                    let amount = deriv_stake(*amount);
                                    log::info!("Proposal request: {} ${:.2} growth={}", underlying_symbol, amount, growth_rate);
                                    serde_json::json!({
                                        "proposal": 1,
                                        "contract_type": "ACCU",
                                        "currency": currency,
                                        "underlying_symbol": underlying_symbol,
                                        "amount": amount,
                                        "basis": "stake",
                                        "growth_rate": growth_rate,
                                        "passthrough": {"kind": "proposal", "symbol": underlying_symbol}
                                    })
                                }
                                ApiCommand::RequestOverUnder { underlying_symbol, currency, amount, contract_type, barrier } => {
                                    let amount = deriv_stake(*amount);
                                    serde_json::json!({
                                        "proposal": 1, "amount": amount, "basis": "stake",
                                        "contract_type": contract_type, "currency": currency,
                                        "duration": 1, "duration_unit": "t",
                                        "underlying_symbol": underlying_symbol,
                                        "barrier": barrier.to_string(),
                                        "passthrough": {"kind": "proposal", "symbol": underlying_symbol}
                                    })
                                }
                                ApiCommand::RequestMatchesDiff { underlying_symbol, currency, amount, contract_type, barrier } => {
                                    let amount = deriv_stake(*amount);
                                    serde_json::json!({
                                        "proposal": 1, "amount": amount, "basis": "stake",
                                        "contract_type": contract_type, "currency": currency,
                                        "duration": 1, "duration_unit": "t",
                                        "underlying_symbol": underlying_symbol,
                                        "barrier": barrier.to_string(),
                                        "passthrough": {"kind": "proposal", "symbol": underlying_symbol}
                                    })
                                }
                                ApiCommand::RequestRiseFall { underlying_symbol, currency, amount, contract_type, duration_minutes } => {
                                    let amount = deriv_stake(*amount);
                                    log::info!("Rise/Fall proposal: {} {} expiry={}m stake={:.2}", underlying_symbol, contract_type, duration_minutes, amount);
                                    serde_json::json!({
                                        "proposal": 1, "amount": amount, "basis": "stake",
                                        "contract_type": contract_type, "currency": currency,
                                        "duration": duration_minutes, "duration_unit": "m",
                                        "underlying_symbol": underlying_symbol,
                                        "passthrough": {"kind": "proposal", "symbol": underlying_symbol}
                                    })
                                }
                                ApiCommand::BuyContract { proposal_id, price } => {
                                    log::info!("Dispatching buy request for approved proposal @ ${:.4} (identifier redacted)", price);
                                    serde_json::json!({"buy": proposal_id, "price": price, "passthrough": {"kind": "buy", "proposal_id": proposal_id}})
                                }
                                ApiCommand::SellContract { contract_id } => {
                                    log::info!("Selling tracked contract (identifier redacted)");
                                    serde_json::json!({"sell": contract_id, "price": 0, "passthrough": {"kind": "sell", "contract_id": contract_id}})
                                }
                                ApiCommand::SubscribeContract { contract_id } => {
                                    log::info!("Subscribing to tracked contract updates (identifier redacted)");
                                    serde_json::json!({"proposal_open_contract": 1, "contract_id": contract_id, "subscribe": 1, "passthrough": {"kind": "contract", "contract_id": contract_id}})
                                }
                                ApiCommand::RequestPortfolio => {
                                    log::info!("Requesting portfolio reconciliation");
                                    serde_json::json!({"portfolio": 1, "passthrough": {"kind": "portfolio"}})
                                }
                            };
                            let req_id = next_req_id;
                            next_req_id = next_req_id.saturating_add(1);
                            payload["req_id"] = serde_json::json!(req_id);
                            rate_limit(&mut req_timestamps, MAX_REQ_PER_SEC).await;
                            if let Ok(text) = serde_json::to_string(&payload) {
                                if let Err(e) = write.send(Message::Text(text)).await {
                                    log::error!("WS write failed req_id={}: {}", req_id, e);
                                    let _ = event_tx.send(ApiEvent::Disconnected);
                                    break;
                                } else { log::info!("WS write confirmed req_id={}", req_id); }
                            }
                        }
                        None => {
                            log::info!("Command channel closed");
                            return;
                        }
                    }
                }

                // Shutdown
                _ = shutdown_rx.recv() => {
                    log::info!("Shutdown signal received");
                    let _ = write.send(Message::Close(None)).await;
                    return;
                }
            }
        }
    }
}

async fn handle_ws_message(text: &str, event_tx: &broadcast::Sender<ApiEvent>) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(text).context("malformed WebSocket JSON")?;
    let msg_type = v
        .get("msg_type")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    if matches!(
        msg_type,
        "proposal" | "buy" | "sell" | "tick" | "proposal_open_contract" | "forget"
    ) || v.get("error").is_some()
        || v.get("errors").is_some()
    {
        log::debug!("WS IN type={} payload={}", msg_type, sanitized_json(&v));
    }

    let request_kind = v
        .pointer("/echo_req/passthrough/kind")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let request_contract_id = v
        .pointer("/echo_req/passthrough/contract_id")
        .or_else(|| v.pointer("/echo_req/sell"))
        .map(json_contract_id)
        .filter(|contract_id| !contract_id.is_empty());

    if let Some(err) = v.get("error") {
        let code = err
            .get("code")
            .and_then(|x| x.as_str())
            .unwrap_or("UnknownError")
            .to_string();
        let message = err
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("API error without message")
            .to_string();
        let _ = event_tx.send(ApiEvent::Error {
            code,
            message,
            request_kind: request_kind.clone(),
            request_contract_id: request_contract_id.clone(),
        });
        return Ok(());
    }
    if let Some(errors) = v.get("errors").and_then(|x| x.as_array()) {
        for err in errors {
            let code = err
                .get("code")
                .and_then(|x| x.as_str())
                .unwrap_or("UnknownError")
                .to_string();
            let message = "provider error details redacted".to_string();
            let _ = event_tx.send(ApiEvent::Error {
                code,
                message,
                request_kind: request_kind.clone(),
                request_contract_id: request_contract_id.clone(),
            });
        }
        return Ok(());
    }

    if let Some(portfolio) = v.get("portfolio") {
        let Some(items) = portfolio.get("contracts").and_then(|value| value.as_array()) else {
            let _ = event_tx.send(ApiEvent::Error {
                code: "MalformedPortfolio".to_string(),
                message: "portfolio response failed closed".to_string(),
                request_kind: Some("portfolio".to_string()),
                request_contract_id: None,
            });
            return Ok(());
        };
        let contracts = items
                    .iter()
                    .map(|item| {
                        let contract_id = item
                            .get("contract_id")
                            .map(json_contract_id)
                            .unwrap_or_default();
                        let symbol = item
                            .get("symbol")
                            .or_else(|| item.get("underlying_symbol"))
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let buy_price = item.get("buy_price").map(json_f64).unwrap_or(0.0);
                        let purchase_time = item.get("purchase_time").map(json_u64).unwrap_or(0);
                        let contract_type = item.get("contract_type").and_then(|v| v.as_str()).map(str::to_string);
                        let proposal_id = item.get("proposal_id").and_then(|v| v.as_str()).map(str::to_string);
                        let transaction_id = item.get("transaction_id").map(json_contract_id).filter(|v| !v.is_empty());
                        if !contract_id.is_empty()
                            && !symbol.is_empty()
                            && buy_price.is_finite()
                            && buy_price > 0.0 {
                            Some(PortfolioContract {
                                contract_id,
                                symbol,
                                buy_price,
                                purchase_time,
                                contract_type,
                                proposal_id,
                                transaction_id,
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Option<Vec<_>>>();
        let Some(contracts) = contracts else {
            let _ = event_tx.send(ApiEvent::Error {
                code: "MalformedPortfolio".to_string(),
                message: "portfolio response contained an invalid contract".to_string(),
                request_kind: Some("portfolio".to_string()),
                request_contract_id: None,
            });
            return Ok(());
        };
        let _ = event_tx.send(ApiEvent::Portfolio(contracts));
        return Ok(());
    }

    if let Some(prop) = v.get("proposal") {
        let id = prop
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let ask_price = prop.get("ask_price").map(json_f64).unwrap_or(0.0);
        let payout = prop.get("payout").map(json_f64).unwrap_or(0.0);
        let symbol = prop
            .get("underlying_symbol")
            .or_else(|| prop.get("symbol"))
            .and_then(|x| x.as_str())
            .or_else(|| {
                v.pointer("/echo_req/underlying_symbol")
                    .and_then(|x| x.as_str())
            })
            .or_else(|| {
                v.pointer("/echo_req/passthrough/symbol")
                    .and_then(|x| x.as_str())
            })
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || symbol.is_empty() || !ask_price.is_finite() || ask_price <= 0.0 {
            log::warn!("Ignoring incomplete proposal response (request identifier redacted)");
            return Ok(());
        }
        let _ = event_tx.send(ApiEvent::Proposal {
            id,
            ask_price,
            payout,
            underlying_symbol: symbol,
        });
        return Ok(());
    }

    if let Some(buy) = v.get("buy") {
        let contract_id = buy
            .get("contract_id")
            .map(json_contract_id)
            .unwrap_or_default();
        let buy_price = buy
            .get("buy_price")
            .or_else(|| buy.get("price"))
            .map(json_f64)
            .filter(|price| price.is_finite() && *price > 0.0);
        let proposal_id = v
            .pointer("/echo_req/passthrough/proposal_id")
            .or_else(|| v.pointer("/echo_req/buy"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if contract_id.is_empty() {
            log::warn!("Ignoring buy success without contract identifier (request identifier redacted)");
        } else {
            let _ = event_tx.send(ApiEvent::ContractOpened {
                contract_id,
                buy_price,
                proposal_id,
            });
        }
        return Ok(());
    }

    if let Some(sell) = v.get("sell") {
        let contract_id = sell
            .get("contract_id")
            .map(json_contract_id)
            .unwrap_or_default();
        let sold_for = sell
            .get("sold_for")
            .or_else(|| sell.get("sell_price"))
            .map(json_f64)
            .filter(|amount| amount.is_finite() && *amount >= 0.0);
        match (contract_id.is_empty(), sold_for) {
            (false, Some(sold_for)) => {
                let _ = event_tx.send(ApiEvent::ContractSold {
                    contract_id,
                    sold_for,
                });
            }
            (false, None) => log::warn!("Ignoring sell response without settlement amount (identifier redacted)"),
            _ => log::warn!("Ignoring sell response without contract_id"),
        }
        return Ok(());
    }

    if let Some(poc) = v.get("proposal_open_contract") {
        let contract_id = poc
            .get("contract_id")
            .map(json_contract_id)
            .unwrap_or_default();
        if contract_id.is_empty() {
            log::warn!("Ignoring contract update without contract_id");
            return Ok(());
        }
        let current_spot = poc
            .get("current_spot")
            .or_else(|| poc.get("exit_spot"))
            .map(json_f64)
            .unwrap_or(0.0);
        let bid_price = poc.get("bid_price").map(json_f64).unwrap_or(0.0);
        let is_sold = poc.get("is_sold").map(json_bool).unwrap_or(false)
            || matches!(
                poc.get("status").and_then(|x| x.as_str()),
                Some("sold" | "won" | "lost")
            );
        if is_sold {
            let sold_for = poc
                .get("sell_price")
                .or_else(|| poc.get("payout"))
                .or_else(|| poc.get("bid_price"))
                .map(json_f64)
                .unwrap_or(0.0);
            let _ = event_tx.send(ApiEvent::ContractSold {
                contract_id,
                sold_for,
            });
        } else {
            let _ = event_tx.send(ApiEvent::ContractUpdate {
                contract_id,
                current_spot,
                bid_price,
            });
        }
        return Ok(());
    }

    if let Some(tick) = v.get("tick") {
        let quote = tick.get("quote").map(json_f64).unwrap_or(0.0);
        let symbol = tick
            .get("symbol")
            .and_then(|x| x.as_str())
            .or_else(|| v.pointer("/echo_req/ticks").and_then(|x| x.as_str()))
            .or_else(|| {
                v.pointer("/echo_req/passthrough/symbol")
                    .and_then(|x| x.as_str())
            })
            .unwrap_or_default()
            .to_string();
        let ask = tick
            .get("ask")
            .map(json_f64)
            .filter(|x| *x > 0.0)
            .unwrap_or(quote);
        let bid = tick
            .get("bid")
            .map(json_f64)
            .filter(|x| *x > 0.0)
            .unwrap_or(quote);
        let epoch = tick.get("epoch").map(json_u64).unwrap_or(0);
        let pip_size = tick
            .get("pip_size")
            .map(json_u64)
            .or_else(|| v.get("pip_size").map(json_u64))
            .unwrap_or(2)
            .min(u32::MAX as u64) as u32;
        if symbol.is_empty() || !quote.is_finite() || quote <= 0.0 || epoch == 0 {
            log::warn!(
                "Ignoring malformed tick symbol={} quote={} epoch={}",
                symbol,
                quote,
                epoch
            );
            return Ok(());
        }
        let _ = event_tx.send(ApiEvent::Tick(Tick {
            symbol,
            quote,
            ask,
            bid,
            epoch,
            pip_size,
        }));
        return Ok(());
    }

    if v.get("subscription").is_some() {
        log::debug!("Subscription acknowledgement: {}", sanitized_json(&v));
    }
    Ok(())
}

fn json_bool(v: &serde_json::Value) -> bool {
    v.as_bool()
        .or_else(|| v.as_u64().map(|n| n != 0))
        .or_else(|| {
            v.as_str()
                .map(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        })
        .unwrap_or(false)
}

fn sanitized_json(value: &serde_json::Value) -> String {
    fn redact(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let lower = key.to_ascii_lowercase();
                        let secret = lower.contains("token")
                            || lower.contains("otp")
                            || lower.contains("authorization")
                            || lower.contains("secret")
                            || lower == "wss"
                            || lower == "url"
                            || lower == "id"
                            || lower.ends_with("_id")
                            || lower.contains("account_id")
                            || lower.contains("subscription_id")
                            || lower.contains("proposal_id")
                            || lower.contains("contract_id");
                        (
                            key.clone(),
                            if secret {
                                serde_json::Value::String("[REDACTED]".into())
                            } else {
                                redact(value)
                            },
                        )
                    })
                    .collect(),
            ),
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.iter().map(redact).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&redact(value)).unwrap_or_else(|_| "[unserializable-json]".to_string())
}
/// Extract f64 from a JSON value that may be a number or a string.
fn json_f64(v: &serde_json::Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(0.0)
}

fn json_u64(v: &serde_json::Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(0)
}

fn json_contract_id(v: &serde_json::Value) -> String {
    v.as_str()
        .map(|s| s.to_string())
        .or_else(|| v.as_u64().map(|n| n.to_string()))
        .or_else(|| v.as_i64().map(|n| n.to_string()))
        .unwrap_or_default()
}
