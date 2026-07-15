use cosmwasm_std::{coin, to_json_binary, Addr, Decimal, Empty, Uint128};
use cw4::Member;
use cw_multi_test::{AppResponse, BankSudo, Contract, ContractWrapper, Executor, SudoMsg};
use dao_hooks::nft_stake::{NftStakeChangedExecuteMsg, NftStakeChangedHookMsg};
use dao_hooks::stake::StakeChangedExecuteMsg;
use dao_voting::voting::Vote;

use super::suite::SuiteBuilder;
use crate::error::ContractError;
use crate::msg::{InstantiateMsg, VoteInfo};

const EPOCH: u64 = 7 * 86_400;

fn assert_mutation_event(response: &AppResponse, expected: &[(&str, &str)]) {
    assert!(
        response.events.iter().any(|event| {
            expected.iter().all(|(key, value)| {
                event
                    .attributes
                    .iter()
                    .any(|attribute| attribute.key == *key && attribute.value == *value)
            })
        }),
        "missing event attributes {expected:?} in {:?}",
        response.events
    );
}

fn cw20_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new_with_empty(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    ))
}

fn cw20_stake_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            cw20_stake::contract::execute,
            cw20_stake::contract::instantiate,
            cw20_stake::contract::query,
        )
        .with_migrate(cw20_stake::contract::migrate),
    )
}

fn cw20_voting_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            dao_voting_cw20_staked::contract::execute,
            dao_voting_cw20_staked::contract::instantiate,
            dao_voting_cw20_staked::contract::query,
        )
        .with_reply_empty(dao_voting_cw20_staked::contract::reply)
        .with_migrate(dao_voting_cw20_staked::contract::migrate),
    )
}

fn cw721_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new_with_empty(
        cw721_base::entry::execute,
        cw721_base::entry::instantiate,
        cw721_base::entry::query,
    ))
}

fn cw721_voting_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            dao_voting_cw721_staked::contract::execute,
            dao_voting_cw721_staked::contract::instantiate,
            dao_voting_cw721_staked::contract::query,
        )
        .with_reply_empty(dao_voting_cw721_staked::contract::reply)
        .with_migrate(dao_voting_cw721_staked::contract::migrate),
    )
}

fn native_voting_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            dao_voting_token_staked::contract::execute,
            dao_voting_token_staked::contract::instantiate,
            dao_voting_token_staked::contract::query,
        )
        .with_reply_empty(dao_voting_token_staked::contract::reply)
        .with_migrate(dao_voting_token_staked::contract::migrate),
    )
}

#[test]
fn add_option() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 200)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let gauge_id = 0; // first created gauge

    // gauge returns list all options; it does query adapter at initialization
    let options = suite.query_list_options(&gauge_contract, gauge_id).unwrap();
    assert_eq!(options.len(), 2);

    // add more valid options to gauge adapter
    suite
        .add_valid_option(&gauge_adapter, "addedoption1")
        .unwrap();
    suite
        .add_valid_option(&gauge_adapter, "addedoption2")
        .unwrap();

    // Voting members can add options
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "addedoption1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter2, gauge_id, "addedoption2")
        .unwrap();
    let options = suite.query_list_options(&gauge_contract, gauge_id).unwrap();
    // added options are automatically voted for by creators
    assert_eq!(
        options,
        vec![
            ("addedoption1".to_owned(), Uint128::zero()),
            ("addedoption2".to_owned(), Uint128::zero()),
            ("voter1".to_owned(), Uint128::zero()),
            ("voter2".to_owned(), Uint128::zero())
        ]
    );

    // add another valid option to gauge adapter
    suite
        .add_valid_option(&gauge_adapter, "addedoption3")
        .unwrap();
    // Non-voting members cannot add options
    let err = suite
        .add_option(&gauge_contract, "random_voter", gauge_id, "addedoption3")
        .unwrap_err();
    assert_eq!(
        ContractError::NoVotingPower("random_voter".to_owned()),
        err.downcast().unwrap()
    );
}

