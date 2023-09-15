use apollo_cw_asset::{Asset, AssetInfo, AssetList};
use cosmwasm_std::{
    to_binary, Addr, CosmosMsg, DepsMut, Empty, Env, MessageInfo, Response, Uint128, WasmMsg,
};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;
use cw_vault_standard::{
    extensions::lockup::LockupExecuteMsg,
    msg::{ExtensionExecuteMsg, VaultStandardExecuteMsg as VaultExecuteMsg},
    VaultContract,
};

use crate::{
    msg::{CallbackMsg, ReceiveChoice},
    state::{LOCKUP_IDS, ROUTER},
    ContractError,
};

pub enum RedeemType {
    Normal,
    Lockup(u64),
}

pub fn execute_redeem(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    receive_choice: ReceiveChoice,
    min_out: AssetList,
) -> Result<Response, ContractError> {
    withdraw(
        deps,
        env,
        info,
        vault_address,
        recipient,
        receive_choice,
        min_out,
        RedeemType::Normal,
    )
}

pub fn execute_withdraw_unlocked(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    lockup_id: u64,
    recipient: Option<String>,
    receive_choice: ReceiveChoice,
    min_out: AssetList,
) -> Result<Response, ContractError> {
    // Load users lockup IDs.
    let mut lock_ids = LOCKUP_IDS.load(deps.storage, info.sender.clone())?;

    // Check if lockup ID is valid.
    if !lock_ids.contains(&lockup_id) {
        return Err(ContractError::Unauthorized {});
    }

    // Remove lockup ID from users lockup IDs.
    lock_ids.retain(|x| *x != lockup_id);
    LOCKUP_IDS.save(deps.storage, info.sender.clone(), &lock_ids)?;

    // Proceed with normal withdraw
    withdraw(
        deps,
        env,
        info,
        vault_address,
        recipient,
        receive_choice,
        min_out,
        RedeemType::Lockup(lockup_id),
    )
}

// Called by execute_withdraw and execute_withdraw_unlocked to withdraw assets from the vault.
pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    receive_choice: ReceiveChoice,
    min_out: AssetList,
    withdraw_type: RedeemType,
) -> Result<Response, ContractError> {
    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender), |x| deps.api.addr_validate(&x))?;

    // Query the vault info
    let vault: VaultContract<Empty, Empty> = VaultContract::new(&deps.querier, &vault_address)?;
    let vault_token_denom = &vault.vault_token;
    let vault_base_token = match deps.api.addr_validate(&vault.base_token) {
        Ok(addr) => AssetInfo::cw20(addr),
        Err(_) => AssetInfo::native(&vault.base_token),
    };

    // Get withdraw msg
    let withdraw_msg = match withdraw_type {
        RedeemType::Normal => {
            // Make sure vault token was sent
            if info.funds.len() != 1 || &info.funds[0].denom != vault_token_denom {
                return Err(ContractError::InvalidVaultToken {});
            }
            let vault_token = info.funds[0].clone();

            vault.redeem(vault_token.amount, None)?
        }
        RedeemType::Lockup(lockup_id) => CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_address.to_string(),
            funds: vec![],
            msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::VaultExtension(
                ExtensionExecuteMsg::Lockup(LockupExecuteMsg::WithdrawUnlocked {
                    recipient: None,
                    lockup_id,
                }),
            ))?,
        }),
    };

    Ok(Response::new().add_message(withdraw_msg).add_message(
        CallbackMsg::AfterRedeem {
            receive_choice,
            vault_base_token,
            recipient,
            min_out,
        }
        .into_cosmos_msg(&env)?,
    ))
}

