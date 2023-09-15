use apollo_utils::submessages::{find_event, parse_attribute_value};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};
use cw2::set_contract_version;
use cw_vault_standard::extensions::lockup::{
    UNLOCKING_POSITION_ATTR_KEY, UNLOCKING_POSITION_CREATED_EVENT_TYPE,
};

use crate::deposit::{
    callback_deposit, callback_enforce_min_out, callback_provide_liquidity, execute_deposit,
};
use crate::error::ContractError;
use crate::lockup::execute_unlock;
use crate::msg::{CallbackMsg, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::query::{
    query_depositable_assets, query_receive_choices, query_user_unlocking_positions,
};
use crate::state::{LIQUIDITY_HELPER, LOCKUP_IDS, ROUTER, TEMP_UNLOCK_CALLER};
use crate::withdraw::{
    callback_after_redeem, callback_after_withdraw_liq, execute_redeem, execute_withdraw_unlocked,
};

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
    LIQUIDITY_HELPER.save(deps.storage, &msg.liquidity_helper.check(deps.api)?)?;

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
            min_out,
        } => {
            let assets = assets.check(deps.api)?;
            execute_deposit(
                deps,
                env,
                info,
                assets,
                api.addr_validate(&vault_address)?,
                recipient,
                min_out,
            )
        }
        ExecuteMsg::Redeem {
            vault_address,
            recipient,
            receive_choice,
            min_out,
        } => {
            let min_out = min_out.check(deps.api)?;
            execute_redeem(
                deps,
                env,
                info,
                api.addr_validate(&vault_address)?,
                recipient,
                receive_choice,
                min_out,
            )
        }
        ExecuteMsg::Unlock { vault_address } => {
            execute_unlock(deps, env, info, api.addr_validate(&vault_address)?)
        }
        ExecuteMsg::WithdrawUnlocked {
            vault_address,
            lockup_id,
            recipient,
            receive_choice,
            min_out,
        } => {
            let min_out = min_out.check(deps.api)?;
            execute_withdraw_unlocked(
                deps,
                env,
                info,
                api.addr_validate(&vault_address)?,
                lockup_id,
                recipient,
                receive_choice,
                min_out,
            )
        }
        ExecuteMsg::Callback(msg) => {
            // Can only be called by self
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            match msg {
                CallbackMsg::ProvideLiquidity {
                    vault_address,
                    recipient,
                    pool,
                    deposit_asset_info,
                } => callback_provide_liquidity(
                    deps,
                    env,
                    info,
                    vault_address,
                    recipient,
                    pool,
                    deposit_asset_info,
                ),
                CallbackMsg::Deposit {
                    vault_address,
                    recipient,
                    deposit_asset_info,
                } => callback_deposit(
                    deps,
                    env,
                    info,
                    vault_address,
                    recipient,
                    deposit_asset_info,
                ),
                CallbackMsg::EnforceMinOut {
                    asset,
                    recipient,
                    balance_before,
                    min_out,
                } => callback_enforce_min_out(
                    deps,
                    env,
                    info,
                    asset,
                    recipient,
                    balance_before,
                    min_out,
                ),
                CallbackMsg::AfterRedeem {
                    receive_choice,
                    vault_base_token,
                    recipient,
                    min_out,
                } => callback_after_redeem(
                    deps,
                    env,
                    receive_choice,
                    vault_base_token,
                    recipient,
                    min_out,
                ),
                CallbackMsg::AfterWithdrawLiq {
                    assets,
                    receive_choice,
                    recipient,
                    min_out,
                } => callback_after_withdraw_liq(
                    deps,
                    env,
                    assets,
                    receive_choice,
                    recipient,
                    min_out,
                ),
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::DepositableAssets { vault_address } => to_binary(&query_depositable_assets(
            deps,
            deps.api.addr_validate(&vault_address)?,
        )?),
        QueryMsg::ReceiveChoices { vault_address } => to_binary(&query_receive_choices(
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

pub const UNLOCK_REPLY_ID: u64 = 143u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        UNLOCK_REPLY_ID => {
            let response = msg.result.into_result().map_err(ContractError::Generic)?;

            // Parse lockup ID from events
            let lockup_id: u64 = parse_attribute_value(
                find_event(
                    &response,
                    &format!("wasm-{UNLOCKING_POSITION_CREATED_EVENT_TYPE}"),
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
