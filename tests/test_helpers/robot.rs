use apollo_cw_asset::{AssetList, AssetListUnchecked};
use apollo_utils::assets::{increase_allowance_msgs, separate_natives_and_cw20s};
use cosmwasm_std::{
    testing::{mock_dependencies, mock_env},
    to_binary, Api, CosmosMsg, StdResult, Uint128, WasmMsg,
};
use cw20::Cw20ExecuteMsg;
use cw_it::osmosis::OsmosisVaultRobot;
use osmosis_test_tube::{
    cosmrs::proto::cosmwasm::wasm::v1::MsgExecuteContractResponse, Account, Module, Runner,
    SigningAccount, Wasm,
};
use vault_zapper::msg::{ExecuteMsg, InstantiateMsg};

pub struct VaultZapperRobot<'a, R: Runner<'a>> {
    pub app: &'a R,
    pub vault_robot: OsmosisVaultRobot<'a, R>,
    pub wasm: Wasm<'a, R>,
    pub vault_zapper_addr: String,
}

impl<'a, R: Runner<'a>> VaultZapperRobot<'a, R> {
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
            wasm,
            vault_zapper_addr,
        }
    }

    pub fn deposit(&self, signer: &SigningAccount, assets: AssetList) {
        let (funds, cw20s) = separate_natives_and_cw20s(&assets);

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
        let msg = ExecuteMsg::Deposit {
            assets: assets.into(),
            vault_address: self.vault_robot.vault_addr.clone(),
            recipient: None,
            min_out: Uint128::new(1),
        };
        self.wasm
            .execute(&self.vault_zapper_addr, &msg, &funds, signer)
            .unwrap();
    }
}