pub fn callback_after_redeem(
    deps: DepsMut,
    env: Env,
    receive_choice: ReceiveChoice,
    vault_base_token: AssetInfo,
    recipient: Addr,
    min_out: AssetList,
) -> Result<Response, ContractError> {
    // Check contract's balance of vault's base token
    let base_token_balance =
        vault_base_token.query_balance(&deps.querier, &env.contract.address)?;
    let base_token = Asset::new(vault_base_token.clone(), base_token_balance);

    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &vault_base_token).ok();

    // Check requested withdrawal assets
    match &receive_choice {
        ReceiveChoice::SwapTo(requested_asset) => {
            // If the requested denom is the same as the vaults withdrawal asset, just send it to the
            // recipient.
            if requested_asset == &vault_base_token {
                return Ok(Response::new().add_message(base_token.transfer_msg(recipient)?));
            }

            // Check if the withdrawable asset is an LP token.
            let router = ROUTER.load(deps.storage)?;

            if let Some(pool) = pool {
                // Add messages to withdraw liquidity
                let withdraw_liq_res =
                    pool.withdraw_liquidity(deps.as_ref(), &env, base_token, AssetList::new())?;
                return Ok(withdraw_liq_res.add_message(
                    CallbackMsg::AfterWithdrawLiq {
                        assets: pool.pool_assets(deps.as_ref())?,
                        receive_choice,
                        recipient,
                        min_out,
                    }
                    .into_cosmos_msg(&env)?,
                ));
            } else {
                // Basket liquidate the asset withdrawn from the vault
                let min_out = unwrap_min_out(min_out, requested_asset)?;
                let msgs = router.basket_liquidate_msgs(
                    vec![base_token].into(),
                    &requested_asset,
                    Some(min_out),
                    Some(recipient.to_string()),
                )?;
                return Ok(Response::new().add_messages(msgs));
            }
        }
        ReceiveChoice::BaseToken => {
            return Ok(Response::new().add_message(base_token.transfer_msg(recipient)?));
        }
        ReceiveChoice::Underlying => {
            if let Some(pool) = pool {
                let pool_assets = pool.pool_assets(deps.as_ref())?;

                let res =
                    pool.withdraw_liquidity(deps.as_ref(), &env, base_token, AssetList::new())?;
                return Ok(res.add_message(
                    CallbackMsg::AfterWithdrawLiq {
                        assets: pool_assets,
                        receive_choice,
                        recipient,
                        min_out,
                    }
                    .into_cosmos_msg(&env)?,
                ));
            } else {
                return Err(ContractError::UnsupportedWithdrawal {});
            }
        }
    }
}

pub fn callback_after_withdraw_liq(
    deps: DepsMut,
    env: Env,
    assets: Vec<AssetInfo>,
    receive_choice: ReceiveChoice,
    recipient: Addr,
    min_out: AssetList,
) -> Result<Response, ContractError> {
    let router = ROUTER.load(deps.storage)?;

    let asset_balances =
        AssetList::query_asset_info_balances(assets, &deps.querier, &env.contract.address)?;

    match receive_choice {
        ReceiveChoice::SwapTo(requested_asset) => {
            let min_out = unwrap_min_out(min_out, &requested_asset)?;
            // Subtract the requested asset balance from min_out, as we will
            // transfer this amount to the recipient.
            let requested_asset_balance = asset_balances
                .find(&requested_asset)
                .map_or(Uint128::zero(), |x| x.amount);
            let min_out = min_out.saturating_sub(requested_asset_balance);

            // Add messages to basket liquidate the assets withdrawn from the LP, but filter out
            // the requested asset as we can't swap an asset to itself.
            let mut msgs = router.basket_liquidate_msgs(
                asset_balances
                    .to_vec()
                    .into_iter()
                    .filter(|x| x.info != requested_asset)
                    .collect::<Vec<_>>()
                    .into(),
                &requested_asset,
                Some(min_out),
                Some(recipient.to_string()),
            )?;

            // Add message to send the requested asset to the recipient if the balance is greater
            // than 0.
            if requested_asset_balance > Uint128::zero() {
                msgs.push(
                    Asset::new(requested_asset, requested_asset_balance).transfer_msg(recipient)?,
                );
            }

            Ok(Response::new().add_messages(msgs))
        }
        ReceiveChoice::Underlying => {
            // Verify min_out and then just send the assets to the recipient
            for min_asset in min_out.into_iter() {
                if asset_balances
                    .find(&min_asset.info)
                    .map(|x| x.amount)
                    .unwrap_or_default()
                    < min_asset.amount
                {
                    return Err(ContractError::MinOutNotMet {
                        min_out: min_asset.amount,
                        actual: asset_balances
                            .find(&min_asset.info)
                            .map(|x| x.amount)
                            .unwrap_or_default(),
                    });
                }
            }

            let msgs = asset_balances.transfer_msgs(recipient)?;
            Ok(Response::new().add_messages(msgs))
        }
        ReceiveChoice::BaseToken => {
            panic!("Should not be possible to receive base token from callback_after_withdraw_liq")
        }
    }
}

/// Unwraps a single asset amount from an AssetList.
fn unwrap_min_out(
    min_out: AssetList,
    requested_asset: &AssetInfo,
) -> Result<Uint128, ContractError> {
    // Since we are requesting a single asset out, make sure the min_out argument contains
    // the requested asset.
    if min_out.len() > 1 || (min_out.len() == 1 && &min_out.to_vec()[0].info != requested_asset) {
        return Err(ContractError::InvalidMinOut {});
    }
    if min_out.len() == 1 {
        Ok(min_out.to_vec()[0].amount)
    } else {
        Ok(Uint128::zero())
    }
}
