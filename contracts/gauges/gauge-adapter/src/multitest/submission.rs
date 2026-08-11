use cosmwasm_std::{coin, to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, Uint128, WasmMsg};
use cw20::Cw20ExecuteMsg;
use cw_denom::UncheckedDenom;
use cw_multi_test::{AppResponse, Executor};
use cw_ownable::Action;

use crate::{
    msg::{
        AllSubmissionsResponse, AssetUnchecked, ExecuteMsg, LiabilitiesResponse,
        QueryMsg as AdapterQueryMsg, ReceiveMsg, SampleGaugeMsgsResponse, SubmissionResponse,
    },
    multitest::suite::{addr, submit_cw20_create, Suite},
    ContractError,
};

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
fn create_default_submission() {
    let suite = Suite::new_native(None);
    let community_pool = suite.community_pool.clone();

    // Created during instantiation.
    let res: SubmissionResponse = suite
        .query(&AdapterQueryMsg::Submission {
            address: community_pool.to_string(),
        })
        .unwrap();
    assert_eq!(
        res,
        SubmissionResponse {
            sender: suite.adapter.clone(),
            name: "Unimpressed".to_owned(),
            url: "Those funds go back to the community pool".to_owned(),
            address: community_pool,
        },
    );
}

#[test]
fn create_submission_no_required_deposit() {
    let mut suite = Suite::new_native(None);
    let owner = suite.owner.clone();
    let recipient = addr("recipient");
    suite.mint_native(&owner, coin(1_000, "juno"));

    // Sending funds when no deposit required is an error.
    let err = suite
        .create_submission(&owner, &recipient, Some(coin(1_000, "juno")))
        .unwrap_err();
    assert_eq!(
        ContractError::InvalidDepositAmount {
            correct_amount: Uint128::zero(),
        },
        err.downcast().unwrap()
    );

    // Without funds it succeeds.
    suite.create_submission(&owner, &recipient, None).unwrap();

    let res: SubmissionResponse = suite
        .query(&AdapterQueryMsg::Submission {
            address: recipient.to_string(),
        })
        .unwrap();
    assert_eq!(
        res,
        SubmissionResponse {
            sender: owner,
            name: "DAOers".to_owned(),
            url: "https://daodao.zone".to_owned(),
            address: recipient,
        },
    );
}

#[test]
fn submission_string_limits_accept_exact_and_reject_over() {
    let mut suite = Suite::new_native(None);
    let owner = suite.owner.clone();

    let exact = ExecuteMsg::CreateSubmission {
        name: "n".repeat(128),
        url: "u".repeat(512),
        address: "exact-recipient".to_owned(),
    };
    suite.execute(&owner, &exact, &[]).unwrap();

    for (name, url, field, max) in [
        ("n".repeat(129), "url".to_owned(), "name", 128usize),
        ("name".to_owned(), "u".repeat(513), "url", 512usize),
    ] {
        let err = suite
            .execute(
                &owner,
                &ExecuteMsg::CreateSubmission {
                    name,
                    url,
                    address: format!("over-{field}"),
                },
                &[],
            )
            .unwrap_err();
        assert_eq!(
            ContractError::StringTooLong {
                field: field.to_owned(),
                max,
            },
            err.downcast().unwrap()
        );
    }
}

#[test]
fn overwrite_existing_submission() {
    let mut suite = Suite::new_native(None);
    let owner = suite.owner.clone();
    let recipient = addr("recipient");

    suite.create_submission(&owner, &recipient, None).unwrap();

    let res: SubmissionResponse = suite
        .query(&AdapterQueryMsg::Submission {
            address: recipient.to_string(),
        })
        .unwrap();
    assert_eq!(res.sender, owner);
    assert_eq!(res.url, "https://daodao.zone");

    // Submitting to the same recipient as a different sender is not allowed.
    let intruder = addr("intruder");
    let err = suite
        .create_submission(&intruder, &recipient, None)
        .unwrap_err();
    assert_eq!(
        ContractError::UnauthorizedSubmission {},
        err.downcast().unwrap()
    );

    // Overwriting as the original author works.
    suite.create_submission(&owner, &recipient, None).unwrap();
}

