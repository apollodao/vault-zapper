use cosmwasm_schema::cw_serde;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{
    to_binary, Addr, Coin, CosmosMsg, Env, QuerierWrapper, Response, StdResult, Uint128, WasmMsg,
};

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

/// Struct that helps keep track of how much of each coin belongs to the contract
/// and how much was sent by the caller.
#[cw_serde]
pub struct CoinBalances {
    /// The coins that belong to this contract
    pub contract_balances: Vec<Coin>,
    /// The coins that were sent by the caller
    pub caller_balances: Vec<Coin>,
}

impl CoinBalances {
    pub fn new(querier: &QuerierWrapper, env: &Env, funds: &Vec<Coin>) -> StdResult<Self> {
        let mut contract_balances = querier.query_all_balances(env.contract.address.to_string())?;

        // Deduct the received funds from the current balances
        for coin in funds {
            contract_balances
                .iter_mut()
                .find(|c| c.denom == coin.denom)
                .map(|c| c.amount -= coin.amount);
        }

        Ok(Self {
            contract_balances,
            caller_balances: funds.clone(),
        })
    }

    pub fn get_caller_balance(&self, denom: &str) -> Uint128 {
        self.caller_balances
            .iter()
            .find(|c| c.denom == denom)
            .map(|c| c.amount)
            .unwrap_or_default()
    }

    pub fn get_contract_balance(&self, denom: &str) -> Uint128 {
        self.contract_balances
            .iter()
            .find(|c| c.denom == denom)
            .map(|c| c.amount)
            .unwrap_or_default()
    }

    /// Update the struct to add any newly received funds to the caller_balances.
    /// Should be called in a CallbackMsg handler.
    pub fn update_balances(&mut self, querier: &QuerierWrapper, env: &Env) -> StdResult<()> {
        let new_balances = querier.query_all_balances(env.contract.address.to_string())?;

        // For every coin in new_balances:
        // Calculate the difference between the new balance and the old balance.
        // Add the difference to the caller_balance.
        for coin in new_balances {
            let old_balance = self
                .contract_balances
                .iter()
                .find(|c| c.denom == coin.denom)
                .map(|c| c.amount)
                .unwrap_or_default();

            let difference = coin.amount.checked_sub(old_balance)?;
            if difference > Uint128::zero() {
                self.caller_balances
                    .iter_mut()
                    .find(|c| c.denom == coin.denom)
                    .map(|c| c.amount += difference);
            }
        }

        Ok(())
    }
}
