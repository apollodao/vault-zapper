use apollo_utils::assets::receive_assets;
use cosmwasm_std::{
    to_binary, Addr, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, StdResult,
    WasmMsg,
};
use cosmwasm_vault_standard::VaultInfoResponse;
use cosmwasm_vault_standard::{
    ExtensionExecuteMsg, ExtensionQueryMsg, VaultStandardExecuteMsg, VaultStandardQueryMsg,
};
use cw_asset::{Asset, AssetInfo, AssetList};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;

use crate::helpers::TokenBalances;
use crate::{msg::CallbackMsg, state::ROUTER, ContractError};

pub fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    caller_funds: AssetList,
    vault_address: Addr,
    recipient: Option<String>,
    slippage_tolerance: Option<Decimal>,
) -> Result<Response, ContractError> {
    // Unwrap recipient or use sender
    let recipient = recipient.map_or(Ok(info.sender.clone()), |x| deps.api.addr_validate(&x))?;

    let router = ROUTER.load(deps.storage)?;

    let receive_assets_res = receive_assets(&info, &env, &caller_funds)?;

    // Query the vault info
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;

    let deposit_asset_info = AssetInfo::Native(vault_info.base_token.to_string());

    // Check if coins sent are already same as the depositable assets
    // If yes, then just deposit the coins
    if caller_funds.len() == 1 && &caller_funds.to_vec()[0].info == &deposit_asset_info {
        let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_address.to_string(),
            funds: caller_funds
                .into_iter()
                .filter_map(|a| {
                    let native: StdResult<Coin> = a.try_into();
                    if let Ok(coin) = native {
                        Some(coin)
                    } else {
                        None
                    }
                })
                .collect(),
            msg: to_binary(&VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Deposit {
                amount: caller_funds.to_vec()[0].amount,
                recipient: Some(recipient.to_string()),
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

    // Get the amount of tokens sent by the caller and how much was already in the contract.
    let token_balances = TokenBalances::new(deps.as_ref(), &env, &caller_funds)?;

    // Basket Liquidate deposited coins
    // We only liquidate the coins that are not already the target asset
    let liquidate_coins: Vec<Coin> = info
        .funds
        .into_iter()
        .filter(|x| x.denom != receive_asset_info.to_string())
        .collect();
    let mut msgs = if liquidate_coins.len() > 0 {
        router.basket_liquidate_msgs(liquidate_coins.into(), &receive_asset_info, None, None)?
    } else {
        vec![]
    };

    // If the depositable asset is an LP token, we add a message to provide liquidity for this pool
    if let Some(pool) = pool {
        msgs.push(
            CallbackMsg::ProvideLiquidity {
                vault_address,
                recipient,
                pool,
                coin_balances: token_balances,
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
                coin_balances: token_balances,
                deposit_asset_info,
            }
            .into_cosmos_msg(&env)?,
        );
    }

    Ok(receive_assets_res.add_messages(msgs))
}

pub fn callback_provide_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    pool: Pool,
    mut coin_balances: TokenBalances,
    deposit_asset_info: AssetInfo,
    receive_asset_info: AssetInfo,
    slippage_tolerance: Option<Decimal>,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Update coin balances
    coin_balances.update_balances(deps.as_ref(), &env)?;

    // Provide liquidity with all assets returned from the basket liquidation
    // and any that the caller sent with the original message.
    let provide_liquidity_assets: AssetList = vec![Asset::new(
        deposit_asset_info.clone(),
        coin_balances.get_caller_balance(&receive_asset_info),
    )]
    .into();

    // Simulate providing liquidity
    let lp_tokens_received =
        pool.simulate_provide_liquidity(deps.as_ref(), &env, provide_liquidity_assets.clone())?;

    // Provide liquidity to the pool
    let mut response = pool.provide_liquidity(
        deps.as_ref(),
        &env,
        provide_liquidity_assets,
        if let Some(slippage_tolerance) = slippage_tolerance {
            lp_tokens_received.amount * (Decimal::one() - slippage_tolerance)
        } else {
            lp_tokens_received.amount
        },
    )?;

    // Deposit any LP tokens the caller sent with the original message plus those
    // received from this liquidity provision.
    let amount_to_deposit = coin_balances
        .get_caller_balance(&deposit_asset_info)
        .checked_add(lp_tokens_received.amount)?;

    // Deposit the coins into the vault
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: vec![Coin {
            denom: deposit_asset_info.to_string(),
            amount: amount_to_deposit,
        }],
        msg: to_binary(&VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            amount: amount_to_deposit,
            recipient: Some(recipient.to_string()),
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
    mut coin_balances: TokenBalances,
    deposit_asset_info: AssetInfo,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Update the coin balances
    coin_balances.update_balances(deps.as_ref(), &env)?;

    // Deposit the coins into the vault
    let caller_balance = coin_balances.get_caller_balance(&deposit_asset_info);
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: vec![Coin {
            denom: deposit_asset_info.to_string(),
            amount: caller_balance,
        }],
        msg: to_binary(&VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            amount: caller_balance,
            recipient: Some(recipient.to_string()),
        })?,
    });

    Ok(Response::new().add_message(deposit_msg))
}
