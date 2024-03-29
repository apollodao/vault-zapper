use cosmwasm_std::{
    to_json_binary, Addr, CosmosMsg, DepsMut, Empty, Env, MessageInfo, ReplyOn, Response, SubMsg,
    WasmMsg,
};
use cw_vault_standard::extensions::lockup::LockupExecuteMsg;
use cw_vault_standard::{
    ExtensionExecuteMsg, ExtensionQueryMsg, VaultInfoResponse, VaultStandardExecuteMsg,
    VaultStandardQueryMsg,
};

use crate::contract::UNLOCK_REPLY_ID;
use crate::state::TEMP_LOCK_KEY;
use crate::ContractError;

pub fn execute_unlock(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    vault_address: Addr,
) -> Result<Response, ContractError> {
    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_token_denom = vault_info.vault_token;

    // Make sure vault token was sent
    if info.funds.len() != 1 || info.funds[0].denom != vault_token_denom {
        return Err(ContractError::InvalidVaultToken {});
    }
    let vault_token = info.funds[0].clone();

    // Call unlock on the vault
    let unlock_msg: CosmosMsg<Empty> = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: vec![vault_token.clone()],
        msg: to_json_binary(
            &VaultStandardExecuteMsg::<ExtensionExecuteMsg>::VaultExtension(
                ExtensionExecuteMsg::Lockup(LockupExecuteMsg::Unlock {
                    amount: vault_token.amount,
                }),
            ),
        )?,
    });

    // Temporarily store the caller's address and the vault address so we can read
    // it in the reply entrypoint
    TEMP_LOCK_KEY.save(deps.storage, &(info.sender, vault_address))?;

    // We must add the unlock message as a submessage and parse the Lock ID in the
    // reply entrypoint.
    Ok(Response::new().add_submessage(SubMsg {
        gas_limit: None,
        id: UNLOCK_REPLY_ID,
        msg: unlock_msg,
        reply_on: ReplyOn::Success,
    }))
}
