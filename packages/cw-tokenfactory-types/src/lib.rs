pub mod msg;

pub mod cosmos;
pub mod cosmwasm;
pub use osmosis_std::types::osmosis::tokenfactory::v1beta1 as osmosis;
pub mod thorchain;

// helpers for both osmosis types (osmosis_std crate) and cosmwasm types. it
// needs to be named `shim` because osmosis_std assumes it exists.
mod shim;
#[cfg(not(any(
    feature = "osmosis_tokenfactory",
    feature = "cosmwasm_tokenfactory",
    feature = "thorchain_tokenfactory"
)))]
compile_error!("enable exactly one tokenfactory backend feature");
#[cfg(any(
    all(feature = "osmosis_tokenfactory", feature = "cosmwasm_tokenfactory"),
    all(feature = "osmosis_tokenfactory", feature = "thorchain_tokenfactory"),
    all(feature = "cosmwasm_tokenfactory", feature = "thorchain_tokenfactory")
))]
compile_error!("tokenfactory backend features are mutually exclusive");
