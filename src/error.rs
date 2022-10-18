use cosmwasm_std::{OverflowError, StdError};
use cw_dex::CwDexError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CwDexError(#[from] CwDexError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Vault must have exactly one deposit coin")]
    UnsupportedVault {},
}
