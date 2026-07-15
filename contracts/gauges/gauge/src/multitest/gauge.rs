use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_multi_test::AppResponse;
use dao_voting::voting::Vote;

use super::suite::{Suite, SuiteBuilder};

use crate::error::ContractError;
use crate::msg::{GaugeMigrationConfig, GaugeResponse};

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

#[test]
fn mutation_events_expose_stable_indexer_fields() {
    let voter = "voter";
    let owner = "owner";
    let keeper = "keeper";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter, 100)])
        .with_core_balance((1_000, "ujuno"))
        .build();
    let gauge = init_gauge(&mut suite, &[voter]);

    let (adapter, created) = suite
        .instantiate_adapter_and_create_gauge_with_response(
            gauge.clone(),
            &[voter],
            (1_000, "ujuno"),
            None,
            Some(100),
        )
        .unwrap();
    assert_mutation_event(
        &created,
        &[
            ("action", "create_gauge"),
            ("sender", owner),
            ("gauge_id", "0"),
        ],
    );

    suite.add_valid_option(&adapter, "temporary").unwrap();
    let added = suite.add_option(&gauge, voter, 0, "temporary").unwrap();
    assert_mutation_event(
        &added,
        &[
            ("action", "add_option"),
            ("sender", voter),
            ("gauge_id", "0"),
            ("option", "temporary"),
        ],
    );
    suite.invalidate_option(&adapter, "temporary").unwrap();
    let removed = suite.remove_option(&gauge, owner, 0, "temporary").unwrap();
    assert_mutation_event(
        &removed,
        &[
            ("action", "remove_option"),
            ("sender", owner),
            ("gauge_id", "0"),
            ("option", "temporary"),
        ],
    );

    let updated = suite
        .update_gauge(
            owner,
            gauge.clone(),
            0,
            Some(EPOCH + 1),
            Some(Decimal::percent(10)),
            Some(5),
            Some(Decimal::percent(50)),
        )
        .unwrap();
    assert_mutation_event(
        &updated,
        &[
            ("action", "update_gauge"),
            ("sender", owner),
            ("gauge_id", "0"),
            ("epoch_size", "604801"),
            ("max_options_selected", "5"),
        ],
    );

    let stopped = suite.stop_gauge(&gauge, owner, 0).unwrap();
    assert_mutation_event(
        &stopped,
        &[
            ("action", "stop_gauge"),
            ("sender", owner),
            ("gauge_id", "0"),
        ],
    );
    let resumed = suite.resume_gauge(&gauge, owner, 0).unwrap();
    assert_mutation_event(
        &resumed,
        &[
            ("action", "resume_gauge"),
            ("sender", owner),
            ("gauge_id", "0"),
        ],
    );

    let voted = suite
        .place_votes(
            &gauge,
            voter,
            0,
            Some(vec![(voter.to_owned(), Decimal::one())]),
        )
        .unwrap();
    assert_mutation_event(
        &voted,
        &[
            ("action", "place_vote"),
            ("sender", voter),
            ("gauge_id", "0"),
            ("option_count", "1"),
            ("voting_power", "100"),
        ],
    );

    suite.advance_time(EPOCH + 1);
    let reset = suite.reset_gauge(keeper, &gauge, 0, 100).unwrap();
    assert_mutation_event(
        &reset,
        &[
            ("action", "reset_gauge"),
            ("sender", keeper),
            ("gauge_id", "0"),
            // Removed options are deleted immediately; reset processes only
            // the remaining active voter option.
            ("processed", "1"),
            ("complete", "true"),
        ],
    );
    suite
        .place_votes(
            &gauge,
            voter,
            0,
            Some(vec![(voter.to_owned(), Decimal::one())]),
        )
        .unwrap();
    let executed = suite.execute_options(&gauge, keeper, 0).unwrap();
    assert_mutation_event(
        &executed,
        &[
            ("action", "execute_tally"),
            ("sender", keeper),
            ("gauge_id", "0"),
            ("selected_count", "1"),
            ("message_count", "1"),
        ],
    );
}

