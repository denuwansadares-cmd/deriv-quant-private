use super::*;

const ALL_STATES: [TradeState; 9] = [
    TradeState::Idle,
    TradeState::CollectingTicks,
    TradeState::Analysing,
    TradeState::ProposalPending,
    TradeState::ProposalReady,
    TradeState::BuyPending,
    TradeState::ContractOpen,
    TradeState::Settling,
    TradeState::CooldownOrReanalyse,
];

fn expected_transition(from: TradeState, to: TradeState) -> bool {
    use TradeState::*;
    matches!(
        (from, to),
        (Idle, CollectingTicks)
            | (CollectingTicks, Analysing)
            | (Analysing, CollectingTicks)
            | (Analysing, ProposalPending)
            | (ProposalPending, ProposalReady)
            | (ProposalPending, CollectingTicks)
            | (ProposalReady, BuyPending)
            | (ProposalReady, CollectingTicks)
            | (BuyPending, ContractOpen)
            | (BuyPending, CollectingTicks)
            | (ContractOpen, Settling)
            | (Settling, CooldownOrReanalyse)
            | (CooldownOrReanalyse, CollectingTicks)
    )
}

#[derive(Debug)]
struct LifecycleHarness {
    state: TradeState,
    pending_symbol: Option<String>,
    active_contract: Option<String>,
    buy_reconciliation_pending: bool,
    sell_requested: bool,
    settlement_count: usize,
}

impl LifecycleHarness {
    fn ready_for_proposal(symbol: &str) -> Self {
        let mut state = TradeState::Idle;
        assert!(state.transition(TradeState::CollectingTicks));
        assert!(state.transition(TradeState::Analysing));
        assert!(state.transition(TradeState::ProposalPending));
        Self {
            state,
            pending_symbol: Some(symbol.to_string()),
            active_contract: None,
            buy_reconciliation_pending: false,
            sell_requested: false,
            settlement_count: 0,
        }
    }

    fn proposal(&mut self, symbol: &str) -> bool {
        if self.state != TradeState::ProposalPending
            || self.pending_symbol.as_deref() != Some(symbol)
        {
            return false;
        }
        self.state.transition(TradeState::ProposalReady)
            && self.state.transition(TradeState::BuyPending)
    }

    fn opened(&mut self, contract_id: &str) -> bool {
        if self.state != TradeState::BuyPending || self.active_contract.is_some() {
            return false;
        }
        self.buy_reconciliation_pending = false;
        self.sell_requested = false;
        if self.state.transition(TradeState::ContractOpen) {
            self.active_contract = Some(contract_id.to_string());
            true
        } else {
            false
        }
    }

    fn request_sell(&mut self) -> bool {
        if self.state != TradeState::ContractOpen
            || self.active_contract.is_none()
            || self.sell_requested
        {
            return false;
        }
        self.sell_requested = true;
        true
    }

    fn sold(&mut self, contract_id: &str) -> bool {
        if self.active_contract.as_deref() != Some(contract_id) {
            return false;
        }
        assert!(self.state.transition(TradeState::Settling));
        self.settlement_count += 1;
        self.active_contract = None;
        self.sell_requested = false;
        self.pending_symbol = None;
        assert!(self.state.transition(TradeState::CooldownOrReanalyse));
        assert!(self.state.transition(TradeState::CollectingTicks));
        true
    }

    fn disconnected(&mut self) {
        if self.state == TradeState::BuyPending {
            self.buy_reconciliation_pending = true;
        }
        if self.active_contract.is_none() && self.state != TradeState::BuyPending {
            self.pending_symbol = None;
            if matches!(
                self.state,
                TradeState::ProposalPending | TradeState::ProposalReady
            ) {
                assert!(self.state.transition(TradeState::CollectingTicks));
            }
        }
    }

    fn recover_from_portfolio(
        &mut self,
        contracts: &[PortfolioContract],
        entry_epoch: u64,
    ) -> bool {
        if self.state != TradeState::BuyPending {
            return false;
        }
        self.buy_reconciliation_pending = false;
        self.sell_requested = false;
        let recovered = self.pending_symbol.as_deref().and_then(|symbol| {
            contracts
                .iter()
                .filter(|contract| {
                    contract.symbol == symbol && contract.purchase_time >= entry_epoch
                })
                .max_by_key(|contract| contract.purchase_time)
        });
        if let Some(contract) = recovered {
            assert!(self.state.transition(TradeState::ContractOpen));
            self.active_contract = Some(contract.contract_id.clone());
            true
        } else {
            self.pending_symbol = None;
            assert!(self.state.transition(TradeState::CollectingTicks));
            false
        }
    }
}

