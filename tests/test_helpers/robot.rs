use std::ops::Deref;
use std::str::FromStr;

use apollo_cw_asset::{Asset, AssetInfo, AssetList, AssetListUnchecked};
use apollo_utils::assets::{increase_allowance_msgs, separate_natives_and_cw20s};
use cosmwasm_std::testing::{mock_dependencies, mock_env};
use cosmwasm_std::{to_binary, Api, Coin, CosmosMsg, StdResult, Uint128, WasmMsg};
use cw20::Cw20ExecuteMsg;
use cw_dex::traits::Pool;
use cw_dex_router::operations::SwapOperationsList;
use cw_it::helpers::{bank_balance_query, bank_send};
use cw_it::osmosis_test_tube::cosmrs::proto::cosmwasm::wasm::v1::MsgExecuteContractResponse;
use cw_it::osmosis_test_tube::{
    Account, Bank, Module, OsmosisTestApp, Runner, SigningAccount, Wasm,
};
use osmosis_test_tube::cosmrs::proto::cosmos::bank::v1beta1::QueryBalanceRequest;
use osmosis_vault_test_helpers::robot::OsmosisVaultRobot;
use vault_zapper::msg::{ExecuteMsg, InstantiateMsg, ZapTo};

pub struct OsmosisVaultZapperRobot<'a, R: Runner<'a>> {
    pub app: &'a R,
    pub vault_robot: OsmosisVaultRobot<'a, R>,
    pub vault_zapper_addr: String,
}

impl<'a, R: Runner<'a>> TestRobot<'a, R> for OsmosisVaultZapperRobot<'a, R> {
    fn app(&self) -> &'a R {
        self.app
    }
}

impl<'a, R: Runner<'a>> OsmosisVaultZapperRobot<'a, R> {
    pub fn app(&self) -> &R {
        self.app
    }

    pub fn new(app: &'a R, admin: &SigningAccount, vault_robot: OsmosisVaultRobot<'a, R>) -> Self {
        let wasm = Wasm::new(app);

        // Instantiate the contract
        let init_msg = InstantiateMsg {
            router: vault_robot.router.clone().into(),
        };
        let res = wasm
            .instantiate(
                vault_robot.code_ids["vault_zapper"],
                &init_msg,
                Some(&admin.address()),
                Some("Vault Zapper"),
                &[],
                admin,
            )
            .unwrap();
        let vault_zapper_addr = res.data.address;

        Self {
            app,
            vault_robot,
            vault_zapper_addr,
        }
    }

    pub fn zap_in(&self, signer: &SigningAccount, assets: AssetList, funds: Vec<Coin>) -> &Self {
        let (_, cw20s) = separate_natives_and_cw20s(&assets);

        // Increase allowance for any CW20s
        let allowance_msgs: Vec<CosmosMsg> = cw20s
            .into_iter()
            .map(|x| {
                Ok(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: x.address,
                    msg: to_binary(&Cw20ExecuteMsg::IncreaseAllowance {
                        spender: self.vault_zapper_addr.clone(),
                        amount: x.amount,
                        expires: None,
                    })?,
                    funds: vec![],
                }))
            })
            .collect::<StdResult<Vec<_>>>()
            .unwrap();
        if !allowance_msgs.is_empty() {
            self.app
                .execute_cosmos_msgs::<MsgExecuteContractResponse>(&allowance_msgs, signer)
                .unwrap();
        }

        // Execute the deposit
        let msg = ExecuteMsg::ZapIn {
            assets: assets.into(),
            vault_address: self.vault_robot.vault_addr.clone(),
            recipient: None,
            min_out: Uint128::new(1),
        };
        self.wasm()
            .execute(&self.vault_zapper_addr, &msg, &funds, signer)
            .unwrap();

        self
    }

    pub fn zap_out(
        &self,
        signer: &SigningAccount,
        asset: Asset,
        funds: Vec<Coin>,
        zap_to: AssetInfo,
        recipient: Option<String>,
    ) -> &Self {
        let msg = ExecuteMsg::ZapOut {
            vault_address: self.vault_robot.vault_addr.clone(),
            amount: asset.amount,
            zap_to,
            recipient,
        };

        self.wasm()
            .execute(&self.vault_zapper_addr, &msg, &funds, signer)
            .unwrap();

        self
    }

    pub fn query_vault_token_balance(&self, account: impl Into<String>) -> Uint128 {
        self.vault_robot.query_vault_token_balance(&account.into())
    }

    pub fn assert_vault_token_balance_eq(
        &self,
        account: impl Into<String>,
        expected: impl Into<Uint128>,
    ) -> &Self {
        self.vault_robot
            .assert_vault_token_balance_eq(&account.into(), expected.into());

        self
    }

    pub fn assert_vault_token_balance_gt(
        &self,
        account: impl Into<String>,
        expected: impl Into<Uint128>,
    ) -> &Self {
        self.vault_robot
            .assert_vault_token_balance_gt(&account.into(), expected.into());

        self
    }

    pub fn assert_base_token_balance_eq(
        &self,
        account: impl Into<String>,
        expected: impl Into<Uint128>,
    ) -> &Self {
        self.vault_robot
            .assert_base_token_balance_eq(&account.into(), expected.into());

        self
    }

    pub fn assert_base_token_balance_gt(
        &self,
        account: impl Into<String>,
        expected: impl Into<Uint128>,
    ) -> &Self {
        self.vault_robot
            .assert_base_token_balance_gt(&account.into(), expected.into());

        self
    }
}