#[test]
fn create_submission_required_deposit() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let owner = suite.owner.clone();
    let recipient = addr("recipient");
    suite.mint_native(&owner, coin(1_000, "wynd"));
    suite.mint_native(&owner, coin(1_000, "juno"));

    // No funds → PaymentError.
    let err = suite
        .create_submission(&owner, &recipient, None)
        .unwrap_err();
    assert_eq!(
        ContractError::PaymentError(cw_utils::PaymentError::NoFunds {}),
        err.downcast().unwrap()
    );

    // Right denom, wrong amount.
    let err = suite
        .create_submission(
            &owner,
            &recipient,
            Some(Coin {
                denom: "juno".into(),
                amount: Uint128::new(999),
            }),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::InvalidDepositAmount {
            correct_amount: Uint128::new(1_000),
        },
        err.downcast().unwrap()
    );

    // Wrong denom, right amount.
    let err = suite
        .create_submission(
            &owner,
            &recipient,
            Some(Coin {
                denom: "wynd".into(),
                amount: Uint128::new(1_000),
            }),
        )
        .unwrap_err();
    assert_eq!(
        ContractError::InvalidDepositType {},
        err.downcast().unwrap()
    );

    // Valid submission.
    let created = suite
        .create_submission(&owner, &recipient, Some(coin(1_000, "juno")))
        .unwrap();
    assert_mutation_event(
        &created,
        &[
            ("action", "create_submission"),
            ("sender", "owner"),
            ("submission", "recipient"),
            ("depositor", "owner"),
            ("bond_denom", "juno"),
            ("bond_amount", "1000"),
            ("bond_state", "active"),
            ("liabilities", "1000"),
        ],
    );

    let res: SubmissionResponse = suite
        .query(&AdapterQueryMsg::Submission {
            address: recipient.to_string(),
        })
        .unwrap();
    assert_eq!(res.sender, owner);
    assert_eq!(res.address, recipient);

    let liabilities: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(liabilities.asset.unwrap().amount, Uint128::new(1_000));
    assert_eq!(liabilities.escrow_balance, Uint128::new(1_000));

    // Same-sender metadata updates are deposit-free and preserve the original
    // one-time bond. Sending a second bond is rejected atomically.
    let updated = suite.create_submission(&owner, &recipient, None).unwrap();
    assert_mutation_event(
        &updated,
        &[("action", "update_submission"), ("bond_state", "active")],
    );
    suite.mint_native(&owner, coin(1_000, "juno"));
    let err = suite
        .create_submission(&owner, &recipient, Some(coin(1_000, "juno")))
        .unwrap_err();
    assert_eq!(
        ContractError::DepositOnMetadataUpdate {},
        err.downcast().unwrap()
    );
    let liabilities: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(liabilities.asset.unwrap().amount, Uint128::new(1_000));
}

#[test]
fn create_receive_required_deposit() {
    let (mut suite, deposit_cw20) = Suite::new_cw20_deposit();
    let owner = suite.owner.clone();
    let recipient_addr = owner.clone();

    // A second cw20 for the "wrong cw20" error path.
    let bad_cw20 = suite.instantiate_cw20();

    let binary_msg = to_json_binary(&ReceiveMsg::CreateSubmission {
        name: "DAOers".to_string(),
        url: "https://daodao.zone".to_string(),
        address: recipient_addr.to_string(),
    })
    .unwrap();

    // Sending from the wrong cw20 fails (we impersonate the cw20 contract).
    let err = suite
        .execute(
            &bad_cw20,
            &ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                sender: recipient_addr.to_string(),
                amount: Uint128::new(1_000),
                msg: binary_msg.clone(),
            }),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::InvalidDepositType {},
        err.downcast().unwrap(),
    );

    // Right cw20 but less than required fails.
    let err = suite
        .execute(
            &deposit_cw20,
            &ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                sender: recipient_addr.to_string(),
                amount: Uint128::new(999),
                msg: binary_msg.clone(),
            }),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::InvalidDepositAmount {
            correct_amount: Uint128::new(1_000),
        },
        err.downcast().unwrap()
    );

    // A forged receive callback with the correct nominal amount still fails
    // unless the configured token balance actually covers the new liability.
    let err = suite
        .execute(
            &deposit_cw20,
            &ExecuteMsg::Receive(cw20::Cw20ReceiveMsg {
                sender: recipient_addr.to_string(),
                amount: Uint128::new(1_000),
                msg: binary_msg.clone(),
            }),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::EscrowShortfall {
            balance: Uint128::zero(),
            liabilities: Uint128::new(1_000)
        },
        err.downcast().unwrap()
    );

    // Valid submission via correct cw20.
    let adapter = suite.adapter.clone();
    suite
        .cw20_send(&deposit_cw20, &recipient_addr, &adapter, 1_000, binary_msg)
        .unwrap();

    let all: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::AllSubmissions {
            start_after: None,
            limit: None,
        })
        .unwrap();
    // default (community-pool refund) + the one we just added.
    assert_eq!(all.submissions.len(), 2);
}