#[test]
fn remove_option() {
    let owner = "owner";
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 200)])
        .with_core_balance((1_000, "ujuno"))
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();
    let gauge_contract = proposal_modules[1].clone();

    let adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let gauge_id = 0; // first created gauge

    // gauge returns list all options; it does query adapter at initialization
    let options = suite.query_list_options(&gauge_contract, gauge_id).unwrap();
    assert_eq!(
        options,
        vec![
            ("voter1".to_owned(), Uint128::zero()),
            ("voter2".to_owned(), Uint128::zero())
        ]
    );

    // add new valid options to the gauge adapter
    suite.add_valid_option(&adapter, "addedoption1").unwrap();
    suite.add_valid_option(&adapter, "addedoption2").unwrap();

    // Voting members can add options
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "addedoption1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter2, gauge_id, "addedoption2")
        .unwrap();
    let options = suite.query_list_options(&gauge_contract, gauge_id).unwrap();
    // added options are automatically voted for by creators
    assert_eq!(
        options,
        vec![
            ("addedoption1".to_owned(), Uint128::zero()),
            ("addedoption2".to_owned(), Uint128::zero()),
            ("voter1".to_owned(), Uint128::zero()),
            ("voter2".to_owned(), Uint128::zero())
        ]
    );

    // Remove an option with an active voter. It disappears from selection
    // immediately, and replacement ignores the now-absent historical option.
    suite
        .place_vote(
            &gauge_contract,
            voter1,
            gauge_id,
            Some("addedoption1".to_owned()),
        )
        .unwrap();
    suite
        .remove_option(&gauge_contract, owner, gauge_id, "addedoption1")
        .unwrap();
    assert!(suite
        .query_selected_set(&gauge_contract, gauge_id)
        .unwrap()
        .is_empty());
    suite
        .place_vote(&gauge_contract, voter1, gauge_id, Some(voter1.to_owned()))
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge_contract, gauge_id).unwrap(),
        vec![(voter1.to_owned(), Uint128::new(100))]
    );

    // Anyone else cannot remove options
    let err = suite
        .remove_option(&gauge_contract, voter1, gauge_id, "addedoption2")
        .unwrap_err();

    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());

    let options = suite.query_list_options(&gauge_contract, gauge_id).unwrap();
    // one has been removed
    assert_eq!(
        options,
        vec![
            ("addedoption2".to_owned(), Uint128::zero()),
            ("voter1".to_owned(), Uint128::new(100)),
            ("voter2".to_owned(), Uint128::zero())
        ]
    );

    suite
        .place_vote(
            &gauge_contract,
            voter2,
            gauge_id,
            Some("addedoption2".to_owned()),
        )
        .unwrap();
    suite.invalidate_option(&adapter, "addedoption2").unwrap();
    // Adapter validity is pull-synchronized for the bounded selected set, so
    // a rejected marketing entry cannot remain payable through stale local
    // orchestrator state.
    assert_eq!(
        suite.query_selected_set(&gauge_contract, gauge_id).unwrap(),
        vec![(voter1.to_owned(), Uint128::new(100))]
    );

    // owner can remove an option that is no longer valid
    suite
        .remove_option(&gauge_contract, owner, gauge_id, "addedoption2")
        .unwrap();

    // Both options are now removed
    let options = suite.query_list_options(&gauge_contract, gauge_id).unwrap();
    assert_eq!(
        options,
        vec![
            ("voter1".to_owned(), Uint128::new(100)),
            ("voter2".to_owned(), Uint128::zero())
        ]
    );
    let health = suite.query_gauge_health(&gauge_contract, gauge_id).unwrap();
    assert!(health.scan_complete);
    assert!(health.consistent);
    assert_eq!(health.option_count, 2);
    assert_eq!(health.active_option_count, 2);
    assert_eq!(health.invalid_option_count, 0);
    assert_eq!(health.indexed_option_count, 2);
    assert_eq!(health.tally_sum, Uint128::new(100));
    assert_eq!(health.total_cast, Uint128::new(100));
    assert_eq!(health.mismatch_count, 0);

    // Epoch execution pull-checks adapter validity and pays only the remaining
    // active option; neither locally removed option can re-enter the sample.
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();
    // Removed voting power is subtracted from TOTAL_CAST, so the sole
    // remaining active option receives the full allocation.
    assert_eq!(suite.query_balance(voter1, "ujuno").unwrap(), 1000);
    assert_eq!(suite.query_balance(voter2, "ujuno").unwrap(), 0);
}

#[test]
fn removed_option_survives_real_cw4_power_hook_and_vote_replacement() {
    let owner = "owner";
    let voter = "voter";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter, 100)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter.to_owned(), None)
        .unwrap();
    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite.place_vote_single(voter, proposal, Vote::Yes).unwrap();
    suite.next_block();
    suite
        .execute_single_proposal(voter.to_owned(), proposal)
        .unwrap();
    let gauge = suite.query_proposal_modules().unwrap()[1].clone();

    // Register through the real DAO/CW4 governance path rather than invoking
    // the orchestrator's hook entry point directly.
    suite
        .propose_add_membership_change_hook(voter.to_owned(), gauge.clone())
        .unwrap();
    let hook_proposal = suite.list_proposals().unwrap()[1];
    suite
        .place_vote_single(voter, hook_proposal, Vote::Yes)
        .unwrap();
    suite.next_block();
    suite
        .execute_single_proposal(voter.to_owned(), hook_proposal)
        .unwrap();

    suite
        .instantiate_adapter_and_create_gauge(
            gauge.clone(),
            &[voter, "replacement"],
            (1_000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    suite
        .place_vote(&gauge, voter, 0, Some(voter.to_owned()))
        .unwrap();
    suite.remove_option(&gauge, owner, 0, voter).unwrap();

    // Updating the member exercises the real CW4 hook against a stored vote
    // for an absent option. It must neither underflow nor recreate the option.
    let changed = suite
        .force_update_members(
            vec![],
            vec![Member {
                addr: voter.to_owned(),
                weight: 250,
            }],
        )
        .unwrap();
    assert_mutation_event(
        &changed,
        &[
            ("action", "member_changed_hook"),
            ("hook_caller", suite.group_contract.as_str()),
            ("member_count", "1"),
            ("member", voter),
            ("updated_votes", "1"),
        ],
    );
    suite.next_block();
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());

    suite
        .place_vote(&gauge, voter, 0, Some("replacement".to_owned()))
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![("replacement".to_owned(), Uint128::new(250))]
    );
    let health = suite.query_gauge_health(&gauge, 0).unwrap();
    assert!(health.consistent);
    assert_eq!(health.invalid_option_count, 0);
}

