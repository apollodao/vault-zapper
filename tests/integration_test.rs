mod test_helpers;
use std::{collections::HashSet, ops::Range};

use crate::test_helpers::robot::VaultZapperRobot;

use apollo_cw_asset::{Asset, AssetInfo, AssetInfoUnchecked, AssetList};
use cosmwasm_std::{
    instantiate2_address, testing::MockApi, to_binary, Coin, CosmosMsg, Decimal, Empty, Querier,
    StdError, Uint128, WasmMsg,
};
use cw20::Cw20Coin;
use cw20_base::msg::InstantiateMsg as Cw20InstantiateMsg;
use cw_dex::{osmosis::OsmosisPool, Pool};
use cw_dex_router::{
    helpers::CwDexRouterUnchecked,
    operations::{SwapOperation, SwapOperationsList},
};
use cw_it::osmosis::{
    pool_with_denom, reward_pool, test_pool, OsmosisPoolType, OsmosisTestPool, OsmosisVaultRobot,
};
use cw_it::{
    config::TestConfig,
    helpers::{instantiate_contract, instantiate_contract_with_funds, upload_wasm_files},
};
use liquidity_helper::LiquidityHelperUnchecked;
use osmosis_test_tube::{
    cosmrs::proto::cosmwasm::wasm::v1::{
        MsgExecuteContractResponse, MsgInstantiateContractResponse,
    },
    Account, Gamm, Module, OsmosisTestApp, Runner, SigningAccount, Wasm,
};
use proptest::prop_compose;
use proptest::{collection::vec, option, prelude::*};
use test_case::test_case;

pub(crate) const UOSMO: &str = "uosmo";
pub(crate) const UATOM: &str = "uatom";
pub(crate) const UION: &str = "uion";
pub(crate) const STAKE: &str = "stake";
pub(crate) const INITIAL_BALANCE: u128 = u128::MAX;
pub(crate) const TEST_CONFIG_PATH: &str = "tests/configs/osmosis.yaml";
pub(crate) const TWO_WEEKS_IN_SECONDS: u64 = 60 * 60 * 24 * 14;
pub(crate) const SIXTY_FOUR_BITS: u128 = 18446744073709551616u128;
pub(crate) const HUNDRED_BITS: u128 = 1267650600228229401496703205376u128;
pub(crate) const CW20_WASM_FILE: &str = "tests/artifacts/cw20_base.wasm";

pub(crate) const LIQUIDITY_RANGE: Range<u128> = u64::MAX as u128..(u128::MAX / 100);

pub fn setup_test_runner() -> (OsmosisTestApp, Vec<SigningAccount>) {
    let app = OsmosisTestApp::new();

    // Setup accounts with initial balances
    let mut coins = vec![
        Coin::new(INITIAL_BALANCE, UATOM),
        Coin::new(INITIAL_BALANCE, UOSMO),
        Coin::new(INITIAL_BALANCE, UION),
        Coin::new(INITIAL_BALANCE, STAKE),
    ];
    for i in 0..8 {
        coins.push(Coin::new(INITIAL_BALANCE, format!("denom{}", i)));
    }
    let accs = app.init_accounts(&coins, 4).unwrap();
    // let admin = &accs[0];

    // Upload CW20 contract
    // let wasm = Wasm::new(&app);
    // let wasm_byte_code = std::fs::read(CW20_WASM_FILE).unwrap();
    // let code_id = wasm
    //     .store_code(&wasm_byte_code, None, admin)
    //     .map_err(|e| StdError::generic_err(format!("{:?}", e)))
    //     .unwrap()
    //     .data
    //     .code_id;

    // Instantiate some CW20's for testing
    // instantiate2_address()
    // for i in 0..2 {
    //     let contract_name = format!("cw20{}", i);
    //     let init_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Instantiate2 {
    //         admin: Some(admin.address()),
    //         code_id,
    //         funds: vec![],
    //         label: contract_name.clone(),
    //         salt: to_binary(&contract_name).unwrap(),
    //         msg: to_binary(&Cw20InstantiateMsg {
    //             name: contract_name.clone(),
    //             symbol: contract_name.clone(),
    //             decimals: 6,
    //             initial_balances: accs
    //                 .iter()
    //                 .map(|acc| Cw20Coin {
    //                     address: acc.address().to_string(),
    //                     amount: Uint128::new(u128::MAX),
    //                 })
    //                 .collect(),
    //             mint: None,
    //             marketing: None,
    //         })
    //         .unwrap(),
    //     });
    //     let cw20_addr = app
    //         .execute_cosmos_msgs::<MsgInstantiateContractResponse>(&[init_msg], admin)
    //         .unwrap()
    //         .data
    //         .address;
    //     println!("cw20_addr for salt {}: {}", contract_name, cw20_addr);
    // }

    (app, accs)
}