#[test]
fn every_allowed_and_invalid_state_transition_is_frozen() {
    for from in ALL_STATES {
        for to in ALL_STATES {
            assert_eq!(
                from.can_transition(to),
                expected_transition(from, to),
                "unexpected transition classification: {from:?} -> {to:?}"
            );
            let mut actual = from;
            let accepted = actual.transition(to);
            assert_eq!(accepted, expected_transition(from, to));
            assert_eq!(actual, if accepted { to } else { from });
        }
    }
}

#[test]
fn execution_pending_state_set_is_frozen() {
    for state in ALL_STATES {
        assert_eq!(
            state.execution_pending(),
            matches!(
                state,
                TradeState::ProposalPending
                    | TradeState::ProposalReady
                    | TradeState::BuyPending
                    | TradeState::ContractOpen
                    | TradeState::Settling
            ),
            "execution_pending changed for {state:?}"
        );
    }
}

#[test]
fn proposal_buy_open_sell_settlement_happy_path_is_frozen() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_50");
    assert!(lifecycle.proposal("R_50"));
    assert_eq!(lifecycle.state, TradeState::BuyPending);
    assert!(lifecycle.opened("contract-42"));
    assert_eq!(lifecycle.state, TradeState::ContractOpen);
    assert!(lifecycle.request_sell());
    assert!(lifecycle.sold("contract-42"));
    assert_eq!(lifecycle.state, TradeState::CollectingTicks);
    assert_eq!(lifecycle.settlement_count, 1);
    assert!(lifecycle.active_contract.is_none());
}

#[test]
fn disconnect_during_proposal_aborts_pending_entry() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_25");
    lifecycle.disconnected();
    assert_eq!(lifecycle.state, TradeState::CollectingTicks);
    assert!(lifecycle.pending_symbol.is_none());
    assert!(!lifecycle.buy_reconciliation_pending);
}

#[test]
fn disconnect_during_buy_preserves_uncertainty_for_portfolio_recovery() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_75");
    assert!(lifecycle.proposal("R_75"));
    lifecycle.disconnected();
    assert_eq!(lifecycle.state, TradeState::BuyPending);
    assert_eq!(lifecycle.pending_symbol.as_deref(), Some("R_75"));
    assert!(lifecycle.buy_reconciliation_pending);
}

#[test]
fn disconnect_during_sell_preserves_the_active_contract() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_100");
    assert!(lifecycle.proposal("R_100"));
    assert!(lifecycle.opened("open-1"));
    assert!(lifecycle.request_sell());
    lifecycle.disconnected();
    assert_eq!(lifecycle.state, TradeState::ContractOpen);
    assert_eq!(lifecycle.active_contract.as_deref(), Some("open-1"));
    assert!(lifecycle.sold("open-1"));
}

#[test]
fn duplicate_and_out_of_order_lifecycle_messages_are_ignored() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_10");
    assert!(!lifecycle.opened("too-early"));
    assert!(!lifecycle.sold("too-early"));
    assert!(!lifecycle.proposal("R_25"));

    assert!(lifecycle.proposal("R_10"));
    assert!(!lifecycle.proposal("R_10"));
    assert!(lifecycle.opened("contract-1"));
    assert!(!lifecycle.opened("contract-1"));
    assert!(!lifecycle.opened("contract-2"));
    assert!(!lifecycle.sold("wrong-contract"));
    assert!(lifecycle.sold("contract-1"));
    assert!(!lifecycle.sold("contract-1"));
    assert_eq!(lifecycle.settlement_count, 1);
}

#[test]
fn portfolio_recovery_selects_latest_matching_post_entry_contract() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_50");
    assert!(lifecycle.proposal("R_50"));
    lifecycle.disconnected();
    let contracts = vec![
        PortfolioContract {
            contract_id: "old".into(),
            symbol: "R_50".into(),
            buy_price: 1.0,
            purchase_time: 99,
            contract_type: None,
            proposal_id: None,
            transaction_id: None,
        },
        PortfolioContract {
            contract_id: "other-market".into(),
            symbol: "R_25".into(),
            buy_price: 1.0,
            purchase_time: 105,
            contract_type: None,
            proposal_id: None,
            transaction_id: None,
        },
        PortfolioContract {
            contract_id: "newer".into(),
            symbol: "R_50".into(),
            buy_price: 1.0,
            purchase_time: 102,
            contract_type: None,
            proposal_id: None,
            transaction_id: None,
        },
        PortfolioContract {
            contract_id: "newest".into(),
            symbol: "R_50".into(),
            buy_price: 1.0,
            purchase_time: 106,
            contract_type: None,
            proposal_id: None,
            transaction_id: None,
        },
    ];
    assert!(lifecycle.recover_from_portfolio(&contracts, 100));
    assert_eq!(lifecycle.state, TradeState::ContractOpen);
    assert_eq!(lifecycle.active_contract.as_deref(), Some("newest"));
}

