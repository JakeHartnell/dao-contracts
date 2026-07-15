use cosmwasm_std::StdError;
use cw_utils::{ParseReplyError, PaymentError};
use dao_voting::threshold::ActiveThresholdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error(transparent)]
    ActiveThresholdError(#[from] ActiveThresholdError),

    #[error(transparent)]
    ParseReplyError(#[from] ParseReplyError),

    #[error(transparent)]
    PaymentError(#[from] PaymentError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Unapproved {kind} code id {code_id}")]
    UnapprovedCode { kind: String, code_id: u64 },

    #[error("Checksum mismatch for {kind} code id {code_id}")]
    ChecksumMismatch { kind: String, code_id: u64 },

    #[error("Got a submessage reply with unknown id: {id}")]
    UnknownReplyId { id: u64 },
}
