#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    ensure, to_json_binary, Addr, Binary, CosmosMsg, Deps, DepsMut, Empty, Env, MessageInfo, Order,
    Reply, Response, StdResult, SubMsg, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw_abc::msg::{
    DenomResponse, ExecuteMsg as AbcExecuteMsg, InstantiateMsg as AbcInstantiateMsg,
    QueryMsg as AbcQueryMsg,
};
use cw_storage_plus::{Bound, Item, Map};
use cw_utils::parse_reply_instantiate_data;
use dao_interface::{
    msg::QueryMsg as DaoQueryMsg, state::ModuleInstantiateCallback, token::TokenFactoryCallback,
    voting::Query as VotingModuleQueryMsg,
};

use crate::{
    error::ContractError,
    msg::{ExecuteMsg, InstantiateMsg, QueryMsg},
};

// I-4: align with the cw-abc convention.
const CONTRACT_NAME: &str = "crates.io:dao-abc-factory";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_ABC_REPLY_ID: u64 = 1;

const DAOS: Map<Addr, Empty> = Map::new("daos");
const CURRENT_DAO: Item<Addr> = Item::new("current_dao");
const VOTING_MODULE: Item<Addr> = Item::new("voting_module");
const APPROVED_ABC: Map<u64, Binary> = Map::new("approved_abc");
const APPROVED_ISSUERS: Map<u64, Binary> = Map::new("approved_issuers");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ensure!(
        !msg.approved_abc.is_empty() && !msg.approved_token_issuers.is_empty(),
        ContractError::Std(cosmwasm_std::StdError::generic_err(
            "approval lists must not be empty"
        ))
    );
    for approval in msg.approved_abc {
        ensure!(
            chain_checksum(deps.as_ref(), approval.code_id)? == approval.checksum,
            ContractError::ChecksumMismatch {
                kind: "abc".into(),
                code_id: approval.code_id
            }
        );
        APPROVED_ABC.save(deps.storage, approval.code_id, &approval.checksum)?;
    }
    for approval in msg.approved_token_issuers {
        ensure!(
            chain_checksum(deps.as_ref(), approval.code_id)? == approval.checksum,
            ContractError::ChecksumMismatch {
                kind: "token issuer".into(),
                code_id: approval.code_id
            }
        );
        APPROVED_ISSUERS.save(deps.storage, approval.code_id, &approval.checksum)?;
    }

    Ok(Response::new().add_attribute("method", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AbcFactory {
            code_id,
            instantiate_msg,
        } => execute_token_factory_factory(deps, env, info, code_id, instantiate_msg),
    }
}

pub fn execute_token_factory_factory(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    code_id: u64,
    msg: AbcInstantiateMsg,
) -> Result<Response, ContractError> {
    require_approved(deps.as_ref(), &APPROVED_ABC, "abc", code_id)?;
    require_approved(
        deps.as_ref(),
        &APPROVED_ISSUERS,
        "token issuer",
        msg.token_issuer_code_id,
    )?;
    // Reverse-handshake authentication: the caller (info.sender) claims to
    // be a DAO's voting module. We accept the claim only if (a) the caller
    // responds to `VotingModuleQueryMsg::Dao` with some DAO address, and
    // (b) that DAO responds to `QueryMsg::VotingModule` with the caller's
    // address. This closes the impostor-voting-module attack where any
    // contract could spoof the relationship and have ownership transferred
    // to an attacker-chosen address.
    let dao: Addr = deps
        .querier
        .query_wasm_smart(&info.sender, &VotingModuleQueryMsg::Dao {})?;
    let claimed_voting_module: Addr = deps
        .querier
        .query_wasm_smart(&dao, &DaoQueryMsg::VotingModule {})?;
    ensure!(
        claimed_voting_module == info.sender,
        ContractError::Unauthorized {}
    );

    // Save voting module address
    VOTING_MODULE.save(deps.storage, &info.sender)?;

    DAOS.save(deps.storage, dao.clone(), &Empty {})?;
    CURRENT_DAO.save(deps.storage, &dao)?;

    // Instantiate new contract, further setup is handled in the
    // SubMsg reply.
    let msg = SubMsg::reply_on_success(
        WasmMsg::Instantiate {
            // No admin as we want the bonding curve contract to be immutable
            admin: None,
            code_id,
            msg: to_json_binary(&msg)?,
            funds: vec![],
            label: "cw_abc".to_string(),
        },
        INSTANTIATE_ABC_REPLY_ID,
    );

    Ok(Response::new().add_submessage(msg))
}

fn chain_checksum(deps: Deps, code_id: u64) -> Result<Binary, ContractError> {
    let info: cosmwasm_std::CodeInfoResponse =
        deps.querier
            .query(&cosmwasm_std::QueryRequest::Wasm(WasmQuery::CodeInfo {
                code_id,
            }))?;
    Ok(Binary::from(info.checksum.to_vec()))
}

