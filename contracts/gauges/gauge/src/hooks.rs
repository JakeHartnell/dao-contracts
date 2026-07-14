use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, DepsMut, SubMsg, Uint128, WasmMsg};
use cw_hooks::Hooks;

use crate::error::ContractError;
use crate::state::Vote;
use crate::state::{NEXT_VOTE_HOOK_REPLY_ID, VOTE_HOOK_REPLIES};

/// Keep vote-hook replies in their own half of the u64 namespace so future
/// reply-driven features can reserve the lower half.
const FIRST_VOTE_HOOK_REPLY_ID: u64 = 1 << 63;

/// Hook fired from the orchestrator on `PlaceVotes`. Subscribers can use it
/// to drive participation rewards, off-chain notifications, analytics, etc.
///
/// Payload is the *new* state after the vote: `votes` may be empty (the
/// voter abstained / cleared their position). `voting_power` is the power
/// the orchestrator read for the voter on this call.
#[cw_serde]
pub enum GaugeVoteHookMsg {
    NewVotes {
        gauge_id: u64,
        voter: String,
        votes: Vec<Vote>,
        voting_power: Uint128,
        height: u64,
    },
}

/// Outer envelope that subscribed contracts will receive. Match on
/// `GaugeVoteHook(..)` to handle.
#[cw_serde]
pub enum GaugeVoteHookExecuteMsg {
    GaugeVoteHook(GaugeVoteHookMsg),
}

/// Build the `SubMsg` list for every currently-registered hook. Each submsg
/// uses a stable, namespaced reply ID associated with the hook address. Both
/// successful and failed calls reply so the temporary association is cleaned.
pub fn new_vote_hook_msgs(
    hooks: Hooks,
    deps: DepsMut,
    gauge_id: u64,
    voter: Addr,
    votes: Vec<Vote>,
    voting_power: Uint128,
    height: u64,
) -> Result<Vec<SubMsg>, ContractError> {
    let msg = to_json_binary(&GaugeVoteHookExecuteMsg::GaugeVoteHook(
        GaugeVoteHookMsg::NewVotes {
            gauge_id,
            voter: voter.into_string(),
            votes,
            voting_power,
            height,
        },
    ))?;
    let addresses = hooks.query_hooks(deps.as_ref())?.hooks;
    let mut next = NEXT_VOTE_HOOK_REPLY_ID
        .may_load(deps.storage)?
        .unwrap_or(FIRST_VOTE_HOOK_REPLY_ID);
    let mut messages = Vec::with_capacity(addresses.len());
    for address in addresses {
        let address = deps.api.addr_validate(&address)?;
        let id = next;
        next = next
            .checked_add(1)
            .ok_or(ContractError::VoteHookReplyIdExhausted {})?;
        VOTE_HOOK_REPLIES.save(deps.storage, id, &address)?;
        let execute = WasmMsg::Execute {
            contract_addr: address.to_string(),
            msg: msg.clone(),
            funds: vec![],
        };
        messages.push(SubMsg::reply_always(execute, id));
    }
    NEXT_VOTE_HOOK_REPLY_ID.save(deps.storage, &next)?;
    Ok(messages)
}
