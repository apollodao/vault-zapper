pub mod contract;
pub mod deposit;
pub mod error;
pub mod helpers;
pub mod lockup;
pub mod msg;
pub mod query;
pub mod state;
pub mod withdraw;

pub use crate::error::ContractError;

// Force selecting either `astroport` or `osmosis` features, or both.
#[cfg(not(any(feature = "astroport", feature = "osmosis")))]
compile_error!("Must select either `astroport` or `osmosis` feature, or both");
