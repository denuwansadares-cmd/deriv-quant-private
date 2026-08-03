// Shared boundaries between orchestration, brokers, and strategy engines.
// Implementations may change without requiring the execution loop to know
// their transport or selection details.

trait Broker {
    type Event: Clone + Send + 'static;

    fn command_sender(&self) -> BrokerCommandSender;
    fn subscribe_events(&self) -> broadcast::Receiver<Self::Event>;
    fn shutdown_sender(&self) -> broadcast::Sender<()>;
}

impl Broker for ApiHandle {
    type Event = ApiEvent;

    fn command_sender(&self) -> BrokerCommandSender {
        BrokerCommandSender { inner: self.cmd_tx.clone() }
    }

    fn subscribe_events(&self) -> broadcast::Receiver<Self::Event> {
        self.event_rx()
    }

    fn shutdown_sender(&self) -> broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }
}

#[derive(Clone)]
struct BrokerCommandSender {
    inner: mpsc::Sender<ApiCommand>,
}

impl BrokerCommandSender {
    fn try_send(
        &self,
        command: ApiCommand,
    ) -> std::result::Result<(), mpsc::error::TrySendError<ApiCommand>> {
        if let Err(reason) = risk_v2_pre_trade(&command) {
            journal_event("risk_v2_reject", serde_json::json!({"reason":reason}));
            return Err(mpsc::error::TrySendError::Full(command));
        }
        let (request_kind, identifier, amount) = match &command {
            ApiCommand::RequestProposal { underlying_symbol, amount, .. } =>
                ("proposal_accumulator", underlying_symbol.as_str(), Some(*amount)),
            ApiCommand::RequestOverUnder { underlying_symbol, amount, .. } =>
                ("proposal_over_under", underlying_symbol.as_str(), Some(*amount)),
            ApiCommand::RequestMatchesDiff { underlying_symbol, amount, .. } =>
                ("proposal_matches_diff", underlying_symbol.as_str(), Some(*amount)),
            ApiCommand::RequestRiseFall { underlying_symbol, amount, .. } =>
                ("proposal_rise_fall", underlying_symbol.as_str(), Some(*amount)),
            ApiCommand::BuyContract { proposal_id, price } =>
                ("buy", proposal_id.as_str(), Some(*price)),
            ApiCommand::SellContract { contract_id } =>
                ("sell", contract_id.as_str(), None),
            ApiCommand::SubscribeContract { contract_id } =>
                ("subscribe_contract", contract_id.as_str(), None),
            ApiCommand::RequestPortfolio => ("portfolio", "portfolio", None),
        };
        let normalized_identifier = normalized_id(identifier);
        journal_event("order_request", serde_json::json!({
            "request_kind": request_kind,
            "normalized_identifier": normalized_identifier,
            "amount": amount,
        }));
        let result = self.inner.try_send(command);
        journal_event("order_dispatch_result", serde_json::json!({
            "request_kind": request_kind,
            "normalized_identifier": normalized_identifier,
            "accepted": result.is_ok(),
            "reason": if result.is_ok() { "queued" } else { "channel_rejected" },
        }));
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StrategyDecision {
    contract_type: ContractType,
    reason: &'static str,
}

trait StrategyEngine {
    type Outcome;

    fn initial_decision(&mut self) -> StrategyDecision;
    fn after_outcome(&mut self, outcome: Self::Outcome) -> StrategyDecision;
}

#[derive(Debug, Clone, Copy)]
struct ContractOutcome {
    contract_type: ContractType,
    won: bool,
}

struct EnabledContractStrategy {
    digit_families_enabled: bool,
    random_mode: bool,
    rotator: ContractTypeRotator,
}

impl EnabledContractStrategy {
    fn new(digit_families_enabled: bool, random_mode: bool) -> Self {
        Self {
            digit_families_enabled,
            random_mode,
            rotator: ContractTypeRotator::new(),
        }
    }

    fn select(&mut self, initial: bool) -> StrategyDecision {
        if !self.digit_families_enabled {
            return StrategyDecision {
                contract_type: ContractType::Accumulator,
                reason: "only_enabled_family",
            };
        }
        if self.random_mode {
            return StrategyDecision {
                contract_type: random_contract_type(),
                reason: "uniform_random",
            };
        }
        let (contract_type, reason) = self.rotator.next_type();
        StrategyDecision {
            contract_type,
            reason: if initial {
                "performance_rotation_initial"
            } else {
                reason
            },
        }
    }
}

impl StrategyEngine for EnabledContractStrategy {
    type Outcome = ContractOutcome;

    fn initial_decision(&mut self) -> StrategyDecision {
        let decision = self.select(true);
        journal_event("contract_strategy_decision", serde_json::json!({
            "contract_type": decision.contract_type.to_string(),
            "reason": decision.reason,
        }));
        decision
    }

    fn after_outcome(&mut self, outcome: Self::Outcome) -> StrategyDecision {
        self.rotator
            .record_result(outcome.contract_type, outcome.won);
        let decision = self.select(false);
        journal_event("contract_strategy_decision", serde_json::json!({
            "contract_type": decision.contract_type.to_string(),
            "reason": decision.reason,
            "previous_contract_type": outcome.contract_type.to_string(),
            "previous_won": outcome.won,
        }));
        decision
    }
}

#[cfg(test)]
mod interface_tests {
    use super::*;

    #[test]
    fn disabled_contract_families_cannot_be_selected() {
        let mut strategy = EnabledContractStrategy::new(false, true);
        assert_eq!(
            strategy.initial_decision().contract_type,
            ContractType::Accumulator
        );
        for contract_type in [
            ContractType::OverUnder,
            ContractType::MatchesDiff,
            ContractType::RiseFall,
        ] {
            assert_eq!(
                strategy
                    .after_outcome(ContractOutcome {
                        contract_type,
                        won: false,
                    })
                    .contract_type,
                ContractType::Accumulator
            );
        }
    }

    #[test]
    fn api_handle_implements_the_shared_broker_boundary() {
        let (command_tx, _command_rx) = mpsc::channel(1);
        let (event_tx, _) = broadcast::channel(1);
        let (shutdown_tx, _) = broadcast::channel(1);
        let broker = ApiHandle::new(command_tx, event_tx, shutdown_tx);

        let _commands: BrokerCommandSender = broker.command_sender();
        let _events: broadcast::Receiver<ApiEvent> = broker.subscribe_events();
        let _shutdown: broadcast::Sender<()> = broker.shutdown_sender();
    }
}
