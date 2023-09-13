use common::setup;
use cw_it::OwnedTestRunner;

pub mod common;

#[test]
fn instantiate_works() {
    let owned_runner: OwnedTestRunner = common::get_test_runner();
    setup(&owned_runner.as_ref());
}
