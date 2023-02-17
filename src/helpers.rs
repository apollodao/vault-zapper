use cosmwasm_schema::cw_serde;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use apollo_cw_asset::{AssetInfo, AssetList};
use cosmwasm_std::{to_binary, Addr, CosmosMsg, Deps, Env, StdResult, Uint128, WasmMsg};

use crate::msg::ExecuteMsg;

/// CwTemplateContract is a wrapper around Addr that provides a lot of helpers
/// for working with this.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct CwTemplateContract(pub Addr);

impl CwTemplateContract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call<T: Into<ExecuteMsg>>(&self, msg: T) -> StdResult<CosmosMsg> {
        let msg = to_binary(&msg.into())?;
        Ok(WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds: vec![],
        }
        .into())
    }
}

/// Struct that helps keep track of how much of each coin belongs to the
/// contract and how much was sent by the caller.
#[cw_serde]
pub struct TokenBalances {
    /// The coins that belong to this contract
    pub contract_balances: AssetList,
    /// The coins that were sent by the caller
    pub caller_balances: AssetList,
}

impl TokenBalances {
    pub fn new(deps: Deps, env: &Env, caller_funds: &AssetList) -> StdResult<Self> {
        let mut contract_balances =
            caller_funds.query_balances(&deps.querier, &env.contract.address)?;

        // Deduct the received native funds from the current balances
        // We only do this for native coins, since CW20's are not yet received
        for asset in caller_funds {
            if let Some(c) = contract_balances
                .iter_mut()
                .find(|c| c.info == asset.info && c.info.is_native())
            {
                c.amount -= asset.amount;
            };
        }

        Ok(Self {
            contract_balances: contract_balances.into(),
            caller_balances: caller_funds.clone(),
        })
    }

    pub fn get_caller_balance(&self, asset: &AssetInfo) -> Uint128 {
        self.caller_balances
            .find(asset)
            .map(|c| c.amount)
            .unwrap_or_default()
    }

    /// Update the struct to add any newly received funds to the
    /// caller_balances. Should be called in a CallbackMsg handler.
    pub fn update_balances(&mut self, deps: Deps, env: &Env) -> StdResult<()> {
        let mut new_balances = self
            .contract_balances
            .query_balances(&deps.querier, &env.contract.address)?;

        // Any new funds received by the contract should be added to the
        // caller_balances. So we can simply deduct the contract balances from
        // the new current balances.
        new_balances.deduct_many(&self.contract_balances)?;
        self.caller_balances = new_balances;
        Ok(())
    }
}
