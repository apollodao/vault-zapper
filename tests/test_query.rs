use apollo_cw_asset::AssetInfo;
use common::setup;
use cw_dex::traits::Pool;
use cw_it::OwnedTestRunner;
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
