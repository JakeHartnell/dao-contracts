use cosmwasm_std::StdError;
use cw_ownable::OwnershipError;
use cw_utils::PaymentError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error(transparent)]
    Ownership(#[from] OwnershipError),

    #[error(transparent)]
    Payment(#[from] PaymentError),

    #[error("Option {0} already exists")]
    OptionAlreadyExists(String),

    #[error("Option {0} does not exist")]
    OptionDoesNotExist(String),

    #[error("InstantiateMsg must include at least one option")]
    NoOptions {},

    #[error("Too many options: {count}; maximum is {max}")]
    TooManyOptions { count: usize, max: usize },
}