#[test]
fn removal_handles_zero_and_many_active_voters() {
    let owner = "owner";
    let voters = ["voter1", "voter2", "voter3"];
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voters[0], 100), (voters[1], 200), (voters[2], 300)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voters[0].to_owned(), None)
        .unwrap();
    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    for voter in voters {
        suite.place_vote_single(voter, proposal, Vote::Yes).unwrap();
    }
    suite.next_block();
    suite
        .execute_single_proposal(voters[0].to_owned(), proposal)
        .unwrap();
    let gauge = suite.query_proposal_modules().unwrap()[1].clone();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge.clone(),
            &["unused", "crowded", "replacement"],
            (1_000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    // A zero-tally option is deleted without retaining unbounded tombstones.
    suite.remove_option(&gauge, owner, 0, "unused").unwrap();

    // All three voters reference the same option before it is removed.
    for voter in voters {
        suite
            .place_vote(&gauge, voter, 0, Some("crowded".to_owned()))
            .unwrap();
    }
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![("crowded".to_owned(), Uint128::new(600))]
    );
    suite.remove_option(&gauge, owner, 0, "crowded").unwrap();
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());

    // Replacing two votes and explicitly abstaining the third ignores the
    // absent historical option without underflowing tally or total cast.
    for voter in &voters[..2] {
        suite
            .place_vote(&gauge, *voter, 0, Some("replacement".to_owned()))
            .unwrap();
    }
    suite.place_vote(&gauge, voters[2], 0, None).unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![("replacement".to_owned(), Uint128::new(300))]
    );
    let health = suite.query_gauge_health(&gauge, 0).unwrap();
    assert!(health.consistent);
    assert_eq!(health.invalid_option_count, 0);
    assert_eq!(health.total_cast, Uint128::new(300));
    assert_eq!(health.tally_sum, Uint128::new(300));
}

fn simple_vote(
    voter: &str,
    option: &str,
    percentage: u64,
    cast: impl Into<Option<u64>>,
) -> VoteInfo {
    VoteInfo {
        voter: voter.to_string(),
        votes: vec![crate::state::Vote {
            option: option.to_string(),
            weight: Decimal::percent(percentage),
        }],
        cast: cast.into(),
    }
}

fn multi_vote(voter: &str, votes: &[(&str, u64)], cast: impl Into<Option<u64>>) -> VoteInfo {
    let votes = votes
        .iter()
        .map(|(opt, percentage)| crate::state::Vote {
            option: opt.to_string(),
            weight: Decimal::percent(*percentage),
        })
        .collect();
    VoteInfo {
        voter: voter.to_string(),
        votes,
        cast: cast.into(),
    }
}

#[test]
fn vote_for_option() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 200)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let gauge_id = 0; // first created gauge

    // vote for option from adapter (voting members are by default
    // options in adapter in this test suite)
    suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![(voter1.to_owned(), Decimal::percent(90))]),
        )
        .unwrap();
    assert_eq!(
        simple_vote(voter1, voter1, 90, suite.current_time()),
        suite
            .query_vote(&gauge_contract, gauge_id, voter1)
            .unwrap()
            .unwrap(),
    );
    // check tally is proper
    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    assert_eq!(selected_set, vec![(voter1.to_string(), Uint128::new(90))]);

    // add new valid options to the gauge adapter
    suite.add_valid_option(&gauge_adapter, "option1").unwrap();
    suite.add_valid_option(&gauge_adapter, "option2").unwrap();

    // change vote for option added through gauge
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option2")
        .unwrap();
    // voter2 drops vote as well
    suite
        .place_votes(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(vec![
                ("option1".to_owned(), Decimal::percent(50)),
                ("option2".to_owned(), Decimal::percent(50)),
            ]),
        )
        .unwrap();
    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 90, suite.current_time()),
            multi_vote(
                voter2,
                &[("option1", 50), ("option2", 50)],
                suite.current_time()
            ),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    // placing vote again overwrites previous ones
    suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![("option1".to_owned(), Decimal::percent(90))]),
        )
        .unwrap();
    suite
        .place_votes(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(vec![("option1".to_owned(), Decimal::percent(90))]),
        )
        .unwrap();
    assert_eq!(
        vec![
            simple_vote(voter1, "option1", 90, suite.current_time()),
            simple_vote(voter2, "option1", 90, suite.current_time()),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    // vote for non-existing option
    let err = suite
        .place_vote(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some("random option".to_owned()),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::OptionDoesNotExists {
            option: "random option".to_owned(),
            gauge_id
        },
        err.downcast().unwrap()
    );
}

#[test]
fn remove_vote() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 200)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let gauge_id = 0; // first created gauge

    // vote for option from adapter (voting members are by default
    // options in adapter in this test suite)
    suite
        .place_vote(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(voter1.to_owned()),
        )
        .unwrap();
    suite
        .place_vote(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(voter1.to_owned()),
        )
        .unwrap();
    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 100, suite.current_time()),
            simple_vote(voter2, voter1, 100, suite.current_time()),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    // remove vote
    suite
        .place_vote(&gauge_contract, voter1.to_owned(), gauge_id, None)
        .unwrap();
    assert_eq!(
        vec![simple_vote(voter2, voter1, 100, suite.current_time())],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );
    assert_eq!(
        suite.query_vote(&gauge_contract, gauge_id, voter1).unwrap(),
        None
    );
    assert_eq!(
        suite.query_vote(&gauge_contract, gauge_id, voter2).unwrap(),
        Some(simple_vote(voter2, voter1, 100, suite.current_time())),
    );

    // remove nonexisting vote
    let err = suite
        .place_vote(&gauge_contract, voter1.to_owned(), gauge_id, None)
        .unwrap_err();
    assert_eq!(
        ContractError::CannotRemoveNonexistingVote {},
        err.downcast().unwrap()
    );
}

