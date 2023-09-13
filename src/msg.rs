use apollo_cw_asset::{AssetInfo, AssetListUnchecked};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{to_binary, Addr, CosmosMsg, Env, StdResult, Uint128, WasmMsg};
use cw_dex::Pool;
use cw_dex_router::helpers::CwDexRouterUnchecked;
use liquidity_helper::LiquidityHelperUnchecked;

#[cw_serde]
pub struct InstantiateMsg {
    pub router: CwDexRouterUnchecked,
    pub liquidity_helper: LiquidityHelperUnchecked,
}

#[cw_serde]
pub enum ExecuteMsg {
    Deposit {
        /// The assets to deposit
        assets: AssetListUnchecked,
        /// The address of the vault to deposit into
        vault_address: String,
        /// The recipient of the vault tokens
        recipient: Option<String>,
        /// The minimum amount of vault tokens to receive. If the amount of
        /// vault tokens received is less than this, the transaction will fail.
        min_out: Uint128,
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
    /// Provide liquidity to a pool
    ProvideLiquidity {
        /// The vaults address
        vault_address: Addr,
        /// The recipient of the vault tokens
        recipient: Addr,
        /// The pool to provide liquidity to
        pool: Pool,
        /// The asset info of the vault's deposit asset
        deposit_asset_info: AssetInfo,
    },
    /// Performs the actual deposit into the vault
    Deposit {
        vault_address: Addr,
        recipient: Addr,
        deposit_asset_info: AssetInfo,
    },
    /// Enforce that the minimum amount of vault tokens are received
    EnforceMinOut {
        /// The asset to check the balance of
        asset: AssetInfo,
        /// The address to check the balance of
        recipient: Addr,
        /// The recipient's balance of `asset` before the transaction
        balance_before: Uint128,
        /// The minimum amount of `asset` to receive. If the amount of
        /// `asset` received is less than this, the transaction will fail.
        min_out: Uint128,
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
    /// Returns Vec<AssetInfo>. The user may deposit any amount of several of
    /// these.
    #[returns(Vec<AssetInfo>)]
    DepositableAssets { vault_address: String },

    /// Returns Vec<AssetInfo>. The user may chose one of the options in
    /// this vec when calling Withdraw or WithdrawUnlocked.
    #[returns(Vec<AssetInfo>)]
    WithdrawableAssets { vault_address: String },

    /// Returns Vec<UnlockingPosition>. The user may withdraw from these
    /// positions if they have finished unlocking by calling
    /// WithdrawUnlocked.
    #[returns(Vec<cw_vault_standard::extensions::lockup::UnlockingPosition>)]
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