#[test]
fn return_deposits_no_required_deposit() {
    let mut suite = Suite::new_native(None);
    let err = suite
        .execute_owner(&ExecuteMsg::ReturnDeposits {})
        .unwrap_err();
    assert_eq!(ContractError::NoDepositToRefund {}, err.downcast().unwrap());
}

#[test]
fn return_deposits_no_admin() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let intruder = addr("intruder");
    let err = suite
        .execute(&intruder, &ExecuteMsg::ReturnDeposits {}, &[])
        .unwrap_err();
    assert_eq!(
        ContractError::Ownership(cw_ownable::OwnershipError::NotOwner),
        err.downcast().unwrap()
    );
}

#[test]
fn return_deposits_required_native_deposit() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let owner = suite.owner.clone();
    let recipient = addr("recipient");

    suite.mint_native(&owner, coin(1_000, "juno"));
    suite
        .create_submission(&owner, &recipient, Some(coin(1_000, "juno")))
        .unwrap();

    assert_eq!(suite.native_balance(&owner, "juno"), Uint128::zero());
    assert_eq!(suite.native_balance(&recipient, "juno"), Uint128::zero());
    let adapter = suite.adapter.clone();
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::new(1_000));

    let refunded = suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();
    assert_mutation_event(
        &refunded,
        &[
            ("action", "return_deposits"),
            ("sender", "owner"),
            ("processed", "2"),
            ("complete", "true"),
            ("next_cursor", "none"),
            ("message_count", "1"),
            ("refunded_amount", "1000"),
            ("liabilities", "0"),
        ],
    );
    assert_eq!(suite.native_balance(&owner, "juno"), Uint128::new(1_000));
    assert_eq!(suite.native_balance(&recipient, "juno"), Uint128::zero());
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::zero());

    // Repeating the idempotent wind-down neither repays nor recreates debt.
    suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();
    assert_eq!(suite.native_balance(&owner, "juno"), Uint128::new(1_000));
    let liabilities: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(liabilities.asset.unwrap().amount, Uint128::zero());

    // Metadata remains editable without recreating debt, and its event must
    // report the persisted lifecycle rather than claiming the bond is active.
    let updated = suite.create_submission(&owner, &recipient, None).unwrap();
    assert_mutation_event(
        &updated,
        &[
            ("action", "update_submission"),
            ("bond_state", "refunded"),
            ("submission", "recipient"),
        ],
    );

    // A later rejection only removes the registry row; the refunded bond is
    // not transferred a second time.
    let rejected = suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: recipient.to_string(),
            soft: false,
        })
        .unwrap();
    assert_mutation_event(
        &rejected,
        &[
            ("action", "reject"),
            ("sender", "owner"),
            ("submission", "recipient"),
            ("kind", "hard"),
            ("bond_state", "refunded"),
            ("bond_amount", "1000"),
            ("liabilities", "0"),
        ],
    );
    assert_eq!(suite.native_balance(&owner, "juno"), Uint128::new(1_000));
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::zero());
}

#[test]
fn unsolicited_native_transfer_is_surplus_not_a_liability() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let depositor = addr("bond-depositor");
    let recipient = addr("bond-recipient");
    let donor = addr("surplus-donor");
    suite.mint_native(&depositor, coin(1_000, "juno"));
    suite.mint_native(&donor, coin(777, "juno"));
    suite
        .create_submission(&depositor, &recipient, Some(coin(1_000, "juno")))
        .unwrap();

    suite
        .app
        .execute(
            donor,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: suite.adapter.to_string(),
                amount: vec![coin(777, "juno")],
            }),
        )
        .unwrap();

    let before: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(before.asset.unwrap().amount, Uint128::new(1_000));
    assert_eq!(before.escrow_balance, Uint128::new(1_777));

    suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();
    assert_eq!(
        suite.native_balance(&depositor, "juno"),
        Uint128::new(1_000)
    );
    assert_eq!(
        suite.native_balance(&suite.adapter, "juno"),
        Uint128::new(777)
    );
    let after: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(after.asset.unwrap().amount, Uint128::zero());
    assert_eq!(after.escrow_balance, Uint128::new(777));
}

