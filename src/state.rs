use cosmwasm_std::Addr;
use cw_dex_router::helpers::CwDexRouter;
use cw_storage_plus::{Item, Map};

pub const LOCKUP_IDS: Map<Addr, Vec<u64>> = Map::new("lockup_ids");

pub const ROUTER: Item<CwDexRouter> = Item::new("router");
