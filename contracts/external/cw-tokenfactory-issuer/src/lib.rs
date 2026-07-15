#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

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

/// The smart contract itself, including the execute, instantiate, query, migrate
/// and reply entry points
pub mod contract;
/// Private error module, ContractError is re-exported in the public interface
mod error;
/// Contract methods that can be executed and alter state
pub mod execute;
/// Helper functions used for validation and checks
pub mod helpers;
/// Contract hooks
pub mod hooks;
/// Contract messages describing the API of the contract as well as responses
/// from contract queries
pub mod msg;
/// Contract queries
pub mod queries;
/// The contract state
pub mod state;

/// Error messages used in this contract
pub use crate::error::ContractError;
