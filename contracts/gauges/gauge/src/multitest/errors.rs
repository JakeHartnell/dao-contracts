//! Focused error-path coverage for the gauge orchestrator.
//!
//! Most happy-path tests live in `voting.rs`, `gauge.rs`, etc. This file
//! exercises validation errors that the existing tests don't trigger,
//! so the contract's input-validation surface has direct regression
//! coverage.

use cosmwasm_std::{Addr, Decimal};
use cw_multi_test::Executor;
use dao_voting::voting::Vote;

use super::suite::{Suite, SuiteBuilder};
use crate::error::ContractError;
use crate::msg::{ExecuteMsg, GaugeConfig};

const VOTER1: &str = "voter1";
const VOTER2: &str = "voter2";

/// Stand up a DAO with the gauge wired as a proposal module and return
/// `(suite, gauge_contract_addr)`. Tests that just need a live gauge use
/// this to skip the cw4-membership + proposal-module dance.
fn dao_with_gauge() -> (Suite, Addr) {
    let mut suite = SuiteBuilder::new()
        .with_voting_members(&[(VOTER1, 100), (VOTER2, 200)])
        .build();

    suite.next_block();
    suite
        .propose_update_proposal_module(VOTER1.to_string(), None)
        .unwrap();
    suite.next_block();

    let proposal = suite.list_proposals().unwrap()[0];
    suite
        .place_vote_single(VOTER1, proposal, Vote::Yes)
        .unwrap();
    suite
        .place_vote_single(VOTER2, proposal, Vote::Yes)
        .unwrap();
    suite.next_block();
    suite
        .execute_single_proposal(VOTER1.to_string(), proposal)
        .unwrap();

    let proposal_modules = suite.query_proposal_modules().unwrap();
    let gauge_contract = proposal_modules[1].clone();
    (suite, gauge_contract)
}

/// Create a valid base config that tests can mutate one field of.
fn base_config(suite: &mut Suite) -> GaugeConfig {
    suite
        .instantiate_adapter_and_return_config(&[VOTER1, VOTER2], (1000, "ujuno"), None, None)
        .unwrap()
}

fn create_with(suite: &mut Suite, gauge_contract: &Addr, config: GaugeConfig) -> anyhow::Error {
    let owner = suite.owner.clone();
    suite
        .app
        .execute_contract(
            Addr::unchecked(owner),
            gauge_contract.clone(),
            &ExecuteMsg::CreateGauge(config),
            &[],
        )
        .unwrap_err()
}

// ---------------------------------------------------------------- create_gauge

#[test]
fn create_gauge_rejects_too_short_epoch() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    let mut config = base_config(&mut suite);
    config.epoch_size = 59;
    let err = create_with(&mut suite, &gauge_contract, config);
    assert_eq!(ContractError::EpochSizeTooShort {}, err.downcast().unwrap());
}

#[test]
fn create_gauge_rejects_min_percent_at_one() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    let mut config = base_config(&mut suite);
    config.min_percent_selected = Some(Decimal::one());
    let err = create_with(&mut suite, &gauge_contract, config);
    assert_eq!(
        ContractError::MinPercentSelectedTooBig {},
        err.downcast().unwrap()
    );
}

#[test]
fn create_gauge_rejects_max_options_zero() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    let mut config = base_config(&mut suite);
    config.max_options_selected = 0;
    let err = create_with(&mut suite, &gauge_contract, config);
    assert_eq!(
        ContractError::MaxOptionsSelectedTooSmall {},
        err.downcast().unwrap()
    );
}

#[test]
fn create_gauge_rejects_max_available_percent_at_one() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    let mut config = base_config(&mut suite);
    config.max_available_percentage = Some(Decimal::one());
    let err = create_with(&mut suite, &gauge_contract, config);
    assert_eq!(
        ContractError::MaxAvailablePercentTooBig {},
        err.downcast().unwrap()
    );
}

#[test]
fn create_gauge_rejects_zero_reset_interval() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    let mut config = base_config(&mut suite);
    config.reset_epoch = Some(0);
    let err = create_with(&mut suite, &gauge_contract, config);
    assert_eq!(
        ContractError::InvalidResetInterval {},
        err.downcast().unwrap()
    );
}

// --------------------------------------------------------- nonexistent gauge

#[test]
fn operations_on_missing_gauge_return_gauge_missing() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    // Create one gauge so the contract is non-empty; we'll probe id 99.
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    // place_votes on a missing gauge:
    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            99,
            Some(vec![(VOTER1.to_owned(), Decimal::one())]),
        )
        .unwrap_err();
    assert_eq!(ContractError::GaugeMissing(99), err.downcast().unwrap());
}

// --------------------------------------------------------------- place_votes

#[test]
fn place_votes_rejects_weight_sum_over_one() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1, VOTER2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let bad_sum = Decimal::percent(60) + Decimal::percent(60);
    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![
                (VOTER1.to_owned(), Decimal::percent(60)),
                (VOTER2.to_owned(), Decimal::percent(60)),
            ]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::TooMuchVotingWeight(bad_sum),
        err.downcast().unwrap()
    );

    // Incremental checked validation must reject an adversarial Decimal at
    // the first entry instead of overflowing an unchecked vector sum.
    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![
                (VOTER1.to_owned(), Decimal::MAX),
                (VOTER2.to_owned(), Decimal::MAX),
            ]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::TooMuchVotingWeight(Decimal::MAX),
        err.downcast().unwrap()
    );
}

