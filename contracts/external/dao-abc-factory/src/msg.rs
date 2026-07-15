use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Binary;
use cw_abc::msg::InstantiateMsg as AbcInstantiateMsg;

#[cw_serde]
pub struct ApprovedCode {
    pub code_id: u64,
    pub checksum: Binary,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub approved_abc: Vec<ApprovedCode>,
    pub approved_token_issuers: Vec<ApprovedCode>,
}

#[cw_serde]
pub enum ExecuteMsg {
    AbcFactory {
        instantiate_msg: AbcInstantiateMsg,
        code_id: u64,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(dao_interface::voting::InfoResponse)]
    Info {},
    #[returns(Vec<cosmwasm_std::Addr>)]
    Daos {
        start_after: Option<String>,
        limit: Option<u32>,
    },
}
