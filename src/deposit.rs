use apollo_cw_asset::{Asset, AssetInfo, AssetList};
use apollo_utils::assets::receive_assets;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, DepsMut, Empty, Env, Event, MessageInfo, Response, Uint128,
};
use cw_vault_standard::VaultContract;

use crate::helpers::VaultHelper;
use crate::msg::{CallbackMsg, Pool};
use crate::state::{ASTROPORT_LIQUIDITY_MANAGER, LIQUIDITY_HELPER, ROUTER};
use crate::ContractError;

pub fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    assets: AssetList,
    vault_address: Addr,
    recipient: Option<String>,
    min_out: Uint128,
) -> Result<Response, ContractError> {
    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender.clone()), |x| deps.api.addr_validate(&x))?;

    let receive_assets_res = receive_assets(&info, &env, &assets)?;

    // Query the vault info to get the deposit asset
    let vault: VaultContract<Empty, Empty> = VaultContract::new(&deps.querier, &vault_address)?;
    let deposit_asset_info = match deps.api.addr_validate(&vault.base_token) {
        Ok(addr) => AssetInfo::cw20(addr),
        Err(_) => AssetInfo::native(&vault.base_token),
    };

    // Add a message to enforce the minimum amount of vault tokens received
    let vault_token = AssetInfo::native(&vault.vault_token);
    let balance_before = vault_token.query_balance(&deps.querier, recipient.clone())?;
    let enforce_min_out_msg = CallbackMsg::EnforceMinOut {
        assets: vec![vault_token.clone()],
        recipient: recipient.clone(),
        balances_before: vec![Asset::new(vault_token.clone(), balance_before)].into(),
        min_out: vec![Asset::new(vault_token.clone(), min_out)].into(),
    }
    .into_cosmos_msg(&env)?;

    let event = Event::new("apollo/vault-zapper/execute_deposit")
        .add_attribute("assets", to_json_binary(&assets)?.to_string())
        .add_attribute("vault_address", &vault_address)
        .add_attribute("recipient", &recipient)
        .add_attribute("min_out", min_out);

    // Check if coins sent are already same as the depositable assets
    // If yes, then just deposit the coins
    if assets.len() == 1 && assets.to_vec()[0].info == deposit_asset_info {
        let amount = assets.to_vec()[0].amount;
        let msgs = vault.increase_allowance_and_deposit(
            amount,
            &deposit_asset_info,
            Some(recipient.to_string()),
        )?;

        return Ok(receive_assets_res
            .add_messages(msgs)
            .add_message(enforce_min_out_msg)
            .add_event(event));
    }

    //Check if the depositable asset is an LP token
    let astroport_liquidity_manager = ASTROPORT_LIQUIDITY_MANAGER.may_load(deps.storage)?;
    let pool = Pool::get_pool_for_lp_token(
        deps.as_ref(),
        &deposit_asset_info,
        astroport_liquidity_manager,
    )
    .ok();

    // Set the target of the basket liquidation, depending on if depositable asset
    // is an LP token or not
    let receive_asset_infos = match &pool {
        Some(pool) => {
            // Get the assets in the pool
            pool.get_pool_liquidity(deps.as_ref())?
                .into_iter()
                .map(|x| x.info.clone())
                .collect()
        }
        None => {
            //Not an LP token. Use the depositable_asset as the target for the basket
            // liquidation
            vec![deposit_asset_info.clone()]
        }
    };

    // Basket Liquidate deposited coins
    // We only liquidate the coins that are not already the target asset
    let liquidate_coins = assets
        .into_iter()
        .filter_map(|a| {
            if !receive_asset_infos.contains(&a.info) {
                a.try_into().ok()
            } else {
                None
            }
        })
        .collect::<Vec<Coin>>();
    let receive_asset_info = receive_asset_infos[0].clone();
    let mut msgs = if !liquidate_coins.is_empty() {
        let router = ROUTER.load(deps.storage)?;
        router.basket_liquidate_msgs(liquidate_coins.into(), &receive_asset_info, None, None)?
    } else {
        vec![]
    };

    // If the depositable asset is an LP token, we add a message to provide
    // liquidity for this pool
    if let Some(pool) = pool {
        msgs.push(
            CallbackMsg::ProvideLiquidity {
                vault_address,
                recipient,
                pool,
                deposit_asset_info,
            }
            .into_cosmos_msg(&env)?,
        )
    } else {
        // If the depositable asset is not an LP token, we add a message to deposit the
        // coins into the vault
        msgs.push(
            CallbackMsg::Deposit {
                vault_address,
                recipient,
                deposit_asset_info,
            }
            .into_cosmos_msg(&env)?,
        );
    }

    Ok(receive_assets_res
        .add_messages(msgs)
        .add_message(enforce_min_out_msg)
        .add_event(event))
}

