#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Reply, Response, StdResult,
    SubMsg, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw_curves::DecimalPlaces;
use cw_storage_plus::Bound;
use cw_tokenfactory_issuer::msg::{
    DenomUnit, ExecuteMsg as IssuerExecuteMsg, InstantiateMsg as IssuerInstantiateMsg, Metadata,
};
use cw_utils::parse_reply_instantiate_data;

use crate::abc::{CommonsPhase, CurveFn};
use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::state::{
    CurveState, HatchContributionsMigrationProgress, CURVE_STATE, CURVE_TYPE,
    FUNDING_POOL_FORWARDING, HATCHERS, HATCH_CONTRIBUTIONS_MIGRATION, IS_PAUSED, MAX_SUPPLY, PHASE,
    PHASE_CONFIG, SUPPLY_DENOM, TEMP_SUPPLY, TOKEN_ISSUER_CONTRACT, TOTAL_HATCH_CONTRIBUTIONS,
};
use crate::{commands, queries};

// version info for migration info
pub(crate) const CONTRACT_NAME: &str = "crates.io:cw-abc";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_TOKEN_FACTORY_ISSUER_REPLY_ID: u64 = 0;

/// Hard protocol bound; callers cannot select an unbounded batch size.
pub(crate) const HATCH_MIGRATION_BATCH_SIZE: usize = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let InstantiateMsg {
        token_issuer_code_id,
        funding_pool_forwarding,
        supply,
        reserve,
        curve_type,
        phase_config,
        hatcher_allowlist,
    } = msg;

    phase_config.validate()?;

    // H-5: bound token decimals. rust_decimal supports at most scale 28.
    // internally; pow(38) ≈ 3.4e38 < u128::MAX, but pow(39) overflows.
    // Reject at instantiate so the contract is not bricked on first
    // buy or sell.
    const MAX_DECIMALS: u8 = 28;
    if supply.decimals > MAX_DECIMALS {
        return Err(ContractError::InvalidDecimals {
            decimals: supply.decimals,
            max: MAX_DECIMALS,
        });
    }
    if reserve.decimals > MAX_DECIMALS {
        return Err(ContractError::InvalidDecimals {
            decimals: reserve.decimals,
            max: MAX_DECIMALS,
        });
    }

    // Validate and store the funding pool forwarding
    if let Some(funding_pool_forwarding) = funding_pool_forwarding {
        FUNDING_POOL_FORWARDING.save(
            deps.storage,
            &deps.api.addr_validate(&funding_pool_forwarding)?,
        )?;
    }

    if supply.subdenom.is_empty() {
        return Err(ContractError::SupplyTokenError(
            "Token subdenom must not be empty.".to_string(),
        ));
    }

    if let Some(max_supply) = supply.max_supply {
        MAX_SUPPLY.save(deps.storage, &max_supply)?;
    }

    curve_type.validate(
        DecimalPlaces::new(supply.decimals, reserve.decimals),
        supply.max_supply,
    )?;
    // Save the curve type only after comprehensive validation.
    CURVE_TYPE.save(deps.storage, &curve_type)?;

    PHASE_CONFIG.save(deps.storage, &phase_config)?;

    // TODO don't hardcode this? Make it configurable? Hatch config can be optional
    PHASE.save(deps.storage, &CommonsPhase::Hatch)?;

    // Initialize owner to sender
    cw_ownable::initialize_owner(deps.storage, deps.api, Some(info.sender.as_str()))?;

    // Setup the curve state
    let normalization_places = DecimalPlaces::new(supply.decimals, reserve.decimals);
    let curve_state = CurveState::new(reserve.denom, normalization_places);

    // Save subdenom for handling in the reply
    TEMP_SUPPLY.save(deps.storage, &supply)?;

    // Instantiate cw-token-factory-issuer contract
    let msg = SubMsg::reply_always(
        WasmMsg::Instantiate {
            // Contract is immutable, no admin
            admin: None,
            code_id: token_issuer_code_id,
            msg: to_json_binary(&IssuerInstantiateMsg::NewToken {
                subdenom: supply.subdenom,
            })?,
            funds: info.funds.clone(),
            label: "cw-tokenfactory-issuer".to_string(),
        },
        INSTANTIATE_TOKEN_FACTORY_ISSUER_REPLY_ID,
    );

    // Save the curve state
    CURVE_STATE.save(deps.storage, &curve_state)?;
    TOTAL_HATCH_CONTRIBUTIONS.save(deps.storage, &Uint128::zero())?;

    // Set the paused state
    IS_PAUSED.save(deps.storage, &false)?;

    // L-3: set hatcher allowlist inline rather than via a self-call. The
    // previous self-call required an auth-bypass branch in
    // `update_hatch_allowlist` that could be a footgun for any future code
    // path that introduces additional self-calls.
    if let Some(hatcher_allowlist) = hatcher_allowlist {
        commands::update_hatch_allowlist(deps.branch(), env, info, hatcher_allowlist, vec![])?;
    }

    Ok(Response::default().add_submessage(msg))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    // Continuation is permissionless and callable even while paused. Every
    // ordinary execute remains locked while migration progress exists.
    if matches!(msg, ExecuteMsg::ContinueHatchContributionsMigration {}) {
        return continue_hatch_contributions_migration(deps, env);
    }
    if HATCH_CONTRIBUTIONS_MIGRATION
        .may_load(deps.storage)?
        .is_some()
    {
        return Err(ContractError::MigrationInProgress {});
    }

    // If paused, then only the owner can perform actions
    if IS_PAUSED.load(deps.storage)? {
        cw_ownable::assert_owner(deps.storage, &info.sender)
            .map_err(|_| ContractError::Paused {})?;
    }

    match msg {
        ExecuteMsg::Buy {
            min_tokens,
            deadline,
        } => commands::buy(deps, env, info, min_tokens, deadline),
        ExecuteMsg::Sell {
            min_reserve,
            deadline,
        } => commands::sell(deps, env, info, min_reserve, deadline),
        ExecuteMsg::Close {} => commands::close(deps, info),
        ExecuteMsg::Donate {} => commands::donate(deps, env, info),
        ExecuteMsg::Withdraw { amount } => commands::withdraw(deps, env, info, amount),
        ExecuteMsg::UpdateFundingPoolForwarding { address } => {
            commands::update_funding_pool_forwarding(deps, env, info, address)
        }
        ExecuteMsg::UpdateMaxSupply { max_supply } => {
            commands::update_max_supply(deps, info, max_supply)
        }
        ExecuteMsg::UpdateCurve { curve_type } => commands::update_curve(deps, info, curve_type),
        ExecuteMsg::UpdateHatchAllowlist { to_add, to_remove } => {
            commands::update_hatch_allowlist(deps, env, info, to_add, to_remove)
        }
        ExecuteMsg::TogglePause {} => commands::toggle_pause(deps, info),
        ExecuteMsg::UpdatePhaseConfig(update_msg) => {
            commands::update_phase_config(deps, env, info, update_msg)
        }
        ExecuteMsg::UpdateOwnership(action) => {
            commands::update_ownership(deps, &env, &info, action)
        }
        ExecuteMsg::AbortHatch {} => commands::abort_hatch(deps, env, info),
        ExecuteMsg::ClaimRefund {} => commands::claim_refund(deps, env, info),
        ExecuteMsg::ContinueHatchContributionsMigration {} => unreachable!(),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    // default implementation stores curve info as enum, you can do something else in a derived
    // contract and just pass in your custom curve to do_execute
    let curve_type = CURVE_TYPE.load(deps.storage)?;
    let curve_fn = curve_type.to_curve_fn();
    do_query(deps, env, msg, curve_fn)
}

