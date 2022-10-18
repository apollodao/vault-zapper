use cosmos_vault_standard::msg::{
    AssetsResponse, ExecuteMsg as VaultExecuteMsg, ExtensionExecuteMsg, ExtensionQueryMsg,
    QueryMsg as VaultQueryMsg, VaultInfo,
};
use cosmwasm_std::{to_binary, Addr, CosmosMsg, DepsMut, Env, MessageInfo, Response, WasmMsg};
use cw_asset::{Asset, AssetInfo};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::{helpers::merge_responses, msg::WithdrawAssets, state::ROUTER, ContractError};

pub fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    withdraw_assets: WithdrawAssets,
) -> Result<Response, ContractError> {
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
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: vault_address.to_string(),
                    funds: info.funds,
                    msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Withdraw {
                        receiver: Some(recipient.to_string()),
                    })?,
                }));
                return Ok(Response::new().add_messages(msgs));
            } else {
                // Add message to withdraw from vault, but return assets to this contract.
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: vault_address.to_string(),
                    funds: info.funds,
                    msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Withdraw {
                        receiver: None,
                    })?,
                }));
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
                let provide_liq_res = pool.withdraw_liquidity(
                    deps.as_ref(),
                    asset_withdrawn_from_vault,
                    env.contract.address.clone(),
                )?;
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
                msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: vault_address.to_string(),
                    funds: info.funds,
                    msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Withdraw {
                        receiver: None,
                    })?,
                }));
                let res =
                    pool.withdraw_liquidity(deps.as_ref(), asset_withdrawn_from_vault, recipient)?;
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
