use cosmwasm_std::{Addr, Deps, Env, StdError, StdResult};
use cw_asset::AssetInfo;
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::state::LOCKUP_IDS;
use crate::{msg::WithdrawAssets, state::ROUTER};

use cosmwasm_vault_standard::extensions::lockup::{LockupQueryMsg, UnlockingPosition};
use cosmwasm_vault_standard::{ExtensionQueryMsg, VaultInfoResponse, VaultStandardQueryMsg};

pub fn query_depositable_assets(deps: Deps, vault_address: Addr) -> StdResult<Vec<String>> {
    let router = ROUTER.load(deps.storage)?;

    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;

    let deposit_asset_info = AssetInfo::Native(vault_info.base_token.to_string());

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
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;

    let withdraw_asset_info = AssetInfo::Native(vault_info.base_token.to_string());

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

pub fn query_user_unlocking_positions(
    deps: Deps,
    env: Env,
    vault_address: Addr,
    user: Addr,
) -> StdResult<Vec<UnlockingPosition>> {
    let mut user_lockup_ids = LOCKUP_IDS.load(deps.storage, user).unwrap_or_default();
    user_lockup_ids.sort();
    let mut unlocking_positions: Vec<UnlockingPosition> = deps.querier.query_wasm_smart(
        vault_address,
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::VaultExtension(ExtensionQueryMsg::Lockup(
            LockupQueryMsg::UnlockingPositions {
                owner: env.contract.address.to_string(),
                start_after: if user_lockup_ids.len() > 0 && user_lockup_ids[0] > 0 {
                    Some(user_lockup_ids[0] - 1)
                } else {
                    None
                },
                limit: None,
            },
        )),
    )?;
    unlocking_positions.retain(|p| user_lockup_ids.contains(&p.id));
    Ok(unlocking_positions)
}