#[test]
fn create_gauge() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .build();

    let gauge_contract = init_gauge(&mut suite, &[voter1, voter2]);

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let response = suite.query_gauge(gauge_contract, 0).unwrap();
    assert_eq!(
        response,
        GaugeResponse {
            id: 0,
            title: "gauge".to_owned(),
            adapter: gauge_adapter.to_string(),
            epoch_size: EPOCH,
            min_percent_selected: Some(Decimal::percent(5)),
            max_options_selected: 10,
            max_available_percentage: None,
            is_stopped: false,
            next_epoch: suite.current_time() + 7 * 86400,
            reset: None,
        }
    );
}

#[test]
fn gauge_can_upgrade_from_older_version() {
    let voter1 = "voter1";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100)])
        .build();

    let gauge_contract = init_gauge(&mut suite, &[voter1]);

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &["option1", "option2"],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    // The suite instantiates the gauge with an older cw2 version and migrates
    // it to a distinct current code ID.
    suite.auto_migrate_gauge(&gauge_contract, None).unwrap();

    let response = suite.query_gauge(gauge_contract, 0).unwrap();
    assert_eq!(
        response,
        GaugeResponse {
            id: 0,
            title: "gauge".to_owned(),
            adapter: gauge_adapter.to_string(),
            epoch_size: EPOCH,
            min_percent_selected: Some(Decimal::percent(5)),
            max_options_selected: 10,
            max_available_percentage: None,
            is_stopped: false,
            next_epoch: suite.current_time() + 7 * 86400,
            reset: None,
        }
    );
}

#[test]
fn gauge_migrate_with_next_epochs() {
    let voter1 = "voter1";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100)])
        .build();

    let gauge_contract = init_gauge(&mut suite, &[voter1]);

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &["option1", "option2"],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    // previous settings
    let response = suite.query_gauge(gauge_contract.clone(), 0).unwrap();
    assert_eq!(
        response,
        GaugeResponse {
            id: 0,
            title: "gauge".to_owned(),
            adapter: gauge_adapter.to_string(),
            epoch_size: EPOCH,
            min_percent_selected: Some(Decimal::percent(5)),
            max_options_selected: 10,
            max_available_percentage: None,
            is_stopped: false,
            next_epoch: suite.current_time() + 7 * 86400,
            reset: None,
        }
    );

    // now let's migrate the gauge and make sure nothing breaks
    let gauge_id = 0;
    // change next epoch from 7 to 14 days
    suite
        .auto_migrate_gauge(
            &gauge_contract,
            vec![(
                gauge_id,
                GaugeMigrationConfig {
                    next_epoch: Some(suite.current_time() + 14 * 86400),
                    reset: None,
                },
            )],
        )
        .unwrap();

    let response = suite.query_gauge(gauge_contract.clone(), 0).unwrap();
    assert_eq!(
        response,
        GaugeResponse {
            id: 0,
            title: "gauge".to_owned(),
            adapter: gauge_adapter.to_string(),
            epoch_size: EPOCH,
            min_percent_selected: Some(Decimal::percent(5)),
            max_options_selected: 10,
            max_available_percentage: None,
            is_stopped: false,
            next_epoch: suite.current_time() + 14 * 86400,
            reset: None,
        }
    );

    // try to migrate updating next epoch on nonexisting gauge_id
    // actually generic error makes it more difficult to debug in presentable form, I think this is
    // enough
    let _err = suite
        .auto_migrate_gauge(
            &gauge_contract,
            vec![(
                420,
                GaugeMigrationConfig {
                    next_epoch: Some(suite.current_time() + 14 * 86400),
                    reset: None,
                },
            )],
        )
        .unwrap_err();
}

