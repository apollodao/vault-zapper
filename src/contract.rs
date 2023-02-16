use apollo_utils::submessages::{find_event, parse_attribute_value};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};
use cosmwasm_vault_standard::extensions::lockup::{
    UNLOCKING_POSITION_ATTR_KEY, UNLOCKING_POSITION_CREATED_EVENT_TYPE,
};
use cw2::set_contract_version;

use crate::deposit::{callback_deposit, callback_provide_liquidity, execute_deposit};
use crate::error::ContractError;
use crate::lockup::execute_unlock;
use crate::msg::{CallbackMsg, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::query::{
    query_depositable_assets, query_user_unlocking_positions, query_withdrawable_assets,
};
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
            assets,
            vault_address,
            recipient,
            slippage_tolerance,
        } => {
            let assets = assets.check(deps.api)?;
            execute_deposit(
                deps,
                env,
                info,
                assets,
                api.addr_validate(&vault_address)?,
                recipient,
                slippage_tolerance,
            )
        }
        ExecuteMsg::Withdraw {
            vault_address,
            recipient,
            zap_to: withdraw_assets,
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
            zap_to: withdraw_assets,
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
                coin_balances,
                slippage_tolerance,
            } => callback_provide_liquidity(
                deps,
                env,
                info,
                vault_address,
                recipient,
                pool,
                coin_balances,
                slippage_tolerance,
            ),
            CallbackMsg::Deposit {
                vault_address,
                recipient,
                coin_balances,
                deposit_asset_info,
            } => callback_deposit(
                deps,
                env,
                info,
                vault_address,
                recipient,
                coin_balances,
                deposit_asset_info,
            ),
        },
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::DepositableAssets { vault_address } => to_binary(&query_depositable_assets(
            deps,
            deps.api.addr_validate(&vault_address)?,
        )?),
        QueryMsg::WithdrawableAssets { vault_address } => to_binary(&query_withdrawable_assets(
            deps,
            deps.api.addr_validate(&vault_address)?,
        )?),
        QueryMsg::UnlockingPositions {
            vault_address,
            owner,
        } => to_binary(&query_user_unlocking_positions(
            deps,
            env,
            deps.api.addr_validate(&vault_address)?,
            deps.api.addr_validate(&owner)?,
        )?),
    }
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

            // Parse lockup ID from events
            let lockup_id: u64 = parse_attribute_value(
                find_event(
                    &response,
                    &format!("wasm-{}", UNLOCKING_POSITION_CREATED_EVENT_TYPE),
                )?,
                UNLOCKING_POSITION_ATTR_KEY,
            )?;

            // Read temporarily stored caller address
            let caller_addr = TEMP_UNLOCK_CALLER.load(deps.storage)?;

            //Read users lock Ids.
            let mut lock_ids = LOCKUP_IDS
                .load(deps.storage, caller_addr.clone())
                .unwrap_or_default();

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

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
