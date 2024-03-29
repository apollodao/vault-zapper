pub mod robot;
use cw_it::cw_multi_test::{StargateKeeper, StargateMessageHandler};
use cw_it::multi_test::modules::TokenFactory;
use cw_it::multi_test::MultiTestRunner;
use cw_it::test_tube::SigningAccount;
use cw_it::{OwnedTestRunner, TestRunner};
pub use robot::*;

#[cfg(feature = "osmosis-test-tube")]
use cw_it::osmosis_test_tube::OsmosisTestApp;

pub const UNOPTIMIZED_PATH: &str = "target/wasm32-unknown-unknown/release";
pub const DEPENDENCY_ARTIFACTS_DIR: &str = "tests/test_artifacts";

/// The fee you need to pay to create a new denom with Token Factory.
pub const DENOM_CREATION_FEE: &str = "10000000uosmo";

pub const TOKEN_FACTORY: &TokenFactory =
    &TokenFactory::new("factory", 32, 16, 59 + 16, DENOM_CREATION_FEE);

pub fn get_test_runner<'a>() -> OwnedTestRunner<'a> {
    match option_env!("TEST_RUNNER").unwrap_or("multi-test") {
        "multi-test" => {
            let mut stargate_keeper = StargateKeeper::new();
            TOKEN_FACTORY.register_msgs(&mut stargate_keeper);

            OwnedTestRunner::MultiTest(MultiTestRunner::new_with_stargate("osmo", stargate_keeper))
        }
        #[cfg(feature = "osmosis-test-tube")]
        "osmosis-test-app" => OwnedTestRunner::OsmosisTestApp(OsmosisTestApp::new()),
        _ => panic!("Unsupported test runner type"),
    }
}

pub fn setup<'a>(
    runner: &'a TestRunner<'a>,
    vault_lock_duration: u64,
) -> (VaultZapperRobot<'a>, SigningAccount) {
    let admin = VaultZapperRobot::default_account(runner);
    let deps = VaultZapperRobot::instantiate_deps(
        runner,
        DEPENDENCY_ARTIFACTS_DIR,
        vault_lock_duration,
        &admin,
    );
    let robot = VaultZapperRobot::instantiate(runner, deps, UNOPTIMIZED_PATH, &admin);

    (robot, admin)
}
