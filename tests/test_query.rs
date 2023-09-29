use std::str::FromStr;

use apollo_cw_asset::{Asset, AssetInfo};
use common::{
    setup, VaultRobot, VaultZapperDependencies, VaultZapperRobot, DENOM_CREATION_FEE,
    DEPENDENCY_ARTIFACTS_DIR, UNOPTIMIZED_PATH,
};
use cosmwasm_std::{coin, Addr, Coin, Decimal, Timestamp, Uint128};
use cw_dex::pool::Pool;
use cw_dex::traits::Pool as PoolTrait;
use cw_it::helpers::Unwrap;
use cw_it::robot::TestRobot;
use cw_it::test_tube::Account;
use cw_it::traits::CwItRunner;
use cw_it::OwnedTestRunner;
use cw_vault_standard::extensions::lockup::UnlockingPosition;
use cw_vault_standard_test_helpers::traits::CwVaultStandardRobot;
use liquidity_helper::LiquidityHelperUnchecked;
use locked_astroport_vault::msg::InstantiateMsg as AstroportVaultInstantiateMsg;
use locked_astroport_vault_test_helpers::robot::LockedAstroportVaultRobot;
use locked_astroport_vault_test_helpers::router::CwDexRouterRobot;
use vault_zapper::msg::ReceiveChoice;

pub mod common;

#[test]
fn query_depositable_assets() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, _admin) = setup(&runner, 0);

    let pool_assets = robot.deps.pool_assets.clone();

    let mut depositable_assets = robot.zapper_query_depositable_assets();

    let mut expected = [
        &[
            robot.deps.vault_pool.lp_token(),
            AssetInfo::native("uastro"), /* There is a swap path from uastro to each of the pool
                                          * assets */
        ],
        pool_assets.as_slice(),
    ]
    .concat();

    // Sort both so we can compare them
    depositable_assets.sort_by_key(|a| a.to_string());
    expected.sort_by_key(|a| a.to_string());

    assert_eq!(depositable_assets, expected);
}

#[test]
fn query_receive_choices() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, _admin) = setup(&runner, 0);

    let pool_assets = robot.deps.pool_assets.clone();

    let expected = vec![
        ReceiveChoice::SwapTo(AssetInfo::native("uastro")),
        ReceiveChoice::SwapTo(pool_assets[0].clone()),
        ReceiveChoice::SwapTo(pool_assets[1].clone()),
        ReceiveChoice::BaseToken,
        ReceiveChoice::Underlying,
    ];

    let receive_choices = robot.zapper_query_receive_choices();

    assert_eq!(receive_choices, expected);
}

