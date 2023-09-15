use apollo_cw_asset::{Asset, AssetInfo};
use common::setup;
use cosmwasm_std::Uint128;
use cw_dex::traits::Pool;
use cw_it::{
    astroport::robot::AstroportTestRobot, helpers::Unwrap, test_tube::Account, OwnedTestRunner,
};
use cw_vault_standard_test_helpers::traits::CwVaultStandardRobot;
use vault_zapper::msg::ZapTo;

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
            AssetInfo::native("uastro"), // There is a swap path from uastro to each of the pool assets
        ],
        pool_assets.as_slice(),
    ]
    .concat();

    // Sort both so we can compare them
    depositable_assets.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    expected.sort_by(|a, b| a.to_string().cmp(&b.to_string()));

    assert_eq!(depositable_assets, expected);
}

