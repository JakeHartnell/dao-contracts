//! Audit-defense tests. One or more rejection tests per Critical / High /
//! Medium finding, asserting the defended-against behavior is now rejected.
//!
//! Tests use `mock_dependencies` directly. Cw-multi-test integration tests
//! that exercise the issuer round-trip (mint/burn), C-2 factory auth, and
//! full M-5 ClaimRefund flow are tracked as
//! a separate follow-on; the precondition checks below cover the auth /
//! validation surface.

use cosmwasm_std::{
    coin,
    testing::{mock_dependencies, mock_env, mock_info},
    Addr, Decimal, Timestamp, Uint128,
};

use crate::abc::{CommonsPhase, CurveType, MinMax, VestingSchedule};
use crate::commands;
use crate::commands::insert_into_priority_queue;
use crate::contract;
use crate::msg::{InstantiateMsg, MigrateMsg, UpdatePhaseConfigMsg};
use crate::state::{
    HatcherAllowlistConfig, HatcherAllowlistConfigType, HatcherAllowlistEntry, HatcherState,
    RefundSnapshot, CURVE_STATE, HATCHERS, PHASE, REFUND_SNAPSHOT, SUPPLY_DENOM,
    TOKEN_ISSUER_CONTRACT, TOTAL_HATCH_CONTRIBUTIONS,
};
use crate::testing::{default_instantiate_msg, mock_init, TEST_CREATOR, TEST_RESERVE_DENOM};
use crate::ContractError;
use cosmwasm_std::Uint64;

fn linear_msg() -> InstantiateMsg {
    let curve_type = CurveType::Linear {
        slope: Uint128::new(1),
        scale: 1,
    };
    default_instantiate_msg(2, 8, curve_type)
}

#[test]
fn curve_validation_rejects_unsupported_decimal_inputs_without_panicking() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.curve_type = CurveType::Constant {
        value: Uint128::new(1),
        scale: 29,
    };
    assert!(matches!(
        mock_init(deps.as_mut(), msg),
        Err(ContractError::InvalidCurve { .. })
    ));

    let mut msg = linear_msg();
    msg.curve_type = CurveType::Constant {
        value: Uint128::new(1u128 << 96),
        scale: 0,
    };
    assert!(matches!(
        mock_init(deps.as_mut(), msg),
        Err(ContractError::CurveError(_))
    ));
}

#[test]
fn curve_validation_rejects_huge_power_exponent() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.curve_type = CurveType::Power {
        slope: Uint128::one(),
        scale: 0,
        exponent_num: u32::MAX,
        exponent_den: 1,
    };
    assert!(matches!(
        mock_init(deps.as_mut(), msg),
        Err(ContractError::InvalidCurve { .. })
    ));
}

#[test]
fn curve_validation_rejects_sigmoid_without_bounded_domain() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.curve_type = CurveType::Sigmoid {
        amplitude: Uint128::one(),
        amplitude_scale: 0,
        steepness_num: 1,
        steepness_den: 1,
        midpoint: Uint128::zero(),
        midpoint_scale: 0,
    };
    assert!(matches!(
        mock_init(deps.as_mut(), msg),
        Err(ContractError::InvalidCurve { .. })
    ));
}

#[test]
fn update_max_supply_revalidates_stored_sigmoid_domain() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.supply.max_supply = Some(Uint128::new(10));
    msg.curve_type = CurveType::Sigmoid {
        amplitude: Uint128::one(),
        amplitude_scale: 0,
        steepness_num: 1,
        steepness_den: 1,
        midpoint: Uint128::zero(),
        midpoint_scale: 0,
    };
    mock_init(deps.as_mut(), msg).unwrap();

    assert!(matches!(
        commands::update_max_supply(
            deps.as_mut(),
            mock_info(TEST_CREATOR, &[]),
            Some(Uint128::new(3100)),
        ),
        Err(ContractError::InvalidCurve { .. })
    ));
}

