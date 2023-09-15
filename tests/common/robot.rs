use std::str::FromStr;

use apollo_cw_asset::{AssetInfo, AssetList, AssetListUnchecked};
use apollo_utils::assets::separate_natives_and_cw20s;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{assert_approx_eq, coin, Coin, Coins, Decimal, Uint128};
use cw_dex::Pool;
use cw_dex_router::helpers::CwDexRouterUnchecked;
use cw_it::astroport::robot::AstroportTestRobot;
use cw_it::astroport::utils::AstroportContracts;
use cw_it::cw_multi_test::ContractWrapper;
use cw_it::helpers::Unwrap;
use cw_it::robot::TestRobot;
use cw_it::test_tube::{Account, Module, SigningAccount, Wasm};
use cw_it::traits::CwItRunner;
use cw_it::{ContractType, TestRunner};
use cw_vault_standard::extensions::lockup::UnlockingPosition;
use cw_vault_standard_test_helpers::traits::CwVaultStandardRobot;
use liquidity_helper::LiquidityHelperUnchecked;
use locked_astroport_vault_test_helpers::robot::LockedAstroportVaultRobot;
use locked_astroport_vault_test_helpers::router::CwDexRouterRobot;
use vault_zapper::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, ZapTo};

pub const VAULT_ZAPPER_WASM_NAME: &str = "vault_zapper.wasm";
pub const ASTROPORT_ARTIFACTS_DIR: &str = "astroport-artifacts";
pub const ASTROPORT_LIQUIDITY_HELPER_WASM_NAME: &str = "astroport_liquidity_helper.wasm";
/// The fee you need to pay to create a new denom with Token Factory.
pub const DENOM_CREATION_FEE: &str = "10000000uosmo";

#[cw_serde]
struct AstroportLiquidityHelperInstantiateMsg {
    astroport_factory: String,
}

/// The default coins to fund new accounts with
pub const DEFAULT_COINS: &str =
    "1000000000000000000uosmo,1000000000000000000untrn,1000000000000000000uaxl,1000000000000000000uastro";
pub enum VaultRobot<'a> {
    // Osmosis(OsmosisVaultRobot), //TODO: add osmosis vault robot
    Astroport(LockedAstroportVaultRobot<'a>),
}

impl<'a> TestRobot<'a, TestRunner<'a>> for VaultRobot<'a> {
    fn runner(&self) -> &'a TestRunner<'a> {
        match self {
            VaultRobot::Astroport(robot) => robot.runner(),
        }
    }
}

impl<'a> CwVaultStandardRobot<'a, TestRunner<'a>> for VaultRobot<'a> {
    fn vault_addr(&self) -> String {
        match self {
            VaultRobot::Astroport(robot) => robot.vault_addr(),
        }
    }

    fn query_base_token_balance(&self, address: impl Into<String>) -> Uint128 {
        match self {
            VaultRobot::Astroport(robot) => robot.query_base_token_balance(address),
        }
    }
}

pub struct VaultZapperDependencies<'a> {
    pub astroport_contracts: AstroportContracts,
    pub cw_dex_router_robot: CwDexRouterRobot<'a>,
    pub liquidity_helper_addr: String,
    pub vault_robot: VaultRobot<'a>,
    pub vault_pool: Pool,
    pub pool_assets: Vec<AssetInfo>,
}

pub struct VaultZapperRobot<'a> {
    pub runner: &'a TestRunner<'a>,
    pub vault_zapper_addr: String,
    pub deps: VaultZapperDependencies<'a>,
}

impl<'a> TestRobot<'a, TestRunner<'a>> for VaultZapperRobot<'a> {
    fn runner(&self) -> &'a TestRunner<'a> {
        self.runner
    }
}

impl<'a> AstroportTestRobot<'a, TestRunner<'a>> for VaultZapperRobot<'a> {
    fn astroport_contracts(&self) -> &AstroportContracts {
        &self.deps.astroport_contracts
    }
}

impl<'a> CwVaultStandardRobot<'a, TestRunner<'a>> for VaultZapperRobot<'a> {
    fn vault_addr(&self) -> String {
        self.deps.vault_robot.vault_addr()
    }

    fn query_base_token_balance(&self, address: impl Into<String>) -> Uint128 {
        self.deps.vault_robot.query_base_token_balance(address)
    }
}

