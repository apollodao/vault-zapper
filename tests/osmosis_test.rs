use apollo_utils::submessages::parse_attribute_value;
use cosmwasm_std::{testing::MockStorage, Api, BankMsg, BlockInfo, Coin, ContractInfo, CosmosMsg, Decimal, Deps, DepsMut, Empty, Env, Event, QuerierWrapper, StdError, StdResult, Storage, Timestamp, Uint128, wasm_execute};
use cw_asset::{Asset, AssetInfo, AssetInfoUnchecked, AssetList};
use vault_zapper::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use osmosis_vault::msg::{InstantiateMsg as VaultInstantiateMsg};
use apollo_vault::state::ConfigUnchecked;
use cw_dex::osmosis::OsmosisPool;
use cw_dex::traits::Pool;
use cw_dex_router::helpers::CwDexRouterUnchecked;
use cw_it::{
    config::TestConfig,
    helpers::{bank_balance_query, upload_wasm_files},
    mock_api::OsmosisMockApi,
};
use osmosis_liquidity_helper::helpers::{LiquidityHelperBase, LiquidityHelperUnchecked};
use osmosis_std::types::osmosis::gamm::{ v1beta1::PoolParams, poolmodels::balancer::v1beta1::MsgCreateBalancerPool};
use osmosis_testing::{
    cosmrs::{
        proto::cosmwasm::wasm::v1::{MsgExecuteContractResponse, QuerySmartContractStateRequest},
        Any,
    },
    Account, ExecuteResponse, Gamm, Module, OsmosisTestApp, Runner, RunnerResult, SigningAccount,
    Wasm,
};

use test_case::test_case;

const TEST_CONFIG_PATH: &str = "tests/configs/osmosis.yaml";

const UOSMO: &str = "uosmo";
const UATOM: &str = "uatom";
const UION: &str = "uion";
const VAULT_TOKEN: &str = "vt";

const ONE_MILLION: Uint128 = Uint128::new(1_000_000);
const TWO_MILLION: Uint128 = Uint128::new(2_000_000);
// One hundred trillion is the LP token factor on osmosis.
const HUNDRED_TRILLION: Uint128 = Uint128::new(100_000_000_000_000);
const ONE_TRILLION: Uint128 = Uint128::new(1_000_000_000_000);
const INITIAL_LIQUIDITY: Uint128 = ONE_MILLION;

const EVEN_POOL_IDX: usize = 0;
const UNEVEN_POOL_IDX: usize = 1;

pub struct Codes {
    pub router: u64,
    pub zapper: u64,
    pub vault: u64,
    pub liquidity_helper: u64,
}

pub struct Contracts {
    pub router: String,
    pub zapper: String,
    pub vault: String,
    pub liquidity_helper: String,
}

pub fn setup() -> (OsmosisTestApp, Vec<SigningAccount>, Vec<u64>, Codes) {
    let config = TestConfig::from_yaml(TEST_CONFIG_PATH);
    config.build();

    let runner = OsmosisTestApp::new();

    let accs = runner
        .init_accounts(
            &[
                Coin {
                    denom: UOSMO.to_string(),
                    amount: ONE_TRILLION,
                },
                Coin {
                    denom: UATOM.to_string(),
                    amount: ONE_TRILLION,
                },
                Coin {
                    denom: UION.to_string(),
                    amount: ONE_TRILLION,
                },
            ],
            10,
        )
        .unwrap();

    // Upload test contract wasm file
    let zapper_code_id = upload_wasm_files(&runner, &accs[0], config.clone()).unwrap()["vault_zapper_test_contract"];
    let router_code_id = upload_wasm_files(&runner, &accs[0], config.clone()).unwrap()["cw_dex_router_test_contract"];
    let vault_code_id = upload_wasm_files(&runner, &accs[0], config.clone()).unwrap()["osmosis_vault_test_contract"];
    let liquidity_helper_code_id = upload_wasm_files(&runner, &accs[0], config).unwrap()["osmosis_liquidity_helper_test_contract"];

    let mut pools = Vec::new();
    let gamm = Gamm::new(&runner);
    let pool_id = gamm
        .create_basic_pool(
            &[
                Coin {
                    denom: UOSMO.to_string(),
                    amount: INITIAL_LIQUIDITY,
                },
                Coin {
                    denom: UATOM.to_string(),
                    amount: INITIAL_LIQUIDITY,
                },
            ],
            &accs[0],
        )
        .unwrap()
        .data
        .pool_id;
    println!("Pool ID: {}", pool_id);
    pools.push(pool_id);

    // Create a 67/33 balance pool
    // TODO: Does not create a balancer pool but an unbalanced 5050 pool
    let pool_id = gamm
        .create_basic_pool(
            &[
                Coin {
                    denom: UOSMO.to_string(),
                    amount: Uint128::new(67) * INITIAL_LIQUIDITY,
                },
                Coin {
                    denom: UION.to_string(),
                    amount: Uint128::new(33) * INITIAL_LIQUIDITY,
                },
            ],
            &accs[0],
        )
        .unwrap()
        .data
        .pool_id;
    println!("Pool ID: {}", pool_id);
    pools.push(pool_id);

    (runner, accs, pools, Codes{ router: router_code_id, zapper: zapper_code_id, vault: vault_code_id, liquidity_helper: liquidity_helper_code_id })
}