#[test]
fn migrate_reconstructs_missing_hatch_contribution_aggregate() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    TOTAL_HATCH_CONTRIBUTIONS.remove(&mut deps.storage);
    HATCHERS
        .save(
            &mut deps.storage,
            &Addr::unchecked("one"),
            &HatcherState {
                contributed: Uint128::new(3),
                ..HatcherState::default()
            },
        )
        .unwrap();
    HATCHERS
        .save(
            &mut deps.storage,
            &Addr::unchecked("two"),
            &HatcherState {
                contributed: Uint128::new(4),
                ..HatcherState::default()
            },
        )
        .unwrap();

    contract::migrate(deps.as_mut(), mock_env(), MigrateMsg {}).unwrap();

    assert_eq!(
        TOTAL_HATCH_CONTRIBUTIONS.load(&deps.storage).unwrap(),
        Uint128::new(7)
    );
}

// ============================================================
// C-1: update_curve rejection
// ============================================================

#[test]
fn c1_update_curve_rejects_in_hatch() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();

    // PHASE defaults to Hatch after instantiate.
    let res = commands::update_curve(
        deps.as_mut(),
        mock_info(TEST_CREATOR, &[]),
        CurveType::Constant {
            value: Uint128::new(1),
            scale: 0,
        },
    );
    match res {
        Err(ContractError::InvalidPhase { actual, .. }) => assert_eq!(actual, "Hatch"),
        other => panic!("expected InvalidPhase Hatch, got {:?}", other),
    }
}

#[test]
fn c1_update_curve_rejects_in_open() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    PHASE.save(&mut deps.storage, &CommonsPhase::Open).unwrap();

    let res = commands::update_curve(
        deps.as_mut(),
        mock_info(TEST_CREATOR, &[]),
        CurveType::Constant {
            value: Uint128::new(1),
            scale: 0,
        },
    );
    match res {
        Err(ContractError::InvalidPhase { actual, .. }) => assert_eq!(actual, "Open"),
        other => panic!("expected InvalidPhase Open, got {:?}", other),
    }
}

#[test]
fn c1_update_curve_rejects_when_drift_exceeds_tolerance() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    PHASE
        .save(&mut deps.storage, &CommonsPhase::Closed)
        .unwrap();

    // Force a non-zero (reserve, supply) state.
    let mut curve_state = CURVE_STATE.load(&deps.storage).unwrap();
    curve_state.reserve = Uint128::new(1_000_000);
    curve_state.supply = Uint128::new(100);
    CURVE_STATE.save(&mut deps.storage, &curve_state).unwrap();

    // Try to swap to a Constant curve that implies a vastly different
    // reserve at the recorded supply.
    let res = commands::update_curve(
        deps.as_mut(),
        mock_info(TEST_CREATOR, &[]),
        CurveType::Constant {
            value: Uint128::new(1),
            scale: 0,
        },
    );
    match res {
        Err(ContractError::CurveDriftExceeded { .. }) => {}
        other => panic!("expected CurveDriftExceeded, got {:?}", other),
    }
}

#[test]
fn c1_update_curve_rejects_non_owner() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    PHASE
        .save(&mut deps.storage, &CommonsPhase::Closed)
        .unwrap();

    let res = commands::update_curve(
        deps.as_mut(),
        mock_info("not-the-owner", &[]),
        CurveType::Linear {
            slope: Uint128::new(1),
            scale: 1,
        },
    );
    assert!(matches!(res, Err(ContractError::Ownership(_))));
}

// ============================================================
// H-2: UpdatePhaseConfigMsg::Closed variant removed
// ============================================================

#[test]
fn h2_update_phase_config_closed_no_longer_deserializes() {
    // Json with `{"closed": {}}` should fail to parse as the new enum.
    let res: Result<UpdatePhaseConfigMsg, _> = serde_json::from_str(r#"{"closed": {}}"#);
    assert!(res.is_err(), "Closed variant should no longer exist");
    // Sanity: a known-good variant still parses.
    let res: Result<UpdatePhaseConfigMsg, _> =
        serde_json::from_str(r#"{"open": {"exit_fee": null, "entry_fee": null}}"#);
    assert!(res.is_ok());
}

// ============================================================
// H-3 + H-4: Strict-< 100% fee validators
// ============================================================

#[test]
fn h3_instantiate_rejects_100_percent_hatch_entry_fee() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.hatch.entry_fee = Decimal::percent(100);
    let err = mock_init(deps.as_mut(), msg).unwrap_err();
    assert!(matches!(err, ContractError::HatchPhaseConfigError(_)));
}

#[test]
fn h3_instantiate_rejects_100_percent_open_entry_fee() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.open.entry_fee = Decimal::percent(100);
    let err = mock_init(deps.as_mut(), msg).unwrap_err();
    assert!(matches!(err, ContractError::OpenPhaseConfigError(_)));
}