#[test]
fn place_votes_rejects_malformed_entries_before_accounting() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1, VOTER2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    for duplicate in [
        vec![VOTER1, VOTER1, VOTER2],
        vec![VOTER1, VOTER2, VOTER1],
        vec![VOTER2, VOTER1, VOTER1],
    ] {
        let votes = duplicate
            .into_iter()
            .map(|option| (option.to_owned(), Decimal::percent(25)))
            .collect::<Vec<_>>();
        let err = suite
            .place_votes(&gauge_contract, VOTER1.to_owned(), 0, Some(votes))
            .unwrap_err();
        assert_eq!(
            ContractError::DuplicateVoteOption {
                option: VOTER1.to_owned()
            },
            err.downcast().unwrap()
        );
    }

    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![(VOTER1.to_owned(), Decimal::zero())]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::ZeroVoteWeight {
            option: VOTER1.to_owned()
        },
        err.downcast().unwrap()
    );

    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![(String::new(), Decimal::one())]),
        )
        .unwrap_err();
    assert_eq!(ContractError::EmptyVoteOption {}, err.downcast().unwrap());

    let too_many = (0..101)
        .map(|i| (format!("option-{i}"), Decimal::percent(1)))
        .collect::<Vec<_>>();
    let err = suite
        .place_votes(&gauge_contract, VOTER1.to_owned(), 0, Some(too_many))
        .unwrap_err();
    assert_eq!(
        ContractError::TooManyVoteEntries {
            count: 101,
            max: 100
        },
        err.downcast().unwrap()
    );

    // The exact entry bound is accepted by structural validation. The first
    // adapter-invalid option is the later error, proving this is not an
    // off-by-one rejection at 100.
    let at_limit = (0..100)
        .map(|i| (format!("limit-option-{i}"), Decimal::percent(1)))
        .collect::<Vec<_>>();
    let err = suite
        .place_votes(&gauge_contract, VOTER1.to_owned(), 0, Some(at_limit))
        .unwrap_err();
    assert_eq!(
        ContractError::OptionDoesNotExists {
            option: "limit-option-0".to_owned(),
            gauge_id: 0,
        },
        err.downcast().unwrap()
    );

    let exact_length = "x".repeat(128);
    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![(exact_length.clone(), Decimal::one())]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::OptionDoesNotExists {
            option: exact_length,
            gauge_id: 0,
        },
        err.downcast().unwrap()
    );

    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![("x".repeat(129), Decimal::one())]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::StringTooLong {
            field: "option".to_owned(),
            max: 128,
        },
        err.downcast().unwrap()
    );

    assert!(suite
        .query_vote(&gauge_contract, 0, VOTER1)
        .unwrap()
        .is_none());
    assert!(suite
        .query_selected_set(&gauge_contract, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn place_votes_rejects_voter_with_no_power() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1, VOTER2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let err = suite
        .place_votes(
            &gauge_contract,
            "stranger".to_owned(),
            0,
            Some(vec![(VOTER1.to_owned(), Decimal::one())]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::NoVotingPower("stranger".to_owned()),
        err.downcast().unwrap()
    );
}

#[test]
fn place_votes_rejects_option_that_does_not_exist() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1, VOTER2],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let err = suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![("nonexistent".to_owned(), Decimal::one())]),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::OptionDoesNotExists {
            option: "nonexistent".to_owned(),
            gauge_id: 0
        },
        err.downcast().unwrap()
    );
}

// ----------------------------------------------------------------- execute

#[test]
fn execute_before_epoch_returns_epoch_not_reached() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();
    // Vote so the selected set is nonempty, then execute immediately.
    suite
        .place_votes(
            &gauge_contract,
            VOTER1.to_owned(),
            0,
            Some(vec![(VOTER1.to_owned(), Decimal::one())]),
        )
        .unwrap();

    let err = suite
        .execute_options(&gauge_contract, VOTER1, 0)
        .unwrap_err();
    let downcast: ContractError = err.downcast().unwrap();
    matches!(downcast, ContractError::EpochNotReached { gauge_id: 0, .. });
}

// ----------------------------------------------------------------- auth

#[test]
fn stop_gauge_requires_owner() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let err = suite
        .stop_gauge(&gauge_contract, "intruder", 0)
        .unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());
}

#[test]
fn update_gauge_requires_owner() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    let err = suite
        .update_gauge(
            "intruder",
            gauge_contract.clone(),
            0,
            None,
            None,
            None,
            None,
        )
        .unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());
}

#[test]
fn hooks_reject_unauthorized_caller() {
    let (mut suite, gauge_contract) = dao_with_gauge();
    suite
        .instantiate_adapter_and_create_gauge(
            gauge_contract.clone(),
            &[VOTER1],
            (1000, "ujuno"),
            None,
            None,
        )
        .unwrap();

    // MemberChangedHook from a random caller should fail.
    let err = suite
        .app
        .execute_contract(
            Addr::unchecked("not-the-hook-caller"),
            gauge_contract,
            &ExecuteMsg::MemberChangedHook(cw4::MemberChangedHookMsg { diffs: vec![] }),
            &[],
        )
        .unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());
}
