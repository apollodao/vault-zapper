use cosmwasm_std::{Addr, WasmMsg};
use cw_asset::Asset;
use cw_dex_router::helpers::CwDexRouter;
use cw_storage_plus::Item;

pub const ROUTER: Item<CwDexRouter> = Item::new("router");

// I'm not aware of any way to send data to our own reply entrypoint, so we must
// save the caller of ExecuteMsg::Unlock here to be able to fetch it in the reply entrypoint...
pub const TEMP_UNLOCK_CALLER: Item<Addr> = Item::new("temp_unlock_caller");

pub struct WithdrawMsg {
    pub msg: WasmMsg,
    pub redeem_amount: Asset,
}
