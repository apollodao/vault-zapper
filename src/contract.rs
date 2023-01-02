use crate::deposit::{callback_deposit, callback_provide_liquidity, execute_deposit};
use crate::error::ContractError;
use crate::msg::{CallbackMsg, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::query::{query_depositable_assets, query_withdrawable_assets};
use crate::state::ROUTER;
use crate::withdraw::{execute_withdraw, execute_withdraw_unlocked};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use cw2::set_contract_version;

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
            withdraw_assets,
        } => execute_withdraw(
            deps,
            env,
            info,
            api.addr_validate(&vault_address)?,
            recipient,
            withdraw_assets,
        ),
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
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::DepositableAssets { vault_address } => to_binary(&query_depositable_assets(
            deps,
            deps.api.addr_validate(&vault_address)?,
        )?),
        QueryMsg::WithdrawableAssets { vault_address } => to_binary(&query_withdrawable_assets(
            deps,
            deps.api.addr_validate(&vault_address)?,
        )?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
