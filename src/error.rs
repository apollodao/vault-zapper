use cosmwasm_std::{OverflowError, StdError};
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
}
