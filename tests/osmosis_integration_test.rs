mod test_helpers;
use std::ops::Range;

use crate::test_helpers::robot::OsmosisVaultZapperRobot;

use apollo_cw_asset::{Asset, AssetInfo, AssetList};
use cosmwasm_std::{Coin, Decimal, Uint128};

use cw_it::osmosis::{pool_with_denom, test_pool, OsmosisPoolType, OsmosisTestPool};
use cw_it::osmosis_test_tube::{Account, OsmosisTestApp, SigningAccount};
use osmosis_vault_test_helpers::robot::OsmosisVaultRobot;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::{option, prop_compose};
use test_case::test_case;
use test_helpers::robot::TestRobot;

pub(crate) const UOSMO: &str = "uosmo";
pub(crate) const UATOM: &str = "uatom";
pub(crate) const UION: &str = "uion";
pub(crate) const STAKE: &str = "stake";
pub(crate) const INITIAL_BALANCE: u128 = u128::MAX;
pub(crate) const TEST_CONFIG_PATH: &str = "tests/configs/osmosis.yaml";
pub(crate) const SIXTY_FOUR_BITS: u128 = 18446744073709551616u128;
pub(crate) const HUNDRED_BITS: u128 = 1267650600228229401496703205376u128;
pub(crate) const CW20_WASM_FILE: &str = "tests/artifacts/cw20_base.wasm";

pub(crate) const LIQUIDITY_RANGE: Range<u128> = u64::MAX as u128..(u128::MAX / 100);

const INITIAL_LP_AMOUNT: u128 = 100_000_000_000_000_000_000;

fn setup_zapper_robot<'a>(
    app: &'a OsmosisTestApp,
    base_pool: OsmosisTestPool,
    reward1_pool: OsmosisTestPool,
    reward2_pool: Option<OsmosisTestPool>,
    performance_fee: Decimal,
) -> (
    OsmosisVaultZapperRobot<'a, OsmosisTestApp>,
    [SigningAccount; 2],
) {
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
    let admin = app.init_account(&coins).unwrap();
    let treasury = app.init_account(&coins).unwrap();

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
        app,
        &admin,
        &admin,
        &treasury,
        base_pool,
        reward1_pool,
        reward2_pool,
        reward_liquidation_target,
        performance_fee,
        TEST_CONFIG_PATH,
    );

    (
        OsmosisVaultZapperRobot::new(&app, &admin, vault_robot),
        [admin, treasury],
    )
}

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
    //         .execute_cosmos_msgs::<MsgInstantiateContractResponse>(&[init_msg],
    // admin)         .unwrap()
    //         .data
    //         .address;
    //     println!("cw20_addr for salt {}: {}", contract_name, cw20_addr);
    // }

    (app, accs)
}

