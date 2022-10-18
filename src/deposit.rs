use cosmos_vault_standard::msg::VaultInfo;
use cosmos_vault_standard::msg::{
    ExecuteMsg as VaultExecuteMsg, ExtensionExecuteMsg, ExtensionQueryMsg,
    QueryMsg as VaultQueryMsg,
};
use cosmwasm_std::{
    to_binary, Addr, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, WasmMsg,
};
use cw_asset::{Asset, AssetInfo};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::{msg::CallbackMsg, state::ROUTER, ContractError};

pub fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Option<String>,
    slippage_tolerance: Option<Decimal>,
) -> Result<Response, ContractError> {
    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender), |x| deps.api.addr_validate(&x))?;

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
    // To support vaults that have multiple deposit assets we must somehow swap
    // to a specific ratio of multiple assets, which is not trivial.
    if depositable_assets.len() != 1 {
        return Err(ContractError::UnsupportedVault {});
    }
    let depositable_asset = AssetInfo::Native(depositable_assets[0].clone());

    // Check if coins sent are already same as the depositable assets
    // If yes, then just deposit the coins
    if info.funds.len() == 1 && info.funds[0].denom == depositable_asset.to_string() {
        let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_address.to_string(),
            funds: info.funds,
            msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Deposit {
                receiver: Some(recipient.to_string()),
            })?,
        });
        return Ok(Response::new().add_message(deposit_msg));
    }

    //Check if the depositable asset is an LP token
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &depositable_asset).ok();

    //Set the target of the basket liquidation, depending on if depositable asset is an LP token or not
    let receive_asset_info = match pool {
        Some(x) => {
            // Get the assets in the pool
            let pool_tokens: Vec<AssetInfo> = x
                .as_trait()
                .get_pool_liquidity(deps.as_ref())?
                .into_iter()
                .map(|x| x.info.clone())
                .collect();

            // We just choose the first asset in the pool as the target for the basket liquidation.
            // This could probably be optimized, but for now I think it's fine.
            pool_tokens
                .first()
                .ok_or(ContractError::UnsupportedVault {})?
                .clone()
        }
        None => {
            //Not an LP token. Use the depositable_asset as the target for the basket liquidation
            depositable_asset.clone()
        }
    };

    // Query the receive asset balance of this contract to pass into the callback.
    // We do this so that we only provide liquidity or deposit the users sent assets,
    // and not any assets that were already in the contract.
    let mut receive_asset_balance =
        receive_asset_info.query_balance(&deps.querier, env.contract.address.to_string())?;

    // If the receive_asset was also sent by the user, we must deduct this since
    // we want the contract balance prior to the users deposit.
    let receive_assets_sent_by_caller = info
        .funds
        .iter()
        .find(|x| x.denom == receive_asset_info.to_string());
    if let Some(x) = receive_assets_sent_by_caller {
        receive_asset_balance = receive_asset_balance.checked_sub(x.amount)?;
    }
    let receive_asset_before = Asset::new(receive_asset_info.clone(), receive_asset_balance);

    // In the case that the depositable asset is an LP token, we must also do the above for the depositable asset,
    // since in this case depositable_asset != receive_asset.
    let deposit_asset_before = if depositable_asset == receive_asset_info {
        receive_asset_before.clone()
    } else {
        let mut deposit_asset_balance =
            depositable_asset.query_balance(&deps.querier, env.contract.address.to_string())?;
        let deposit_assets_sent_by_caller = info
            .funds
            .iter()
            .find(|x| x.denom == depositable_asset.to_string());
        if let Some(x) = deposit_assets_sent_by_caller {
            deposit_asset_balance = deposit_asset_balance.checked_sub(x.amount)?;
        }
        Asset::new(depositable_asset.clone(), deposit_asset_balance)
    };

    // Basket Liquidate deposited coins
    // We only liquidate the coins that are not already the target asset
    let liquidate_coins: Vec<Coin> = info
        .funds
        .into_iter()
        .filter(|x| x.denom != receive_asset_info.to_string())
        .collect();
    let mut msgs =
        router.basket_liquidate_msgs(liquidate_coins.into(), &receive_asset_info, None, None)?;

    // If the depositable asset is an LP token, we add a message to provide liquidity for this pool
    if let Some(pool) = pool {
        msgs.push(
            CallbackMsg::ProvideLiquidity {
                vault_address,
                recipient,
                pool,
                deposit_asset_before,
                receive_asset_before,
                slippage_tolerance,
            }
            .into_cosmos_msg(&env)?,
        )
    } else {
        // If the depositable asset is not an LP token, we add a message to deposit the coins into the vault
        msgs.push(
            CallbackMsg::Deposit {
                vault_address,
                recipient,
                deposit_asset_before: receive_asset_before,
            }
            .into_cosmos_msg(&env)?,
        );
    }

    Ok(Response::new().add_messages(msgs))
}

pub fn callback_provide_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    pool: Pool,
    deposit_asset_before: Asset,
    receive_asset_before: Asset,
    slippage_tolerance: Option<Decimal>,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Get the amount of the asset that was received from the basket liquidation
    let receive_asset_balance = receive_asset_before
        .info
        .query_balance(&deps.querier, env.contract.address.clone())?;
    let assets_received = receive_asset_balance.checked_sub(receive_asset_before.amount)?;

    let assets = vec![Asset::new(receive_asset_before.info, assets_received)].into();

    // Provide liquidity to the pool
    let mut response = pool.provide_liquidity(
        deps.as_ref(),
        assets,
        env.contract.address.clone(),
        slippage_tolerance,
    )?;

    // Add a message to deposit the coins into the vault
    response = response.add_message(
        CallbackMsg::Deposit {
            vault_address,
            recipient,
            deposit_asset_before,
        }
        .into_cosmos_msg(&env)?,
    );

    Ok(response)
}

pub fn callback_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    deposit_asset_before: Asset,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Query the deposit asset balance of this contract
    let deposit_asset_balance = deposit_asset_before
        .info
        .query_balance(&deps.querier, env.contract.address)?;

    let assets_received = deposit_asset_balance.checked_sub(deposit_asset_before.amount)?;

    // Deposit the coins into the vault
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: vec![Coin {
            denom: deposit_asset_before.info.to_string(),
            amount: assets_received,
        }],
        msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            receiver: Some(recipient.to_string()),
        })?,
    });

    Ok(Response::new().add_message(deposit_msg))
}
