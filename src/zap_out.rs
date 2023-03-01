use crate::msg::ZapTo;
use crate::state::{WithdrawMsg, LOCKUP_IDS, ROUTER};
use crate::ContractError;
use apollo_cw_asset::{Asset, AssetInfo};
use apollo_utils::assets::receive_asset;
use apollo_utils::responses::merge_responses;
use cosmwasm_std::{
    wasm_execute, Addr, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128,
};
use cw_dex::traits::Pool as PoolTrait;
use cw_dex::Pool;
use cw_vault_standard::extensions::lockup::{LockupExecuteMsg, LockupQueryMsg, UnlockingPosition};
use cw_vault_standard::{
    ExtensionExecuteMsg, ExtensionQueryMsg, VaultInfoResponse, VaultStandardExecuteMsg,
    VaultStandardQueryMsg,
};

pub fn execute_zap_out(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    vault_address: Addr,
    amount: Uint128,
    zap_to: AssetInfo,
    recipient: Option<Addr>,
) -> Result<Response, ContractError> {
    let recipient = recipient.unwrap_or_else(|| info.sender.clone());

    // Query the vault info to get the base token
    let vault_info: VaultInfoResponse = deps.querier.query_wasm_smart(
        vault_address.to_string(),
        &VaultStandardQueryMsg::<ExtensionQueryMsg>::Info {},
    )?;
    let base_token = Asset::new(
        AssetInfo::from_str(deps.api, &vault_info.base_token),
        amount,
    );

    // Get the base tokens sent to the contract
    let mut response = receive_asset(&info, &env, &base_token)?;

    // Check if withdrawal asset is an LP token.
    let pool = Pool::get_pool_for_lp_token(deps.as_ref(), &base_token.info).ok();

    let router = ROUTER.load(deps.storage)?;

    if let Some(pool) = pool {
        // Add messages to withdraw liquidity
        let withdraw_liq_res = pool.withdraw_liquidity(deps.as_ref(), &env, base_token.clone())?;
        response = merge_responses(vec![response, withdraw_liq_res]);

        // Simulate withdrawal of liquidity to get the assets that will be returned
        let assets_withdrawn_from_lp =
            pool.simulate_withdraw_liquidity(deps.as_ref(), &base_token)?;

        // Add messages to basket liquidate the assets withdrawn from the LP
        response = response.add_messages(
            router.basket_liquidate_msgs(
                assets_withdrawn_from_lp
                    .into_iter()
                    .cloned()
                    .filter(|a| a.info != zap_to)
                    .collect::<Vec<_>>()
                    .into(),
                &zap_to,
                None,
                Some(recipient.to_string()),
            )?,
        );

        // If one of the underlying LP assets is the requested asset, add a message to
        // send it to the recipient
        if let Some(asset) = assets_withdrawn_from_lp.find(&zap_to) {
            response = response.add_message(asset.transfer_msg(recipient)?);
        }
    } else {
        // Basket liquidate the asset withdrawn from the vault
        response = response.add_messages(router.basket_liquidate_msgs(
            vec![base_token].into(),
            &zap_to,
            None,
            Some(recipient.to_string()),
        )?);
    }

    Ok(response)
}
