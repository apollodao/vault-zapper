use cosmwasm_schema::cw_serde;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{to_binary, Addr, CosmosMsg, Deps, Env, Response, StdResult, Uint128, WasmMsg};
use cw_asset::{Asset, AssetInfo, AssetList};

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
            Self::get_contract_balances_helper(deps, env, caller_funds)?.to_vec();

        // Deduct the received funds from the current balances
        for asset in caller_funds {
            contract_balances
                .iter_mut()
                .find(|c| c.info == asset.info)
                .map(|c| c.amount -= asset.amount);
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
        let new_balances = Self::get_contract_balances_helper(deps, env, &self.contract_balances)?;

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
                caller_balances
                    .iter_mut()
                    .find(|a| a.info == asset.info)
                    .map(|a| a.amount += difference);
                self.caller_balances = caller_balances.into();
            }
        }

        Ok(())
    }

    fn get_contract_balances_helper(
        deps: Deps,
        env: &Env,
        assets_to_query: &AssetList,
    ) -> StdResult<AssetList> {
        // get all native token balances on contract
        let mut contract_balances: AssetList = deps
            .querier
            .query_all_balances(env.contract.address.to_string())?
            .into();
        let contract_assets: Vec<AssetInfo> = contract_balances
            .into_iter()
            .map(|c| c.info.to_owned())
            .collect();
        // if provided, query balances for assets not included in above queried balances
        // should only be cw20s
        if assets_to_query.len() > 0 {
            let other_contract_balances = assets_to_query
                .into_iter()
                .filter_map(|a| {
                    if matches!(a.info, AssetInfo::Cw20(_)) && !contract_assets.contains(&a.info) {
                        let contract_balance: Uint128 = deps
                            .querier
                            .query_wasm_smart(
                                a.info.to_string(),
                                &cw20::Cw20QueryMsg::Balance {
                                    address: env.contract.address.to_string(),
                                },
                            )
                            .unwrap_or_default();
                        Some(Asset {
                            info: a.info.to_owned(),
                            amount: contract_balance,
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<Asset>>();
            contract_balances.add_many(&other_contract_balances.into())?;
        }
        Ok(contract_balances)
    }
}
