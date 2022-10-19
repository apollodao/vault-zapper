use cosmwasm_std::{Addr, Deps, StdError, StdResult};
use cw_asset::AssetInfo;
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::{msg::WithdrawAssets, state::ROUTER};

use cosmos_vault_standard::msg::{ExtensionQueryMsg, QueryMsg as VaultQueryMsg, VaultInfo};

pub fn query_depositable_assets(deps: Deps, vault_address: Addr) -> StdResult<Vec<String>> {
    let router = ROUTER.load(deps.storage)?;

    // Query the vault info
    let vault_info: VaultInfo = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let depositable_assets: Vec<String> = vault_info
        .deposit_coins
        .iter()
        .map(|x| x.denom.clone())
        .collect();

    // For now we only support vaults that have one deposit asset.
    // TODO: Support vaults with multiple deposit assets.
    if depositable_assets.len() != 1 {
        return Err(StdError::generic_err("Unsupported vault"));
    }
    let deposit_asset_info = AssetInfo::Native(depositable_assets[0].clone());

    // Check if deposit asset is an LP token
    let pool = Pool::get_pool_for_lp_token(deps, &deposit_asset_info).ok();

    // If deposit asset is an LP token, the target of the basket liquidation is
    // the first asset in the pool. Otherwise it is just the deposit asset.
    let target_asset = match pool {
        Some(pool) => {
            // Get the assets in the pool
            let pool_tokens: Vec<AssetInfo> = pool
                .get_pool_liquidity(deps)?
                .into_iter()
                .map(|x| x.info.clone())
                .collect();

            // We just choose the first asset in the pool as the target for the basket liquidation.
            // This could probably be optimized, but for now I think it's fine.
            pool_tokens
                .first()
                .ok_or(StdError::generic_err("Unsupported vault"))?
                .clone()
        }
        None => deposit_asset_info.clone(),
    };

    let supported_offer_assets =
        router.query_supported_offer_assets(&deps.querier, &target_asset)?;

    let mut depositable_assets: Vec<String> = vec![deposit_asset_info.to_string()];

    // Get only native coins from supported offer assets
    for asset in supported_offer_assets {
        if let AssetInfo::Native(denom) = asset {
            depositable_assets.push(denom);
        }
    }

    Ok(depositable_assets)
}

pub fn query_withdrawable_assets(
    deps: Deps,
    vault_address: Addr,
) -> StdResult<Vec<WithdrawAssets>> {
    let router = ROUTER.load(deps.storage)?;

    // Query the vault info
    let vault_info: VaultInfo = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let vault_assets: Vec<String> = vault_info
        .deposit_coins
        .iter()
        .map(|x| x.denom.clone())
        .collect();

    // For now we only support vaults that have one deposit asset.
    // TODO: Support vaults with multiple deposit assets.
    if vault_assets.len() != 1 {
        return Err(StdError::generic_err("Unsupported vault"));
    }
    let withdraw_asset_info = AssetInfo::Native(vault_assets[0].clone());

    // Check if the withdrawn asset is an LP token
    let pool = Pool::get_pool_for_lp_token(deps, &withdraw_asset_info).ok();

    // Create withdrawable assets vec with first one being the withdraw asset
    let mut withdrawable_assets: Vec<WithdrawAssets> =
        vec![WithdrawAssets::Single(withdraw_asset_info.to_string())];

    let supported_ask_assets: Vec<AssetInfo> = match pool {
        Some(pool) => {
            // Get the assets in the pool
            let pool_tokens: Vec<AssetInfo> = pool
                .get_pool_liquidity(deps)?
                .into_iter()
                .map(|x| x.info.clone())
                .collect();

            // Get supported ask assets for each of the assets in the pool
            let supported_ask_assets_per_pool_token = pool_tokens
                .iter()
                .map(|offer_asset| router.query_supported_ask_assets(&deps.querier, offer_asset))
                .collect::<StdResult<Vec<_>>>()?;

            // Keep only the ask assets that are supported for all pool tokens
            let mut supported_ask_assets: Vec<AssetInfo> = vec![];
            for ask_assets in supported_ask_assets_per_pool_token {
                if supported_ask_assets.is_empty() {
                    supported_ask_assets = ask_assets;
                } else {
                    supported_ask_assets = supported_ask_assets
                        .iter()
                        .filter(|ask_asset| ask_assets.contains(ask_asset))
                        .cloned()
                        .collect();
                }
            }

            // Add the multi-token case where equal to the tokens in the pair
            withdrawable_assets.push(WithdrawAssets::Multi(
                pool_tokens
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>(),
            ));

            supported_ask_assets
        }
        None => {
            // Withdrawn asset is not an LP token. Get all supported ask assets
            router.query_supported_ask_assets(&deps.querier, &withdraw_asset_info)?
        }
    };

    // Add all supported ask assets as single withdrawal options
    for ask_asset in supported_ask_assets {
        if let AssetInfo::Native(denom) = ask_asset {
            withdrawable_assets.push(WithdrawAssets::Single(denom));
        }
    }

    Ok(withdrawable_assets)
}
