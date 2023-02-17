use cosmwasm_schema::cw_serde;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use apollo_cw_asset::{Asset, AssetInfo, AssetList};
use cosmwasm_std::{to_binary, Addr, CosmosMsg, Deps, Env, Response, StdResult, Uint128, WasmMsg};

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

/// Merge several Response objects into one. Currently ignores the data fields.
pub(crate) fn merge_responses(responses: Vec<Response>) -> Response {
    let mut merged = Response::default();
    for response in responses {
        merged = merged
            .add_attributes(response.attributes)
            .add_events(response.events)
            .add_messages(
                response
                    .messages
                    .iter()
                    .map(|m| m.msg.clone())
                    .collect::<Vec<_>>(),
            );
    }
    merged
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

        // Deduct the received funds from the current balances
        for asset in caller_funds {
            if let Some(c) = contract_balances.iter_mut().find(|c| c.info == asset.info) {
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
        let new_balances = self
            .contract_balances
            .query_balances(&deps.querier, &env.contract.address)?;

        // For every coin in new_balances:
        // Calculate the difference between the new balance and the old balance.
        // Add the difference to the caller_balance.
        for asset in &new_balances {
            let old_balance = self
                .caller_balances
                .find(&asset.info)
                .map(|a| a.amount)
                .unwrap_or_default();

            let difference = asset.amount.checked_sub(old_balance)?;
            if difference > Uint128::zero() {
                let mut caller_balances = self.caller_balances.to_vec();
                if let Some(a) = caller_balances.iter_mut().find(|a| a.info == asset.info) {
                    a.amount += difference;
                };
                self.caller_balances = caller_balances.into();
            }
        }

        Ok(())
    }
}