#[test]
fn votes_stays_the_same_after_execution() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let reward_to_distribute = (1000, "ujuno");
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .with_core_balance(reward_to_distribute)
        .build();

    suite.next_block();
    let gauge_config = suite
        .instantiate_adapter_and_return_config(&[voter1, voter2], reward_to_distribute, None, None)
        .unwrap();
    suite
        .propose_update_proposal_module(voter1.to_string(), vec![gauge_config])
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();
    let gauge_contract = proposal_modules[1].clone();

    let gauge_id = 0;

    // vote for one of the options in gauge
    suite
        .place_vote(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(voter1.to_owned()), // option to vote for
        )
        .unwrap();
    suite
        .place_vote(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(voter1.to_owned()),
        )
        .unwrap();

    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    // voter1 was option voted for with two 100 voting powers combined
    assert_eq!(selected_set, vec![("voter1".to_owned(), Uint128::new(200))]);

    // before advancing specified epoch tally won't get sampled
    suite.advance_time(EPOCH);

    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 100, suite.current_time() - EPOCH),
            simple_vote(voter2, voter1, 100, suite.current_time() - EPOCH)
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 100, suite.current_time() - EPOCH),
            simple_vote(voter2, voter1, 100, suite.current_time() - EPOCH)
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );
    assert_eq!(
        suite.query_vote(&gauge_contract, gauge_id, voter1).unwrap(),
        Some(simple_vote(
            voter1,
            voter1,
            100,
            suite.current_time() - EPOCH
        )),
    );
    assert_eq!(
        suite.query_vote(&gauge_contract, gauge_id, voter2).unwrap(),
        Some(simple_vote(
            voter2,
            voter1,
            100,
            suite.current_time() - EPOCH
        )),
    );
}

#[test]
fn vote_for_max_capped_option() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            Some(Decimal::percent(10)),
            None,
        )
        .unwrap();

    let gauge_id = 0; // first created gauge

    // wait until epoch passes
    suite.advance_time(EPOCH);

    // add more valid options to gauge adapter
    suite.add_valid_option(&gauge_adapter, "option1").unwrap();
    suite.add_valid_option(&gauge_adapter, "option2").unwrap();

    // change vote for option added through gauge
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option2")
        .unwrap();

    // vote 100% voting power on 'voter1' option (100 weight)
    suite
        .place_vote(
            &gauge_contract,
            voter1,
            gauge_id,
            Some("option1".to_owned()),
        )
        .unwrap();
    // vote 10% voting power on 'voter2' option (10 weight)
    suite
        .place_votes(
            &gauge_contract,
            voter2,
            gauge_id,
            vec![("option2".to_owned(), Decimal::percent(10))],
        )
        .unwrap();

    assert_eq!(
        vec![
            multi_vote(voter1, &[("option1", 100)], suite.current_time()),
            multi_vote(voter2, &[("option2", 10)], suite.current_time()),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    // Despite 'option1' having 100 voting power and option2 having 10 voting power,
    // because of max vote cap set to 10% now 'option1' will have its power decreased to 10% * 110
    // 'option2' stays at 10 voting power as it was below 10% of total votes
    assert_eq!(
        selected_set,
        vec![
            ("option1".to_owned(), Uint128::new(11)),
            ("option2".to_owned(), Uint128::new(10))
        ]
    );
}

#[test]
fn membership_voting_power_change() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 200)])
        .with_core_balance((10000, "ujuno"))
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    // Setup membership change hooks
    suite
        .propose_add_membership_change_hook(voter1.to_string(), gauge_contract.clone())
        .unwrap();
    let proposal = suite.list_proposals().unwrap()[1];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    let gauge_id = 0; // first created gauge

    // vote for option from adapter (voting members are by default
    // options in adapter in this test suite)
    suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![(voter1.to_owned(), Decimal::percent(90))]),
        )
        .unwrap();
    assert_eq!(
        simple_vote(voter1, voter1, 90, suite.current_time()),
        suite
            .query_vote(&gauge_contract, gauge_id, voter1)
            .unwrap()
            .unwrap(),
    );
    // check tally is proper
    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    assert_eq!(selected_set, vec![(voter1.to_string(), Uint128::new(90))]);

    // add new valid options to the gauge adapter
    suite.add_valid_option(&gauge_adapter, "option1").unwrap();
    suite.add_valid_option(&gauge_adapter, "option2").unwrap();

    // change vote for option added through gauge
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option2")
        .unwrap();
    // voter2 drops vote as well
    suite
        .place_votes(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(vec![
                ("option1".to_owned(), Decimal::percent(50)),
                ("option2".to_owned(), Decimal::percent(50)),
            ]),
        )
        .unwrap();
    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 90, suite.current_time()),
            multi_vote(
                voter2,
                &[("option1", 50), ("option2", 50)],
                suite.current_time()
            ),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    let pre_voter1_takeover_gauge_set =
        suite.query_selected_set(&gauge_contract, gauge_id).unwrap();

    // Voter one's option is least popular
    assert_eq!(
        pre_voter1_takeover_gauge_set,
        vec![
            ("option2".to_string(), Uint128::new(100)),
            ("option1".to_string(), Uint128::new(100)),
            ("voter1".to_string(), Uint128::new(90))
        ]
    );

    // Force update members, giving voter 1 more power
    suite
        .force_update_members(
            vec![],
            vec![Member {
                addr: voter1.to_string(),
                weight: 1000,
            }],
        )
        .unwrap();
    suite.next_block();

    let current_gauge_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();

    // Currect selected set should be different than before voter1 got power
    assert_ne!(pre_voter1_takeover_gauge_set, current_gauge_set);

    // Voter1 option is now most popular
    assert_eq!(
        current_gauge_set,
        vec![
            ("voter1".to_string(), Uint128::new(900)),
            ("option2".to_string(), Uint128::new(100)),
            ("option1".to_string(), Uint128::new(100))
        ]
    );

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    // Force update members, kick out voter 1
    suite
        .force_update_members(vec![voter1.to_string()], vec![])
        .unwrap();
    suite.next_block();

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    let current_gauge_set = suite
        .query_last_executed_set(&gauge_contract, gauge_id)
        .unwrap();

    // Voter1 removed and so is the one thing they voted for
    assert_eq!(
        current_gauge_set,
        Some(vec![
            ("option2".to_string(), Uint128::new(100)),
            ("option1".to_string(), Uint128::new(100))
        ])
    );
}