#[test]
fn return_deposits_required_native_deposit_multiple_deposits() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let owner = suite.owner.clone();
    let recipient = addr("recipient");
    let einstein = addr("einstein");

    suite.mint_native(&owner, coin(1_000, "juno"));
    suite.mint_native(&einstein, coin(1_000, "juno"));

    suite
        .create_submission(&owner, &recipient, Some(coin(1_000, "juno")))
        .unwrap();
    suite
        .create_submission(&einstein, &einstein, Some(coin(1_000, "juno")))
        .unwrap();

    suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();
    assert_eq!(suite.native_balance(&owner, "juno"), Uint128::new(1_000));
    assert_eq!(suite.native_balance(&einstein, "juno"), Uint128::new(1_000));
    assert_eq!(suite.native_balance(&recipient, "juno"), Uint128::zero());
    let adapter = suite.adapter.clone();
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::zero());
}

#[test]
fn return_deposits_progresses_in_idempotent_batches() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let depositor = addr("batch-depositor");
    suite.mint_native(&depositor, coin(51_000, "juno"));
    for index in 0..51 {
        suite
            .create_submission(
                &depositor,
                &addr(&format!("recipient-{index}")),
                Some(coin(1_000, "juno")),
            )
            .unwrap();
    }

    let first_batch = suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();
    assert_mutation_event(
        &first_batch,
        &[
            ("action", "return_deposits"),
            ("sender", "owner"),
            ("processed", "50"),
            ("complete", "false"),
            ("message_count", "49"),
            ("refunded_amount", "49000"),
            ("liabilities", "2000"),
        ],
    );
    let progress: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    // The synthetic default row consumes one scan slot but carries no debt.
    assert_eq!(progress.asset.unwrap().amount, Uint128::new(2_000));
    assert!(progress.refund_cursor.is_some());
    assert!(!progress.refunds_complete);

    let new_owner = addr("refund-new-owner");
    suite
        .execute_owner(&ExecuteMsg::UpdateOwnership(Action::TransferOwnership {
            new_owner: new_owner.to_string(),
            expiry: None,
        }))
        .unwrap();
    suite
        .execute(
            &new_owner,
            &ExecuteMsg::UpdateOwnership(Action::AcceptOwnership),
            &[],
        )
        .unwrap();
    let final_batch = suite
        .execute(&new_owner, &ExecuteMsg::ReturnDeposits {}, &[])
        .unwrap();
    assert_mutation_event(
        &final_batch,
        &[
            ("action", "return_deposits"),
            ("sender", "refund-new-owner"),
            ("processed", "2"),
            ("complete", "true"),
            ("next_cursor", "none"),
            ("message_count", "2"),
            ("refunded_amount", "2000"),
            ("liabilities", "0"),
        ],
    );
    let complete: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(complete.asset.unwrap().amount, Uint128::zero());
    assert!(complete.refund_cursor.is_none());
    assert!(complete.refunds_complete);
    assert_eq!(
        suite.native_balance(&depositor, "juno"),
        Uint128::new(51_000)
    );

    let repeated = suite
        .execute(&new_owner, &ExecuteMsg::ReturnDeposits {}, &[])
        .unwrap();
    assert_mutation_event(
        &repeated,
        &[
            ("action", "return_deposits"),
            ("processed", "0"),
            ("complete", "true"),
            ("next_cursor", "none"),
            ("message_count", "0"),
            ("refunded_amount", "0"),
            ("liabilities", "0"),
        ],
    );
    assert_eq!(
        suite.native_balance(&depositor, "juno"),
        Uint128::new(51_000)
    );

    let err = suite
        .create_submission(
            &depositor,
            &addr("late-recipient"),
            Some(coin(1_000, "juno")),
        )
        .unwrap_err();
    assert_eq!(ContractError::RefundInProgress {}, err.downcast().unwrap());
}

#[test]
fn return_deposits_required_cw20_deposit() {
    let (mut suite, cw20) = Suite::new_cw20_deposit();
    let owner = suite.owner.clone();
    let adapter = suite.adapter.clone();
    let recipient = addr("recipient");

    let inner = to_json_binary(&ReceiveMsg::CreateSubmission {
        name: "DAOers".to_string(),
        url: "https://daodao.zone".to_string(),
        address: recipient.to_string(),
    })
    .unwrap();
    suite
        .cw20_send(&cw20, &owner, &adapter, 1_000, inner)
        .unwrap();

    assert_eq!(suite.cw20_balance(&cw20, &owner), Uint128::new(999_000));
    assert_eq!(suite.cw20_balance(&cw20, &recipient), Uint128::zero());
    assert_eq!(suite.cw20_balance(&cw20, &adapter), Uint128::new(1_000));

    suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();

    assert_eq!(suite.cw20_balance(&cw20, &owner), Uint128::new(1_000_000));
    // Refund target is the submission sender (owner), not the recipient.
    assert_eq!(suite.cw20_balance(&cw20, &recipient), Uint128::zero());
    assert_eq!(suite.cw20_balance(&cw20, &adapter), Uint128::zero());
}

