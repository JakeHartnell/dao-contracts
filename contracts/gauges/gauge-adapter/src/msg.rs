use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Uint128};
use cw20::Cw20ReceiveMsg;
use cw_denom::UncheckedDenom;
pub use gauge_interface::{
    AdapterQueryMsg, AllOptionsResponse, CheckOptionResponse, SampleGaugeMsgsResponse,
};

#[cw_serde]
pub struct InstantiateMsg {
    /// The owner of the contract — typically the DAO core module. Holds the
    /// authority to `ReturnDeposits`, `Reject`, and transfer / renounce
    /// ownership via the standard `cw_ownable` flow.
    pub owner: String,
    /// Deposit required for valid submission. This option allows to reduce spam.
    pub required_deposit: Option<AssetUnchecked>,
    /// Address of contract where each deposit is transferred.
    pub community_pool: String,
    /// Total reward amount.
    pub reward: AssetUnchecked,
}

#[cw_ownable::cw_ownable_execute]
#[cw_serde]
pub enum ExecuteMsg {
    /// Implements the Cw20 receiver interface.
    Receive(Cw20ReceiveMsg),
    /// Save info about team that wants to participate.
    /// Only for native tokens as required deposit.
    CreateSubmission {
        name: String,
        url: String,
        address: String,
    },
    /// Refunds active bonds in bounded batches of at most 50 submissions.
    /// Call repeatedly until the response reports `complete=true`.
    ReturnDeposits {},
    /// Owner-only: remove a submission from the registry. If a deposit was
    /// required, `soft = true` refunds it to the original sender (good-
    /// faith reject) and `soft = false` forfeits it to the community pool
    /// (spam / malicious reject). Rejecting the default community-pool
    /// submission is an error.
    Reject {
        /// Destination address of the submission to reject.
        submission: String,
        soft: bool,
    },
}

#[cw_serde]
pub enum ReceiveMsg {
    /// Save info about team that wants to participate.
    /// Only for CW20 tokens as required deposit.
    CreateSubmission {
        name: String,
        url: String,
        address: String,
    },
}

#[cw_serde]
pub struct MigrateMsg {}

// Queries copied from gauge-orchestrator for now (we could use a common crate for this).
/// Queries the gauge requires from the adapter contract in order to function.
#[cw_ownable::cw_ownable_query]
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(crate::state::Config)]
    Config {},
    #[returns(AllOptionsResponse)]
    AllOptions {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    #[returns(CheckOptionResponse)]
    CheckOption { option: String },
    #[returns(SampleGaugeMsgsResponse)]
    SampleGaugeMsgs {
        /// Option along with weight.
        /// Sum of all weights should be 1.0 (within rounding error).
        selected: Vec<(String, Decimal)>,
        epoch_budget: Option<Uint128>,
        available_balance: Option<Uint128>,
        denom: Option<String>,
    },

    // Marketing-gauge specific queries to help on frontend
    #[returns(SubmissionResponse)]
    Submission { address: String },
    #[returns(AllSubmissionsResponse)]
    AllSubmissions {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Return all submissions made by `sender`. Useful for "my submissions"
    /// views in registration flows.
    #[returns(AllSubmissionsResponse)]
    SubmissionsBySender {
        sender: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    #[returns(LiabilitiesResponse)]
    Liabilities {},
}

#[cw_serde]
pub struct SubmissionResponse {
    pub sender: Addr,
    pub name: String,
    pub url: String,
    pub address: Addr,
}

#[cw_serde]
pub struct AllSubmissionsResponse {
    pub submissions: Vec<SubmissionResponse>,
}

#[cw_serde]
pub struct LiabilitiesResponse {
    pub asset: Option<crate::state::Asset>,
    pub escrow_balance: Uint128,
    pub refund_cursor: Option<Addr>,
    pub refunds_complete: bool,
}

#[cw_serde]
pub struct AssetUnchecked {
    pub denom: UncheckedDenom,
    pub amount: Uint128,
}

#[cfg(test)]
mod schema_smoke_tests {
    use super::*;
    use cosmwasm_std::{from_json, to_json_binary};

    #[test]
    fn representative_external_payloads_deserialize() {
        let _: InstantiateMsg = from_json(
            br#"{"owner":"owner","required_deposit":{"denom":{"native":"ujuno"},"amount":"1000"},"community_pool":"community","reward":{"denom":{"native":"ujuno"},"amount":"5000"}}"#,
        )
        .unwrap();
        let _: ExecuteMsg = from_json(
            br#"{"create_submission":{"name":"DAO","url":"https://dao.example","address":"recipient"}}"#,
        )
        .unwrap();
        let _: QueryMsg = from_json(br#"{"liabilities":{}}"#).unwrap();
        let _: LiabilitiesResponse = from_json(
            br#"{"asset":null,"escrow_balance":"0","refund_cursor":null,"refunds_complete":false}"#,
        )
        .unwrap();
        let _: MigrateMsg = from_json(br#"{}"#).unwrap();

        for protocol in [
            AdapterQueryMsg::AllOptions {
                start_after: None,
                limit: Some(25),
            },
            AdapterQueryMsg::CheckOption {
                option: "recipient".to_owned(),
            },
            AdapterQueryMsg::SampleGaugeMsgs {
                selected: vec![("recipient".to_owned(), Decimal::percent(50))],
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