#[test]
fn empty_portfolio_resolves_uncertain_buy_as_not_opened() {
    let mut lifecycle = LifecycleHarness::ready_for_proposal("R_50");
    assert!(lifecycle.proposal("R_50"));
    lifecycle.disconnected();
    assert!(!lifecycle.recover_from_portfolio(&[], 100));
    assert_eq!(lifecycle.state, TradeState::CollectingTicks);
    assert!(lifecycle.active_contract.is_none());
    assert!(!lifecycle.buy_reconciliation_pending);
}

#[tokio::test]
async fn malformed_api_messages_are_rejected_without_trade_events() {
    let (tx, mut rx) = broadcast::channel(16);
    assert!(handle_ws_message("{not-json", &tx).await.is_err());

    for message in [
        r#"{"msg_type":"proposal","proposal":{"id":"","ask_price":"NaN","symbol":"R_10"}}"#,
        r#"{"msg_type":"buy","buy":{"buy_price":"1.00"}}"#,
        r#"{"msg_type":"sell","sell":{"contract_id":"42","sold_for":"NaN"}}"#,
        r#"{"msg_type":"tick","tick":{"symbol":"R_10","quote":"bad","epoch":"0"}}"#,
        r#"{"msg_type":"proposal_open_contract","proposal_open_contract":{"status":"open"}}"#,
    ] {
        handle_ws_message(message, &tx).await.unwrap();
    }
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn malformed_portfolio_entries_are_filtered_individually() {
    let (tx, mut rx) = broadcast::channel(4);
    handle_ws_message(
        r#"{"msg_type":"portfolio","portfolio":{"contracts":[
            {"contract_id":"good","symbol":"R_10","buy_price":"1.25","purchase_time":"101"},
            {"contract_id":"","symbol":"R_10","buy_price":"1.00"},
            {"contract_id":"bad-price","symbol":"R_10","buy_price":"NaN"},
            {"contract_id":"no-symbol","buy_price":"1.00"}
        ]}}"#,
        &tx,
    )
    .await
    .unwrap();
    match rx.recv().await.unwrap() {
        ApiEvent::Portfolio(contracts) => {
            assert_eq!(contracts.len(), 1);
            assert_eq!(contracts[0].contract_id, "good");
            assert_eq!(contracts[0].buy_price, 1.25);
        }
        event => panic!("unexpected event: {event:?}"),
    }
}

#[tokio::test]
async fn duplicate_parser_messages_remain_visible_for_strategy_level_deduplication() {
    let (tx, mut rx) = broadcast::channel(4);
    let message = r#"{"msg_type":"buy","buy":{"contract_id":"42","buy_price":"1.25"}}"#;
    handle_ws_message(message, &tx).await.unwrap();
    handle_ws_message(message, &tx).await.unwrap();
    assert!(matches!(
        rx.recv().await.unwrap(),
        ApiEvent::ContractOpened { .. }
    ));
    assert!(matches!(
        rx.recv().await.unwrap(),
        ApiEvent::ContractOpened { .. }
    ));
}

#[test]
fn random_market_rotation_never_leaves_the_configured_market_set() {
    let configured = AVAILABLE_INDICES
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut rotator = IndexRotator::new(AVAILABLE_INDICES.iter().map(|s| s.to_string()).collect());
    for _ in 0..(AVAILABLE_INDICES.len() * 20) {
        let selected = rotator.next_index();
        assert!(configured.contains(selected.as_str()));
    }
}

#[test]
fn stake_policy_uses_its_balance_derived_cap_and_signal_risk() {
    assert_eq!(stake_for_signal_risk(1.0, 0.0), 1.0);
    assert_eq!(stake_for_signal_risk(5.0, 0.0), 5.0);
    assert_eq!(stake_for_signal_risk(5.0, 0.5), 3.0);
    assert_eq!(stake_for_signal_risk(5.0, 1.0), 1.0);
    assert_eq!(stake_for_signal_risk(1_000.0, 0.0), 10.0);
    assert_eq!(deriv_stake(1.005), 1.0);
    assert_eq!(deriv_stake(1.006), 1.01);
}