#[test]
fn unsolicited_cw20_transfer_is_surplus_not_a_liability() {
    let (mut suite, cw20) = Suite::new_cw20_deposit();
    let owner = suite.owner.clone();
    let adapter = suite.adapter.clone();
    let recipient = addr("bond-recipient");
    let inner = to_json_binary(&ReceiveMsg::CreateSubmission {
        name: "DAOers".to_owned(),
        url: "https://daodao.zone".to_owned(),
        address: recipient.to_string(),
    })
    .unwrap();
    suite
        .cw20_send(&cw20, &owner, &adapter, 1_000, inner)
        .unwrap();

    suite
        .app
        .execute_contract(
            owner.clone(),
            cw20.clone(),
            &Cw20ExecuteMsg::Transfer {
                recipient: adapter.to_string(),
                amount: Uint128::new(777),
            },
            &[],
        )
        .unwrap();

    let before: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(before.asset.unwrap().amount, Uint128::new(1_000));
    assert_eq!(before.escrow_balance, Uint128::new(1_777));

    suite.execute_owner(&ExecuteMsg::ReturnDeposits {}).unwrap();
    assert_eq!(suite.cw20_balance(&cw20, &owner), Uint128::new(999_223));
    assert_eq!(suite.cw20_balance(&cw20, &adapter), Uint128::new(777));
    let after: LiabilitiesResponse = suite.query(&AdapterQueryMsg::Liabilities {}).unwrap();
    assert_eq!(after.asset.unwrap().amount, Uint128::zero());
    assert_eq!(after.escrow_balance, Uint128::new(777));
}

#[test]
fn sample_gauge_msgs_cw20() {
    let (mut suite, cw20) = Suite::new_cw20_reward(None);
    let owner = suite.owner.clone();
    let addr_1 = addr("addr1");
    let addr_2 = addr("addr2");
    let addr_3 = addr("addr3");
    let reward = Uint128::new(1_000_000);

    suite
        .execute(
            &owner,
            &ExecuteMsg::CreateSubmission {
                name: "name".to_string(),
                url: "https://test.url".to_string(),
                address: addr_1.to_string(),
            },
            &[],
        )
        .unwrap();
    suite
        .execute(
            &owner,
            &ExecuteMsg::CreateSubmission {
                name: "name".to_string(),
                url: "https://test.url".to_string(),
                address: addr_2.to_string(),
            },
            &[],
        )
        .unwrap();

    let selected = vec![
        (addr_1.to_string(), Decimal::percent(41)),
        (addr_2.to_string(), Decimal::percent(33)),
        (addr_3.to_string(), Decimal::percent(26)),
    ];

    let res: SampleGaugeMsgsResponse = suite
        .query(&AdapterQueryMsg::SampleGaugeMsgs {
            selected,
            epoch_budget: None,
            available_balance: None,
            denom: None,
        })
        .unwrap();
    assert_eq!(res.execute.len(), 3);
    assert_eq!(
        res.execute,
        [
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cw20.to_string(),
                msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: addr_1.to_string(),
                    amount: reward * Decimal::percent(41),
                })
                .unwrap(),
                funds: vec![],
            }),
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cw20.to_string(),
                msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: addr_2.to_string(),
                    amount: reward * Decimal::percent(33),
                })
                .unwrap(),
                funds: vec![],
            }),
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cw20.to_string(),
                msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: addr_3.to_string(),
                    amount: reward * Decimal::percent(26),
                })
                .unwrap(),
                funds: vec![],
            }),
        ]
    );

    // Suppress unused-import lint in case helper not used.
    let _ = submit_cw20_create;
}

// ------------------------------------------------------- Reject submission