#[test]
fn h4_instantiate_rejects_100_percent_open_exit_fee() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.open.exit_fee = Decimal::percent(100);
    let err = mock_init(deps.as_mut(), msg).unwrap_err();
    assert!(matches!(err, ContractError::InvalidExitFee {}));
}

#[test]
fn h3_h4_update_phase_config_rejects_100_percent() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    PHASE.save(&mut deps.storage, &CommonsPhase::Open).unwrap();

    // Open entry_fee = 100% rejected.
    let err = commands::update_phase_config(
        deps.as_mut(),
        mock_env(),
        mock_info(TEST_CREATOR, &[]),
        UpdatePhaseConfigMsg::Open {
            exit_fee: None,
            entry_fee: Some(Decimal::percent(100)),
        },
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::OpenPhaseConfigError(_)));

    // Open exit_fee = 100% rejected.
    let err = commands::update_phase_config(
        deps.as_mut(),
        mock_env(),
        mock_info(TEST_CREATOR, &[]),
        UpdatePhaseConfigMsg::Open {
            exit_fee: Some(Decimal::percent(100)),
            entry_fee: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::InvalidExitFee {}));
}

// ============================================================
// H-5: decimals < 38 enforced
// ============================================================

#[test]
fn h5_instantiate_rejects_supply_decimals_38() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.supply.decimals = 38;
    let err = mock_init(deps.as_mut(), msg).unwrap_err();
    assert!(matches!(
        err,
        ContractError::InvalidDecimals { decimals: 38, .. }
    ));
}

#[test]
fn h5_instantiate_rejects_reserve_decimals_39() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.reserve.decimals = 39;
    let err = mock_init(deps.as_mut(), msg).unwrap_err();
    assert!(matches!(
        err,
        ContractError::InvalidDecimals { decimals: 39, .. }
    ));
}

#[test]
fn h5_instantiate_accepts_decimals_18() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.supply.decimals = 18;
    msg.reserve.decimals = 18;
    let res = mock_init(deps.as_mut(), msg);
    assert!(res.is_ok(), "decimals=18 should be allowed: {:?}", res);
}

// ============================================================
// H-6: contribution_limits ordering
// ============================================================

#[test]
fn h6_instantiate_rejects_inverted_contribution_limits() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.hatch.contribution_limits = MinMax {
        min: Uint128::new(1000),
        max: Uint128::new(100), // min > max
    };
    let err = mock_init(deps.as_mut(), msg).unwrap_err();
    assert!(matches!(err, ContractError::HatchPhaseConfigError(_)));
}

#[test]
fn h6_instantiate_accepts_equal_contribution_limits() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.hatch.contribution_limits = MinMax {
        min: Uint128::new(100),
        max: Uint128::new(100), // fixed-amount hatch
    };
    let res = mock_init(deps.as_mut(), msg);
    assert!(res.is_ok());
}

// ============================================================
// M-2: cw2 migrate guard
// ============================================================

#[test]
fn m2_migrate_rejects_foreign_contract_name() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();

    // Overwrite cw2 contract name to something foreign.
    cw2::set_contract_version(&mut deps.storage, "crates.io:not-cw-abc", "0.0.0").unwrap();

    let err = contract::migrate(deps.as_mut(), mock_env(), crate::msg::MigrateMsg {}).unwrap_err();
    match err {
        ContractError::InvalidMigration {
            expected, actual, ..
        } => {
            assert_eq!(expected, "crates.io:cw-abc");
            assert_eq!(actual, "crates.io:not-cw-abc");
        }
        other => panic!("expected InvalidMigration, got {:?}", other),
    }
}

#[test]
fn m2_migrate_accepts_matching_contract_name() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    let res = contract::migrate(deps.as_mut(), mock_env(), crate::msg::MigrateMsg {});
    assert!(res.is_ok());
}

// ============================================================
// M-1: priority queue ordering
// ============================================================

