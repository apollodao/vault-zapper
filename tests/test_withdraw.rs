use apollo_cw_asset::{Asset, AssetInfo, AssetList, AssetUnchecked};
use common::setup;
use cosmwasm_std::{Decimal, Uint128};
use cw_dex::traits::Pool;
use cw_it::astroport::robot::AstroportTestRobot;
use cw_it::helpers::Unwrap;
use cw_it::test_tube::Account;
use cw_it::OwnedTestRunner;
use cw_vault_standard_test_helpers::traits::CwVaultStandardRobot;
use test_case::test_case;
use vault_zapper::msg::ReceiveChoice;

pub mod common;

#[test_case(0, true; "no lock, via ReceiveChoice::SwapTo")]
#[test_case(0, false; "no lock, via ReceiveChoice::BaseToken")]
#[test_case(300, true; "with lockup, via ReceiveChoice::SwapTo")]
#[test_case(300, false; "with lockup, via ReceiveChoice::BaseToken")]
fn withdraw_base_token(lock_duration: u64, via_swap_to: bool) {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, lock_duration);

    // Deposit the LP token of the vault
    let balance = robot.query_base_token_balance(admin.address());
    let deposit_amount = Uint128::new(1000000);
    let deposit_asset_info = robot.deps.vault_pool.lp_token();
    let deposit_asset = Asset::new(deposit_asset_info.clone(), deposit_amount);

    robot
        .zapper_deposit(
            vec![deposit_asset].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_base_token_balance_eq(admin.address(), balance - deposit_amount);

    let receive_choice = if via_swap_to {
        ReceiveChoice::SwapTo(deposit_asset_info.clone())
    } else {
        ReceiveChoice::BaseToken
    };
    if lock_duration == 0 {
        robot
            .zapper_redeem_all(
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    deposit_asset_info.clone().into(),
                    deposit_amount + Uint128::new(1),
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_redeem_all(
                None,
                receive_choice,
                vec![AssetUnchecked::new(
                    deposit_asset_info.clone().into(),
                    deposit_amount,
                )],
                Unwrap::Ok,
                &admin,
            );
    } else {
        robot
            .zapper_unlock_all(&admin)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                AssetList::new(),
                Unwrap::Err("Claim has not yet matured"),
                &admin,
            )
            .increase_time(lock_duration)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    deposit_asset_info.clone().into(),
                    deposit_amount + Uint128::new(1),
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice,
                vec![AssetUnchecked::new(
                    deposit_asset_info.clone().into(),
                    deposit_amount,
                )],
                Unwrap::Ok,
                &admin,
            );
    }

    robot
        .assert_vault_token_balance_eq(admin.address(), 0u128)
        .assert_base_token_balance_eq(admin.address(), balance);
}

#[test_case(0; "no lock")]
#[test_case(300; "with lockup")]
fn withdraw_one_asset_in_pool(lock_duration: u64) {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, lock_duration);

    let deposit_asset_info = robot.deps.pool_assets[0].clone();
    let balance_before =
        robot.query_asset_balance(&deposit_asset_info.clone().into(), &admin.address());
    let deposit_amount = Uint128::new(1000000);
    let deposit_asset = Asset::new(deposit_asset_info.clone(), deposit_amount);
    assert!(balance_before > deposit_amount);

    // Deposit
    robot
        .zapper_deposit(
            vec![deposit_asset].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_asset_balance_eq(
            &deposit_asset_info.clone().into(),
            &admin.address(),
            balance_before - deposit_amount,
        );

    let receive_choice = ReceiveChoice::SwapTo(deposit_asset_info.clone());
    if lock_duration == 0 {
        robot.zapper_redeem_all(
            None,
            receive_choice.clone(),
            vec![AssetUnchecked::new(
                deposit_asset_info.clone().into(),
                deposit_amount * (Decimal::one() - Decimal::permille(3)),
            )],
            Unwrap::Err("Minimum amount not met"),
            &admin,
        );
        robot.zapper_redeem_all(
            None,
            receive_choice,
            vec![AssetUnchecked::new(
                deposit_asset_info.clone().into(),
                deposit_amount * (Decimal::one() - Decimal::permille(4)),
            )],
            Unwrap::Ok,
            &admin,
        );
    } else {
        robot
            .zapper_unlock_all(&admin)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                AssetList::new(),
                Unwrap::Err("Claim has not yet matured"),
                &admin,
            )
            .increase_time(lock_duration)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    deposit_asset_info.clone().into(),
                    deposit_amount * (Decimal::one() - Decimal::permille(3)),
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice,
                vec![AssetUnchecked::new(
                    deposit_asset_info.clone().into(),
                    deposit_amount * (Decimal::one() - Decimal::permille(4)),
                )],
                Unwrap::Ok,
                &admin,
            );
    };

    let deposit_asset_balance_after = robot
        .assert_vault_token_balance_eq(admin.address(), 0u128)
        .query_asset_balance(&deposit_asset_info.into(), &admin.address());

    // Assert that approx 0.3% was lost due to swap fees
    let balance_diff = balance_before - deposit_asset_balance_after;
    assert!(Decimal::from_ratio(balance_diff, deposit_amount) < Decimal::permille(4));
}

