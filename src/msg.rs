use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{to_binary, Addr, Coin, CosmosMsg, Decimal, Env, StdResult, Uint128, WasmMsg};
use cw_asset::{Asset, AssetInfo};
use cw_dex::Pool;
use cw_dex_router::helpers::CwDexRouterUnchecked;

#[cw_serde]
pub struct InstantiateMsg {
    pub router: CwDexRouterUnchecked,
}

#[cw_serde]
pub enum ExecuteMsg {
    Deposit {
        vault_address: String,
        recipient: Option<String>,
        slippage_tolerance: Option<Decimal>,
    },
    Withdraw {
        vault_address: String,
        recipient: Option<String>,
        withdraw_assets: WithdrawAssets,
    },
    Unlock {
        vault_address: String,
        recipient: Option<String>,
    },
    WithdrawUnlocked {
        vault_address: String,
        lockup_id: u64,
        recipient: Option<String>,
        withdraw_assets: WithdrawAssets,
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
        /// The contracts pre-call balance of the asset we are receiving from the basket liquidation
        receive_asset_before: Asset,
        /// The contracts pre-call balance of the asset that will be deposited into the vault
        deposit_asset_before: Asset,
        /// An optional slippage tolerance to use when providing liquidity
        slippage_tolerance: Option<Decimal>,
    },
    Deposit {
        vault_address: Addr,
        recipient: Addr,
        deposit_asset_before: Asset,
    },
}

impl CallbackMsg {
    pub fn into_cosmos_msg(&self, env: &Env) -> StdResult<CosmosMsg> {
        Ok(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_binary(self)?,
            funds: vec![],
        }))
    }
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Returns Vec<Coin>. The user may deposit any amount of several of these.
    /// The zapper will call BasketLiquidate on all those tokens that are not
    /// in the pool. It will then call ProvideLiquidity.
    #[returns(Vec<Coin>)]
    DepositableAssets { vault: String },

    /// Returns Vec<WithdrawableAsset>. The user may withdraw only one of these.
    /// TODO: How to handle the case where the user wants to withdraw the assets in the pool?
    #[returns(Vec<WithdrawAssets>)]
    WithdrawableAssets { vault: String },
}

#[cw_serde]
pub enum WithdrawAssets {
    Single(String),
    Multi(Vec<String>),
}

#[test]
pub fn test_withdrawable_asset() {
    //Example response for ATOM-OSMO pool
    let example_response: Vec<WithdrawAssets> = vec![
        WithdrawAssets::Single("osmo".to_string()),
        WithdrawAssets::Single("usdc".to_string()),
        WithdrawAssets::Single("atom".to_string()),
        WithdrawAssets::Multi(vec!["atom".to_string(), "osmo".to_string()]),
    ];
}
