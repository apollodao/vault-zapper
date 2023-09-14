use apollo_cw_asset::{Asset, AssetInfo};
use common::setup;
use cosmwasm_std::Uint128;
use cw_dex::traits::Pool;
use cw_it::{
    astroport::robot::AstroportTestRobot, helpers::Unwrap, test_tube::Account, OwnedTestRunner,
};
use cw_vault_standard_test_helpers::traits::CwVaultStandardRobot;

pub mod common;

#[test]
fn deposit_lp_token_works() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 0);

    // Deposit the LP token of the vault
    let balance = robot.query_base_token_balance(admin.address());
    let deposit_amount = Uint128::new(1000000);
    let deposit_asset = Asset::new(robot.deps.vault_pool.lp_token(), deposit_amount);

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
}

#[test]
fn deposit_one_asset_of_pool_works() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 0);

    let asset = robot.deps.pool_assets[0].clone();
    let balance = robot.query_asset_balance(&asset.clone().into(), &admin.address());
    let deposit_amount = Uint128::new(1000000);
    assert!(balance > deposit_amount);

    robot
        .zapper_deposit(
            vec![Asset::new(asset.clone(), deposit_amount)].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_asset_balance_eq(&asset.into(), &admin.address(), balance - deposit_amount);
}

#[test]
fn deposit_both_assets_of_pool_works() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 0);

    let asset1 = robot.deps.pool_assets[0].clone();
    let asset1_balance = robot.query_asset_balance(&asset1.clone().into(), &admin.address());
    let asset1_deposit_amount = Uint128::new(1000000);
    assert!(asset1_balance > asset1_deposit_amount);
    let asset2 = robot.deps.pool_assets[1].clone();
    let asset2_balance = robot.query_asset_balance(&asset2.clone().into(), &admin.address());
    let asset2_deposit_amount = Uint128::new(3000000);
    assert!(asset2_balance > asset2_deposit_amount);

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
            &asset1.into(),
            &admin.address(),
            asset1_balance - asset1_deposit_amount,
        )
        .assert_asset_balance_eq(
            &asset2.into(),
            &admin.address(),
            asset2_balance - asset2_deposit_amount,
        );
}

#[test]
fn deposit_asset_not_in_pool() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 0);

    let asset = AssetInfo::native("uastro");
    let pool_assets = &robot.deps.pool_assets;
    assert!(!pool_assets.contains(&asset));
    let balance = robot.query_asset_balance(&asset.clone().into(), &admin.address());
    let deposit_amount = Uint128::new(1000000);
    assert!(balance > deposit_amount);

    robot
        .zapper_deposit(
            vec![Asset::new(asset.clone(), deposit_amount)].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_asset_balance_eq(&asset.into(), &admin.address(), balance - deposit_amount);
}

#[test]
fn deposit_lp_min_out_respected() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 0);

    // Deposit the LP token of the vault
    let balance = robot.query_base_token_balance(admin.address());
    let deposit_amount = Uint128::new(1000000);
    let deposit_asset = Asset::new(robot.deps.vault_pool.lp_token(), deposit_amount);

    robot
        .zapper_deposit(
            vec![deposit_asset.clone()].into(),
            None,
            deposit_amount * Uint128::new(1_000_000_000_000),
            Unwrap::Err("Minimum amount not met"),
            &admin,
        )
        .zapper_deposit(
            vec![deposit_asset].into(),
            None,
            deposit_amount * Uint128::new(1_000_000),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_base_token_balance_eq(admin.address(), balance - deposit_amount);
}

#[test]
fn deposit_one_asset_of_pool_min_out_respected() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 0);

    let asset = robot.deps.pool_assets[0].clone();
    let balance = robot.query_asset_balance(&asset.clone().into(), &admin.address());
    let deposit_amount = Uint128::new(1000000);
    assert!(balance > deposit_amount);

    robot
        .zapper_deposit(
            vec![Asset::new(asset.clone(), deposit_amount)].into(),
            None,
            deposit_amount * Uint128::new(1_000_000_000_000),
            Unwrap::Err("Minimum amount not met"),
            &admin,
        )
        .zapper_deposit(
            vec![Asset::new(asset.clone(), deposit_amount)].into(),
            None,
            deposit_amount * Uint128::new(1_000),
            Unwrap::Ok,
            &admin,
        )
        .assert_vault_token_balance_gt(admin.address(), 0u128)
        .assert_asset_balance_eq(&asset.into(), &admin.address(), balance - deposit_amount);
}