#[test]
fn soft_reject_refunds_bond_and_removes_submission() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let owner = suite.owner.clone();
    let project = addr("project");
    let recipient = addr("recipient");

    // Project submits with bond.
    suite.mint_native(&project, coin(1_000, "juno"));
    suite
        .create_submission(&project, &recipient, Some(coin(1_000, "juno")))
        .unwrap();
    assert_eq!(suite.native_balance(&project, "juno"), Uint128::zero());
    let adapter = suite.adapter.clone();
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::new(1_000));

    // Admin soft-rejects.
    suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: recipient.to_string(),
            soft: true,
        })
        .unwrap();

    // Submission gone from registry; bond returned to project (the sender).
    let res: Result<SubmissionResponse, _> = suite.query(&AdapterQueryMsg::Submission {
        address: recipient.to_string(),
    });
    assert!(res.is_err());
    assert_eq!(suite.native_balance(&project, "juno"), Uint128::new(1_000));
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::zero());
    let _ = owner;
}

#[test]
fn hard_reject_forfeits_bond_to_community_pool() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let project = addr("project");
    let recipient = addr("recipient");
    let community_pool = suite.community_pool.clone();

    suite.mint_native(&project, coin(1_000, "juno"));
    suite
        .create_submission(&project, &recipient, Some(coin(1_000, "juno")))
        .unwrap();

    suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: recipient.to_string(),
            soft: false,
        })
        .unwrap();

    // Bond went to community pool, not the project.
    assert_eq!(suite.native_balance(&project, "juno"), Uint128::zero());
    assert_eq!(
        suite.native_balance(&community_pool, "juno"),
        Uint128::new(1_000),
    );
    let adapter = suite.adapter.clone();
    assert_eq!(suite.native_balance(&adapter, "juno"), Uint128::zero());
}

#[test]
fn reject_with_no_required_deposit_just_removes() {
    let mut suite = Suite::new_native(None);
    let owner = suite.owner.clone();
    let recipient = addr("recipient");

    suite.create_submission(&owner, &recipient, None).unwrap();
    // No bond was paid, so soft vs hard is moot.
    suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: recipient.to_string(),
            soft: true,
        })
        .unwrap();

    let res: Result<SubmissionResponse, _> = suite.query(&AdapterQueryMsg::Submission {
        address: recipient.to_string(),
    });
    assert!(res.is_err());
}

#[test]
fn reject_requires_admin() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));
    let project = addr("project");
    let recipient = addr("recipient");
    suite.mint_native(&project, coin(1_000, "juno"));
    suite
        .create_submission(&project, &recipient, Some(coin(1_000, "juno")))
        .unwrap();

    let intruder = addr("intruder");
    let err = suite
        .execute(
            &intruder,
            &ExecuteMsg::Reject {
                submission: recipient.to_string(),
                soft: true,
            },
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::Ownership(cw_ownable::OwnershipError::NotOwner),
        err.downcast().unwrap()
    );
}

#[test]
fn reject_missing_submission_errors() {
    let mut suite = Suite::new_native(None);
    let err = suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: "ghost".to_string(),
            soft: true,
        })
        .unwrap_err();
    assert_eq!(
        ContractError::SubmissionNotFound("ghost".to_string()),
        err.downcast().unwrap(),
    );
}

#[test]
fn reject_default_community_pool_submission_errors() {
    let mut suite = Suite::new_native(None);
    let community_pool = suite.community_pool.clone();
    let err = suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: community_pool.to_string(),
            soft: true,
        })
        .unwrap_err();
    assert_eq!(
        ContractError::CannotRejectDefault {},
        err.downcast().unwrap(),
    );
}

#[test]
fn reject_with_cw20_bond_routes_correctly() {
    let (mut suite, cw20) = Suite::new_cw20_deposit();
    let owner = suite.owner.clone();
    let adapter = suite.adapter.clone();
    let community_pool = suite.community_pool.clone();
    let recipient = addr("recipient");

    // Submit via cw20 Send.
    let inner = to_json_binary(&ReceiveMsg::CreateSubmission {
        name: "DAOers".into(),
        url: "https://daodao.zone".into(),
        address: recipient.to_string(),
    })
    .unwrap();
    suite
        .cw20_send(&cw20, &owner, &adapter, 1_000, inner)
        .unwrap();
    assert_eq!(suite.cw20_balance(&cw20, &adapter), Uint128::new(1_000));

    // Hard-reject → cw20 bond goes to community pool.
    suite
        .execute_owner(&ExecuteMsg::Reject {
            submission: recipient.to_string(),
            soft: false,
        })
        .unwrap();
    assert_eq!(suite.cw20_balance(&cw20, &adapter), Uint128::zero());
    assert_eq!(
        suite.cw20_balance(&cw20, &community_pool),
        Uint128::new(1_000)
    );
}