#[test]
fn real_native_staked_module_updates_gauge_through_registered_hook() {
    let owner = "owner";
    let voter = "voter";
    let denom = "ustake";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(owner, 1)])
        .build();
    suite
        .app
        .sudo(SudoMsg::Bank(BankSudo::Mint {
            to_address: voter.to_owned(),
            amount: vec![coin(300, denom)],
        }))
        .unwrap();

    let voting_code = suite.app.store_code(native_voting_contract());
    let voting = suite
        .app
        .instantiate_contract(
            voting_code,
            Addr::unchecked(owner),
            &dao_voting_token_staked::msg::InstantiateMsg {
                token_info: dao_voting_token_staked::msg::TokenInfo::Existing {
                    denom: denom.to_owned(),
                },
                unstaking_duration: None,
                active_threshold: None,
            },
            &[],
            "real native voting module",
            None,
        )
        .unwrap();
    suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            voting.clone(),
            &dao_voting_token_staked::msg::ExecuteMsg::Stake {},
            &[coin(100, denom)],
        )
        .unwrap();
    suite.next_block();

    let gauge = suite
        .app
        .instantiate_contract(
            suite.gauge_code_id,
            Addr::unchecked(owner),
            &InstantiateMsg {
                voting_powers: voting.to_string(),
                hook_caller: voting.to_string(),
                owner: owner.to_owned(),
                gauges: None,
            },
            &[],
            "gauge with real native voting",
            Some(owner.to_owned()),
        )
        .unwrap();
    suite
        .app
        .execute_contract(
            Addr::unchecked(owner),
            voting.clone(),
            &dao_voting_token_staked::msg::ExecuteMsg::AddHook {
                addr: gauge.to_string(),
            },
            &[],
        )
        .unwrap();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge.clone(),
            &[voter, "replacement"],
            (1_000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    suite
        .place_votes(
            &gauge,
            voter.to_owned(),
            0,
            Some(vec![(voter.to_owned(), Decimal::one())]),
        )
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![(voter.to_owned(), Uint128::new(100))]
    );

    let stake_response = suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            voting.clone(),
            &dao_voting_token_staked::msg::ExecuteMsg::Stake {},
            &[coin(100, denom)],
        )
        .unwrap();
    assert_mutation_event(
        &stake_response,
        &[
            ("action", "stake_change_hook"),
            ("kind", "stake"),
            ("voter", voter),
            ("amount", "100"),
            ("updated_votes", "1"),
        ],
    );
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![(voter.to_owned(), Uint128::new(200))]
    );

    suite.remove_option(&gauge, owner, 0, voter).unwrap();
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());

    let unstake_response = suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            voting,
            &dao_voting_token_staked::msg::ExecuteMsg::Unstake {
                amount: Uint128::new(50),
            },
            &[],
        )
        .unwrap();
    assert_mutation_event(
        &unstake_response,
        &[
            ("action", "stake_change_hook"),
            ("kind", "unstake"),
            ("voter", voter),
            ("amount", "50"),
            ("updated_votes", "1"),
        ],
    );
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());
    suite.next_block();
    suite
        .place_vote(&gauge, voter, 0, Some("replacement".to_owned()))
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![("replacement".to_owned(), Uint128::new(150))]
    );
}