fn instantiate_test_contracts<'a, R: Runner<'a>>(
    runner: &'a R,
    code_ids: &Codes,
    pool_id: u64,
    signer: &SigningAccount,
) -> Contracts {
    let wasm = Wasm::new(runner);
    let router = wasm.instantiate(code_ids.router, &Empty{}, None, None, &[], signer)
        .unwrap()
        .data
        .address;

    let zapper_init_msg = InstantiateMsg {
        router: CwDexRouterUnchecked::new(router.to_string()),
    };

    let zapper = wasm.instantiate(code_ids.zapper, &zapper_init_msg, None, None, &[], signer)
        .unwrap()
        .data
        .address;

    let liquidity_helper = wasm.instantiate(code_ids.liquidity_helper, &Empty {}, None, None, &[], signer)
        .unwrap()
        .data
        .address;

    let vault_init_msg = VaultInstantiateMsg {
        admin: signer.address(),
        pool_id,
        lockup_duration: 0,
        config: ConfigUnchecked {
            performance_fee: Default::default(),
            treasury: signer.address(),
            router: CwDexRouterUnchecked::new(router.to_string()),
            reward_assets: vec![AssetInfoUnchecked::Native(UOSMO.to_string())],
            reward_liquidation_target: AssetInfoUnchecked::Native(UOSMO.to_string()),
            pool_assets: vec![AssetInfoUnchecked::Native(UOSMO.to_string()), AssetInfoUnchecked::Native(UION.to_string())],
            force_withdraw_whitelist: vec![],
            liquidity_helper: LiquidityHelperUnchecked::new(liquidity_helper.to_string()),
        },
        vault_token_subdenom: VAULT_TOKEN.to_string(),
        base_token: AssetInfoUnchecked::Native(format!("gamm/pool/{}", pool_id)),
    };

    let wasm = Wasm::new(runner);
    let vault = wasm.instantiate(code_ids.vault, &vault_init_msg, None, None, &[], signer)
        .unwrap()
        .data
        .address;

    Contracts { router, zapper, vault, liquidity_helper }
}

fn native_assetlist_from_slice(assets: &[(&str, Uint128)]) -> AssetList {
    assets
        .iter()
        .map(|(denom, amount)| Coin {
            denom: denom.to_string(),
            amount: *amount,
        })
        .collect::<Vec<_>>()
        .into()
}

fn send_funds_to_contract<'a, R: Runner<'a>>(
    runner: &'a R,
    contract_addr: &str,
    assets: AssetList,
    signer: &SigningAccount
) {
    // Send funds to contract
    let send_msg: CosmosMsg = BankMsg::Send {
        to_address: contract_addr.to_string(),
        amount: assets
            .into_iter()
            .map(|a| a.try_into())
            .collect::<StdResult<Vec<Coin>>>()
            .unwrap(),
    }
    .into();
    runner
        .execute_cosmos_msgs::<Any>(&[send_msg], signer)
        .unwrap();
}

