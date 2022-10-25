use cosmos_vault_standard::msg::VaultInfo;
use cosmos_vault_standard::msg::{
    ExecuteMsg as VaultExecuteMsg, ExtensionExecuteMsg, ExtensionQueryMsg,
    QueryMsg as VaultQueryMsg,
};
use cosmwasm_std::{
    to_binary, Addr, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, WasmMsg,
};
use cw_asset::{Asset, AssetInfo, AssetList};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::helpers::CoinBalances;
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
    let deposit_asset_info = AssetInfo::Native(depositable_assets[0].clone());

    // Check if coins sent are already same as the depositable assets
    // If yes, then just deposit the coins
    if info.funds.len() == 1 && info.funds[0].denom == deposit_asset_info.to_string() {
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
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &deposit_asset_info).ok();

    //Set the target of the basket liquidation, depending on if depositable asset is an LP token or not
    let receive_asset_info = match &pool {
        Some(x) => {
            // Get the assets in the pool
            let pool_tokens: Vec<AssetInfo> = x
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
            deposit_asset_info.clone()
        }
    };

    // Get the amount of coins sent by the caller and how much was already in the contract.
    let coin_balances = CoinBalances::new(&deps.querier, &env, &info.funds)?;

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
                coin_balances,
                deposit_asset_info,
                receive_asset_info,
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
                coin_balances,
                deposit_asset_info,
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
    mut coin_balances: CoinBalances,
    deposit_asset_info: AssetInfo,
    receive_asset_info: AssetInfo,
    slippage_tolerance: Option<Decimal>,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Update coin balances
    coin_balances.update_balances(&deps.querier, &env)?;

    // Provide liquidity with all assets returned from the basket liquidation
    // and any that the caller sent with the original message.
    let provide_liquidity_assets: AssetList = vec![Asset::new(
        deposit_asset_info.clone(),
        coin_balances.get_caller_balance(&receive_asset_info.to_string()),
    )]
    .into();

    // Simulate providing liquidity
    let lp_tokens_received =
        pool.simulate_provide_liquidity(deps.as_ref(), provide_liquidity_assets.clone())?;

    // Provide liquidity to the pool
    let mut response = pool.provide_liquidity(
        deps.as_ref(),
        &env,
        &info,
        provide_liquidity_assets,
        env.contract.address.clone(),
        slippage_tolerance,
    )?;

    // Deposit any LP tokens the caller sent with the original message plus those
    // received from this liquidity provision.
    let amount_to_deposit = coin_balances
        .get_caller_balance(&deposit_asset_info.to_string())
        .checked_add(lp_tokens_received.amount)?;

    // Deposit the coins into the vault
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: vec![Coin {
            denom: deposit_asset_info.to_string(),
            amount: amount_to_deposit,
        }],
        msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            receiver: Some(recipient.to_string()),
        })?,
    });
    response = response.add_message(deposit_msg);

    Ok(response)
}

pub fn callback_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    mut coin_balances: CoinBalances,
    deposit_asset_info: AssetInfo,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Update the coin balances
    coin_balances.update_balances(&deps.querier, &env)?;

    // Deposit the coins into the vault
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: vec![Coin {
            denom: deposit_asset_info.to_string(),
            amount: coin_balances.get_caller_balance(&deposit_asset_info.to_string()),
        }],
        msg: to_binary(&VaultExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            receiver: Some(recipient.to_string()),
        })?,
    });

    Ok(Response::new().add_message(deposit_msg))
}
