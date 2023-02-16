use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{to_binary, Addr, CosmosMsg, Decimal, Env, StdResult, WasmMsg};
use cw_asset::{AssetInfo, AssetListUnchecked};
use cw_dex::Pool;
use cw_dex_router::helpers::CwDexRouterUnchecked;

use crate::helpers::TokenBalances;

#[cw_serde]
pub struct InstantiateMsg {
    pub router: CwDexRouterUnchecked,
}

#[cw_serde]
pub enum ExecuteMsg {
    Deposit {
        assets: AssetListUnchecked,
        vault_address: String,
        recipient: Option<String>,
        slippage_tolerance: Option<Decimal>,
    },
    Withdraw {
        vault_address: String,
        recipient: Option<String>,
        zap_to: ZapTo,
    },
    Unlock {
        vault_address: String,
    },
    WithdrawUnlocked {
        vault_address: String,
        lockup_id: u64,
        recipient: Option<String>,
        zap_to: ZapTo,
    },
    Callback(CallbackMsg),
}

#[cw_serde]
pub enum CallbackMsg {
    ProvideLiquidity {
        /// The vaults address
        vault_address: Addr,
        /// The recipient of the vault tokens
        recipient: Addr,
        /// The pool to provide liquidity to
        pool: Pool,
        /// The coin balances of the contract and the coins received by the caller
        coin_balances: TokenBalances,
        /// An optional slippage tolerance to use when providing liquidity
        slippage_tolerance: Option<Decimal>,
    },
    Deposit {
        vault_address: Addr,
        recipient: Addr,
        coin_balances: TokenBalances,
        deposit_asset_info: AssetInfo,
    },
}

impl CallbackMsg {
    pub fn into_cosmos_msg(&self, env: &Env) -> StdResult<CosmosMsg> {
        Ok(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_binary(&ExecuteMsg::Callback(self.clone()))?,
            funds: vec![],
        }))
    }
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Returns Vec<AssetInfo>. The user may deposit any amount of several of these.
    #[returns(Vec<AssetInfo>)]
    DepositableAssets { vault_address: String },

    /// Returns Vec<AssetInfo>. The user may chose one of the options in
    /// this vec when calling Withdraw or WithdrawUnlocked.
    #[returns(Vec<AssetInfo>)]
    WithdrawableAssets { vault_address: String },

    /// Returns Vec<UnlockingPosition>. The user may withdraw from these positions
    /// if they have finished unlocking by calling WithdrawUnlocked.
    #[returns(Vec<cosmwasm_vault_standard::extensions::lockup::UnlockingPosition>)]
    UnlockingPositions {
        vault_address: String,
        owner: String,
    },
}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
pub enum ZapTo {
    /// Zap to asset
    Asset(AssetInfo),
    /// Zap to underlying LP assets
    Underlying {},
}

#[test]
pub fn test_withdrawable_asset() {
    //Example response for ATOM-OSMO pool
    let _example_response: Vec<ZapTo> = vec![
        ZapTo::Asset(AssetInfo::Native("osmo".to_string())),
        ZapTo::Asset(AssetInfo::Native("usdc".to_string())),
        ZapTo::Asset(AssetInfo::Native("atom".to_string())),
        ZapTo::Underlying {},
    ];
}
