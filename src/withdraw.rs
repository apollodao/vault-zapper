use crate::msg::Pool;
use apollo_cw_asset::{Asset, AssetInfo, AssetList};
use apollo_utils::assets::receive_assets;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, CosmosMsg, DepsMut, Empty, Env, Event, MessageInfo, Response, Uint128,
    WasmMsg,
};
use cw_vault_standard::extensions::lockup::LockupExecuteMsg;
use cw_vault_standard::msg::{ExtensionExecuteMsg, VaultStandardExecuteMsg as VaultExecuteMsg};
use cw_vault_standard::VaultContract;

use crate::msg::{CallbackMsg, ReceiveChoice};
use crate::state::{ASTROPORT_LIQUIDITY_MANAGER, LOCKUP_IDS, ROUTER};
use crate::ContractError;

#[cw_serde]
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
    let key = LOCKUP_IDS.key((info.sender.clone(), vault_address.clone(), lockup_id));

    // Check if lockup ID is valid.
    if !key.has(deps.storage) {
        return Err(ContractError::Unauthorized {});
    }

    // Remove lockup ID from users lockup IDs.
    key.remove(deps.storage);

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

// Called by execute_withdraw and execute_withdraw_unlocked to withdraw assets
// from the vault.
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
            msg: to_json_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::VaultExtension(
                ExtensionExecuteMsg::Lockup(LockupExecuteMsg::WithdrawUnlocked {
                    recipient: None,
                    lockup_id,
                }),
            ))?,
        }),
    };

    let event = Event::new("apollo/vault-zapper/withdraw")
        .add_attribute("vault_address", &vault_address)
        .add_attribute("recipient", &recipient)
        .add_attribute(
            "receive_choice",
            to_json_binary(&receive_choice)?.to_string(),
        )
        .add_attribute("withdraw_type", to_json_binary(&withdraw_type)?.to_string())
        .add_attribute("min_out", to_json_binary(&min_out)?.to_string());

    Ok(Response::new()
        .add_message(withdraw_msg)
        .add_message(
            CallbackMsg::AfterRedeem {
                receive_choice,
                vault_base_token,
                recipient,
                min_out,
            }
            .into_cosmos_msg(&env)?,
        )
        .add_event(event))
}