/// attach adaptor in instantiate
#[test]
fn execute_gauge() {
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

    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    assert_eq!(
        suite.query_balance(voter1, reward_to_distribute.1).unwrap(),
        1000u128
    );
}

#[test]
fn allocation_selection_matrix() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .build();
    let gauge_contract = init_gauge(&mut suite, &[voter1, voter2]);

    // Each row is a separate gauge so its threshold/cap can be evaluated
    // independently while using the same real orchestrator and adapter stack.
    let mut adapters = Vec::new();
    for cap in [
        None,
        Some(Decimal::percent(60)),
        Some(Decimal::percent(40)),
        None,
        Some(Decimal::permille(1)),
    ] {
        adapters.push(
            suite
                .instantiate_adapter_and_create_gauge(
                    gauge_contract.clone(),
                    &[voter1, voter2],
                    (1_000, "ujuno"),
                    cap,
                    None,
                )
                .unwrap(),
        );
    }

    // No votes means no selected options and therefore a no-op execution path.
    assert!(suite
        .query_selected_set(&gauge_contract, 0)
        .unwrap()
        .is_empty());

    // Partial turnout: allocation is relative to cast power. A sole voter is a
    // single 100-power winner even though another 100-power member abstained.
    suite
        .place_vote(&gauge_contract, voter1, 0, Some(voter1.to_owned()))
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge_contract, 0).unwrap(),
        vec![(voter1.to_owned(), Uint128::new(100))]
    );

    // A valid non-empty selection may intentionally produce no adapter
    // messages. Execution still records the set and advances the epoch.
    suite.set_adapter_return_empty(&adapters[0], true).unwrap();
    suite.advance_time(EPOCH);
    let response = suite.execute_options(&gauge_contract, voter1, 0).unwrap();
    assert_mutation_event(
        &response,
        &[
            ("action", "execute_tally"),
            ("selected_count", "1"),
            ("message_count", "0"),
        ],
    );
    assert_eq!(
        suite.query_last_executed_set(&gauge_contract, 0).unwrap(),
        Some(vec![(voter1.to_owned(), Uint128::new(100))])
    );

    // A 60% cap is above each actual 50% share and changes nothing.
    for voter in [voter1, voter2] {
        suite
            .place_vote(&gauge_contract, voter, 1, Some(voter.to_owned()))
            .unwrap();
    }
    assert_eq!(
        suite.query_selected_set(&gauge_contract, 1).unwrap(),
        vec![
            (voter2.to_owned(), Uint128::new(100)),
            (voter1.to_owned(), Uint128::new(100)),
        ]
    );

    // A 40% cap is below each actual 50% share. Excess is burned: both powers
    // become 80 (40% of 200 cast), rather than being renormalized to 100 each.
    for voter in [voter1, voter2] {
        suite
            .place_vote(&gauge_contract, voter, 2, Some(voter.to_owned()))
            .unwrap();
    }
    assert_eq!(
        suite.query_selected_set(&gauge_contract, 2).unwrap(),
        vec![
            (voter2.to_owned(), Uint128::new(80)),
            (voter1.to_owned(), Uint128::new(80)),
        ]
    );

    // Raise the default 5% threshold to 6%. The exact 5% option is excluded,
    // while the 95% option remains selected.
    suite
        .update_gauge(
            "owner",
            gauge_contract.clone(),
            3,
            None,
            Some(Decimal::percent(6)),
            None,
            None,
        )
        .unwrap();
    suite
        .place_votes(
            &gauge_contract,
            voter1,
            3,
            vec![
                (voter1.to_owned(), Decimal::percent(95)),
                (voter2.to_owned(), Decimal::percent(5)),
            ],
        )
        .unwrap();
    assert_eq!(
        suite.query_selected_set(&gauge_contract, 3).unwrap(),
        vec![(voter1.to_owned(), Uint128::new(95))]
    );

    // A tiny cap that floors to zero removes the candidate, making execution
    // a documented no-op instead of constructing a zero denominator/amount.
    suite
        .place_vote(&gauge_contract, voter1, 4, Some(voter1.to_owned()))
        .unwrap();
    assert!(suite
        .query_selected_set(&gauge_contract, 4)
        .unwrap()
        .is_empty());
}

