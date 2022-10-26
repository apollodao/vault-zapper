use cosmos_vault_standard::{
    extensions::lockup::LockupExecuteMsg,
    msg::{
        AssetsResponse, ExecuteMsg as VaultExecuteMsg, ExtensionExecuteMsg, ExtensionQueryMsg,
        QueryMsg as VaultQueryMsg, VaultInfo,
    },
};
use cosmwasm_std::{
    to_binary, Addr, Coin, CosmosMsg, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
    WasmMsg,
};
use cw_asset::{Asset, AssetInfo};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::{
    helpers::merge_responses,
    msg::WithdrawAssets,
    state::{LOCKUP_IDS, ROUTER},
    ContractError,
};

pub fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: WithdrawAssets,
) -> Result<Response, ContractError> {
    let get_withdraw_msg = |vault_address: String, vault_token: Coin, recipient: Option<String>| {
        Ok::<CosmosMsg, StdError>(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_address,
            funds: vec![vault_token.clone()],
            msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Redeem {
                recipient,
                amount: vault_token.amount,
            })?,
        }))
    };

    withdraw(
        deps,
        env,
        info,
        vault_address,
        recipient,
        withdraw_assets,
        get_withdraw_msg,
    )
}

pub fn execute_withdraw_unlocked(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    lockup_id: u64,
    recipient: Option<String>,
    withdraw_assets: WithdrawAssets,
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
    let get_withdraw_msg = |vault_address: String, vault_token: Coin, recipient: Option<String>| {
        Ok::<CosmosMsg, StdError>(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_address,
            funds: vec![vault_token],
            msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::VaultExtension(
                ExtensionExecuteMsg::Lockup(LockupExecuteMsg::WithdrawUnlocked {
                    recipient,
                    lockup_id: Some(lockup_id),
                }),
            ))?,
        }))
    };

    withdraw(
        deps,
        env,
        info,
        vault_address,
        recipient,
        withdraw_assets,
        get_withdraw_msg,
    )
}

// Called by execute_withdraw and execute_withdraw_unlocked to withdraw assets from the vault.
pub fn withdraw<F>(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: WithdrawAssets,
    get_withdraw_msg: F,
) -> Result<Response, ContractError>
where
    F: Fn(String, Coin, Option<String>) -> StdResult<CosmosMsg>,
{
    let router = ROUTER.load(deps.storage)?;

    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender), |x| deps.api.addr_validate(&x))?;

    // Query the vault info
    let vault_info: VaultInfo = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_token_denom = vault_info.vault_token_denom;
    let vault_assets: Vec<String> = vault_info
        .deposit_coins
        .iter()
        .map(|x| x.denom.clone())
        .collect();

    //For now we only support vaults with one deposit/withdraw asset
    if vault_assets.len() != 1 {
        return Err(ContractError::UnsupportedVault {});
    }
    let vault_asset = AssetInfo::Native(vault_assets[0].clone());

    // Make sure vault token was sent
    if info.funds.len() != 1 || info.funds[0].denom != vault_token_denom {
        return Err(ContractError::InvalidVaultToken {});
    }
    let vault_token = info.funds[0].clone();

    // Check if withdrawal asset is an LP token.
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &vault_asset).ok();

    // Create list of messages to return
    let mut msgs: Vec<CosmosMsg> = vec![];

    // Simulate withdrawal to know how many assets we will receive,
    // and then swap these for the requested asset.
    let assets_withdrawn_from_vault: AssetsResponse = deps.querier.query_wasm_smart(
        vault_address.clone(),
        &VaultQueryMsg::<ExtensionQueryMsg>::PreviewRedeem {
            shares: info.funds[0].amount,
        },
    )?;
    let asset_withdrawn_from_vault: Asset = assets_withdrawn_from_vault.coins[0].clone().into();

    // Check requested withdrawal assets
    match withdraw_assets {
        WithdrawAssets::Single(requested_denom) => {
            // If the requested denom is the same as the vaults withdrawal asset
            // just withdraw directly to the recipient.
            if requested_denom == vault_asset.to_string() {
                msgs.push(get_withdraw_msg(
                    vault_address.to_string(),
                    vault_token,
                    Some(recipient.to_string()),
                )?);
                return Ok(Response::new().add_messages(msgs));
            } else {
                // Add message to withdraw from vault, but return assets to this contract.
                msgs.push(get_withdraw_msg(
                    vault_address.to_string(),
                    vault_token,
                    None,
                )?);
            }

            let mut response = Response::new().add_messages(msgs);

            // Check if the withdrawable asset is an LP token. If it is, add a message
            // to withdraw liquidity first.
            if let Some(pool) = pool {
                // Simulate withdrawal of liquidity to get the assets that will be returned
                let assets_withdrawn_from_lp = pool.simulate_withdraw_liquidity(
                    deps.as_ref(),
                    asset_withdrawn_from_vault.clone(),
                )?;

                // Add messages to withdraw liquidity
                let provide_liq_res =
                    pool.withdraw_liquidity(deps.as_ref(), &env, asset_withdrawn_from_vault)?;
                response = merge_responses(vec![response, provide_liq_res]);

                // Add messages to basket liquidate the assets withdrawn from the LP
                response = response.add_messages(router.basket_liquidate_msgs(
                    assets_withdrawn_from_lp,
                    &AssetInfo::native(requested_denom),
                    None,
                    Some(recipient.to_string()),
                )?);
            } else {
                // Basket liquidate the assets withdrawn from the vault
                response = response.add_messages(router.basket_liquidate_msgs(
                    assets_withdrawn_from_vault.coins.into(),
                    &AssetInfo::native(requested_denom),
                    None,
                    Some(recipient.to_string()),
                )?);
            }

            return Ok(response);
        }
        WithdrawAssets::Multi(requested_denoms) => {
            // We currently only support withdrawing multiple assets if these
            // the vault returns an LP token and the requested assets match the
            // assets in the pool.
            // TODO: Support withdrawing multiple assets that are not in the vault.
            // To do this we need to add functionality to cw-dex-router.
            if let Some(pool) = pool {
                // Check that the requested assets match the assets in the pool
                let pool_assets = pool
                    .get_pool_liquidity(deps.as_ref())?
                    .into_iter()
                    .map(|x| x.info.to_string())
                    .collect::<Vec<_>>();
                if requested_denoms != pool_assets {
                    return Err(ContractError::Generic(
                        "Requested assets do not match assets in pool".to_string(),
                    ));
                }

                // Add message to withdraw asset from vault, withdraw liquidity,
                // and return withdrawn assets to recipient.
                msgs.push(get_withdraw_msg(
                    vault_address.to_string(),
                    vault_token,
                    None,
                )?);
                let res =
                    pool.withdraw_liquidity(deps.as_ref(), &env, asset_withdrawn_from_vault)?;
                return Ok(merge_responses(vec![
                    Response::new().add_messages(msgs),
                    res,
                ]));
            } else {
                return Err(ContractError::UnsupportedWithdrawal {});
            }
        }
    }
}