#[test]
fn secret_redaction_is_recursive_case_insensitive_and_preserves_safe_fields() {
    let redacted = sanitized_json(&serde_json::json!({
        "Authorization": "Bearer real-secret",
        "nested": {
            "access_token": "token-value",
            "OTP": "otp-value",
            "wss": "wss://host/?otp=value",
            "safe": "R_50"
        },
        "items": [{"client_secret": "secret-value"}, {"url": "https://secret"}],
        "contract_id": "raw-identifier-42",
        "count": 42
    }));
    for secret in [
        "real-secret",
        "token-value",
        "otp-value",
        "wss://host",
        "secret-value",
        "https://secret",
    ] {
        assert!(!redacted.contains(secret), "secret leaked: {secret}");
    }
    assert!(redacted.contains("R_50"));
    assert!(!redacted.contains("raw-identifier-42"));
    assert!(redacted.contains("\"count\":42"));
    assert!(redacted.matches("[REDACTED]").count() >= 5);
}

#[test]
fn every_implemented_contract_family_has_a_command_shape() {
    let commands = [
        (
            ContractType::Accumulator,
            ApiCommand::RequestProposal {
                underlying_symbol: "R_10".into(),
                currency: "USD".into(),
                amount: 1.0,
                growth_rate: ACCU_GROWTH_RATE,
            },
        ),
        (
            ContractType::OverUnder,
            ApiCommand::RequestOverUnder {
                underlying_symbol: "R_10".into(),
                currency: "USD".into(),
                amount: 1.0,
                contract_type: "DIGITOVER".into(),
                barrier: 5,
            },
        ),
        (
            ContractType::MatchesDiff,
            ApiCommand::RequestMatchesDiff {
                underlying_symbol: "R_10".into(),
                currency: "USD".into(),
                amount: 1.0,
                contract_type: "DIGITDIFF".into(),
                barrier: 5,
            },
        ),
        (
            ContractType::RiseFall,
            ApiCommand::RequestRiseFall {
                underlying_symbol: "R_10".into(),
                currency: "USD".into(),
                amount: 0.50,
                contract_type: "CALL".into(),
                duration_ticks: RISE_FALL_DURATION_TICKS,
            },
        ),
    ];

    assert_eq!(
        commands.each_ref().map(|(kind, _)| *kind),
        [
            ContractType::Accumulator,
            ContractType::OverUnder,
            ContractType::MatchesDiff,
            ContractType::RiseFall,
        ]
    );
    assert_eq!(ContractType::Accumulator.to_string(), "ACCU");
    assert_eq!(ContractType::OverUnder.to_string(), "OVER_UNDER");
    assert_eq!(ContractType::MatchesDiff.to_string(), "MATCHES_DIFF");
    assert_eq!(ContractType::RiseFall.to_string(), "RISE_FALL");
    const {
        assert!(!ENABLE_RANDOM_CONTRACT_FAMILIES);
    }
    const {
        assert!(!ENABLE_RISE_FALL);
    }

    for (_, command) in commands {
        match command {
            ApiCommand::RequestProposal {
                amount,
                growth_rate,
                ..
            } => {
                assert_eq!(amount, 1.0);
                assert_eq!(growth_rate, ACCU_GROWTH_RATE);
            }
            ApiCommand::RequestOverUnder {
                contract_type,
                barrier,
                ..
            } => {
                assert_eq!(contract_type, "DIGITOVER");
                assert_eq!(barrier, 5);
            }
            ApiCommand::RequestMatchesDiff {
                contract_type,
                barrier,
                ..
            } => {
                assert_eq!(contract_type, "DIGITDIFF");
                assert_eq!(barrier, 5);
            }
            ApiCommand::RequestRiseFall {
                contract_type,
                duration_ticks,
                ..
            } => {
                assert_eq!(contract_type, "CALL");
                assert_eq!(duration_ticks, RISE_FALL_DURATION_TICKS);
            }
            _ => unreachable!("unexpected family command"),
        }
    }
}

#[tokio::test]
async fn api_error_arrays_preserve_request_context_and_order() {
    let (tx, mut rx) = broadcast::channel(4);
    handle_ws_message(
        r#"{"msg_type":"proposal","errors":[
            {"code":"First","message":"one"},
            {"code":"Second","message":"two"}
        ],"echo_req":{"passthrough":{"kind":"proposal"}}}"#,
        &tx,
    )
    .await
    .unwrap();
    assert!(matches!(
        rx.recv().await.unwrap(),
        ApiEvent::Error { code, request_kind: Some(kind), .. }
            if code == "First" && kind == "proposal"
    ));
    assert!(matches!(
        rx.recv().await.unwrap(),
        ApiEvent::Error { code, request_kind: Some(kind), .. }
            if code == "Second" && kind == "proposal"
    ));
}
