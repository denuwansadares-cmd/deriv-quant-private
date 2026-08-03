#[cfg(test)]
mod schema_contract_tests {
    #[test]
    fn pinned_deriv_contract_manifest_is_valid_and_complete() {
        let pin: serde_json::Value = serde_json::from_str(include_str!(
            "../../schemas/deriv/pinned-contracts.json"
        ))
        .expect("pinned Deriv contract manifest must be valid JSON");
        assert_eq!(pin["rest_openapi"], "3.1.0");
        assert_eq!(pin["archive_name"], "schemas.zip");
        for contract in [
            "proposal_request_required",
            "proposal_response_required",
            "buy_request_required",
            "buy_response_required",
            "sell_request_required",
            "sell_response_required",
        ] {
            assert!(
                pin["websocket_contracts"][contract]
                    .as_array()
                    .is_some_and(|fields| !fields.is_empty()),
                "missing pinned contract {contract}"
            );
        }
    }

    #[test]
    fn active_wire_examples_satisfy_pinned_required_fields() {
        let proposal = serde_json::json!({
            "proposal": 1, "amount": 1.0, "basis": "stake", "currency": "USD",
            "contract_type": "ACCU", "underlying_symbol": "R_100"
        });
        let buy = serde_json::json!({"buy": "proposal-id", "price": 1.0});
        let sell = serde_json::json!({"sell": "contract-id", "price": 0});
        for (payload, required) in [
            (&proposal, &["proposal", "amount", "basis", "currency", "contract_type", "underlying_symbol"][..]),
            (&buy, &["buy", "price"][..]),
            (&sell, &["sell", "price"][..]),
        ] {
            for field in required {
                assert!(payload.get(*field).is_some(), "wire payload missing {field}");
            }
        }
    }
}