/// Small helper method to setup the gauge contract.
/// Make sure that `voter` has voting power.
fn init_gauge(suite: &mut Suite, voters: &[&str]) -> Addr {
    suite.next_block();
    suite
        .propose_update_proposal_module(voters[0], None)
        .unwrap();
    suite.next_block();
    let proposal = suite.list_proposals().unwrap()[0];
    for voter in voters {
        suite
            .place_vote_single(*voter, proposal, Vote::Yes)
            .unwrap();
    }
    suite.next_block();
    suite.execute_single_proposal(voters[0], proposal).unwrap();
    let proposal_modules = suite.query_proposal_modules().unwrap();

    // Second proposal module is cw proposal single, first one is newly added gauge
    assert_eq!(proposal_modules.len(), 2);
    proposal_modules[1].clone()
}

#[test]
fn query_last_execution() {
    let voter1 = "voter1";
    let voter2 = "voter2";

    let reward_to_distribute = (2000, "ujuno");

    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .with_core_balance(reward_to_distribute)
        .build();

    let gauge_contract = init_gauge(&mut suite, &[voter1, voter2]);

    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2, gauge_contract.as_str()],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    let gauge_id = 0;

    assert_eq!(
        suite
            .query_last_executed_set(&gauge_contract, gauge_id)
            .unwrap(),
        None,
        "not executed yet"
    );

    // vote
    suite
        .place_vote(&gauge_contract, voter1, gauge_id, Some(voter1.to_owned()))
        .unwrap();
    suite
        .place_votes(
            &gauge_contract,
            voter2,
            gauge_id,
            vec![
                (gauge_contract.to_string(), Decimal::percent(40)),
                (voter2.to_owned(), Decimal::percent(60)),
            ],
        )
        .unwrap();
    // wait until epoch passes
    suite.advance_time(EPOCH);
    // execute
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    // should return the executed set now
    let expected_votes = Some(vec![
        (voter1.to_owned(), 100u128.into()),
        (voter2.to_string(), 60u128.into()),
        (gauge_contract.to_string(), 40u128.into()),
    ]);
    assert_eq!(
        suite
            .query_last_executed_set(&gauge_contract, gauge_id)
            .unwrap(),
        expected_votes
    );

    // change votes
    suite
        .place_vote(&gauge_contract, voter1, gauge_id, Some(voter2.to_owned()))
        .unwrap();
    suite
        .place_vote(&gauge_contract, voter2, gauge_id, None)
        .unwrap();

    // wait until epoch passes
    suite.advance_time(EPOCH);

    // should not change last execution yet
    assert_eq!(
        suite
            .query_last_executed_set(&gauge_contract, gauge_id)
            .unwrap(),
        expected_votes
    );

    // execute
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    // now it should be changed
    assert_eq!(
        suite
            .query_last_executed_set(&gauge_contract, gauge_id)
            .unwrap(),
        Some(vec![(voter2.to_owned(), 100u128.into())])
    );
}

#[test]
fn execute_gauge_twice_same_epoch() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let reward_to_distribute = (2000, "ujuno");
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .with_core_balance(reward_to_distribute)
        .build();

    suite.next_block();
    let gauge_config = suite
        .instantiate_adapter_and_return_config(&[voter1, voter2], (1000, "ujuno"), None, None) // reward per
        // epoch
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

    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    assert_eq!(
        suite.query_balance(voter1, reward_to_distribute.1).unwrap(),
        1000u128
    );

    // execution twice same time won't work
    let err = suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap_err();
    let next_epoch = suite.current_time() + EPOCH;
    assert_eq!(
        ContractError::EpochNotReached {
            gauge_id,
            current_epoch: suite.current_time(),
            next_epoch
        },
        err.downcast().unwrap()
    );

    // just before next epoch fails as well
    suite.advance_time(EPOCH - 1);
    let err = suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap_err();
    assert_eq!(
        ContractError::EpochNotReached {
            gauge_id,
            current_epoch: suite.current_time(),
            next_epoch
        },
        err.downcast().unwrap()
    );

    // another epoch is fine
    suite.advance_time(EPOCH);
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();

    assert_eq!(
        suite.query_balance(voter1, reward_to_distribute.1).unwrap(),
        2000u128
    );
}