impl<'a> VaultZapperRobot<'a> {
    /// Returns the contract code to be able to upload the contract
    pub fn contract(runner: &TestRunner, _artifacts_dir: &str) -> ContractType {
        match runner {
            TestRunner::MultiTest(_) => ContractType::MultiTestContract(Box::new(
                ContractWrapper::new_with_empty(
                    vault_zapper::contract::execute,
                    vault_zapper::contract::instantiate,
                    vault_zapper::contract::query,
                )
                .with_reply(vault_zapper::contract::reply),
            )),
            #[cfg(feature = "osmosis-test-tube")]
            TestRunner::OsmosisTestApp(_) => {
                let path = format!("{}/{}", _artifacts_dir, VAULT_ZAPPER_WASM_NAME);
                println!("Loading contract from {}", path);
                ContractType::Artifact(Artifact::Local(path))
            }
            _ => panic!("Unsupported test runner"),
        }
    }

    /// Creates a new account with default coins
    pub fn default_account(runner: &TestRunner) -> SigningAccount {
        runner
            .init_account(&Coins::from_str(DEFAULT_COINS).unwrap().into_vec())
            .unwrap()
    }

    /// Uploads and instantiates the contracts that the vault zapper depends on
    pub fn instantiate_deps(
        runner: &'a TestRunner,
        dependency_artifacts_dir: &str,
        vault_lock_duration: u64,
        signer: &SigningAccount,
    ) -> VaultZapperDependencies<'a> {
        // TODO: Support Osmosis vault with osmosis liquidity helper
        let vault_dependencies =
            LockedAstroportVaultRobot::instantiate_deps(runner, signer, dependency_artifacts_dir);
        let vault_treasury_addr = runner.init_account(&[]).unwrap().address();
        let (reward_vault_robot, axl_ntrn_pool, _astro_ntrn_pool) =
            LockedAstroportVaultRobot::new_axlr_ntrn_vault(
                runner,
                LockedAstroportVaultRobot::contract(runner, dependency_artifacts_dir),
                Coin::from_str(DENOM_CREATION_FEE).unwrap(),
                vault_treasury_addr,
                Decimal::percent(5),
                vault_lock_duration,
                &vault_dependencies,
                signer,
            );