/// Generates a random AssetInfo with native denoms ranging from denom0 to denom7
pub fn random_asset_info() -> impl Strategy<Value = AssetInfo> {
    prop_oneof![
        "denom[0-7]".prop_map(|denom| AssetInfo::Native(denom.to_string())),
        // TODO: Support CW20 via MsgInstantiateContract2
    ]
}

prop_compose! {
    // Generates a random asset with a random amount in the given range, or a
    // random amount in the range 1..u128::MAX if no range is given
    pub fn random_asset(amount_range: Option<Range<u128>>)
        (
            amount_range in Just(amount_range.unwrap_or(1u128..u64::MAX as u128)),
            asset_info in random_asset_info()
        )(
            asset in amount_range.prop_map(move |amount|
                (Asset {info: asset_info.clone(),amount: Uint128::new(amount)})
        )) -> Asset {
        asset
    }
}

prop_compose! {
    /// Generates a random AssetList with a random number of assets in the
    /// given range, or a random number of assets in the range 1..10 if no
    /// range is given. Each asset has a random amount in the given range, or
    /// a random amount in the range 1..u128::MAX if no range is given.
    pub fn random_asset_list(count_range: Option<Range<usize>>, amount_range: Option<Range<u128>>)
        (assets in vec(random_asset(amount_range), count_range.unwrap_or(1..10))) -> AssetList {
        AssetList::from(assets)
    }
}

prop_compose! {
    /// Generates a random AssetList with native tokens from the given denoms
    pub fn random_asset_list_from_native_denoms(denoms: Vec<String>)
    (assets in vec(1..u64::MAX,1..denoms.len()).prop_flat_map(move |x|
        Just(denoms.iter().take(x.len()).zip(x).map(|(denom,amount)|
            Asset::native(denom, amount)).collect::<Vec<_>>()
        )
    )) -> AssetList {
        AssetList::from(assets)
    }
}

prop_compose! {
    /// Generates a random AssetList with native tokens from the denoms in the
    /// given pool's liquidity with amounts no more than the pool's liquidity
    /// (Limitation in Osmosis: cannot provide more than the pool's liquidity)
    pub fn random_asset_list_from_pool_liquidity(liquidity: Vec<Coin>)
    (liquidity in Just(liquidity.clone()), something in (1..liquidity.len()).prop_flat_map(move |count| {
        Just(liquidity.iter().take(count).map(move |coin|
            (1..coin.amount.u128())).collect::<Vec<_>>())
        }))(assets in something.prop_flat_map(move |amount| {
            Just(amount.iter().zip(liquidity.clone()).map(|(amount,coin)|
                Asset::native(coin.denom.as_str(), *amount)).collect::<Vec<_>>())
    })) -> AssetList {
        AssetList::from(assets)
    }
}

