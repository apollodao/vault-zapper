use cosmwasm_std::{
    wasm_execute, Addr, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128,
};
use cosmwasm_vault_standard::extensions::lockup::{LockupQueryMsg, UnlockingPosition};
use cosmwasm_vault_standard::{
    extensions::lockup::LockupExecuteMsg, ExtensionExecuteMsg, ExtensionQueryMsg,
    VaultInfoResponse, VaultStandardExecuteMsg, VaultStandardQueryMsg,
};
use cw_asset::{Asset, AssetInfo};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::state::{WithdrawMsg, LOCKUP_IDS, ROUTER};
use crate::{helpers::merge_responses, msg::WithdrawAssets, ContractError};

pub fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: WithdrawAssets,
) -> Result<Response, ContractError> {
    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_token_denom = vault_info.vault_token;
    let vault_asset = AssetInfo::Native(vault_info.base_token.to_string());

    // Make sure vault token was sent
    if info.funds.len() != 1 || &info.funds[0].denom != &vault_token_denom {
        return Err(ContractError::InvalidVaultToken {});
    }
    let vault_token = info.funds[0].clone();

    let amount_redeemed_from_vault: Uint128 = deps.querier.query_wasm_smart(
        vault_address.clone(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::PreviewRedeem {
            amount: vault_token.amount,
        },
    )?;

    let get_withdraw_msg = |vault_address: String, recipient: Option<String>| {
        Ok::<WithdrawMsg, StdError>(WithdrawMsg {
            msg: wasm_execute(
                vault_address,
                &VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Redeem {
                    recipient,
                    amount: vault_token.amount,
                },
                info.funds.to_vec(),
            )?,
            redeem_amount: Asset::new(vault_asset.clone(), amount_redeemed_from_vault),
        })
    };

    withdraw(
        deps,
        env,
        &info,
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
    let mut lock_ids = LOCKUP_IDS
        .load(deps.storage, info.sender.clone())
        .unwrap_or_default();

    // Check if lockup ID is valid.
    if !lock_ids.contains(&lockup_id) {
        return Err(ContractError::Std(StdError::not_found(format!(
            "lockup_id {}",
            lockup_id
        ))));
    }

    // Remove lockup ID from users lockup IDs.
    lock_ids.retain(|x| *x != lockup_id);
    LOCKUP_IDS.save(deps.storage, info.sender.clone(), &lock_ids)?;

    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_asset = AssetInfo::Native(vault_info.base_token.to_string());

    let unlocking_position: UnlockingPosition = deps.querier.query_wasm_smart(
        vault_address.clone(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::VaultExtension(ExtensionQueryMsg::Lockup(
            LockupQueryMsg::UnlockingPosition { lockup_id },
        )),
    )?;
    let amount_redeemed_from_vault = unlocking_position.base_token_amount;

    // Proceed with normal withdraw
    let get_withdraw_msg = |vault_address: String, recipient: Option<String>| {
        Ok::<WithdrawMsg, StdError>(WithdrawMsg {
            msg: wasm_execute(
                vault_address,
                &VaultStandardExecuteMsg::<ExtensionExecuteMsg>::VaultExtension(
                    ExtensionExecuteMsg::Lockup(LockupExecuteMsg::WithdrawUnlocked {
                        recipient,
                        lockup_id,
                    }),
                ),
                vec![],
            )?,
            redeem_amount: Asset::new(vault_asset.clone(), amount_redeemed_from_vault),
        })
    };

    withdraw(
        deps,
        env,
        &info,
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
    info: &MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: WithdrawAssets,
    get_withdraw_msg: F,
) -> Result<Response, ContractError>
where
    F: Fn(String, Option<String>) -> StdResult<WithdrawMsg>,
{
    let router = ROUTER.load(deps.storage)?;

    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender.clone()), |x| deps.api.addr_validate(&x))?;

    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_asset = AssetInfo::Native(vault_info.base_token.to_string());

    // Check if withdrawal asset is an LP token.
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &vault_asset).ok();

    // Create list of messages to return
    let mut msgs = vec![];

    // Check requested withdrawal assets
    match withdraw_assets {
        WithdrawAssets::Single(requested_denom) => {
            // If the requested denom is the same as the vaults withdrawal asset
            // just withdraw directly to the recipient.
            if requested_denom == vault_asset.to_string() {
                msgs.push(
                    get_withdraw_msg(vault_address.to_string(), Some(recipient.to_string()))?.msg,
                );
                return Ok(Response::new().add_messages(msgs));
            } else {
                // Add message to withdraw from vault, but return assets to this contract.
                let withdraw = get_withdraw_msg(vault_address.to_string(), None)?;
                msgs.push(withdraw.msg);

                let mut response = Response::new().add_messages(msgs);

                let asset_withdrawn_from_vault = withdraw.redeem_amount.clone();

                // Check if the withdrawable asset is an LP token. If it is, add a message
                // to withdraw liquidity first.
                if let Some(pool) = pool {
                    // Simulate withdrawal of liquidity to get the assets that will be returned
                    let assets_withdrawn_from_lp = pool
                        .simulate_withdraw_liquidity(deps.as_ref(), &asset_withdrawn_from_vault)?;

                    // Add messages to withdraw liquidity
                    let withdraw_liq_res =
                        pool.withdraw_liquidity(deps.as_ref(), &env, asset_withdrawn_from_vault)?;
                    response = merge_responses(vec![response, withdraw_liq_res]);

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
                        vec![asset_withdrawn_from_vault].into(),
                        &AssetInfo::native(requested_denom),
                        None,
                        Some(recipient.to_string()),
                    )?);
                }

                return Ok(response);
            }
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
                let withdraw = get_withdraw_msg(vault_address.to_string(), None)?;
                msgs.push(withdraw.msg);
                let res = pool.withdraw_liquidity(deps.as_ref(), &env, withdraw.redeem_amount)?;
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