fn require_approved(
    deps: Deps,
    map: &Map<u64, Binary>,
    kind: &str,
    code_id: u64,
) -> Result<(), ContractError> {
    let expected =
        map.may_load(deps.storage, code_id)?
            .ok_or_else(|| ContractError::UnapprovedCode {
                kind: kind.into(),
                code_id,
            })?;
    ensure!(
        chain_checksum(deps, code_id)? == expected,
        ContractError::ChecksumMismatch {
            kind: kind.into(),
            code_id
        }
    );
    Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Info {} => query_info(deps),
        QueryMsg::Daos { start_after, limit } => query_daos(deps, start_after, limit),
    }
}

pub fn query_info(deps: Deps) -> StdResult<Binary> {
    let info = cw2::get_contract_version(deps.storage)?;
    to_json_binary(&dao_interface::voting::InfoResponse { info })
}

pub fn query_daos(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<Binary> {
    to_json_binary(
        &DAOS
            .keys(
                deps.storage,
                None,
                start_after
                    .map(|s| deps.api.addr_validate(&s))
                    .transpose()?
                    .map(Bound::exclusive),
                Order::Descending,
            )
            .take(limit.unwrap_or(25) as usize)
            .collect::<StdResult<Vec<Addr>>>()?,
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        INSTANTIATE_ABC_REPLY_ID => {
            // Load and clear the temp DAO/voting-module state so it does
            // not linger between calls (TempState semantics).
            let dao = CURRENT_DAO.load(deps.storage)?;
            CURRENT_DAO.remove(deps.storage);
            VOTING_MODULE.remove(deps.storage);

            // Parse issuer address from instantiate reply
            let abc_addr = parse_reply_instantiate_data(msg)?.contract_address;

            // Query for denom
            let denom: DenomResponse = deps
                .querier
                .query_wasm_smart(abc_addr.clone(), &AbcQueryMsg::Denom {})?;

            // Query for token contract
            let token_contract: Addr = deps
                .querier
                .query_wasm_smart(abc_addr.clone(), &AbcQueryMsg::TokenContract {})?;

            // Update the owner to be the DAO
            let msg = WasmMsg::Execute {
                contract_addr: abc_addr.clone(),
                msg: to_json_binary(&AbcExecuteMsg::UpdateOwnership(
                    cw_ownable::Action::TransferOwnership {
                        new_owner: dao.to_string(),
                        expiry: None,
                    },
                ))?,
                funds: vec![],
            };

            // DAO must accept ownership transfer. Here we include a
            // ModuleInstantiateCallback message that will be called by the
            // dao-dao-core contract when voting module instantiation is
            // complete.
            let callback = ModuleInstantiateCallback {
                msgs: vec![CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: abc_addr.clone(),
                    msg: to_json_binary(&AbcExecuteMsg::UpdateOwnership(
                        cw_ownable::Action::AcceptOwnership {},
                    ))?,
                    funds: vec![],
                })],
            };

            // Responses for `dao-voting-token-staked` MUST include a
            // TokenFactoryCallback.
            Ok(Response::new()
                .add_message(msg)
                .set_data(to_json_binary(&TokenFactoryCallback {
                    denom: denom.denom,
                    token_contract: Some(token_contract.to_string()),
                    module_instantiate_callback: Some(callback),
                })?))
        }
        _ => Err(ContractError::UnknownReplyId { id: msg.id }),
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;
    use cosmwasm_std::{
        testing::mock_dependencies, ContractResult, HexBinary, QuerierResult, SystemResult,
    };

    fn with_code_checksums(
        deps: &mut cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
    ) {
        deps.querier.update_wasm(|query| -> QuerierResult {
            match query {
                WasmQuery::CodeInfo { code_id } => {
                    let checksum = HexBinary::from(vec![*code_id as u8; 32]);
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&cosmwasm_std::CodeInfoResponse::new(
                            *code_id,
                            "creator".into(),
                            checksum,
                        ))
                        .unwrap(),
                    ))
                }
                _ => panic!("unexpected query"),
            }
        });
    }

    #[test]
    fn rejects_unapproved_abc_and_issuer_code_ids() {
        let deps = mock_dependencies();
        assert!(matches!(
            require_approved(deps.as_ref(), &APPROVED_ABC, "abc", 7),
            Err(ContractError::UnapprovedCode { code_id: 7, .. })
        ));
        assert!(matches!(
            require_approved(deps.as_ref(), &APPROVED_ISSUERS, "token issuer", 9),
            Err(ContractError::UnapprovedCode { code_id: 9, .. })
        ));
    }

    #[test]
    fn rejects_mismatched_abc_and_issuer_checksums() {
        let mut deps = mock_dependencies();
        with_code_checksums(&mut deps);
        APPROVED_ABC
            .save(&mut deps.storage, 7, &Binary::from(vec![0u8; 32]))
            .unwrap();
        APPROVED_ISSUERS
            .save(&mut deps.storage, 9, &Binary::from(vec![0u8; 32]))
            .unwrap();

        assert!(matches!(
            require_approved(deps.as_ref(), &APPROVED_ABC, "abc", 7),
            Err(ContractError::ChecksumMismatch { code_id: 7, .. })
        ));
        assert!(matches!(
            require_approved(deps.as_ref(), &APPROVED_ISSUERS, "token issuer", 9),
            Err(ContractError::ChecksumMismatch { code_id: 9, .. })
        ));
    }
}