#[test_case(0; "no lock")]
#[test_case(300; "with lockup")]
fn redeem_asset_not_in_pool(lock_duration: u64) {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, lock_duration);

    let deposit_asset_info = AssetInfo::native("uastro");
    let pool_assets = &robot.deps.pool_assets;
    assert!(!pool_assets.contains(&deposit_asset_info));
    let balance_before =
        robot.query_asset_balance(&deposit_asset_info.clone().into(), &admin.address());
    let deposit_amount = Uint128::new(1000000);
    assert!(balance_before > deposit_amount);

    robot
        .zapper_deposit(
            vec![Asset::new(deposit_asset_info.clone(), deposit_amount)].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_asset_balance_eq(
            &deposit_asset_info.clone().into(),
            &admin.address(),
            balance_before - deposit_amount,
        );

    let receive_choice = ReceiveChoice::SwapTo(deposit_asset_info.clone());
    if lock_duration == 0 {
        robot
            .zapper_redeem_all(None, receive_choice, AssetList::new(), Unwrap::Ok, &admin)
            .assert_vault_token_balance_eq(admin.address(), 0u128);
    } else {
        robot
            .zapper_unlock_all(&admin)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                AssetList::new(),
                Unwrap::Err("Claim has not yet matured"),
                &admin,
            )
            .increase_time(lock_duration)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice,
                AssetList::new(),
                Unwrap::Ok,
                &admin,
            );
    };

    let deposit_asset_balance_after = robot
        .assert_vault_token_balance_eq(admin.address(), 0u128)
        .query_asset_balance(&deposit_asset_info.into(), &admin.address());

    // Assert that approx X% was lost due to swap fees
    // TODO: Is 12 permille correct?
    let balance_diff = balance_before - deposit_asset_balance_after;
    assert!(Decimal::from_ratio(balance_diff, deposit_amount) < Decimal::permille(12));
}

#[test_case(0; "no lock")]
#[test_case(300; "with lockup")]
fn redeem_both_assets_of_pool(lock_duration: u64) {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, lock_duration);

    let asset1 = robot.deps.pool_assets[0].clone();
    let asset1_balance = robot.query_asset_balance(&asset1.clone().into(), &admin.address());
    let asset1_deposit_amount = Uint128::new(1000000);
    assert!(asset1_balance > asset1_deposit_amount);
    let asset2 = robot.deps.pool_assets[1].clone();
    let asset2_balance = robot.query_asset_balance(&asset2.clone().into(), &admin.address());
    let asset2_deposit_amount = Uint128::new(1000000);
    assert!(asset2_balance > asset2_deposit_amount);

    // Deposit both assets
    robot
        .zapper_deposit(
            vec![
                Asset::new(asset1.clone(), asset1_deposit_amount),
                Asset::new(asset2.clone(), asset2_deposit_amount),
            ]
            .into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_asset_balance_eq(
            &asset1.clone().into(),
            &admin.address(),
            asset1_balance - asset1_deposit_amount,
        )
        .assert_asset_balance_eq(
            &asset2.clone().into(),
            &admin.address(),
            asset2_balance - asset2_deposit_amount,
        );

    // Redeem both assets
    let receive_choice = ReceiveChoice::Underlying;
    let max_rel_diff = "0.000000001"; // One or two units lost due to rounding
    if lock_duration == 0 {
        robot
            .zapper_redeem_all(
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    asset1.clone().into(),
                    asset1_deposit_amount,
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_redeem_all(
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    asset2.clone().into(),
                    asset2_deposit_amount,
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_redeem_all(
                None,
                receive_choice.clone(),
                vec![
                    AssetUnchecked::new(asset1.clone().into(), asset1_deposit_amount),
                    AssetUnchecked::new(asset2.clone().into(), asset2_deposit_amount),
                ],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_redeem_all(
                None,
                receive_choice,
                vec![
                    AssetUnchecked::new(
                        asset1.clone().into(),
                        asset1_deposit_amount - Uint128::one() - Uint128::one(),
                    ),
                    AssetUnchecked::new(
                        asset2.clone().into(),
                        asset2_deposit_amount - Uint128::one() - Uint128::one(),
                    ),
                ],
                Unwrap::Ok,
                &admin,
            )
            .assert_vault_token_balance_eq(admin.address(), 0u128);
    } else {
        robot
            .zapper_unlock_all(&admin)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                AssetList::new(),
                Unwrap::Err("Claim has not yet matured"),
                &admin,
            )
            .increase_time(lock_duration)
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    asset1.clone().into(),
                    asset1_deposit_amount,
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                vec![AssetUnchecked::new(
                    asset2.clone().into(),
                    asset2_deposit_amount,
                )],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice.clone(),
                vec![
                    AssetUnchecked::new(asset1.clone().into(), asset1_deposit_amount),
                    AssetUnchecked::new(asset2.clone().into(), asset2_deposit_amount),
                ],
                Unwrap::Err("Minimum amount not met"),
                &admin,
            )
            .zapper_withdraw_unlocked(
                0,
                None,
                receive_choice,
                vec![
                    AssetUnchecked::new(
                        asset1.clone().into(),
                        asset1_deposit_amount - Uint128::one() - Uint128::one(),
                    ),
                    AssetUnchecked::new(
                        asset2.clone().into(),
                        asset2_deposit_amount - Uint128::one() - Uint128::one(),
                    ),
                ],
                Unwrap::Ok,
                &admin,
            )
            .assert_vault_token_balance_eq(admin.address(), 0u128);
    }

    robot
        .assert_asset_balance_approx_eq(asset1, &admin.address(), asset1_balance, max_rel_diff)
        .assert_asset_balance_approx_eq(asset2, &admin.address(), asset2_balance, max_rel_diff);
}