#[test]
fn query_unlocking_positions_for_one_vault() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 300);

    // Deposit the LP token of the vault
    let deposit_amount = Uint128::new(1000000);
    let deposit_asset = Asset::new(robot.deps.vault_pool.lp_token(), deposit_amount);

    // Deposit and unlock half of the deposited amount
    let vault_token_balance = robot
        .zapper_deposit(
            vec![deposit_asset].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .query_vault_token_balance(admin.address());
    robot.zapper_unlock(vault_token_balance.u128() / 2, &admin);

    // Query the unlocking position and unlock second half
    let current_time = runner.query_block_time_nanos();
    let unlocking_position_0 = UnlockingPosition {
        id: 0,
        base_token_amount: deposit_amount / Uint128::new(2),
        owner: Addr::unchecked(robot.vault_zapper_addr.clone()),
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            current_time + 300_000_000_000,
        )),
    };
    robot
        .assert_zapper_has_unlocking_positions(&admin.address(), &[unlocking_position_0.clone()])
        .zapper_unlock(vault_token_balance.u128() / 2, &admin);

    // Query the unlocking positions
    let current_time = runner.query_block_time_nanos();
    let unlocking_position_1 = UnlockingPosition {
        id: 1,
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            current_time + 300_000_000_000,
        )),
        ..unlocking_position_0.clone()
    };
    robot.assert_zapper_has_unlocking_positions(
        &admin.address(),
        &[unlocking_position_0.clone(), unlocking_position_1.clone()],
    );

    // Query with start_after and limit parameters
    let res =
        robot.zapper_query_user_unlocking_positions_for_vault(&admin.address(), None, Some(1));
    assert_eq!(res.len(), 1);
    assert_eq!(res[0], unlocking_position_0);
    let res =
        robot.zapper_query_user_unlocking_positions_for_vault(&admin.address(), Some(0), None);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0], unlocking_position_1);
    let res =
        robot.zapper_query_user_unlocking_positions_for_vault(&admin.address(), Some(0), Some(1));
    assert_eq!(res.len(), 1);
    assert_eq!(res[0], unlocking_position_1);
    let res =
        robot.zapper_query_user_unlocking_positions_for_vault(&admin.address(), Some(1), Some(1));
    assert_eq!(res.len(), 0);
    let res =
        robot.zapper_query_user_unlocking_positions_for_vault(&admin.address(), Some(0), Some(0));
    assert_eq!(res.len(), 0);

    // Increase time, withdraw and assert that the unlocking positions are removed
    robot
        .increase_time(300)
        .zapper_withdraw_unlocked(
            0,
            None,
            ReceiveChoice::BaseToken,
            vec![],
            Unwrap::Ok,
            &admin,
        )
        .assert_zapper_has_unlocking_positions(&admin.address(), &[unlocking_position_1])
        .zapper_withdraw_unlocked(
            1,
            None,
            ReceiveChoice::BaseToken,
            vec![],
            Unwrap::Ok,
            &admin,
        )
        .assert_zapper_has_unlocking_positions(&admin.address(), &[]);
}

