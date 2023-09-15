use apollo_cw_asset::{Asset, AssetInfo};
use common::setup;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use cw_dex::traits::Pool;
use cw_it::helpers::Unwrap;
use cw_it::test_tube::Account;
use cw_it::traits::CwItRunner;
use cw_it::OwnedTestRunner;
use cw_vault_standard::extensions::lockup::UnlockingPosition;
use cw_vault_standard_test_helpers::traits::CwVaultStandardRobot;
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
fn query_unlocking_positions() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    let runner = owned_runner.as_ref();
    let (robot, admin) = setup(&runner, 300);

    // Deposit the LP token of the vault
    let deposit_amount = Uint128::new(1000000);
    let deposit_asset = Asset::new(robot.deps.vault_pool.lp_token(), deposit_amount);

    let vault_token_balance = robot
        .zapper_deposit(
            vec![deposit_asset].into(),
            None,
            Uint128::one(),
            Unwrap::Ok,
            &admin,
        )
        .query_vault_token_balance(admin.address());

    let current_time = runner.query_block_time_nanos();

    let unlocking_position = UnlockingPosition {
        id: 0,
        base_token_amount: deposit_amount / Uint128::new(2),
        owner: Addr::unchecked(robot.vault_zapper_addr.clone()),
        release_at: cw_utils::Expiration::AtTime(Timestamp::from_nanos(
            current_time + 300_000_000_000,
        )),
    };

    robot
        .zapper_unlock(vault_token_balance.u128() / 2, &admin)
        .assert_zapper_has_unlocking_positions(&admin.address(), &[unlocking_position.clone()])
        .zapper_unlock(vault_token_balance.u128() / 2, &admin)
        .assert_zapper_has_unlocking_positions(
            &admin.address(),
            &[
                unlocking_position.clone(),
                UnlockingPosition {
                    id: 1,
                    ..unlocking_position.clone()
                },
            ],
        )
        .increase_time(300)
        .zapper_withdraw_unlocked(
            0,
            None,
            ReceiveChoice::BaseToken,
            vec![],
            Unwrap::Ok,
            &admin,
        )
        .assert_zapper_has_unlocking_positions(
            &admin.address(),
            &[UnlockingPosition {
                id: 1,
                ..unlocking_position
            }],
        )
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
