#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
use cw2::set_contract_version;

use crate::deposit::{callback_deposit, callback_provide_liquidity, execute_deposit};
use crate::error::ContractError;
use crate::msg::{CallbackMsg, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::ROUTER;

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
        } => todo!(),
        ExecuteMsg::Unlock {
            vault_address,
            recipient,
        } => todo!(),
        ExecuteMsg::WithdrawUnlocked {
            vault_address,
            lockup_id,
            recipient,
            withdraw_assets,
        } => todo!(),
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

#[cfg(test)]
mod tests {}
