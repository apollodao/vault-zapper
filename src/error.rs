use cosmwasm_std::{OverflowError, StdError, Uint128};
use cw_dex::CwDexError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Generic(String),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CwDexError(#[from] CwDexError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Vault must have exactly one deposit coin")]
    UnsupportedVault {},

    #[error("Can only withdraw multiple assets if the vault returns an LP token")]
    UnsupportedWithdrawal {},

    #[error("Invalid vault token sent")]
    InvalidVaultToken {},

    #[error("Minimum amount not met. Expected {min_out}, got {actual}")]
    MinOutNotMet { min_out: Uint128, actual: Uint128 },
}