pub fn callback_provide_liquidity(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    pool: Pool,
    deposit_asset_info: AssetInfo,
) -> Result<Response, ContractError> {
    let pool_asset_balances = AssetList::query_asset_info_balances(
        pool.pool_assets(deps.as_ref())?,
        &deps.querier,
        &env.contract.address,
    )?;

    let liquidity_helper = LIQUIDITY_HELPER.load(deps.storage)?;

    let pool: Binary = match pool {
        #[cfg(feature = "astroport")]
        Pool::Astroport(pool) => to_json_binary(&pool)?,
        #[cfg(feature = "osmosis")]
        Pool::Osmosis(pool) => to_json_binary(&pool)?,
        #[allow(unreachable_patterns)]
        _ => panic!("Unsupported pool type"),
    };

    let provide_liquidity_msgs = liquidity_helper.balancing_provide_liquidity(
        pool_asset_balances,
        Uint128::zero(),
        pool,
        None,
    )?;

    let response = Response::new()
        .add_messages(provide_liquidity_msgs)
        .add_message(
            CallbackMsg::Deposit {
                vault_address,
                recipient,
                deposit_asset_info,
            }
            .into_cosmos_msg(&env)?,
        );

    Ok(response)
}

pub fn callback_deposit(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    deposit_asset_info: AssetInfo,
) -> Result<Response, ContractError> {
    let amount_to_deposit =
        deposit_asset_info.query_balance(&deps.querier, env.contract.address)?;

    let vault: VaultContract<_, _> =
        VaultContract::<Empty, Empty>::new(&deps.querier, &vault_address)?;
    let msgs = vault.increase_allowance_and_deposit(
        amount_to_deposit,
        &deposit_asset_info,
        Some(recipient.to_string()),
    )?;

    Ok(Response::new().add_messages(msgs))
}

pub fn callback_enforce_min_out(
    deps: DepsMut,
    assets: Vec<AssetInfo>,
    recipient: Addr,
    balances_before: AssetList,
    min_out: AssetList,
) -> Result<Response, ContractError> {
    let mut new_balances =
        AssetList::query_asset_info_balances(assets.clone(), &deps.querier, &recipient)?;
    let assets_received = new_balances.deduct_many(&balances_before)?;

    for asset in min_out.iter() {
        let received = assets_received
            .find(&asset.info)
            .map(|x| x.amount)
            .unwrap_or_default();
        if received < asset.amount {
            return Err(ContractError::MinOutNotMet {
                min_out: asset.amount,
                actual: received,
            });
        }
    }

    let event = Event::new("apollo/vault-zapper/callback_enforce_min_out")
        .add_attribute("recipient", recipient)
        .add_attribute("assets", to_json_binary(&assets)?.to_string())
        .add_attribute("min_out", to_json_binary(&min_out)?.to_string())
        .add_attribute(
            "assets_received",
            to_json_binary(&assets_received)?.to_string(),
        );

    Ok(Response::new().add_event(event))
}
