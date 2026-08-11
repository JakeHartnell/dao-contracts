use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Coin;

// Re-export the orchestrator-facing query enum and its response types so
// downstream consumers can depend on either adapter through one type
// surface. Crates that only need the orchestrator-facing surface can
// `use gauge_adapter::msg::AdapterQueryMsg`.
pub use gauge_interface::{
    AdapterQueryMsg, AllOptionsResponse, CheckOptionResponse, SampleGaugeMsgsResponse,
};

#[cw_serde]
pub struct InstantiateMsg {
    /// Owner of the contract — the only address allowed to mutate the option
    /// list and budget, and to transfer / renounce ownership through the
    /// standard `cw_ownable` flow. Typically the DAO's core module.
    pub owner: String,
    /// Initial set of valid options.
    pub options: Vec<String>,
    /// Per-epoch budget distributed proportional to weights.
    pub epoch_budget: Coin,
}

#[cw_ownable::cw_ownable_execute]
#[cw_serde]
pub enum ExecuteMsg {
    /// Owner-only: add a new option to the valid set.
    AddOption { option: String },
    /// Owner-only: remove an option from the valid set.
    RemoveOption { option: String },
    /// Owner-only: replace the per-epoch budget.
    UpdateBudget { epoch_budget: Coin },
}

#[cw_ownable::cw_ownable_query]
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Inspect the stored config (current budget).
    #[returns(crate::state::Config)]
    Config {},
    /// All currently-valid options (proxy for `AdapterQueryMsg::AllOptions`).
    #[returns(AllOptionsResponse)]
    AllOptions {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Check whether `option` is in the valid set.
    #[returns(CheckOptionResponse)]
    CheckOption { option: String },
    /// Translate a selected set into `BankMsg::Send` payouts.
    #[returns(SampleGaugeMsgsResponse)]
    SampleGaugeMsgs {
        /// Option + weight pairs, weights summing to ≤ 1.0.
        selected: Vec<(String, cosmwasm_std::Decimal)>,
        epoch_budget: Option<cosmwasm_std::Uint128>,
        available_balance: Option<cosmwasm_std::Uint128>,
        denom: Option<String>,
    },
}

#[cfg(test)]
mod schema_smoke_tests {
    use super::*;
    use cosmwasm_std::{from_json, to_json_binary};

    #[test]
    fn representative_external_payloads_deserialize() {
        let _: InstantiateMsg = from_json(
            br#"{"owner":"owner","options":["recipient"],"epoch_budget":{"denom":"ujuno","amount":"5000"}}"#,
        )
        .unwrap();
        let _: ExecuteMsg = from_json(br#"{"add_option":{"option":"recipient2"}}"#).unwrap();
        let _: QueryMsg =
            from_json(br#"{"sample_gauge_msgs":{"selected":[["recipient","0.75"]]}}"#).unwrap();
        let _: SampleGaugeMsgsResponse = from_json(br#"{"execute":[]}"#).unwrap();

        for protocol in [
            AdapterQueryMsg::AllOptions {
                start_after: None,
                limit: Some(25),
            },
            AdapterQueryMsg::CheckOption {
                option: "recipient".to_owned(),
            },
            AdapterQueryMsg::SampleGaugeMsgs {
                selected: vec![("recipient".to_owned(), cosmwasm_std::Decimal::percent(50))],
                epoch_budget: None,
                available_balance: None,
                denom: None,
            },
        ] {
            let encoded = to_json_binary(&protocol).unwrap();
            let _: QueryMsg = from_json(encoded).unwrap();
        }
    }
}