#[test]
fn execute_stopped_gauge() {
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

    // stop the gauge by not-owner
    let err = suite
        .stop_gauge(&gauge_contract, voter1, gauge_id)
        .unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());

    // stop the gauge by owner
    suite
        .stop_gauge(&gauge_contract, suite.owner.clone(), gauge_id)
        .unwrap();

    // voting is frozen while stopped
    let err = suite
        .place_vote(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(voter1.to_owned()), // option to vote for
        )
        .unwrap_err();
    assert_eq!(
        ContractError::GaugeStopped(gauge_id),
        err.downcast().unwrap()
    );

    // before advancing specified epoch tally won't get sampled
    suite.advance_time(EPOCH);

    let err = suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap_err();
    assert_eq!(
        ContractError::GaugeStopped(gauge_id),
        err.downcast().unwrap()
    );

    // only the owner can resume
    let err = suite
        .resume_gauge(&gauge_contract, voter1, gauge_id)
        .unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());

    suite
        .resume_gauge(&gauge_contract, suite.owner.clone(), gauge_id)
        .unwrap();
    suite
        .place_vote(
            &gauge_contract,
            voter1.to_owned(),
            gauge_id,
            Some(voter1.to_owned()),
        )
        .unwrap();
    let selected_set = suite.query_selected_set(&gauge_contract, gauge_id).unwrap();
    assert_eq!(selected_set, vec![("voter1".to_owned(), Uint128::new(100))]);

    // Resuming does not impose a turnout threshold: one voter may execute the
    // full epoch allocation even though another voter has not participated.
    suite
        .execute_options(&gauge_contract, voter1, gauge_id)
        .unwrap();
    assert_eq!(
        suite.query_balance(voter1, reward_to_distribute.1).unwrap(),
        reward_to_distribute.0
    );
}