#[test]
fn m1_priority_queue_inserts_in_priority_order() {
    let mut queue: Vec<HatcherAllowlistEntry> = vec![];

    let mk = |addr: &str, prio: Option<u64>| HatcherAllowlistEntry {
        addr: Addr::unchecked(addr),
        config: HatcherAllowlistConfig {
            config_type: HatcherAllowlistConfigType::DAO {
                priority: prio.map(Uint64::new),
            },
            contribution_limits_override: None,
            config_height: 0,
        },
    };

    // Insert in mixed order: priority 3, None, 1, 2, None.
    insert_into_priority_queue(&mut queue, mk("a", Some(3)), Some(Uint64::new(3)));
    insert_into_priority_queue(&mut queue, mk("b", None), None);
    insert_into_priority_queue(&mut queue, mk("c", Some(1)), Some(Uint64::new(1)));
    insert_into_priority_queue(&mut queue, mk("d", Some(2)), Some(Uint64::new(2)));
    insert_into_priority_queue(&mut queue, mk("e", None), None);

    let order: Vec<&str> = queue.iter().map(|e| e.addr.as_str()).collect();
    // Some-priorities ascending: c=1, d=2, a=3. None-entries trail in insertion order: b, e.
    assert_eq!(order, vec!["c", "d", "a", "b", "e"]);
}

// ============================================================
// M-5: Refunding sub-state guards
// ============================================================

fn put_into_refunding(storage: &mut dyn cosmwasm_std::Storage) {
    PHASE.save(storage, &CommonsPhase::Refunding).unwrap();
    REFUND_SNAPSHOT
        .save(
            storage,
            &RefundSnapshot {
                total_pool: Uint128::new(1000),
                total_contributed: Uint128::new(1000),
            },
        )
        .unwrap();
    SUPPLY_DENOM
        .save(storage, &"factory/abc/test".to_string())
        .unwrap();
    TOKEN_ISSUER_CONTRACT
        .save(storage, &Addr::unchecked("issuer"))
        .unwrap();
}

#[test]
fn m5_buy_rejected_in_refunding() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    put_into_refunding(&mut deps.storage);

    let res = commands::buy(
        deps.as_mut(),
        mock_env(),
        mock_info("buyer", &[coin(100, TEST_RESERVE_DENOM)]),
        None,
        None,
    );
    assert!(matches!(res, Err(ContractError::CommonsClosed {})));
}

#[test]
fn m5_withdraw_rejected_in_refunding() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    put_into_refunding(&mut deps.storage);

    let res = commands::withdraw(
        deps.as_mut(),
        mock_env(),
        mock_info(TEST_CREATOR, &[]),
        None,
    );
    match res {
        Err(ContractError::InvalidPhase { actual, .. }) => assert_eq!(actual, "Refunding"),
        other => panic!("expected InvalidPhase Refunding, got {:?}", other),
    }
}

#[test]
fn m5_close_rejected_in_refunding() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    put_into_refunding(&mut deps.storage);

    let res = commands::close(deps.as_mut(), mock_info(TEST_CREATOR, &[]));
    match res {
        Err(ContractError::InvalidPhase { actual, .. }) => assert_eq!(actual, "Refunding"),
        other => panic!("expected InvalidPhase Refunding, got {:?}", other),
    }
}

#[test]
fn m5_claim_refund_rejected_for_non_hatcher() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    put_into_refunding(&mut deps.storage);

    let res = commands::claim_refund(
        deps.as_mut(),
        mock_env(),
        mock_info("not-a-hatcher", &[coin(50, "factory/abc/test")]),
    );
    assert!(matches!(
        res,
        Err(ContractError::SenderNotAllowlisted { .. })
    ));
}

#[test]
fn m5_claim_refund_rejects_double_claim() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    put_into_refunding(&mut deps.storage);

    let hatcher = Addr::unchecked("hatcher");
    let mut state = HatcherState::new();
    state.contributed = Uint128::new(1000);
    state.minted = Uint128::new(50);
    state.already_burned = Uint128::new(50);
    state.claimed_refund = true;
    HATCHERS.save(&mut deps.storage, &hatcher, &state).unwrap();

    let res = commands::claim_refund(
        deps.as_mut(),
        mock_env(),
        mock_info(hatcher.as_str(), &[coin(0, "factory/abc/test")]),
    );
    assert!(matches!(res, Err(ContractError::RefundAlreadyClaimed {})));
}