#[test]
fn real_cw20_staked_module_updates_gauge_through_registered_hook() {
    let owner = "owner";
    let voter = "voter";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(owner, 1)])
        .build();

    let token_code = suite.app.store_code(cw20_contract());
    let stake_code = suite.app.store_code(cw20_stake_contract());
    let voting_code = suite.app.store_code(cw20_voting_contract());
    let token = suite
        .app
        .instantiate_contract(
            token_code,
            Addr::unchecked(owner),
            &cw20_base::msg::InstantiateMsg {
                name: "Voting token".to_owned(),
                symbol: "VOTE".to_owned(),
                decimals: 6,
                initial_balances: vec![cw20::Cw20Coin {
                    address: voter.to_owned(),
                    amount: Uint128::new(300),
                }],
                mint: None,
                marketing: None,
            },
            &[],
            "voting token",
            None,
        )
        .unwrap();
    let staking = suite
        .app
        .instantiate_contract(
            stake_code,
            Addr::unchecked(owner),
            &cw20_stake::msg::InstantiateMsg {
                owner: Some(owner.to_owned()),
                token_address: token.to_string(),
                unstaking_duration: None,
            },
            &[],
            "real cw20 staking",
            None,
        )
        .unwrap();
    let voting = suite
        .app
        .instantiate_contract(
            voting_code,
            Addr::unchecked(owner),
            &dao_voting_cw20_staked::msg::InstantiateMsg {
                token_info: dao_voting_cw20_staked::msg::TokenInfo::Existing {
                    address: token.to_string(),
                    staking_contract: dao_voting_cw20_staked::msg::StakingInfo::Existing {
                        staking_contract_address: staking.to_string(),
                    },
                },
                active_threshold: None,
            },
            &[],
            "real cw20 voting module",
            None,
        )
        .unwrap();

    // Establish voting power before the gauge exists. This stake has no hook
    // receiver yet and proves subsequent PlaceVotes reads the real module.
    suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            token.clone(),
            &cw20::Cw20ExecuteMsg::Send {
                contract: staking.to_string(),
                amount: Uint128::new(100),
                msg: to_json_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
            },
            &[],
        )
        .unwrap();
    suite.next_block();

    let gauge = suite
        .app
        .instantiate_contract(
            suite.gauge_code_id,
            Addr::unchecked(owner),
            &InstantiateMsg {
                voting_powers: voting.to_string(),
                hook_caller: staking.to_string(),
                owner: owner.to_owned(),
                gauges: None,
            },
            &[],
            "gauge with real cw20 voting",
            Some(owner.to_owned()),
        )
        .unwrap();
    suite
        .app
        .execute_contract(
            Addr::unchecked(owner),
            staking.clone(),
            &cw20_stake::msg::ExecuteMsg::AddHook {
                addr: gauge.to_string(),
            },
            &[],
        )
        .unwrap();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge.clone(),
            &[voter, "replacement"],
            (1_000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    suite
        .place_votes(
            &gauge,
            voter.to_owned(),
            0,
            Some(vec![(voter.to_owned(), Decimal::one())]),
        )
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![(voter.to_owned(), Uint128::new(100))]
    );

    // These are real cw20 Send/Unstake calls. cw20-stake emits the hooks; the
    // test never calls the orchestrator hook entry point directly.
    let stake_response = suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            token,
            &cw20::Cw20ExecuteMsg::Send {
                contract: staking.to_string(),
                amount: Uint128::new(100),
                msg: to_json_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
            },
            &[],
        )
        .unwrap();
    assert_mutation_event(
        &stake_response,
        &[
            ("action", "stake_change_hook"),
            ("kind", "stake"),
            ("voter", voter),
            ("amount", "100"),
            ("updated_votes", "1"),
        ],
    );
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![(voter.to_owned(), Uint128::new(200))]
    );

    suite.remove_option(&gauge, owner, 0, voter).unwrap();
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());

    let unstake_response = suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            staking,
            &cw20_stake::msg::ExecuteMsg::Unstake {
                amount: Uint128::new(50),
            },
            &[],
        )
        .unwrap();
    assert_mutation_event(
        &unstake_response,
        &[
            ("action", "stake_change_hook"),
            ("kind", "unstake"),
            ("voter", voter),
            ("amount", "50"),
            ("updated_votes", "1"),
        ],
    );
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());
    suite.next_block();
    suite
        .place_vote(&gauge, voter, 0, Some("replacement".to_owned()))
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![("replacement".to_owned(), Uint128::new(150))]
    );
}

#[test]
fn synthetic_token_staking_hook_accounting() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let hook_caller = "token-staking-contract";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 200)])
        .with_core_balance((10000, "ujuno"))
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module_custom_hook_caller(
            voter1.to_string(),
            hook_caller.to_string(),
            None,
        )
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    let gauge_id = 0; // first created gauge

    // vote for option from adapter (voting members are by default
    // options in adapter in this test suite)
    suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![(voter1.to_owned(), Decimal::percent(90))]),
        )
        .unwrap();
    assert_eq!(
        simple_vote(voter1, voter1, 90, suite.current_time()),
        suite
            .query_vote(&gauge_contract, gauge_id, voter1)
            .unwrap()
            .unwrap(),
    );
    // check tally is proper
    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    assert_eq!(selected_set, vec![(voter1.to_string(), Uint128::new(90))]);

    // add new valid options to the gauge adapter
    suite.add_valid_option(&gauge_adapter, "option1").unwrap();
    suite.add_valid_option(&gauge_adapter, "option2").unwrap();

    // change vote for option added through gauge
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option2")
        .unwrap();
    // voter2 drops vote as well
    suite
        .place_votes(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(vec![
                ("option1".to_owned(), Decimal::percent(50)),
                ("option2".to_owned(), Decimal::percent(50)),
            ]),
        )
        .unwrap();
    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 90, suite.current_time()),
            multi_vote(
                voter2,
                &[("option1", 50), ("option2", 50)],
                suite.current_time()
            ),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    let pre_voter1_takeover_gauge_set =
        suite.query_selected_set(&gauge_contract, gauge_id).unwrap();

    // Voter one's option is least popular
    assert_eq!(
        pre_voter1_takeover_gauge_set,
        vec![
            ("option2".to_string(), Uint128::new(100)),
            ("option1".to_string(), Uint128::new(100)),
            ("voter1".to_string(), Uint128::new(90))
        ]
    );

    // Use hook caller to mock voter1 staking
    suite
        .app
        .execute_contract(
            Addr::unchecked(hook_caller),
            gauge_contract.clone(),
            &StakeChangedExecuteMsg::StakeChangeHook(
                dao_hooks::stake::StakeChangedHookMsg::Stake {
                    addr: Addr::unchecked(voter1),
                    amount: Uint128::new(900),
                },
            ),
            &[],
        )
        .unwrap();

    suite.next_block();

    let current_gauge_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();

    // Currect selected set should be different than before voter1 got power
    assert_ne!(pre_voter1_takeover_gauge_set, current_gauge_set);

    // Voter1 option is now most popular
    assert_eq!(
        current_gauge_set,
        vec![
            ("voter1".to_string(), Uint128::new(900)),
            ("option2".to_string(), Uint128::new(100)),
            ("option1".to_string(), Uint128::new(100))
        ]
    );

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    // Mock voter 1 unstaking
    suite
        .app
        .execute_contract(
            Addr::unchecked(hook_caller),
            gauge_contract.clone(),
            &StakeChangedExecuteMsg::StakeChangeHook(
                dao_hooks::stake::StakeChangedHookMsg::Unstake {
                    addr: Addr::unchecked(voter1),
                    amount: Uint128::new(1000),
                },
            ),
            &[],
        )
        .unwrap();
    suite.next_block();

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    let current_gauge_set = suite
        .query_last_executed_set(&gauge_contract, gauge_id)
        .unwrap();

    // Voter1 removed and so is the one thing they voted for
    assert_eq!(
        current_gauge_set,
        Some(vec![
            ("option2".to_string(), Uint128::new(100)),
            ("option1".to_string(), Uint128::new(100))
        ])
    );
}