pub fn execute_zap_base_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    base_token: Asset,
    recipient: Option<String>,
    receive_choice: ReceiveChoice,
    min_out: AssetList,
) -> Result<Response, ContractError> {
    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender.clone()), |x| deps.api.addr_validate(&x))?;

    let receive_assets_res = receive_assets(&info, &env, &vec![base_token.clone()].into())?;

    let event = Event::new("apollo/vault-zapper/execute_zap_base_tokens")
        .add_attribute("base_token", to_json_binary(&base_token.info)?.to_string())
        .add_attribute("recipient", &recipient)
        .add_attribute(
            "receive_choice",
            to_json_binary(&receive_choice)?.to_string(),
        )
        .add_attribute("min_out", to_json_binary(&min_out)?.to_string());

    Ok(receive_assets_res
        .add_message(
            CallbackMsg::AfterRedeem {
                receive_choice,
                vault_base_token: base_token.info,
                recipient,
                min_out,
            }
            .into_cosmos_msg(&env)?,
        )
        .add_event(event))
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

    let astroport_liquidity_manager = ASTROPORT_LIQUIDITY_MANAGER.may_load(deps.storage)?;
    let pool = Pool::get_pool_for_lp_token(
        deps.as_ref(),
        &vault_base_token,
        astroport_liquidity_manager,
    )
    .ok();

    // Check requested withdrawal assets
    let (res, withdrawal_assets) = match &receive_choice {
        ReceiveChoice::SwapTo(requested_asset) => {
            // If the requested denom is the same as the vaults withdrawal asset, just send
            // it to the recipient.
            if requested_asset == &vault_base_token {
                Ok((
                    Response::new().add_message(base_token.transfer_msg(&recipient)?),
                    vec![base_token.info],
                ))
            } else {
                // Check if the withdrawable asset is an LP token.
                let router = ROUTER.load(deps.storage)?;

                if let Some(pool) = pool {
                    // Add messages to withdraw liquidity
                    let withdraw_liq_res =
                        pool.withdraw_liquidity(deps.as_ref(), &env, base_token, AssetList::new())?;
                    Ok((
                        withdraw_liq_res.add_message(
                            CallbackMsg::AfterWithdrawLiq {
                                assets: pool.pool_assets(deps.as_ref())?,
                                receive_choice: receive_choice.clone(),
                                recipient: recipient.clone(),
                            }
                            .into_cosmos_msg(&env)?,
                        ),
                        vec![requested_asset.clone()],
                    ))
                } else {
                    // Basket liquidate the asset withdrawn from the vault
                    let msgs = router.basket_liquidate_msgs(
                        vec![base_token].into(),
                        requested_asset,
                        None, // Not needed as we have our own min_out enforcement
                        Some(recipient.to_string()),
                    )?;
                    Ok((
                        Response::new().add_messages(msgs),
                        vec![requested_asset.clone()],
                    ))
                }
            }
        }
        ReceiveChoice::BaseToken => Ok((
            Response::new().add_message(base_token.transfer_msg(&recipient)?),
            vec![base_token.info.clone()],
        )),
        ReceiveChoice::Underlying => {
            if let Some(pool) = pool {
                let pool_assets = pool.pool_assets(deps.as_ref())?;

                let res =
                    pool.withdraw_liquidity(deps.as_ref(), &env, base_token, AssetList::new())?;
                Ok((
                    res.add_message(
                        CallbackMsg::AfterWithdrawLiq {
                            assets: pool_assets.clone(),
                            receive_choice,
                            recipient: recipient.clone(),
                        }
                        .into_cosmos_msg(&env)?,
                    ),
                    pool_assets,
                ))
            } else {
                Err(ContractError::UnsupportedWithdrawal {})
            }
        }
    }?;

    // Add a message to enforce the minimum amount of assets received
    let balances_before =
        AssetList::query_asset_info_balances(withdrawal_assets.clone(), &deps.querier, &recipient)?;
    let enforce_min_out_msg = CallbackMsg::EnforceMinOut {
        assets: withdrawal_assets,
        recipient: recipient.clone(),
        balances_before,
        min_out: min_out.clone(),
    }
    .into_cosmos_msg(&env)?;

    Ok(res.add_message(enforce_min_out_msg))
}

pub fn callback_after_withdraw_liq(
    deps: DepsMut,
    env: Env,
    assets: Vec<AssetInfo>,
    receive_choice: ReceiveChoice,
    recipient: Addr,
) -> Result<Response, ContractError> {
    let router = ROUTER.load(deps.storage)?;

    let asset_balances =
        AssetList::query_asset_info_balances(assets, &deps.querier, &env.contract.address)?;

    match receive_choice {
        ReceiveChoice::SwapTo(requested_asset) => {
            let requested_asset_balance = asset_balances
                .find(&requested_asset)
                .map_or(Uint128::zero(), |x| x.amount);

            // Add messages to basket liquidate the assets withdrawn from the LP, but filter
            // out the requested asset as we can't swap an asset to itself.
            let mut msgs = router.basket_liquidate_msgs(
                asset_balances
                    .to_vec()
                    .into_iter()
                    .filter(|x| x.info != requested_asset)
                    .collect::<Vec<_>>()
                    .into(),
                &requested_asset,
                None, // Not needed as we have our own min_out enforcement
                Some(recipient.to_string()),
            )?;

            // Add message to send the requested asset to the recipient if the balance is
            // greater than 0.
            if requested_asset_balance > Uint128::zero() {
                msgs.push(
                    Asset::new(requested_asset, requested_asset_balance).transfer_msg(recipient)?,
                );
            }

            Ok(Response::new().add_messages(msgs))
        }
        ReceiveChoice::Underlying => {
            let msgs = asset_balances.transfer_msgs(recipient)?;
            Ok(Response::new().add_messages(msgs))
        }
        ReceiveChoice::BaseToken => {
            panic!("Should not be possible to receive base token from callback_after_withdraw_liq")
        }
    }
}
