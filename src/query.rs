use std::collections::HashMap;

use apollo_cw_asset::AssetInfo;
use cosmwasm_std::{Addr, Deps, Empty, Env, Order, StdError, StdResult};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;
use cw_storage_plus::Bound;

use crate::msg::ReceiveChoice;
use crate::state::{self, ASTROPORT_LIQUIDITY_MANAGER, DEFAULT_LIMIT, LOCKUP_IDS, ROUTER};

use cw_vault_standard::extensions::lockup::{LockupQueryMsg, UnlockingPosition};
use cw_vault_standard::{ExtensionQueryMsg, VaultContract, VaultStandardQueryMsg};

pub fn query_depositable_assets(deps: Deps, vault_address: Addr) -> StdResult<Vec<AssetInfo>> {
    let router = ROUTER.load(deps.storage)?;

    // Query the vault info
    let vault: VaultContract<Empty, Empty> = VaultContract::new(&deps.querier, &vault_address)?;
    let deposit_asset_info = match deps.api.addr_validate(&vault.base_token) {
        Ok(addr) => AssetInfo::cw20(addr),
        Err(_) => AssetInfo::native(&vault.base_token),
    };

    // Check if deposit asset is an LP token3
    let astroport_liquidity_manager = ASTROPORT_LIQUIDITY_MANAGER.may_load(deps.storage)?;
    let pool =
        Pool::get_pool_for_lp_token(deps, &deposit_asset_info, astroport_liquidity_manager).ok();

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

            // We just choose the first asset in the pool as the target for the basket
            // liquidation. This could probably be optimized, but for now I
            // think it's fine.
            pool_tokens
                .first()
                .ok_or(StdError::generic_err("Unsupported vault"))?
                .clone()
        }
        None => deposit_asset_info.clone(),
    };

    let supported_offer_assets =
        router.query_supported_offer_assets(&deps.querier, &target_asset)?;

    let depositable_assets = [
        &[deposit_asset_info, target_asset],
        supported_offer_assets.as_slice(),
    ]
    .concat();

    Ok(depositable_assets)
}

pub fn query_receive_choices(deps: Deps, vault_address: Addr) -> StdResult<Vec<ReceiveChoice>> {
    let router = ROUTER.load(deps.storage)?;

    // Query the vault info
    let vault: VaultContract<Empty, Empty> = VaultContract::new(&deps.querier, &vault_address)?;
    let withdraw_asset_info = match deps.api.addr_validate(&vault.base_token) {
        Ok(addr) => AssetInfo::cw20(addr),
        Err(_) => AssetInfo::native(&vault.base_token),
    };

    // Check if the withdrawn asset is an LP token
    let astroport_liquidity_manager = ASTROPORT_LIQUIDITY_MANAGER.may_load(deps.storage)?;
    let pool =
        Pool::get_pool_for_lp_token(deps, &withdraw_asset_info, astroport_liquidity_manager).ok();

    let swap_to_choices: Vec<AssetInfo> = match pool {
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
                    supported_ask_assets.retain(|ask_asset| ask_assets.contains(ask_asset));
                }
            }

            supported_ask_assets.extend(pool_tokens);
            supported_ask_assets
        }
        None => {
            // Withdrawn asset is not an LP token. Get all supported ask assets
            router.query_supported_ask_assets(&deps.querier, &withdraw_asset_info)?
        }
    };

    let swap_to_choices = swap_to_choices
        .iter()
        .map(|asset| ReceiveChoice::SwapTo(asset.clone()))
        .collect::<Vec<_>>();

    let receive_choices = [
        swap_to_choices.as_slice(),
        &[ReceiveChoice::BaseToken, ReceiveChoice::Underlying],
    ]
    .concat();

    Ok(receive_choices)
}

pub fn query_user_unlocking_positions_for_vault(
    deps: Deps,
    env: Env,
    vault_address: Addr,
    start_after_id: Option<u64>,
    limit: Option<u32>,
    user: Addr,
) -> StdResult<Vec<UnlockingPosition>> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT) as usize;
    let start: Option<Bound<u64>> = start_after_id.map(Bound::exclusive);

    let user_lockup_ids = LOCKUP_IDS
        .prefix((user, vault_address.clone()))
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit);

    let mut unlocking_positions: Vec<UnlockingPosition> = vec![];

    for res in user_lockup_ids {
        let (lockup_id, _) = res?;

        let unlocking_position = deps.querier.query_wasm_smart::<UnlockingPosition>(
            &vault_address,
            &VaultStandardQueryMsg::<ExtensionQueryMsg>::VaultExtension(ExtensionQueryMsg::Lockup(
                LockupQueryMsg::UnlockingPosition { lockup_id },
            )),
        )?;

        if unlocking_position.owner == env.contract.address {
            unlocking_positions.push(unlocking_position);
        }
    }
    Ok(unlocking_positions)
}

pub fn query_all_user_unlocking_positions(
    deps: Deps,
    env: Env,
    user: Addr,
    start_after_vault_addr: Option<String>,
    start_after_id: Option<u64>,
    limit: Option<u32>,
) -> StdResult<HashMap<Addr, Vec<UnlockingPosition>>> {
    let user_lockup_ids = state::paginate_all_user_unlocking_positions(
        deps,
        user,
        start_after_vault_addr,
        start_after_id,
        limit,
    )?;

    let mut unlocking_positions_per_vault: HashMap<Addr, Vec<UnlockingPosition>> = HashMap::new();

    for item in user_lockup_ids {
        let ((vault_address, lockup_id), _) = item?;

        let unlocking_position = deps.querier.query_wasm_smart::<UnlockingPosition>(
            &vault_address,
            &VaultStandardQueryMsg::<ExtensionQueryMsg>::VaultExtension(ExtensionQueryMsg::Lockup(
                LockupQueryMsg::UnlockingPosition { lockup_id },
            )),
        )?;

        if unlocking_position.owner == env.contract.address {
            if let Some(positions) = unlocking_positions_per_vault.get_mut(&vault_address) {
                positions.push(unlocking_position);
            } else {
                unlocking_positions_per_vault.insert(vault_address, vec![unlocking_position]);
            }
        }
    }

    Ok(unlocking_positions_per_vault)
}