/// We pull out logic here, so we can import this from another contract and set a different Curve.
/// This contacts sets a curve with an enum in [`InstantiateMsg`] and stored in state, but you may want
/// to use custom math not included - make this easily reusable
pub fn do_query(deps: Deps, _env: Env, msg: QueryMsg, curve_fn: CurveFn) -> StdResult<Binary> {
    match msg {
        // custom queries
        QueryMsg::CurveInfo {} => to_json_binary(&queries::query_curve_info(deps, curve_fn)?),
        QueryMsg::CurveType {} => to_json_binary(&CURVE_TYPE.load(deps.storage)?),
        QueryMsg::Denom {} => to_json_binary(&queries::get_denom(deps)?),
        QueryMsg::Donations { start_after, limit } => {
            to_json_binary(&queries::query_donations(deps, start_after, limit)?)
        }
        QueryMsg::FundingPoolForwarding {} => {
            to_json_binary(&FUNDING_POOL_FORWARDING.may_load(deps.storage)?)
        }
        QueryMsg::Hatchers { start_after, limit } => {
            to_json_binary(&queries::query_hatchers(deps, start_after, limit)?)
        }
        QueryMsg::Hatcher { addr } => to_json_binary(&queries::query_hatcher(deps, addr)?),
        QueryMsg::HatcherAllowlist {
            start_after,
            limit,
            config_type,
        } => to_json_binary(&queries::query_hatcher_allowlist(
            deps,
            start_after,
            limit,
            config_type,
        )?),
        QueryMsg::IsPaused {} => to_json_binary(&IS_PAUSED.load(deps.storage)?),
        QueryMsg::MaxSupply {} => to_json_binary(&queries::query_max_supply(deps)?),
        QueryMsg::Ownership {} => to_json_binary(&cw_ownable::get_ownership(deps.storage)?),
        QueryMsg::PhaseConfig {} => to_json_binary(&queries::query_phase_config(deps)?),
        QueryMsg::Phase {} => to_json_binary(&PHASE.load(deps.storage)?),
        QueryMsg::TokenContract {} => to_json_binary(&TOKEN_ISSUER_CONTRACT.load(deps.storage)?),
        QueryMsg::HatchContributionsMigrationStatus {} => {
            to_json_binary(&HATCH_CONTRIBUTIONS_MIGRATION.may_load(deps.storage)?)
        }
        QueryMsg::BuyQuote { payment } => to_json_binary(&queries::query_buy_quote(deps, payment)?),
        QueryMsg::SellQuote { payment } => {
            to_json_binary(&queries::query_sell_quote(deps, payment)?)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    // M-2: verify the stored cw2 contract name matches before overwriting.
    // Migrating from an unrelated contract type would silently overwrite
    // metadata otherwise.
    let stored = cw2::get_contract_version(deps.storage)?;
    if stored.contract != CONTRACT_NAME {
        return Err(ContractError::InvalidMigration {
            expected: CONTRACT_NAME.to_string(),
            actual: stored.contract,
        });
    }

    let curve_type = CURVE_TYPE.load(deps.storage)?;
    let curve_state = CURVE_STATE.load(deps.storage)?;
    curve_type.validate(curve_state.decimals, MAX_SUPPLY.may_load(deps.storage)?)?;
    let phase_config = PHASE_CONFIG.load(deps.storage)?;
    phase_config.validate()?;
    let phase = PHASE.load(deps.storage)?;

    if matches!(&phase, CommonsPhase::Hatch)
        && curve_state.reserve >= phase_config.hatch.initial_raise.max
    {
        return Err(ContractError::HatchRaiseCapAlreadyReached {
            max: phase_config.hatch.initial_raise.max,
            reserve: curve_state.reserve,
        });
    }

    if let Some(total_hatch_contributions) = TOTAL_HATCH_CONTRIBUTIONS.may_load(deps.storage)? {
        if matches!(&phase, CommonsPhase::Hatch) {
            assert_hatch_escrow(deps.as_ref(), &env, &curve_state, total_hatch_contributions)?;
        }
        // Heal a stale marker only after all synchronous active-Hatch checks.
        HATCH_CONTRIBUTIONS_MIGRATION.remove(deps.storage);
    } else if matches!(&phase, CommonsPhase::Hatch)
        && HATCH_CONTRIBUTIONS_MIGRATION
            .may_load(deps.storage)?
            .is_none()
    {
        // Install without touching HATCHERS. Fixed-size permissionless calls
        // reconstruct the aggregate after migration returns.
        HATCH_CONTRIBUTIONS_MIGRATION.save(
            deps.storage,
            &HatchContributionsMigrationProgress {
                cursor: None,
                partial_total: Uint128::zero(),
            },
        )?;
    }
    // A missing aggregate outside Hatch is irrelevant to future lifecycle
    // operations and deliberately requires no legacy scan.

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::default())
}

fn assert_hatch_escrow(
    deps: Deps,
    env: &Env,
    curve_state: &CurveState,
    required: Uint128,
) -> Result<(), ContractError> {
    let retained = deps
        .querier
        .query_balance(&env.contract.address, &curve_state.reserve_denom)?;
    if retained.amount < required {
        return Err(ContractError::InsufficientHatchEscrow {
            required,
            available: retained.amount,
        });
    }
    Ok(())
}

fn continue_hatch_contributions_migration(
    deps: DepsMut,
    env: Env,
) -> Result<Response, ContractError> {
    let mut progress = HATCH_CONTRIBUTIONS_MIGRATION
        .may_load(deps.storage)?
        .ok_or(ContractError::NoMigrationInProgress {})?;
    let start = progress.cursor.as_ref().map(Bound::exclusive);
    let batch = HATCHERS
        .range(deps.storage, start, None, Order::Ascending)
        .take(HATCH_MIGRATION_BATCH_SIZE + 1)
        .collect::<StdResult<Vec<_>>>()?;
    let has_more = batch.len() > HATCH_MIGRATION_BATCH_SIZE;
    let processed_len = batch.len().min(HATCH_MIGRATION_BATCH_SIZE);

    for (_, hatcher) in batch.iter().take(processed_len) {
        progress.partial_total = progress.partial_total.checked_add(hatcher.contributed)?;
    }

    if has_more {
        // Read one bounded lookahead record so an exact multiple of the batch
        // size can finalize without requiring an empty follow-up call. The
        // cursor is the last processed key, never the lookahead key.
        progress.cursor = batch.get(processed_len - 1).map(|(addr, _)| addr.clone());
        HATCH_CONTRIBUTIONS_MIGRATION.save(deps.storage, &progress)?;
        return Ok(Response::new()
            .add_attribute("action", "continue_hatch_contributions_migration")
            .add_attribute("processed", processed_len.to_string())
            .add_attribute("complete", "false"));
    }

    // Check before writing either progress or aggregate. On an underfunded
    // final batch, even direct unit-test storage remains at the prior cursor.
    let curve_state = CURVE_STATE.load(deps.storage)?;
    assert_hatch_escrow(deps.as_ref(), &env, &curve_state, progress.partial_total)?;
    TOTAL_HATCH_CONTRIBUTIONS.save(deps.storage, &progress.partial_total)?;
    HATCH_CONTRIBUTIONS_MIGRATION.remove(deps.storage);
    Ok(Response::new()
        .add_attribute("action", "continue_hatch_contributions_migration")
        .add_attribute("processed", batch.len().to_string())
        .add_attribute("complete", "true")
        .add_attribute("total", progress.partial_total))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        INSTANTIATE_TOKEN_FACTORY_ISSUER_REPLY_ID => {
            // Parse and save address of cw-tokenfactory-issuer
            let issuer_addr = parse_reply_instantiate_data(msg)?.contract_address;
            TOKEN_ISSUER_CONTRACT.save(deps.storage, &deps.api.addr_validate(&issuer_addr)?)?;

            // Load the temporary supply
            let supply = TEMP_SUPPLY.load(deps.storage)?;

            // Clear the temporary state. I-7: keep this immediately after
            // the load so future contributors don't introduce intervening
            // failure paths that leave TEMP_SUPPLY orphaned.
            TEMP_SUPPLY.remove(deps.storage);

            // Format the denom and save it
            // By default, the prefix for token factory tokens is "factory"
            let denom = format!("factory/{}/{}", &issuer_addr, supply.subdenom);

            SUPPLY_DENOM.save(deps.storage, &denom)?;

            // Msgs to be executed to finalize setup
            let mut msgs: Vec<WasmMsg> = vec![
                // Grant an allowance to mint
                WasmMsg::Execute {
                    contract_addr: issuer_addr.clone(),
                    msg: to_json_binary(&IssuerExecuteMsg::SetMinterAllowance {
                        address: env.contract.address.to_string(),
                        // Allowance needs to be max as this the is the amount of tokens
                        // the minter is allowed to mint, not to be confused with max supply
                        // which we have to enforce elsewhere.
                        allowance: Uint128::MAX,
                    })?,
                    funds: vec![],
                },
                // Grant an allowance to burn
                WasmMsg::Execute {
                    contract_addr: issuer_addr.clone(),
                    msg: to_json_binary(&IssuerExecuteMsg::SetBurnerAllowance {
                        address: env.contract.address.to_string(),
                        allowance: Uint128::MAX,
                    })?,
                    funds: vec![],
                },
            ];

            // If metadata, set it by calling the contract
            if let Some(metadata) = supply.metadata {
                // The first denom_unit must be the same as the tf and base denom.
                // It must have an exponent of 0. This the smallest unit of the token.
                // For more info: https://docs.cosmos.network/main/architecture/adr-024-coin-metadata
                let mut denom_units = vec![DenomUnit {
                    denom: denom.clone(),
                    exponent: 0,
                    aliases: vec![supply.subdenom],
                }];

                // Caller can optionally define additional units
                if let Some(mut additional_units) = metadata.additional_denom_units {
                    denom_units.append(&mut additional_units);
                }

                // Sort denom units by exponent, must be in ascending order
                denom_units.sort_by(|a, b| a.exponent.cmp(&b.exponent));

                msgs.push(WasmMsg::Execute {
                    contract_addr: issuer_addr.clone(),
                    msg: to_json_binary(&IssuerExecuteMsg::SetDenomMetadata {
                        metadata: Metadata {
                            description: metadata.description,
                            denom_units,
                            base: denom.clone(),
                            display: metadata.display,
                            name: metadata.name,
                            symbol: metadata.symbol,
                        },
                    })?,
                    funds: vec![],
                });
            }

            Ok(Response::new()
                .add_attribute("cw-tokenfactory-issuer-address", issuer_addr)
                .add_attribute("denom", denom)
                .add_messages(msgs))
        }
        _ => Err(ContractError::UnknownReplyId { id: msg.id }),
    }
}