#[test]
fn real_cw721_staked_module_updates_gauge_through_registered_hook() {
    let owner = "owner";
    let voter = "voter";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(owner, 1)])
        .build();

    let nft_code = suite.app.store_code(cw721_contract());
    let voting_code = suite.app.store_code(cw721_voting_contract());
    let nft = suite
        .app
        .instantiate_contract(
            nft_code,
            Addr::unchecked(owner),
            &cw721_base::msg::InstantiateMsg {
                name: "Voting NFTs".to_owned(),
                symbol: "VNFT".to_owned(),
                minter: owner.to_owned(),
            },
            &[],
            "voting NFTs",
            None,
        )
        .unwrap();
    for token_id in ["1", "2", "3"] {
        suite
            .app
            .execute_contract(
                Addr::unchecked(owner),
                nft.clone(),
                &cw721_base::msg::ExecuteMsg::<Empty, Empty>::Mint {
                    token_id: token_id.to_owned(),
                    owner: voter.to_owned(),
                    token_uri: None,
                    extension: Empty {},
                },
                &[],
            )
            .unwrap();
    }
    let voting = suite
        .app
        .instantiate_contract(
            voting_code,
            Addr::unchecked(owner),
            &dao_voting_cw721_staked::msg::InstantiateMsg {
                nft_contract: dao_voting_cw721_staked::msg::NftContract::Existing {
                    address: nft.to_string(),
                },
                unstaking_duration: None,
                active_threshold: None,
            },
            &[],
            "real cw721 voting module",
            None,
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            nft.clone(),
            &cw721_base::msg::ExecuteMsg::<Empty, Empty>::SendNft {
                contract: voting.to_string(),
                token_id: "1".to_owned(),
                msg: to_json_binary(&Empty {}).unwrap(),
            },
            &[],
        )
        .unwrap();
    suite.next_block();

    let gauge = suite
        .app
        .instantiate_contract(
            suite.gauge_code_id,
            Addr::unchecked(owner),
            &InstantiateMsg {
                voting_powers: voting.to_string(),
                hook_caller: voting.to_string(),
                owner: owner.to_owned(),
                gauges: None,
            },
            &[],
            "gauge with real cw721 voting",
            Some(owner.to_owned()),
        )
        .unwrap();
    suite
        .app
        .execute_contract(
            Addr::unchecked(owner),
            voting.clone(),
            &dao_voting_cw721_staked::msg::ExecuteMsg::AddHook {
                addr: gauge.to_string(),
            },
            &[],
        )
        .unwrap();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge.clone(),
            &[voter, "replacement"],
            (1_000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    suite
        .place_votes(
            &gauge,
            voter.to_owned(),
            0,
            Some(vec![(voter.to_owned(), Decimal::one())]),
        )
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![(voter.to_owned(), Uint128::one())]
    );

    let stake_response = suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            nft,
            &cw721_base::msg::ExecuteMsg::<Empty, Empty>::SendNft {
                contract: voting.to_string(),
                token_id: "2".to_owned(),
                msg: to_json_binary(&Empty {}).unwrap(),
            },
            &[],
        )
        .unwrap();
    assert_mutation_event(
        &stake_response,
        &[
            ("action", "nft_stake_change_hook"),
            ("kind", "stake"),
            ("voter", voter),
            ("token_id", "2"),
            ("token_count", "1"),
            ("updated_votes", "1"),
        ],
    );
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![(voter.to_owned(), Uint128::new(2))]
    );

    suite.remove_option(&gauge, owner, 0, voter).unwrap();
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());

    let unstake_response = suite
        .app
        .execute_contract(
            Addr::unchecked(voter),
            voting,
            &dao_voting_cw721_staked::msg::ExecuteMsg::Unstake {
                token_ids: vec!["1".to_owned()],
            },
            &[],
        )
        .unwrap();
    assert_mutation_event(
        &unstake_response,
        &[
            ("action", "nft_stake_change_hook"),
            ("kind", "unstake"),
            ("voter", voter),
            ("token_count", "1"),
            ("updated_votes", "1"),
        ],
    );
    assert!(suite.query_selected_set(&gauge, 0).unwrap().is_empty());
    suite
        .place_vote(&gauge, voter, 0, Some("replacement".to_owned()))
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge, 0).unwrap(),
        vec![("replacement".to_owned(), Uint128::one())]
    );
}

