use cosmwasm_schema::schemars::JsonSchema;
use cosmwasm_schema::serde::Serialize;

use apollo_cw_asset::AssetInfo;
use cosmwasm_std::{to_binary, CosmosMsg, StdResult, Uint128, WasmMsg};
use cw_vault_standard::VaultContract;

/// A trait to help with depositing an `Asset` into a vault.
pub trait VaultHelper {
    /// Returns a vector of CosmosMsgs that will increase the allowance of the
    /// token if it is a CW20 and deposit the token into the vault.
    fn increase_allowance_and_deposit(
        &self,
        amount: Uint128,
        deposit_asset_info: &AssetInfo,
        recipient: Option<String>,
    ) -> StdResult<Vec<CosmosMsg>>;
}

impl<E, Q> VaultHelper for VaultContract<E, Q>
where
    E: Serialize,
    Q: Serialize + JsonSchema,
{
    fn increase_allowance_and_deposit(
        &self,
        amount: Uint128,
        deposit_asset_info: &AssetInfo,
        recipient: Option<String>,
    ) -> StdResult<Vec<CosmosMsg>> {
        let mut msgs: Vec<CosmosMsg> = vec![];

        if deposit_asset_info.is_native() {
            msgs.push(self.deposit(amount, recipient)?);
        } else {
            // If CW20, first increase allowance
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: deposit_asset_info.to_string(),
                msg: to_binary(&cw20::Cw20ExecuteMsg::IncreaseAllowance {
                    spender: self.addr.to_string(),
                    amount,
                    expires: None,
                })?,
                funds: vec![],
            }));
            msgs.push(self.deposit_cw20(amount, recipient)?);
        };

        Ok(msgs)
    }
}