fn deposit<'a, R: Runner<'a>>(
    runner: &'a R,
    contracts: &Contracts,
    assets: AssetList,
    recipient: Option<String>,
    slippage_tolerance: Option<Decimal>,
    signer: &SigningAccount,
) {
    // Send funds to contract
    send_funds_to_contract(runner, &contracts.zapper, assets.clone(), signer);

    // Provide liquidity
    let deposit_msg = ExecuteMsg::Deposit { assets: assets.into(), vault_address: contracts.vault.to_string(), recipient, slippage_tolerance };
    runner
        .execute_cosmos_msgs::<MsgExecuteContractResponse>(
            &[wasm_execute(contracts.zapper.to_string(), &deposit_msg, vec![]).unwrap().into()],
            signer,
        )
        .unwrap();
}

#[test_case(0, &[(UATOM, INITIAL_LIQUIDITY), (UOSMO, INITIAL_LIQUIDITY)], false, INITIAL_LIQUIDITY * HUNDRED_TRILLION ; "basic pool")]
#[test_case(0, &[(UATOM, Uint128::one()), (UOSMO, Uint128::one())], false, HUNDRED_TRILLION ; "basic pool adding small liquidity")]
#[test_case(0, &[(UATOM, INITIAL_LIQUIDITY), (UOSMO, INITIAL_LIQUIDITY)], true, INITIAL_LIQUIDITY * HUNDRED_TRILLION ; "basic pool simulate min_out")]
#[test_case(0, &[(UATOM, INITIAL_LIQUIDITY), (UOSMO, INITIAL_LIQUIDITY * Decimal::percent(50))], true, 
            INITIAL_LIQUIDITY * Decimal::percent(50) * HUNDRED_TRILLION ; "basic pool uneven assets simulate min_out")]
pub fn test_deposit(
    pool_idx: usize,
    assets: &[(&str, Uint128)],
    min_out: bool,
    expected_lps: Uint128,
) {
    let (runner, accs, pools, code_ids) = setup();
    let admin = &accs[0];
    let pool_id = pools[pool_idx];
    let pool = OsmosisPool::new(pool_id);

    let contract_addrs = instantiate_test_contracts(&runner, &code_ids, 1, admin);

    // Parse assets into AssetList
    let assets = native_assetlist_from_slice(assets);

    // Query admin LP token balance
    let admin_lp_balance =
        bank_balance_query(&runner, admin.address(), pool.lp_token().to_string()).unwrap();
    println!("admin LP token balance: {}", admin_lp_balance);

    //Query depositable assets for vault
    // let wasm = Wasm::new(&runner);
    // let despositable_assets = wasm.query::<_, Vec<String>>(
    //     &contract_addr,
    //     &QueryMsg::DepositableAssets { vault_address },
    // )
    // .unwrap();

    // Provide liquidity
    deposit(
        &runner,
        &contract_addrs,
        assets.clone(),
        None,
        None,
        admin,
    );

    // // Query LP token balance after
    // let lp_token_after =
    //     bank_balance_query(&runner, admin.address(), pool.lp_token().to_string()).unwrap();
    //
    // // Assert that LP token balance doubled
    // assert_eq!(lp_token_after, expected_lps);
}

/// Find event from Vec<Event> response
///
/// Returns a [`StdResult`] containing reference to the event if found otherwise [`StdError`]
fn find_event<'a>(events: &'a Vec<Event>, event_type: &str) -> StdResult<&'a Event> {
    events
        .iter()
        .find(|event| event.ty == event_type)
        .ok_or(StdError::generic_err(format!(
            "No `{}` event found",
            event_type
        )))
}

/// Parse coins from string
fn coin_from_str(s: &str) -> Coin {
    // Find index of first non-digit character
    let idx = s
        .char_indices()
        .find(|(_, c)| !c.is_digit(10))
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());

    // Parse amount and denom from string
    let amount: Uint128 = s[..idx].parse::<u128>().unwrap().into();
    let denom = s[idx..].to_string();

    Coin { denom, amount }
}