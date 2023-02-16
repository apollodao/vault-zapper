use apollo_cw_asset::{Asset, AssetInfo};
use cosmwasm_std::{
    wasm_execute, Addr, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128,
};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;
use cw_vault_standard::extensions::lockup::{LockupExecuteMsg, LockupQueryMsg, UnlockingPosition};
use cw_vault_standard::{
    ExtensionExecuteMsg, ExtensionQueryMsg, VaultInfoResponse, VaultStandardExecuteMsg,
    VaultStandardQueryMsg,
};

use crate::helpers::merge_responses;
use crate::msg::ZapTo;
use crate::state::{WithdrawMsg, LOCKUP_IDS, ROUTER};
use crate::ContractError;

pub fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: ZapTo,
) -> Result<Response, ContractError> {
    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_token_denom = vault_info.vault_token;
    let vault_asset = AssetInfo::Native(vault_info.base_token);

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
            redeem_asset: Asset::new(vault_asset.clone(), amount_redeemed_from_vault),
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
    withdraw_assets: ZapTo,
) -> Result<Response, ContractError> {
    // Load users lockup IDs.
    let mut lock_ids = LOCKUP_IDS
        .load(deps.storage, info.sender.clone())
        .unwrap_or_default();

    // Check if lockup ID is valid.
    if !lock_ids.contains(&lockup_id) {
        return Err(ContractError::Std(StdError::not_found(format!(
            "lockup_id {lockup_id}"
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
    let vault_asset = AssetInfo::Native(vault_info.base_token);

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
            redeem_asset: Asset::new(vault_asset.clone(), amount_redeemed_from_vault),
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

// Called by execute_withdraw and execute_withdraw_unlocked to withdraw assets
// from the vault.
pub fn withdraw<F>(
    deps: DepsMut,
    env: Env,
    info: &MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: ZapTo,
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
    let vault_asset = AssetInfo::Native(vault_info.base_token);

    // Check if withdrawal asset is an LP token.
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &vault_asset).ok();

    // Create list of messages to return
    let mut withdraw_msgs = vec![];

    // Check requested withdrawal assets
    match withdraw_assets {
        ZapTo::Asset(requested_asset) => {
            // If the requested denom is the same as the vaults withdrawal asset
            // just withdraw directly to the recipient.
            if requested_asset == vault_asset {
                withdraw_msgs.push(
                    get_withdraw_msg(vault_address.to_string(), Some(recipient.to_string()))?.msg,
                );
                Ok(Response::new().add_messages(withdraw_msgs))
            } else {
                // Add message to withdraw from vault, but return assets to this contract.
                let withdraw = get_withdraw_msg(vault_address.to_string(), None)?;
                withdraw_msgs.push(withdraw.msg);

                let mut response = Response::new().add_messages(withdraw_msgs);

                let asset_withdrawn_from_vault = withdraw.redeem_asset;

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
                    response = response.add_messages(
                        router.basket_liquidate_msgs(
                            assets_withdrawn_from_lp
                                .into_iter()
                                .cloned()
                                .filter(|a| a.info != requested_asset)
                                .collect::<Vec<_>>()
                                .into(),
                            &requested_asset,
                            None,
                            Some(recipient.to_string()),
                        )?,
                    );

                    // If one of the underlying LP assets is the requested asset, add a message to
                    // send it to the recipient
                    if let Some(asset) = assets_withdrawn_from_lp.find(&requested_asset) {
                        response = response.add_message(asset.transfer_msg(recipient)?);
                    }
                } else {
                    // Basket liquidate the assets withdrawn from the vault
                    response = response.add_messages(router.basket_liquidate_msgs(
                        vec![asset_withdrawn_from_vault].into(),
                        &requested_asset,
                        None,
                        Some(recipient.to_string()),
                    )?);
                }

                Ok(response)
            }
        }
        ZapTo::Underlying {} => {
            // We currently only support withdrawing multiple assets if this
            // vault returns an LP token, in which case we return the underlying
            // LP assets from withdrawing liquidity to the user.
            // TODO: Support withdrawing multiple assets that are not in the vault.
            // To do this we need to add functionality to cw-dex-router.
            if let Some(pool) = pool {
                // Add message to withdraw asset from vault, withdraw liquidity,
                // and return withdrawn assets to recipient.
                let withdraw = get_withdraw_msg(vault_address.to_string(), None)?;
                withdraw_msgs.push(withdraw.msg);
                let assets_withdrawn_from_lp =
                    pool.simulate_withdraw_liquidity(deps.as_ref(), &withdraw.redeem_asset);
                let withdraw_liquidity_res =
                    pool.withdraw_liquidity(deps.as_ref(), &env, withdraw.redeem_asset)?;
                let send_to_recipient_msgs = assets_withdrawn_from_lp
                    .iter()
                    .map(|a| a.transfer_msgs(recipient.to_string()))
                    .collect::<StdResult<Vec<_>>>()?
                    .concat();
                Ok(merge_responses(vec![
                    Response::new().add_messages(withdraw_msgs),
                    withdraw_liquidity_res,
                    Response::new().add_messages(send_to_recipient_msgs),
                ]))
            } else {
                Err(ContractError::UnsupportedWithdrawal {})
            }
        }
    }
}
