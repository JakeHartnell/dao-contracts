use cosmwasm_std::{coin, Addr, Uint128};
use cw_denom::UncheckedDenom;
use cw_multi_test::AppResponse;
use cw_ownable::{Action, Ownership, OwnershipError};

use crate::{
    msg::{AssetUnchecked, ExecuteMsg, QueryMsg as AdapterQueryMsg},
    multitest::suite::{addr, Suite},
    ContractError,
};

fn deposit_required() -> AssetUnchecked {
    AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }
}

fn assert_ownership_event(response: &AppResponse, sender: &Addr, owner: &str, pending_owner: &str) {
    assert!(response.events.iter().any(|event| {
        let has = |key: &str, value: &str| {
            event
                .attributes
                .iter()
                .any(|attribute| attribute.key == key && attribute.value == value)
        };
        has("action", "update_ownership")
            && has("sender", sender.as_str())
            && has("owner", owner)
            && has("pending_owner", pending_owner)
            && event
                .attributes
                .iter()
                .any(|attribute| attribute.key == "pending_expiry")
    }));
}

#[test]
fn ownership_starts_with_instantiator() {
    let suite = Suite::new_native(None);
    let owner = suite.owner.clone();
    let ownership: Ownership<cosmwasm_std::Addr> =
        suite.query(&AdapterQueryMsg::Ownership {}).unwrap();
    assert_eq!(ownership.owner, Some(owner));
    assert!(ownership.pending_owner.is_none());
}

#[test]
fn transfer_ownership_two_step_flow() {
    let mut suite = Suite::new_native(Some(deposit_required()));
    let old_owner = suite.owner.clone();
    let new_owner = addr("new_owner");
    let project = addr("project");
    let target_a = addr("recipient_a");
    let target_b = addr("recipient_b");

    // Two submissions so each phase has its own Reject target.
    suite.mint_native(&project, coin(2_000, "juno"));
    suite
        .create_submission(&project, &target_a, Some(coin(1_000, "juno")))
        .unwrap();
    suite
        .create_submission(&project, &target_b, Some(coin(1_000, "juno")))
        .unwrap();

    // Old owner proposes transfer.
    let transfer = suite
        .execute_owner(&ExecuteMsg::UpdateOwnership(Action::TransferOwnership {
            new_owner: new_owner.to_string(),
            expiry: None,
        }))
        .unwrap();
    assert_ownership_event(
        &transfer,
        &old_owner,
        old_owner.as_str(),
        new_owner.as_str(),
    );

    // Until accepted, pending owner cannot exercise authority.
    let err = suite
        .execute(
            &new_owner,
            &ExecuteMsg::Reject {
                submission: target_a.to_string(),
                soft: true,
            },
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::Ownership(OwnershipError::NotOwner),
        err.downcast().unwrap()
    );

    // New owner accepts.
    let accept = suite
        .execute(
            &new_owner,
            &ExecuteMsg::UpdateOwnership(Action::AcceptOwnership),
            &[],
        )
        .unwrap();
    assert_ownership_event(&accept, &new_owner, new_owner.as_str(), "none");

    // New owner can now reject; old owner cannot.
    suite
        .execute(
            &new_owner,
            &ExecuteMsg::Reject {
                submission: target_a.to_string(),
                soft: true,
            },
            &[],
        )
        .unwrap();
    let err = suite
        .execute(
            &old_owner,
            &ExecuteMsg::Reject {
                submission: target_b.to_string(),
                soft: true,
            },
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::Ownership(OwnershipError::NotOwner),
        err.downcast().unwrap()
    );
}

#[test]
fn renounce_ownership_locks_owner_methods() {
    let mut suite = Suite::new_native(Some(deposit_required()));
    let owner = suite.owner.clone();
    let project = addr("project");
    let recipient = addr("recipient");

    // A submission exists so Reject has a target.
    suite.mint_native(&project, coin(1_000, "juno"));
    suite
        .create_submission(&project, &recipient, Some(coin(1_000, "juno")))
        .unwrap();

    // Owner renounces.
    let renounce = suite
        .execute_owner(&ExecuteMsg::UpdateOwnership(Action::RenounceOwnership))
        .unwrap();
    assert_ownership_event(&renounce, &owner, "none", "none");

    // No one can call owner-gated methods now.
    let err = suite
        .execute(
            &owner,
            &ExecuteMsg::Reject {
                submission: recipient.to_string(),
                soft: true,
            },
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::Ownership(OwnershipError::NoOwner),
        err.downcast().unwrap()
    );

    // Ownership query reflects the renounce.
    let ownership: Ownership<cosmwasm_std::Addr> =
        suite.query(&AdapterQueryMsg::Ownership {}).unwrap();
    assert!(ownership.owner.is_none());
}

#[test]
fn non_owner_cannot_initiate_transfer() {
    let mut suite = Suite::new_native(None);
    let intruder = addr("intruder");
    let err = suite
        .execute(
            &intruder,
            &ExecuteMsg::UpdateOwnership(Action::TransferOwnership {
                new_owner: intruder.to_string(),
                expiry: None,
            }),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::Ownership(OwnershipError::NotOwner),
        err.downcast().unwrap()
    );
}