/// Generates a random AssetInfo with native denoms ranging from denom0 to
/// denom7
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

    // #[test]
    // fn proptest_deposit(((base_pool, reward1_pool, reward2_pool),deposit_assets) in test_pools().prop_flat_map(|test_pools| {
    //     let (base_pool, reward1_pool, reward2_pool) = &test_pools;
    //     let mut denoms = HashSet::new();
    //     base_pool.liquidity.iter().for_each(|coin| {denoms.insert(coin.denom.clone());});
    //     reward1_pool.liquidity.iter().for_each(|coin| {denoms.insert(coin.denom.clone());});
    //     if let Some(reward2_pool) = reward2_pool {
    //         reward2_pool.liquidity.iter().for_each(|coin| {denoms.insert(coin.denom.clone());});
    //     }
    //     let denoms = denoms.into_iter().collect::<Vec<_>>();
    //     (Just(test_pools), random_asset_list_from_native_denoms(denoms))
    // }), performance_permille in 0u128..1000u128) {
    //     test_deposit(base_pool, reward1_pool, reward2_pool, performance_permille, deposit_assets);
    // }

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
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    vec![Asset {
        info: AssetInfo::Native(String::from(UION)),
        amount: Uint128::new(10),
    }].into(),
    None ; "single asset zap in"
)]
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
    vec![Asset {
        info: AssetInfo::Native(String::from(UION)),
        amount: Uint128::new(2_000_000), // Yields 2 UATOM on swap. If out amount is zero contract execution crashes
    }].into(),
    None ; "single asset zap in unbalanced pool"
)]
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
    vec![Asset {
        info: AssetInfo::Native(String::from(UION)),
        amount: Uint128::new(10), // Yields 2 UATOM on swap. If out amount is zero contract execution crashes
    }].into(),
    None => panics ; "single asset zap amount too small"
)]
#[test_case(
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UOSMO)],
        pool_type: OsmosisPoolType::Basic,
    },
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    vec![Asset {
        info: AssetInfo::Native(String::from(UION)),
        amount: Uint128::new(10000),
    }].into(),
    Some(vec![Coin::new(1000, UION)]) => panics ; "too little funds sent"
)]
#[test_case(
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UOSMO)],
        pool_type: OsmosisPoolType::Basic,
    },
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    vec![Asset {
        info: AssetInfo::Native(String::from(UATOM)),
        amount: Uint128::new(10),
    }].into(),
    None ; "zap in first asset of base pool"
)]
#[test_case(
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UOSMO)],
        pool_type: OsmosisPoolType::Basic,
    },
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    vec![
        Asset::new(AssetInfo::native(UION), Uint128::new(10)),
        Asset::new(AssetInfo::native(UOSMO), Uint128::new(10))
    ].into(),
    None ; "double asset zap in"
)]
#[test_case(
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UOSMO)],
        pool_type: OsmosisPoolType::Basic,
    },
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    vec![
        Asset::new(AssetInfo::native("gamm/pool/1"), Uint128::new(10_000_000_000_000_000_000)),
    ].into(),
    None ; "zap in lp token"
)]
#[test_case(
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UOSMO)],
        pool_type: OsmosisPoolType::Basic,
    },
    OsmosisTestPool {
        liquidity: vec![Coin::new(100_000_000, UATOM), Coin::new(100_000_000, UION)],
        pool_type: OsmosisPoolType::Basic,
    },
    None,
    0,
    vec![
        Asset::new(AssetInfo::native("gamm/pool/2"), Uint128::new(10_000_000_000_000_000_000)),
    ].into(),
    None => panics ; "zap in lp token from other pool"
)]
pub fn test_zap_in(
    base_pool: OsmosisTestPool,
    reward1_pool: OsmosisTestPool,
    reward2_pool: Option<OsmosisTestPool>,
    performance_permille: u128,
    assets: AssetList,
    funds: Option<Vec<Coin>>,
) {
    let app = OsmosisTestApp::new();

    let (zapper_robot, [admin, _]) = setup_zapper_robot(
        &app,
        base_pool,
        reward1_pool,
        reward2_pool,
        Decimal::from_ratio(performance_permille, 1000u128),
    );
    let zapper_addr = zapper_robot.vault_zapper_addr.clone();

    let funds = funds.unwrap_or_else(|| assets.clone().try_into().unwrap());

    zapper_robot
        .zap_in(&admin, assets, funds)
        .assert_vault_token_balance_gt(admin.address(), Uint128::zero()) // Admin should have vault tokens after zap in
        .assert_vault_token_balance_eq(&zapper_addr, Uint128::zero()) // Zapper should not have any vault tokens after zap in
        .assert_base_token_balance_eq(&zapper_addr, Uint128::zero()); // Zapper should not have any base tokens after zap in
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
    50,
    Coin::new(1_000_000_000_000_000_000, "gamm/pool/1"), // One percent of initial lp amount
    vec![Coin::new(1_000_000_000_000_000_000, "gamm/pool/1")],
    AssetInfo::Native(UATOM.to_string()) ; "zap out 1 percent of lp supply"
)]
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
    50,
    Coin::new(1_000_000_000_000_000_000, "gamm/pool/1"), // One percent of initial lp amount
    vec![Coin::new(10_000_000_000_000_000_000, "gamma/pool/1")],
    AssetInfo::Native(UATOM.to_string()) => panics ; "wrong funds sent to zapper"
)]
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
    50,
    Coin::new(1_000_000_000_000_000_000, "gamm/pool/2"), // One percent of initial lp amount
    vec![Coin::new(1_000_000_000_000_000_000, "gamma/pool/2")],
    AssetInfo::Native(UATOM.to_string()) => panics ; "from token is not vault base token"
)]
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
    50,
    Coin::new(1_000_000_000_000_000_000, UATOM), // One percent of initial lp amount
    vec![Coin::new(1_000_000_000_000_000_000, UATOM)],
    AssetInfo::Native("random".to_string()) => panics ; "no path to liquidation target"
)]
pub fn test_zap_out(
    base_pool: OsmosisTestPool,
    reward1_pool: OsmosisTestPool,
    reward2_pool: Option<OsmosisTestPool>,
    performance_permille: u128,
    from_asset: Coin,
    funds: Vec<Coin>,
    zap_to: AssetInfo,
) {
    let app = OsmosisTestApp::new();

    let (zapper_robot, [admin, _]) = setup_zapper_robot(
        &app,
        base_pool,
        reward1_pool,
        reward2_pool,
        Decimal::from_ratio(performance_permille, 1000u128),
    );
    let zapper_addr = zapper_robot.vault_zapper_addr.clone();

    let base_tokens_after_zap = INITIAL_LP_AMOUNT - from_asset.amount.u128();

    zapper_robot
        .zap_out(&admin, from_asset.clone().into(), funds, zap_to, None)
        .assert_base_token_balance_eq(admin.address(), base_tokens_after_zap) // Assert correct amount of tokens zapped out
        .assert_base_token_balance_eq(&zapper_addr, Uint128::zero()) // Zapper should not have any base tokens after zap out
        .assert_vault_token_balance_eq(&zapper_addr, Uint128::zero()); // Zapper should not have any vault tokens after zap out
}
