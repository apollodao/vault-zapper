#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};
use cw2::set_contract_version;

use crate::deposit::{callback_deposit, callback_provide_liquidity, execute_deposit};
use crate::error::ContractError;
use crate::lockup::execute_unlock;
use crate::msg::{CallbackMsg, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{LOCKUP_IDS, ROUTER, TEMP_UNLOCK_CALLER};
use crate::withdraw::{execute_withdraw, execute_withdraw_unlocked};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:vault-zapper";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ROUTER.save(deps.storage, &msg.router.check(deps.api)?)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let api = deps.api;
    match msg {
        ExecuteMsg::Deposit {
            vault_address,
            recipient,
            slippage_tolerance,
        } => execute_deposit(
            deps,
            env,
            info,
            api.addr_validate(&vault_address)?,
            recipient,
            slippage_tolerance,
        ),
        ExecuteMsg::Withdraw {
            vault_address,
            recipient,
            withdraw_assets,
        } => execute_withdraw(
            deps,
            env,
            info,
            api.addr_validate(&vault_address)?,
            recipient,
            withdraw_assets,
        ),
        ExecuteMsg::Unlock { vault_address } => {
            execute_unlock(deps, env, info, api.addr_validate(&vault_address)?)
        }
        ExecuteMsg::WithdrawUnlocked {
            vault_address,
            lockup_id,
            recipient,
            withdraw_assets,
        } => execute_withdraw_unlocked(
            deps,
            env,
            info,
            api.addr_validate(&vault_address)?,
            lockup_id,
            recipient,
            withdraw_assets,
        ),
        ExecuteMsg::Callback(msg) => match msg {
            CallbackMsg::ProvideLiquidity {
                vault_address,
                recipient,
                pool,
                receive_asset_before,
                deposit_asset_before,
                slippage_tolerance,
            } => callback_provide_liquidity(
                deps,
                env,
                info,
                vault_address,
                recipient,
                pool,
                deposit_asset_before,
                receive_asset_before,
                slippage_tolerance,
            ),
            CallbackMsg::Deposit {
                vault_address,
                recipient,
                deposit_asset_before,
            } => callback_deposit(
                deps,
                env,
                info,
                vault_address,
                recipient,
                deposit_asset_before,
            ),
        },
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(_deps: Deps, _env: Env, _msg: QueryMsg) -> StdResult<Binary> {
    unimplemented!()
}

pub const UNLOCK_REPLY_ID: u64 = 0u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        UNLOCK_REPLY_ID => {
            let response = msg
                .result
                .into_result()
                .map_err(|x| ContractError::Generic(x))?;

            // Parse lockup ID from data field
            let lockup_id: u64 = from_binary(
                &response
                    .data
                    .ok_or(ContractError::Generic("No data in reply".to_string()))?,
            )?;

            // Read temporarily stored caller address
            let caller_addr = TEMP_UNLOCK_CALLER.load(deps.storage)?;

            //Read users lock Ids.
            let mut lock_ids = LOCKUP_IDS.load(deps.storage, caller_addr.clone())?;

            lock_ids.push(lockup_id);

            // Store lockup_id
            LOCKUP_IDS.save(deps.storage, caller_addr, &lock_ids)?;

            //Erase temp caller address
            TEMP_UNLOCK_CALLER.remove(deps.storage);

            Ok(Response::default())
        }
        _ => Err(ContractError::Generic("Invalid reply id".to_string())),
    }
}

#[cfg(test)]
mod tests {}