#[test]
fn update_gauge() {
    let voter1 = "voter1";
    let voter2 = "voter2";
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(voter1, 100), (voter2, 100)])
        .build();

    let gauge_contract = init_gauge(&mut suite, &[voter1, voter2]);

    let gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let second_gauge_adapter = suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[voter1, voter2],
            (1000, "uusdc"),
            None,
            None,
        )
        .unwrap();

    let response = suite.query_gauges(gauge_contract.clone()).unwrap();
    assert_eq!(
        response,
        vec![
            GaugeResponse {
                id: 0,
                title: "gauge".to_owned(),
                adapter: gauge_adapter.to_string(),
                epoch_size: EPOCH,
                min_percent_selected: Some(Decimal::percent(5)),
                max_options_selected: 10,
                max_available_percentage: None,
                is_stopped: false,
                next_epoch: suite.current_time() + 7 * 86400,
                reset: None,
            },
            GaugeResponse {
                id: 1,
                title: "gauge".to_owned(),
                adapter: second_gauge_adapter.to_string(),
                epoch_size: EPOCH,
                min_percent_selected: Some(Decimal::percent(5)),
                max_options_selected: 10,
                max_available_percentage: None,
                is_stopped: false,
                next_epoch: suite.current_time() + 7 * 86400,
                reset: None,
            }
        ]
    );

    // update parameters on the first gauge
    let owner = suite.owner.clone();
    let new_epoch = EPOCH * 2;
    let new_min_percent = Some(Decimal::percent(10));
    let new_max_options = 15;
    let new_max_available_percentage = Some(Decimal::percent(5));
    suite
        .update_gauge(
            &owner,
            gauge_contract.clone(),
            0,
            new_epoch,
            new_min_percent,
            new_max_options,
            new_max_available_percentage,
        )
        .unwrap();

    let response = suite.query_gauges(gauge_contract.clone()).unwrap();
    assert_eq!(
        response,
        vec![
            GaugeResponse {
                id: 0,
                title: "gauge".to_owned(),
                adapter: gauge_adapter.to_string(),
                epoch_size: new_epoch,
                min_percent_selected: new_min_percent,
                max_options_selected: new_max_options,
                max_available_percentage: new_max_available_percentage,
                is_stopped: false,
                next_epoch: suite.current_time() + 7 * 86400,
                reset: None,
            },
            GaugeResponse {
                id: 1,
                title: "gauge".to_owned(),
                adapter: second_gauge_adapter.to_string(),
                epoch_size: EPOCH,
                min_percent_selected: Some(Decimal::percent(5)),
                max_options_selected: 10,
                max_available_percentage: None,
                is_stopped: false,
                next_epoch: suite.current_time() + 7 * 86400,
                reset: None,
            }
        ]
    );

    // clean setting of min_percent_selected on second gauge
    suite
        .update_gauge(
            &owner,
            gauge_contract.clone(),
            1,
            None,
            Some(Decimal::zero()),
            None,
            None,
        )
        .unwrap();

    let response = suite.query_gauges(gauge_contract.clone()).unwrap();
    assert_eq!(
        response,
        vec![
            GaugeResponse {
                id: 0,
                title: "gauge".to_owned(),
                adapter: gauge_adapter.to_string(),
                epoch_size: new_epoch,
                min_percent_selected: new_min_percent,
                max_options_selected: new_max_options,
                max_available_percentage: new_max_available_percentage,
                is_stopped: false,
                next_epoch: suite.current_time() + 7 * 86400,
                reset: None,
            },
            GaugeResponse {
                id: 1,
                title: "gauge".to_owned(),
                adapter: second_gauge_adapter.to_string(),
                epoch_size: EPOCH,
                min_percent_selected: None,
                max_options_selected: 10,
                max_available_percentage: None,
                is_stopped: false,
                next_epoch: suite.current_time() + 7 * 86400,
                reset: None,
            }
        ]
    );

    // Not owner cannot update gauges
    let err = suite
        .update_gauge(
            "notowner",
            gauge_contract.clone(),
            0,
            new_epoch,
            new_min_percent,
            new_max_options,
            None,
        )
        .unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());

    let err = suite
        .update_gauge(
            &owner,
            gauge_contract.clone(),
            0,
            50,
            new_min_percent,
            new_max_options,
            None,
        )
        .unwrap_err();
    assert_eq!(ContractError::EpochSizeTooShort {}, err.downcast().unwrap());

    let err = suite
        .update_gauge(
            &owner,
            gauge_contract.clone(),
            0,
            new_epoch,
            Some(Decimal::one()),
            new_max_options,
            None,
        )
        .unwrap_err();
    assert_eq!(
        ContractError::MinPercentSelectedTooBig {},
        err.downcast().unwrap()
    );

    let err = suite
        .update_gauge(
            &owner,
            gauge_contract.clone(),
            0,
            new_epoch,
            new_min_percent,
            0,
            None,
        )
        .unwrap_err();
    assert_eq!(
        ContractError::MaxOptionsSelectedTooSmall {},
        err.downcast().unwrap()
    );

    let err = suite
        .update_gauge(
            &owner,
            gauge_contract,
            1,
            None,
            Some(Decimal::zero()),
            None,
            Some(Decimal::percent(101)),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::MaxAvailablePercentTooBig {},
        err.downcast().unwrap()
    );
}