#[test]
fn m5_abort_hatch_rejected_pre_deadline() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(1_000_000);
    msg.phase_config.hatch.hatch_deadline = Some(Timestamp::from_seconds(2_000_000));
    let info = mock_info(TEST_CREATOR, &[]);
    contract::instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    let res = commands::abort_hatch(deps.as_mut(), env, mock_info("anyone", &[]));
    assert!(matches!(res, Err(ContractError::HatchPhaseConfigError(_))));
}

#[test]
fn m5_abort_hatch_rejected_when_no_deadline_configured() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();

    let res = commands::abort_hatch(deps.as_mut(), mock_env(), mock_info("anyone", &[]));
    match res {
        Err(ContractError::HatchPhaseConfigError(s)) => {
            assert!(s.contains("No hatch_deadline"));
        }
        other => panic!("expected HatchPhaseConfigError, got {:?}", other),
    }
}

// ============================================================
// L-3: instantiate-time allowlist no longer needs self-call bypass
// ============================================================

#[test]
fn l3_update_hatch_allowlist_rejects_non_owner() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();

    // Sender = the contract's own address — the previous bypass branch
    // would have admitted this; the post-fix code does not.
    let env = mock_env();
    let res = commands::update_hatch_allowlist(
        deps.as_mut(),
        env.clone(),
        mock_info(env.contract.address.as_str(), &[]),
        vec![],
        vec![],
    );
    assert!(matches!(res, Err(ContractError::Ownership(_))));
}

fn prepare_buys(storage: &mut dyn cosmwasm_std::Storage) {
    TOKEN_ISSUER_CONTRACT
        .save(storage, &Addr::unchecked("issuer"))
        .unwrap();
}

#[test]
fn insecure_address_vesting_cannot_be_configured() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.vesting = VestingSchedule::Linear {
        duration_seconds: 86_400,
    };
    assert!(matches!(
        mock_init(deps.as_mut(), msg),
        Err(ContractError::UnsafeVestingConfiguration {})
    ));
}

#[test]
fn hatch_aggregate_makes_abort_independent_of_hatcher_count() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.phase_config.hatch.hatch_deadline = Some(Timestamp::from_seconds(1));
    msg.phase_config.hatch.initial_raise.min = Uint128::new(500);
    mock_init(deps.as_mut(), msg).unwrap();
    for i in 0..1_000u32 {
        let mut state = HatcherState::new();
        state.contributed = Uint128::one();
        HATCHERS
            .save(&mut deps.storage, &Addr::unchecked(format!("h{i}")), &state)
            .unwrap();
    }
    // Deliberately differs from the per-address sum: AbortHatch must read
    // this O(1) invariant rather than scanning HATCHERS.
    TOTAL_HATCH_CONTRIBUTIONS
        .save(&mut deps.storage, &Uint128::new(77))
        .unwrap();
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(2);
    commands::abort_hatch(deps.as_mut(), env, mock_info("anyone", &[])).unwrap();
    assert_eq!(
        REFUND_SNAPSHOT
            .load(&deps.storage)
            .unwrap()
            .total_contributed,
        Uint128::new(77)
    );
}

#[test]
fn hatch_fees_are_escrowed_and_cap_is_hard() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.funding_pool_forwarding = Some("fee-recipient".to_string());
    msg.curve_type = CurveType::Constant {
        value: Uint128::one(),
        scale: 0,
    };
    msg.phase_config.hatch.initial_raise.max = Uint128::new(100);
    mock_init(deps.as_mut(), msg).unwrap();
    prepare_buys(&mut deps.storage);

    let first = commands::buy(
        deps.as_mut(),
        mock_env(),
        mock_info("one", &[coin(50, TEST_RESERVE_DENOM)]),
        None,
        None,
    )
    .unwrap();
    assert_eq!(first.messages.len(), 1, "hatch only emits mint");
    assert_eq!(
        CURVE_STATE.load(&deps.storage).unwrap().funding,
        Uint128::new(5)
    );
    assert!(matches!(
        commands::withdraw(deps.as_mut(), mock_env(), mock_info(TEST_CREATOR, &[]), None),
        Err(ContractError::InvalidPhase { actual, .. }) if actual == "Hatch"
    ));

    // 61 -> 55 reserve + 6 fee. This reaches 100 reserve exactly and
    // releases all 11 escrowed fee units only after transition to Open.
    let second = commands::buy(
        deps.as_mut(),
        mock_env(),
        mock_info("two", &[coin(61, TEST_RESERVE_DENOM)]),
        None,
        None,
    )
    .unwrap();
    assert_eq!(second.messages.len(), 2, "mint plus escrow release");
    assert_eq!(
        CURVE_STATE.load(&deps.storage).unwrap().funding,
        Uint128::zero()
    );
    assert_eq!(PHASE.load(&deps.storage).unwrap(), CommonsPhase::Open);

    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.curve_type = CurveType::Constant {
        value: Uint128::one(),
        scale: 0,
    };
    msg.phase_config.hatch.initial_raise.max = Uint128::new(100);
    mock_init(deps.as_mut(), msg).unwrap();
    prepare_buys(&mut deps.storage);
    assert!(matches!(
        commands::buy(
            deps.as_mut(),
            mock_env(),
            mock_info("over", &[coin(112, TEST_RESERVE_DENOM)]),
            None,
            None
        ),
        Err(ContractError::InitialRaiseCapExceeded { .. })
    ));
}