#[test]
fn query_unlocking_positions_for_two_vaults() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let admin = VaultZapperRobot::default_account(&runner);
    let vault_lock_duration = 300;

    let vault_dependencies =
        LockedAstroportVaultRobot::instantiate_deps(&runner, &admin, DEPENDENCY_ARTIFACTS_DIR);
    let vault_treasury_addr = runner.init_account(&[]).unwrap().address();

    // Instantiate first vault
    let (axl_ntrn_vault, axl_ntrn_pool, astro_ntrn_pool) =
        LockedAstroportVaultRobot::new_axlr_ntrn_vault(
            &runner,
            LockedAstroportVaultRobot::contract(&runner, DEPENDENCY_ARTIFACTS_DIR),
            Coin::from_str(DENOM_CREATION_FEE).unwrap(),
            vault_treasury_addr.clone(),
            Decimal::percent(5),
            vault_lock_duration,
            &vault_dependencies,
            &admin,
        );
    let vault_1_addr = Addr::unchecked(axl_ntrn_vault.vault_addr());
    // Instantiate second vault
    let init_msg = AstroportVaultInstantiateMsg {
        owner: admin.address(),
        vault_token_subdenom: "testVaultToken".to_string(),
        lock_duration: vault_lock_duration,
        reward_tokens: vec![AssetInfo::native("uastro").into()],
        deposits_enabled: true,
        treasury: vault_treasury_addr.clone(),
        performance_fee: Decimal::percent(5),
        router: vault_dependencies
            .cw_dex_router_robot
            .cw_dex_router
            .clone()
            .into(),
        reward_liquidation_target: AssetInfo::native("uastro").into(),
        pool_addr: astro_ntrn_pool.pair_addr.to_string(),
        astro_token: apollo_cw_asset::AssetInfoUnchecked::native("uastro"),
        astroport_generator: vault_dependencies
            .astroport_contracts
            .generator
            .address
            .clone(),
        liquidity_helper: LiquidityHelperUnchecked::new(
            vault_dependencies.liquidity_helper_addr.clone(),
        ),
    };
    let astro_ntrn_vault_robot = LockedAstroportVaultRobot::new_with_instantiate_msg(
        &runner,
        LockedAstroportVaultRobot::contract(&runner, DEPENDENCY_ARTIFACTS_DIR),
        Coin::from_str(DENOM_CREATION_FEE).unwrap(),
        &init_msg,
        &vault_dependencies,
        &admin,
    );
    let vault_2_addr = Addr::unchecked(&astro_ntrn_vault_robot.vault_addr);

    // Instantiate the zapper and create test robot
    let deps = VaultZapperDependencies {
        astroport_contracts: vault_dependencies.astroport_contracts.clone(),
        cw_dex_router_robot: CwDexRouterRobot {
            runner: &runner,
            cw_dex_router: vault_dependencies.cw_dex_router_robot.cw_dex_router.clone(),
        },
        liquidity_helper_addr: vault_dependencies.liquidity_helper_addr.clone(),
        vault_robot: VaultRobot::Astroport(axl_ntrn_vault),
        pool_assets: axl_ntrn_pool.pool_assets.clone(),
        vault_pool: Pool::Astroport(axl_ntrn_pool.clone()),
    };
    let robot = VaultZapperRobot::instantiate(&runner, deps, UNOPTIMIZED_PATH, &admin);

    // Deposit the LP token of the vault
    let deposit_amount = Uint128::new(1000000);
    let vault_1_deposit_asset = Asset::new(axl_ntrn_pool.lp_token(), deposit_amount);
    let vault_2_deopsit_asset = Asset::new(
        astro_ntrn_pool.lp_token(),
        Uint128::new(1000000) * Uint128::new(1000000),
    );

    // Deposit and unlock half of the deposited amount
    let vault1_vault_token_balance = robot
        .zapper_deposit(
            vec![vault_1_deposit_asset].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .query_vault_token_balance(admin.address());
    robot.zapper_unlock(vault1_vault_token_balance.u128() / 2, &admin);

    let current_time = runner.query_block_time_nanos();
    let vault_1_unlocking_pos_0 = UnlockingPosition {
        id: 0,
        owner: Addr::unchecked(robot.vault_zapper_addr.clone()),
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            current_time + vault_lock_duration * 1_000_000_000,
        )),
        base_token_amount: deposit_amount / Uint128::new(2),
    };

    let vault2_vault_token_balance = robot
        .zapper_deposit_to_vault(
            vec![vault_2_deopsit_asset].into(),
            None,
            &astro_ntrn_vault_robot.vault_addr,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .query_native_token_balance(admin.address(), astro_ntrn_vault_robot.vault_token());
    robot.zapper_unlock_from_vault(
        &astro_ntrn_vault_robot.vault_addr,
        &[coin(
            vault2_vault_token_balance.u128() / 2,
            astro_ntrn_vault_robot.vault_token(),
        )],
        &admin,
    );

    let current_time = runner.query_block_time_nanos();
    let vault_2_unlocking_pos_0 = UnlockingPosition {
        id: 0,
        owner: Addr::unchecked(robot.vault_zapper_addr.clone()),
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            current_time + vault_lock_duration * 1_000_000_000,
        )),
        base_token_amount: Uint128::new(1000000) * Uint128::new(1000000) / Uint128::new(2),
    };

    // Query the unlocking positions
    let res = robot.zapper_query_user_unlocking_positions(&admin.address(), None, None, None);
    assert_eq!(res.len(), 2);
    let positions_for_vault_one = res.get(&vault_1_addr).unwrap();
    let positions_for_vault_two = res.get(&vault_2_addr).unwrap();
    assert_eq!(positions_for_vault_one.len(), 1);
    assert_eq!(positions_for_vault_two.len(), 1);
    assert_eq!(
        positions_for_vault_one,
        &vec![vault_1_unlocking_pos_0.clone()]
    );
    assert_eq!(
        positions_for_vault_two,
        &vec![vault_2_unlocking_pos_0.clone()]
    );

    // Unlock second half for each vault
    robot.zapper_unlock(vault1_vault_token_balance.u128() / 2, &admin);

    let vault_1_unlocking_pos_1 = UnlockingPosition {
        id: 1,
        owner: Addr::unchecked(robot.vault_zapper_addr.clone()),
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            runner.query_block_time_nanos() + vault_lock_duration * 1_000_000_000,
        )),
        base_token_amount: deposit_amount / Uint128::new(2),
    };

    robot.zapper_unlock_from_vault(
        &astro_ntrn_vault_robot.vault_addr,
        &[coin(
            vault2_vault_token_balance.u128() / 2,
            astro_ntrn_vault_robot.vault_token(),
        )],
        &admin,
    );

    let vault_2_unlocking_pos_1 = UnlockingPosition {
        id: 1,
        owner: Addr::unchecked(robot.vault_zapper_addr.clone()),
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            runner.query_block_time_nanos() + vault_lock_duration * 1_000_000_000,
        )),
        base_token_amount: Uint128::new(1000000) * Uint128::new(1000000) / Uint128::new(2),
    };

    // Query the unlocking positions
    let res = robot.zapper_query_user_unlocking_positions(&admin.address(), None, None, None);
    assert_eq!(res.len(), 2);
    let positions_for_vault_one = res.get(&vault_1_addr).unwrap();
    let positions_for_vault_two = res.get(&vault_2_addr).unwrap();
    assert_eq!(positions_for_vault_one.len(), 2);
    assert_eq!(positions_for_vault_two.len(), 2);
    assert_eq!(
        positions_for_vault_one,
        &vec![
            vault_1_unlocking_pos_0.clone(),
            vault_1_unlocking_pos_1.clone()
        ]
    );
    assert_eq!(
        positions_for_vault_two,
        &vec![
            vault_2_unlocking_pos_0.clone(),
            vault_2_unlocking_pos_1.clone()
        ]
    );

    // Query with start_after_vault_addr
    let res = robot.zapper_query_user_unlocking_positions(
        &admin.address(),
        Some(vault_1_addr.to_string()),
        None,
        None,
    );
    assert_eq!(res.len(), 1);
    let positions_for_vault_two = res.get(&vault_2_addr).unwrap();
    assert_eq!(positions_for_vault_two.len(), 2);
    assert_eq!(
        positions_for_vault_two,
        &vec![
            vault_2_unlocking_pos_0.clone(),
            vault_2_unlocking_pos_1.clone()
        ]
    );

    // Query with start_after_vault_addr and start_after_id
    let res = robot.zapper_query_user_unlocking_positions(
        &admin.address(),
        Some(vault_1_addr.to_string()),
        Some(0),
        None,
    );
    assert_eq!(res.len(), 2);
    let positions_for_vault_one = res.get(&vault_1_addr).unwrap();
    let positions_for_vault_two = res.get(&vault_2_addr).unwrap();
    assert_eq!(positions_for_vault_one.len(), 1);
    assert_eq!(positions_for_vault_two.len(), 2);
    assert_eq!(
        positions_for_vault_one,
        &vec![vault_1_unlocking_pos_1.clone()]
    );
    assert_eq!(
        positions_for_vault_two,
        &vec![
            vault_2_unlocking_pos_0.clone(),
            vault_2_unlocking_pos_1.clone()
        ]
    );

    // Query with start_after_vault_addr and start_after_id and limit
    let res = robot.zapper_query_user_unlocking_positions(
        &admin.address(),
        Some(vault_1_addr.to_string()),
        Some(1),
        Some(1),
    );
    assert_eq!(res.len(), 1);
    let positions_for_vault_two = res.get(&vault_2_addr).unwrap();
    assert_eq!(positions_for_vault_two.len(), 1);
    assert_eq!(
        positions_for_vault_two,
        &vec![vault_2_unlocking_pos_0.clone()]
    );
}
