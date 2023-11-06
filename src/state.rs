use std::iter::Take;

use cosmwasm_std::{Addr, Deps, Order, StdError, StdResult};
use cw_dex_router::helpers::CwDexRouter;
use cw_storage_plus::{Bound, Item, Map};
use liquidity_helper::LiquidityHelper;

pub const ROUTER: Item<CwDexRouter> = Item::new("router");
pub const LIQUIDITY_HELPER: Item<LiquidityHelper> = Item::new("liquidity_helper");

pub const ASTROPORT_LIQUIDITY_MANAGER: Item<Addr> = Item::new("astroport_liquidity_manager");

/// Stores the lockup ids for unlocking positions. The key is a tuple of
/// (owner_address, vault_address, lockup_id).
pub const LOCKUP_IDS: Map<(Addr, Addr, u64), ()> = Map::new("lockup_ids");

pub const TEMP_LOCK_KEY: Item<(Addr, Addr)> = Item::new("temp_lock_key");

/// The default limit when paginating and no limit is specified
pub const DEFAULT_LIMIT: u32 = 10;

pub type LockupIdIterator<'a> =
    Take<Box<dyn Iterator<Item = Result<((Addr, u64), ()), StdError>> + 'a>>;

pub fn paginate_all_user_unlocking_positions(
    deps: Deps,
    user: Addr,
    start_after_vault_addr: Option<String>,
    start_after_id: Option<u64>,
    limit: Option<u32>,
) -> StdResult<LockupIdIterator> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT) as usize;

    let start = match (start_after_vault_addr, start_after_id) {
        (Some(vault_addr), Some(id)) => {
            Some(Bound::exclusive((deps.api.addr_validate(&vault_addr)?, id)))
        }
        (Some(vault_addr), None) => Some(Bound::exclusive((
            deps.api.addr_validate(&vault_addr)?,
            u64::MAX,
        ))),
        (None, Some(_)) => {
            return Err(StdError::generic_err(
                "Need to supply start_after_vault_addr if start_after_id is supplied",
            ))
        }
        (None, None) => None,
    };

    let user_lockup_ids = LOCKUP_IDS
        .sub_prefix(user)
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit);

    Ok(user_lockup_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    use cosmwasm_std::testing::mock_dependencies;
    use cosmwasm_std::Storage;

    fn store_lock_id(storage: &mut dyn Storage, user: Addr, vault_address: Addr, lock_id: u64) {
        LOCKUP_IDS
            .save(storage, (user, vault_address, lock_id), &())
            .unwrap();
    }

    #[test]
    fn test_paginate_all_user_unlocking_positions() {
        let mut deps = mock_dependencies();
        let storage = deps.as_mut().storage;

        // Store some lockup ids
        store_lock_id(
            storage,
            Addr::unchecked("addr0001"),
            Addr::unchecked("vault0001"),
            0,
        );

        store_lock_id(
            storage,
            Addr::unchecked("addr0001"),
            Addr::unchecked("vault0001"),
            1,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0001"),
            Addr::unchecked("vault0001"),
            2,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0002"),
            Addr::unchecked("vault0001"),
            3,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0002"),
            Addr::unchecked("vault0001"),
            4,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0001"),
            Addr::unchecked("vault0002"),
            0,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0001"),
            Addr::unchecked("vault0002"),
            1,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0002"),
            Addr::unchecked("vault0002"),
            2,
        );
        store_lock_id(
            storage,
            Addr::unchecked("addr0002"),
            Addr::unchecked("vault0002"),
            3,
        );

        // Query all unlocking positions for addr0001
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0001"),
            None,
            None,
            None,
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 5);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0001"), 0),
                (Addr::unchecked("vault0001"), 1),
                (Addr::unchecked("vault0001"), 2),
                (Addr::unchecked("vault0002"), 0),
                (Addr::unchecked("vault0002"), 1),
            ]
        );

        // Query all unlocking positions for addr0001 with limit
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0001"),
            None,
            None,
            Some(2),
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 2);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0001"), 0),
                (Addr::unchecked("vault0001"), 1),
            ]
        );

        // Query all unlocking positions for addr0001 with start_after_vault_addr
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0001"),
            Some("vault0001".to_string()),
            None,
            None,
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 2);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0002"), 0),
                (Addr::unchecked("vault0002"), 1),
            ]
        );

        // Query all unlocking positions for addr0001 with start_after_vault_addr and
        // start_after_id
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0001"),
            Some("vault0001".to_string()),
            Some(0),
            None,
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 4);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0001"), 1),
                (Addr::unchecked("vault0001"), 2),
                (Addr::unchecked("vault0002"), 0),
                (Addr::unchecked("vault0002"), 1),
            ]
        );

        // Query all unlocking positions for addr0001 with only start_after_id
        let res = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0001"),
            None,
            Some(1),
            None,
        )
        .is_err();
        assert!(res);

        // Query all unlocking positions for addr0002
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0002"),
            None,
            None,
            None,
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 4);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0001"), 3),
                (Addr::unchecked("vault0001"), 4),
                (Addr::unchecked("vault0002"), 2),
                (Addr::unchecked("vault0002"), 3),
            ]
        );

        // Query all unlocking positions for addr0002 with limit
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0002"),
            None,
            None,
            Some(2),
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 2);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0001"), 3),
                (Addr::unchecked("vault0001"), 4),
            ]
        );

        // Query all unlocking positions for addr0002 with start_after_vault_addr
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0002"),
            Some("vault0001".to_string()),
            None,
            Some(2),
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 2);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0002"), 2),
                (Addr::unchecked("vault0002"), 3),
            ]
        );

        // Query all unlocking positions for addr0002 with start_after_vault_addr and
        // start_after_id
        let res: Vec<(Addr, u64)> = paginate_all_user_unlocking_positions(
            deps.as_ref(),
            Addr::unchecked("addr0002"),
            Some("vault0001".to_string()),
            Some(3),
            None,
        )
        .unwrap()
        .map(|x| x.unwrap().0)
        .collect();
        assert_eq!(res.len(), 3);
        assert_eq!(
            res,
            vec![
                (Addr::unchecked("vault0001"), 4),
                (Addr::unchecked("vault0002"), 2),
                (Addr::unchecked("vault0002"), 3),
            ]
        );
    }
}