        let testa = VaultZapperDependencies {
            astroport_contracts: vault_dependencies.astroport_contracts,
            cw_dex_router_robot: vault_dependencies.cw_dex_router_robot,
            liquidity_helper_addr: vault_dependencies.liquidity_helper_addr,
            vault_robot: VaultRobot::Astroport(reward_vault_robot),
            pool_assets: axl_ntrn_pool.pool_assets.clone(),
            vault_pool: Pool::Astroport(axl_ntrn_pool),
        };
        testa
    }

    /// Creates a new `VaultZapperRobot` by uploading and instantiating the contract
    pub fn instantiate(
        runner: &'a TestRunner<'a>,
        dependencies: VaultZapperDependencies<'a>,
        artifacts_dir: &str,
        admin: &SigningAccount,
    ) -> Self {
        let instantiate_msg = InstantiateMsg {
            router: CwDexRouterUnchecked::new(
                dependencies
                    .cw_dex_router_robot
                    .cw_dex_router
                    .addr()
                    .to_string(),
            ),
            liquidity_helper: LiquidityHelperUnchecked::new(
                dependencies.liquidity_helper_addr.clone(),
            ),
        };

        // Upload contract
        let code = Self::contract(runner, artifacts_dir);
        let code_id = runner.store_code(code, admin).unwrap();

        let contract_addr = Wasm::new(runner)
            .instantiate(
                code_id,
                &instantiate_msg,
                Some(&admin.address()),
                None,
                &[],
                admin,
            )
            .unwrap()
            .data
            .address;

        Self {
            runner,
            vault_zapper_addr: contract_addr.to_string(),
            deps: dependencies,
        }
    }

    /// Deposit assets into the vault via the vault zapper
    pub fn zapper_deposit(
        &self,
        assets: AssetList,
        recipient: Option<String>,
        min_out: Uint128,
        unwrap_choice: Unwrap,
        signer: &SigningAccount,
    ) -> &Self {
        // Increase allowance for Cw20s
        let (funds, cw20s) = separate_natives_and_cw20s(&assets);
        for cw20 in cw20s {
            self.increase_cw20_allowance(
                &cw20.address,
                &self.vault_zapper_addr,
                cw20.amount,
                signer,
            );
        }

        unwrap_choice.unwrap(self.wasm().execute(
            &self.vault_zapper_addr,
            &ExecuteMsg::Deposit {
                assets: assets.into(),
                vault_address: self.deps.vault_robot.vault_addr(),
                recipient,
                min_out,
            },
            &funds,
            signer,
        ));

        self
    }

    /// Redeem the specified amount of vault tokens from the vault via the vault zapper
    pub fn zapper_redeem(
        &self,
        amount: impl Into<u128>,
        recipient: Option<String>,
        zap_to: ZapTo,
        min_out: impl Into<AssetListUnchecked>,
        unwrap_choice: Unwrap,
        signer: &SigningAccount,
    ) -> &Self {
        let min_out = min_out.into();
        unwrap_choice.unwrap(self.wasm().execute(
            &self.vault_zapper_addr,
            &ExecuteMsg::Redeem {
                vault_address: self.deps.vault_robot.vault_addr(),
                recipient,
                zap_to,
                min_out,
            },
            &[coin(amount.into(), self.deps.vault_robot.vault_token())],
            signer,
        ));
        self
    }

    /// Redeem all of the signer's vault tokens from the vault via the vault zapper
    pub fn zapper_redeem_all(
        &self,
        recipient: Option<String>,
        zap_to: ZapTo,
        min_out: impl Into<AssetListUnchecked>,
        unwrap_choice: Unwrap,
        signer: &SigningAccount,
    ) -> &Self {
        let balance = self.query_vault_token_balance(signer.address());
        self.zapper_redeem(balance, recipient, zap_to, min_out, unwrap_choice, signer)
    }

    /// Unlock the vault via the vault zapper
    pub fn zapper_unlock(&self, amount: impl Into<u128>, signer: &SigningAccount) -> &Self {
        self.wasm()
            .execute(
                &self.vault_zapper_addr,
                &ExecuteMsg::Unlock {
                    vault_address: self.deps.vault_robot.vault_addr(),
                },
                &[coin(amount.into(), self.deps.vault_robot.vault_token())],
                signer,
            )
            .unwrap();
        self
    }

    /// Unlock all of the signer's vault tokens from the vault via the vault zapper
    pub fn zapper_unlock_all(&self, signer: &SigningAccount) -> &Self {
        let balance = self.query_vault_token_balance(signer.address());
        self.zapper_unlock(balance, signer)
    }

    /// Withdraw unlocked assets from the vault via the vault zapper
    pub fn zapper_withdraw_unlocked(
        &self,
        lockup_id: u64,
        recipient: Option<String>,
        zap_to: ZapTo,
        min_out: impl Into<AssetListUnchecked>,
        unwrap_choice: Unwrap,
        signer: &SigningAccount,
    ) -> &Self {
        let min_out = min_out.into();
        unwrap_choice.unwrap(self.wasm().execute(
            &self.vault_zapper_addr,
            &ExecuteMsg::WithdrawUnlocked {
                vault_address: self.deps.vault_robot.vault_addr(),
                lockup_id,
                recipient,
                zap_to,
                min_out,
            },
            &[],
            signer,
        ));
        self
    }

    /// Increases the test runner's block time by the given number of seconds
    pub fn increase_time(&self, seconds: u64) -> &Self {
        self.runner.increase_time(seconds).unwrap();
        self
    }

    /// Queries the depositable assets for the vault zapper
    pub fn zapper_query_depositable_assets(&self) -> Vec<AssetInfo> {
        self.wasm()
            .query(
                &self.vault_zapper_addr,
                &QueryMsg::DepositableAssets {
                    vault_address: self.vault_addr(),
                },
            )
            .unwrap()
    }

    /// Queries the withdrawable assets for the vault zapper
    pub fn zapper_query_withdrawable_assets(&self) -> Vec<ZapTo> {
        self.wasm()
            .query(
                &self.vault_zapper_addr,
                &QueryMsg::ReceiveChoices {
                    vault_address: self.vault_addr(),
                },
            )
            .unwrap()
    }

    /// Queries the unlocking positions for the vault zapper
    pub fn zapper_query_unlocking_positions(&self, owner: &str) -> Vec<UnlockingPosition> {
        self.wasm()
            .query(
                &self.vault_zapper_addr,
                &QueryMsg::UnlockingPositions {
                    vault_address: self.vault_addr(),
                    owner: owner.to_string(),
                },
            )
            .unwrap()
    }

    /// Asserts that the balance of an Astroport AssetInfo for the given address is approximately
    /// equal to the expected amount, with the given max relative difference as a string percentage.
    pub fn assert_asset_balance_approx_eq(
        &self,
        asset: impl Into<cw_it::astroport::astroport::asset::AssetInfo>,
        address: &str,
        expected: impl Into<Uint128>,
        max_rel_diff: &str,
    ) -> &Self {
        let actual = self.query_asset_balance(&asset.into(), address);
        assert_approx_eq!(actual, expected.into(), max_rel_diff);
        self
    }
}
