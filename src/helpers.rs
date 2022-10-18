use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{to_binary, Addr, CosmosMsg, Response, StdResult, WasmMsg};

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
