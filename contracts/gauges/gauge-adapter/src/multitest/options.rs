use cosmwasm_std::{coin, Uint128};
use cw_denom::UncheckedDenom;

use crate::{
    msg::{
        AllOptionsResponse, AllSubmissionsResponse, AssetUnchecked, CheckOptionResponse,
        QueryMsg as AdapterQueryMsg,
    },
    multitest::suite::{addr, Suite},
};

#[test]
fn option_queries() {
    let mut suite = Suite::new_native(Some(AssetUnchecked {
        denom: UncheckedDenom::Native("juno".into()),
        amount: Uint128::new(1_000),
    }));

    let owner = suite.owner.clone();
    let recipient = addr("recipient");
    let newton = addr("newton");
    let einstein = addr("einstein");
    suite.mint_native(&einstein, coin(1_000, "juno"));
    suite.mint_native(&owner, coin(1_000, "juno"));

    let options: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::AllSubmissions {
            start_after: None,
            limit: None,
        })
        .unwrap();
    // account for the default option (community pool refund target).
    assert_eq!(options.submissions.len(), 1);

    // Valid submission from owner.
    suite
        .create_submission(&owner, &recipient, Some(coin(1_000, "juno")))
        .unwrap();

    // Valid submission from einstein (recipient = self).
    suite
        .create_submission(&einstein, &einstein, Some(coin(1_000, "juno")))
        .unwrap();

    let options: AllOptionsResponse = suite
        .query(&AdapterQueryMsg::AllOptions {
            start_after: None,
            limit: None,
        })
        .unwrap();
    let community_pool = suite.community_pool.clone();
    let mut expected = vec![
        einstein.to_string(),
        community_pool.to_string(),
        recipient.to_string(),
    ];
    expected.sort();
    let mut got = options.options.clone();
    got.sort();
    assert_eq!(got, expected);

    let option: CheckOptionResponse = suite
        .query(&AdapterQueryMsg::CheckOption {
            option: einstein.to_string(),
        })
        .unwrap();
    assert!(option.valid);

    let option: CheckOptionResponse = suite
        .query(&AdapterQueryMsg::CheckOption {
            option: newton.to_string(),
        })
        .unwrap();
    assert!(!option.valid);

    // SubmissionsBySender returns only submissions whose `sender` matches.
    let by_owner: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::SubmissionsBySender {
            sender: owner.to_string(),
            start_after: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(by_owner.submissions.len(), 1);
    assert_eq!(by_owner.submissions[0].address, recipient);

    let by_einstein: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::SubmissionsBySender {
            sender: einstein.to_string(),
            start_after: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(by_einstein.submissions.len(), 1);
    assert_eq!(by_einstein.submissions[0].address, einstein);

    // A sender with no submissions returns empty.
    let by_newton: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::SubmissionsBySender {
            sender: newton.to_string(),
            start_after: None,
            limit: None,
        })
        .unwrap();
    assert!(by_newton.submissions.is_empty());
}

#[test]
fn registry_queries_paginate_over_the_sender_index() {
    let mut suite = Suite::new_native(None);
    let owner = suite.owner.clone();
    let destinations = [
        addr("alpha-recipient"),
        addr("beta-recipient"),
        addr("gamma-recipient"),
    ];
    for destination in &destinations {
        suite.create_submission(&owner, destination, None).unwrap();
    }

    let first: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::SubmissionsBySender {
            sender: owner.to_string(),
            start_after: None,
            limit: Some(2),
        })
        .unwrap();
    assert_eq!(first.submissions.len(), 2);
    let cursor = first.submissions.last().unwrap().address.to_string();
    let second: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::SubmissionsBySender {
            sender: owner.to_string(),
            start_after: Some(cursor),
            limit: Some(2),
        })
        .unwrap();
    assert_eq!(second.submissions.len(), 1);

    suite
        .execute_owner(&crate::msg::ExecuteMsg::Reject {
            submission: destinations[1].to_string(),
            soft: true,
        })
        .unwrap();
    let remaining: AllSubmissionsResponse = suite
        .query(&AdapterQueryMsg::SubmissionsBySender {
            sender: owner.to_string(),
            start_after: None,
            limit: Some(100),
        })
        .unwrap();
    assert_eq!(remaining.submissions.len(), 2);
    assert!(remaining
        .submissions
        .iter()
        .all(|submission| submission.address != destinations[1]));

    let option_page: AllOptionsResponse = suite
        .query(&AdapterQueryMsg::AllOptions {
            start_after: Some(destinations[0].to_string()),
            limit: Some(1),
        })
        .unwrap();
    assert_eq!(option_page.options.len(), 1);
}