#[test]
fn abort_snapshots_full_retained_hatch_pool() {
    let mut deps = mock_dependencies();
    let mut msg = linear_msg();
    msg.funding_pool_forwarding = Some("fee-recipient".to_string());
    msg.phase_config.hatch.hatch_deadline = Some(Timestamp::from_seconds(1));
    msg.phase_config.hatch.initial_raise.min = Uint128::new(100);
    mock_init(deps.as_mut(), msg).unwrap();
    prepare_buys(&mut deps.storage);
    commands::buy(
        deps.as_mut(),
        mock_env(),
        mock_info("one", &[coin(50, TEST_RESERVE_DENOM)]),
        None,
        None,
    )
    .unwrap();
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(2);
    commands::abort_hatch(deps.as_mut(), env, mock_info("anyone", &[])).unwrap();
    assert_eq!(
        REFUND_SNAPSHOT.load(&deps.storage).unwrap().total_pool,
        Uint128::new(50)
    );
}

#[test]
fn buy_rejects_expired_and_degraded_quotes() {
    let mut deps = mock_dependencies();
    mock_init(deps.as_mut(), linear_msg()).unwrap();
    prepare_buys(&mut deps.storage);
    let env = mock_env();
    assert!(matches!(
        commands::buy(
            deps.as_mut(),
            env.clone(),
            mock_info("one", &[coin(10, TEST_RESERVE_DENOM)]),
            None,
            Some(env.block.time.minus_seconds(1))
        ),
        Err(ContractError::DeadlineExpired { .. })
    ));
    assert!(matches!(
        commands::buy(
            deps.as_mut(),
            env,
            mock_info("one", &[coin(10, TEST_RESERVE_DENOM)]),
            Some(Uint128::MAX),
            None
        ),
        Err(ContractError::SlippageExceeded { .. })
    ));
}

#[test]
fn sell_rejects_expired_and_degraded_quotes() {
    let mut deps = mock_dependencies();
    let msg = default_instantiate_msg(
        0,
        0,
        CurveType::Constant {
            value: Uint128::one(),
            scale: 0,
        },
    );
    mock_init(deps.as_mut(), msg).unwrap();
    PHASE.save(&mut deps.storage, &CommonsPhase::Open).unwrap();
    SUPPLY_DENOM
        .save(&mut deps.storage, &"supply".to_string())
        .unwrap();
    CURVE_STATE
        .update(&mut deps.storage, |mut state| -> Result<_, ContractError> {
            state.supply = Uint128::new(100);
            state.reserve = Uint128::new(100);
            Ok(state)
        })
        .unwrap();

    let env = mock_env();
    assert!(matches!(
        commands::sell(
            deps.as_mut(),
            env.clone(),
            mock_info("seller", &[coin(10, "supply")]),
            None,
            Some(env.block.time.minus_seconds(1))
        ),
        Err(ContractError::DeadlineExpired { .. })
    ));
    let err = commands::sell(
        deps.as_mut(),
        env,
        mock_info("seller", &[coin(10, "supply")]),
        Some(Uint128::MAX),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(err, ContractError::SlippageExceeded { .. }),
        "unexpected error: {err:?}"
    );
}