#[test]
fn synthetic_nft_staking_hook_accounting() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let hook_caller = "nft-staking-contract";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 1), (voter2, 2)])
        .with_core_balance((10000, "ujuno"))
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module_custom_hook_caller(
            voter1.to_string(),
            hook_caller.to_string(),
            None,
        )
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    let gauge_contract = proposal_modules[1].clone();

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    let gauge_id = 0; // first created gauge

    // vote for option from adapter (voting members are by default
    // options in adapter in this test suite)
    suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![(voter1.to_owned(), Decimal::percent(100))]),
        )
        .unwrap();
    assert_eq!(
        simple_vote(voter1, voter1, 100, suite.current_time()),
        suite
            .query_vote(&gauge_contract, gauge_id, voter1)
            .unwrap()
            .unwrap(),
    );
    // check tally is proper
    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    assert_eq!(selected_set, vec![(voter1.to_string(), Uint128::one())]);

    // add new valid options to the gauge adapter
    suite.add_valid_option(&gauge_adapter, "option1").unwrap();
    suite.add_valid_option(&gauge_adapter, "option2").unwrap();

    // change vote for option added through gauge
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option1")
        .unwrap();
    suite
        .add_option(&gauge_contract, voter1, gauge_id, "option2")
        .unwrap();
    // voter2 drops vote as well
    suite
        .place_votes(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(vec![
                ("option1".to_owned(), Decimal::percent(50)),
                ("option2".to_owned(), Decimal::percent(50)),
            ]),
        )
        .unwrap();
    assert_eq!(
        vec![
            simple_vote(voter1, voter1, 100, suite.current_time()),
            multi_vote(
                voter2,
                &[("option1", 50), ("option2", 50)],
                suite.current_time()
            ),
        ],
        suite.query_list_votes(&gauge_contract, gauge_id).unwrap()
    );

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    let pre_voter1_takeover_gauge_set =
        suite.query_selected_set(&gauge_contract, gauge_id).unwrap();

    // Voter one's option is least popular
    assert_eq!(
        pre_voter1_takeover_gauge_set,
        vec![
            ("voter1".to_string(), Uint128::new(1)),
            ("option2".to_string(), Uint128::new(1)),
            ("option1".to_string(), Uint128::new(1)),
        ]
    );

    // Mock voter 1 staking NFT
    suite
        .app
        .execute_contract(
            Addr::unchecked(hook_caller),
            gauge_contract.clone(),
            &NftStakeChangedExecuteMsg::NftStakeChangeHook(NftStakeChangedHookMsg::Stake {
                addr: Addr::unchecked(voter1),
                token_id: "1".to_string(),
            }),
            &[],
        )
        .unwrap();

    suite.next_block();

    let current_gauge_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();

    // Currect selected set should be different than before voter1 got power
    assert_ne!(pre_voter1_takeover_gauge_set, current_gauge_set);

    // Voter1 option is now most popular
    assert_eq!(
        current_gauge_set,
        vec![
            ("voter1".to_string(), Uint128::new(2)),
            ("option2".to_string(), Uint128::new(1)),
            ("option1".to_string(), Uint128::new(1))
        ]
    );

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    // Mock voter1 unstaking 2 nfts
    suite
        .app
        .execute_contract(
            Addr::unchecked(hook_caller),
            gauge_contract.clone(),
            &NftStakeChangedExecuteMsg::NftStakeChangeHook(NftStakeChangedHookMsg::Unstake {
                addr: Addr::unchecked(voter1),
                token_ids: vec!["1".to_string(), "2".to_string()],
            }),
            &[],
        )
        .unwrap();
    suite.next_block();

    // Execute after epoch passes
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    let current_gauge_set = suite
        .query_last_executed_set(&gauge_contract, gauge_id)
        .unwrap();

    // Voter1 removed and so is the one thing they voted for
    assert_eq!(
        current_gauge_set,
        Some(vec![
            ("option2".to_string(), Uint128::new(1)),
            ("option1".to_string(), Uint128::new(1))
        ])
    );
}

/// Regression test for the small-voting-power split (PR #844 checklist).
/// A voter with 1 unit of power who splits 50/50 across two options used to
/// have *both* options counted as 0 (Uint128 * Decimal rounds toward zero),
/// silently erasing their voice. The contract now rejects this with
/// `VoteWeightRoundsToZero` so the user can adjust.
#[test]
fn small_voting_power_rejects_round_to_zero_split() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        // voter1 has only 1 unit of power.
        .with_voting_members(&[(voter1, 1), (voter2, 10)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(voter1.to_string(), None)
        .unwrap();

    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(voter1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(voter2, proposal, Vote::Yes)
        .unwrap();

    suite.next_block();
    suite
        .execute_single_proposal(voter1.to_string(), proposal)
        .unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();
    let gauge_contract = proposal_modules[1].clone();

    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    let gauge_id = 0;

    // 50/50 split with 1 unit of power: both options round to 0 → reject.
    let err = suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![
                (voter1.to_owned(), Decimal::percent(50)),
                (voter2.to_owned(), Decimal::percent(50)),
            ]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::VoteWeightRoundsToZero {
            weight: Decimal::percent(50),
            voting_power: Uint128::new(1),
        },
        err.downcast().unwrap(),
    );

    // 100% to a single option works (1 * 1.0 = 1).
    suite
        .place_votes(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(vec![(voter1.to_owned(), Decimal::one())]),
        )
        .unwrap();

    // voter2 has 10 units, so a 50/50 split (5 + 5) is fine.
    suite
        .place_votes(
            &gauge_contract,
            voter2.to_owned(),
            gauge_id,
            Some(vec![
                (voter1.to_owned(), Decimal::percent(50)),
                (voter2.to_owned(), Decimal::percent(50)),
            ]),
        )
        .unwrap();
}
