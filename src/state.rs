use apollo_cw_asset::Asset;
use cosmwasm_std::{Addr, WasmMsg};
use cw_dex_router::helpers::CwDexRouter;
use cw_storage_plus::{Item, Map};
use liquidity_helper::LiquidityHelper;

pub const ROUTER: Item<CwDexRouter> = Item::new("router");
pub const LIQUIDITY_HELPER: Item<LiquidityHelper> = Item::new("liquidity_helper");

pub const LOCKUP_IDS: Map<(Addr, Addr), Vec<u64>> = Map::new("lockup_ids");

pub const TEMP_LOCK_KEY: Item<(Addr, Addr)> = Item::new("temp_lock_key");

pub struct WithdrawMsg {
    pub msg: WasmMsg,
    pub redeem_asset: Asset,
}