prop_compose! {
    fn test_pools()
    ((liquidation_target, base_pool) in test_pool(Some(LIQUIDITY_RANGE)).prop_flat_map(|pool| {
        (Just(pool.liquidity[0].denom.clone()),Just(pool))
    }))
    (
        reward1_pool in pool_with_denom(liquidation_target.clone(), Some(LIQUIDITY_RANGE)),
        reward2_pool in option::of(pool_with_denom(liquidation_target, Some(LIQUIDITY_RANGE))),
        base_pool in Just(base_pool)
    ) -> (OsmosisTestPool, OsmosisTestPool, Option<OsmosisTestPool>) {
        (base_pool, reward1_pool, reward2_pool)
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1,
        max_shrink_iters: 32,
        .. ProptestConfig::default()
    })]

    #[test]
    fn proptest_deposit(((base_pool, reward1_pool, reward2_pool),deposit_assets) in test_pools().prop_flat_map(|test_pools| {
        let (base_pool, reward1_pool, reward2_pool) = &test_pools;
        let mut denoms = HashSet::new();
        base_pool.liquidity.iter().for_each(|coin| {denoms.insert(coin.denom.clone());});
        reward1_pool.liquidity.iter().for_each(|coin| {denoms.insert(coin.denom.clone());});
        if let Some(reward2_pool) = reward2_pool {
            reward2_pool.liquidity.iter().for_each(|coin| {denoms.insert(coin.denom.clone());});
        }
        let denoms = denoms.into_iter().collect::<Vec<_>>();
        (Just(test_pools), random_asset_list_from_native_denoms(denoms))
    }), performance_permille in 0u128..1000u128) {
        test_deposit(base_pool, reward1_pool, reward2_pool, performance_permille, deposit_assets);
    }

    // #[test]
    // fn proptest_deposit(((base_pool, reward1_pool, reward2_pool),deposit_assets) in test_pools().prop_flat_map(|test_pools| {
    //     let (base_pool, reward1_pool, reward2_pool) = &test_pools;
    //     let mut liquidity = base_pool.liquidity.clone();
    //     // Loop over reward1_pool liquidity and add to liquidity if not already in liquidity
    //     // If already in liquidity, replace it if it is smaller.
    //     for coin in reward1_pool.liquidity.iter() {
    //         let mut found = false;
    //         for i in 0..liquidity.len() {
    //             if liquidity[i].denom == coin.denom {
    //                 found = true;
    //                 if liquidity[i].amount > coin.amount {
    //                     liquidity[i] = coin.clone();
    //                 }
    //                 break;
    //             }
    //         }
    //         if !found {
    //             liquidity.push(coin.clone());
    //         }
    //     }
    //     if let Some(reward2_pool) = reward2_pool {
    //         // Loop over reward2_pool liquidity and add to liquidity if not already in liquidity
    //         // If already in liquidity, replace it if it is smaller.
    //         for coin in reward2_pool.liquidity.iter() {
    //             let mut found = false;
    //             for i in 0..liquidity.len() {
    //                 if liquidity[i].denom == coin.denom {
    //                     found = true;
    //                     if liquidity[i].amount > coin.amount {
    //                         liquidity[i] = coin.clone();
    //                     }
    //                     break;
    //                 }
    //             }
    //             if !found {
    //                 liquidity.push(coin.clone());
    //             }
    //         }
    //     }
    //     (Just(test_pools), random_asset_list_from_pool_liquidity(liquidity))
    // }), performance_permille in 0u128..1000u128) {
    //     test_deposit(base_pool, reward1_pool, reward2_pool, performance_permille, deposit_assets);
    // }
}

#[test_case(
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UOSMO)],
        pool_type: OsmosisPoolType::Basic,
    },
    OsmosisTestPool {
        liquidity: vec![Coin::new(100, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    AssetList::from(vec![Asset {
        info: AssetInfo::Native(String::from(UION)),
        amount: Uint128::new(10),
    }]); "test_deposit"
)]
pub fn test_deposit(
    base_pool: OsmosisTestPool,
    reward1_pool: OsmosisTestPool,
    reward2_pool: Option<OsmosisTestPool>,
    performance_permille: u128,
    assets: AssetList,
) {
    println!("Running test_deposit");
    println!("base_pool: {:?}", base_pool);
    println!("reward1_pool: {:?}", reward1_pool);
    println!("reward2_pool: {:?}", reward2_pool);
    println!("performance_permille: {}", performance_permille);
    println!("assets: {:?}", assets);
    let (app, accs) = setup_test_runner();

    let admin = &accs[0];
    let user1 = &accs[1];
    let force_withdraw_admin = &accs[2];
    let treasury = &accs[3];

    let performance_fee = Decimal::from_ratio(performance_permille, 1000u128);

    // Set reward liquidation target to the asset which is in common between the
    // base pool and the reward pool.
    let reward_liquidation_target = base_pool
        .liquidity
        .iter()
        .find(|x| reward1_pool.liquidity.iter().any(|y| x.denom == y.denom))
        .unwrap()
        .denom
        .clone();

    let vault_robot = OsmosisVaultRobot::new(
        &app,
        admin,
        force_withdraw_admin,
        treasury,
        base_pool,
        reward1_pool,
        reward2_pool,
        reward_liquidation_target,
        performance_fee,
        TEST_CONFIG_PATH,
    );

    let vault_zapper_robot = VaultZapperRobot::new(&app, admin, vault_robot);
    println!("---------------------------");

    vault_zapper_robot.deposit(user1, assets);
}