impl<'a> OsmosisVaultZapperRobot<'a, OsmosisTestApp> {
    pub fn increase_time(&self, seconds: u64) -> &Self {
        self.app.increase_time(seconds);
        self
    }
}

pub trait TestRobot<'a, R: Runner<'a> + 'a> {
    fn app(&self) -> &'a R;

    fn wasm(&self) -> Wasm<'a, R> {
        Wasm::new(self.app())
    }

    fn bank(&self) -> Bank<'a, R> {
        Bank::new(self.app())
    }

    fn query_native_token_balance(
        &self,
        account: impl Into<String>,
        denom: impl Into<String>,
    ) -> Uint128 {
        let msg = QueryBalanceRequest {
            address: account.into(),
            denom: denom.into(),
        };

        self.bank()
            .query_balance(&msg)
            .unwrap()
            .balance
            .map(|x| Uint128::from_str(&x.amount))
            .transpose()
            .unwrap()
            .unwrap_or_default()
    }

    fn assert_native_token_balance_eq(
        &self,
        account: impl Into<String>,
        denom: impl Into<String>,
        expected: impl Into<Uint128>,
    ) -> &Self {
        let actual = self.query_native_token_balance(account, denom);
        assert_eq!(actual, expected.into());

        self
    }

    // fn query_native_balance(
    //     &self,
    //     account: impl Into<String>,
    //     denom: impl Into<String>,
    // ) -> Uint128 {
    //     let msg = QueryBalanceRequest {
    //         address: account.into(),
    //         denom: denom.into(),
    //     };

    //     self.bank()
    //         .query_balance(&msg)
    //         .unwrap()
    //         .balance
    //         .map(|x| Uint128::from_str(&x.amount))
    //         .transpose()
    //         .unwrap()
    //         .unwrap_or_default()
    // }

    // fn send_native_tokens(
    //     &self,
    //     from: &SigningAccount,
    //     to: impl Into<String>,
    //     amount: impl Into<Uint128>,
    //     denom: impl Into<String>,
    // ) -> &Self {
    //     let coin = Coin {
    //         amount: amount.into(),
    //         denom: denom.into(),
    //     };

    //     bank_send(self.app(), from, &to.into(), vec![coin]).unwrap();

    //     self
    // }
}
