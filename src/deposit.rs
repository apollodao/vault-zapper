use apollo_cw_asset::{Asset, AssetInfo, AssetList};
use apollo_utils::assets::{increase_allowance_msgs, receive_assets};
use cosmwasm_std::{
    to_binary, Addr, CosmosMsg, DepsMut, Empty, Env, MessageInfo, Response, Uint128, WasmMsg,
};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;
use cw_vault_standard::{
    ExtensionExecuteMsg, ExtensionQueryMsg, VaultInfoResponse, VaultStandardExecuteMsg,
    VaultStandardQueryMsg,
};

use crate::helpers::TokenBalances;
use crate::msg::CallbackMsg;
use crate::state::ROUTER;
use crate::ContractError;

pub fn execute_deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    caller_funds: AssetList,
    vault_address: Addr,
    recipient: Option<String>,
    min_out: Uint128,
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

    let deposit_asset_info = AssetInfo::from_str(deps.api, &vault_info.base_token);

    // Check if coins sent are already same as the depositable assets
    // If yes, then just deposit the coins
    if caller_funds.len() == 1 && &caller_funds.to_vec()[0].info == &deposit_asset_info {
        //Increase allowance if the asset is a CW20 token
        let (allowance_msgs, funds) =
            increase_allowance_msgs(&env, &caller_funds, vault_address.clone())?;

        // Check if the amount sent is greater than the minimum amount
        let amount = caller_funds.to_vec()[0].amount;
        if amount < min_out {
            return Err(ContractError::MinOutNotReceived {
                expected: min_out,
                received: amount,
            });
        }

        let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: vault_address.to_string(),
            funds,
            msg: to_binary(&VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Deposit {
                amount,
                recipient: Some(recipient.to_string()),
            })?,
        });
        return Ok(receive_assets_res
            .add_messages(allowance_msgs)
            .add_message(deposit_msg));
    }

    //Check if the depositable asset is an LP token
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &deposit_asset_info).ok(); //TODO: Must update this fn to support Astroport

    //Set the target of the basket liquidation, depending on if depositable asset
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

    // Get the amount of tokens sent by the caller and how much was already in the
    // contract.
    let token_balances = TokenBalances::new(deps.as_ref(), &env, &caller_funds)?;

    // Basket Liquidate deposited coins
    // We only liquidate the coins that are not already the target asset
    let liquidate_coins: AssetList = caller_funds
        .to_vec()
        .into_iter()
        .filter(|a| !receive_asset_infos.contains(&a.info))
        .collect::<Vec<Asset>>()
        .into();
    let receive_asset_info = receive_asset_infos[0].clone();
    let mut msgs = if !liquidate_coins.len() == 0 {
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
                coin_balances: token_balances,
                min_out,
            }
            .into_cosmos_msg(&env)?,
        )
    } else {
        // If the depositable asset is not an LP token, we add a message to deposit the
        // asset into the vault
        msgs.push(
            CallbackMsg::Deposit {
                vault_address,
                recipient,
                coin_balances: token_balances,
                deposit_asset_info,
                min_out,
            }
            .into_cosmos_msg(&env)?,
        );
    }

    Ok(receive_assets_res.add_messages(msgs))
}

#[allow(clippy::too_many_arguments)]
pub fn callback_provide_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    recipient: Addr,
    pool: Pool,
    mut coin_balances: TokenBalances,
    min_out: Uint128,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Update coin balances
    coin_balances.update_balances(deps.as_ref(), &env)?;

    // Provide liquidity with all assets returned from the basket liquidation
    // and any that the caller sent with the original message.
    let provide_liquidity_assets: AssetList = pool
        .get_pool_liquidity(deps.as_ref())?
        .into_iter()
        .filter_map(|a| {
            let balance = coin_balances.get_caller_balance(&a.info);
            if balance > Uint128::zero() {
                Some(Asset::new(a.info.clone(), balance))
            } else {
                None
            }
        })
        .collect::<Vec<Asset>>()
        .into();

    // Simulate providing liquidity
    let lp_tokens_received =
        pool.simulate_provide_liquidity(deps.as_ref(), &env, provide_liquidity_assets.clone())?;

    // Provide liquidity to the pool
    let mut response = pool.provide_liquidity(
        deps.as_ref(),
        &env,
        provide_liquidity_assets,
        Uint128::one(),
    )?;

    // Query how many vault tokens would be received from depositing the LP tokens
    let vault_tokens_received = deps.querier.query_wasm_smart::<Uint128>(
        &vault_address,
        &VaultStandardQueryMsg::<Empty>::ConvertToShares {
            amount: lp_tokens_received.amount,
        },
    )?;
    if vault_tokens_received < min_out {
        return Err(ContractError::MinOutNotReceived {
            expected: min_out,
            received: vault_tokens_received,
        });
    }

    // Deposit any LP tokens the caller sent with the original message plus those
    // received from this liquidity provision.
    let amount_to_deposit = coin_balances
        .get_caller_balance(&lp_tokens_received.info)
        .checked_add(lp_tokens_received.amount)?;

    // Increase allowance for Cw20 tokens
    let deposit_assets =
        AssetList::from(vec![Asset::new(lp_tokens_received.info, amount_to_deposit)]);
    let (allowance_msgs, funds) =
        increase_allowance_msgs(&env, &deposit_assets, vault_address.clone())?;

    // Deposit the coins into the vault
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds,
        msg: to_binary(&VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            amount: amount_to_deposit,
            recipient: Some(recipient.to_string()),
        })?,
    });
    response = response
        .add_messages(allowance_msgs)
        .add_message(deposit_msg);

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
    min_out: Uint128,
) -> Result<Response, ContractError> {
    // Can only be called by self
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }

    // Update the coin balances
    coin_balances.update_balances(deps.as_ref(), &env)?;

    let deposit_amount = coin_balances.get_caller_balance(&deposit_asset_info);

    // Check that the minimum amount of vault tokens will be received
    let vault_tokens_received = deps.querier.query_wasm_smart::<Uint128>(
        &vault_address,
        &VaultStandardQueryMsg::<Empty>::ConvertToShares {
            amount: deposit_amount,
        },
    )?;
    if vault_tokens_received < min_out {
        return Err(ContractError::MinOutNotReceived {
            expected: min_out,
            received: vault_tokens_received,
        });
    }

    // Increase allowance for Cw20 tokens
    let deposit_assets = AssetList::from(vec![Asset::new(deposit_asset_info, deposit_amount)]);
    let (allowance_msgs, funds) =
        increase_allowance_msgs(&env, &deposit_assets, vault_address.clone())?;

    // Deposit the coins into the vault
    let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: vault_address.to_string(),
        funds: funds,
        msg: to_binary(&VaultStandardExecuteMsg::<ExtensionExecuteMsg>::Deposit {
            amount: deposit_amount,
            recipient: Some(recipient.to_string()),
        })?,
    });

    Ok(Response::new()
        .add_messages(allowance_msgs)
        .add_message(deposit_msg))
}
