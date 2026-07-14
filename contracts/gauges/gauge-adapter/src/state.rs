use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_denom::CheckedDenom;
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    /// Deposit required for valid submission.
    pub required_deposit: Option<Asset>,
    /// Address of contract where each deposit is transferred.
    pub community_pool: Addr,
    /// Total reward amount.
    pub reward: Asset,
}

pub const CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Asset {
    pub denom: CheckedDenom,
    pub amount: Uint128,
}

#[cw_serde]
pub struct Submission {
    pub sender: Addr,
    pub name: String,
    pub url: String,
    /// Exact escrow liability created for this submission. Synthetic entries
    /// have no bond.
    #[serde(default)]
    pub bond: Option<Bond>,
}

#[cw_serde]
pub struct Bond {
    pub asset: Asset,
    pub depositor: Addr,
    pub state: BondState,
}

#[cw_serde]
pub enum BondState {
    Active,
    Refunded,
    Forfeited,
}

// All submissions mapped by fund destination address.
pub const SUBMISSIONS: Map<Addr, Submission> = Map::new("submissions");
/// `(sender, destination)` index for bounded "my submissions" queries.
pub const SUBMISSION_BY_SENDER: Map<(&Addr, &Addr), ()> = Map::new("submission_by_sender");
/// Aggregate amount owed in the single configured deposit denomination.
pub const TOTAL_LIABILITIES: Item<Uint128> = Item::new("total_liabilities");
/// Stable scan cursor for resumable bulk refunds.
pub const REFUND_CURSOR: Item<Addr> = Item::new("refund_cursor");
/// Set after the one-way bond wind-down completes. This distinguishes a
/// never-started scan from a completed scan when no cursor is present.
pub const REFUNDS_COMPLETE: Item<bool> = Item::new("refunds_complete");
