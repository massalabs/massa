use massa_execution_exports::ExecutionError;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::execution::TempFileVestingRange;
use massa_models::prehash::PreHashMap;
use massa_models::slot::Slot;
use massa_models::timeslots;
use massa_time::MassaTime;
use std::path::PathBuf;

use crate::context::ExecutionContext;

#[derive(Debug, Eq, PartialEq)]
pub struct VestingInfo {
    pub(crate) min_balance: Amount,
    pub(crate) max_rolls: u64,
}

pub struct VestingManager {
    pub(crate) vesting_registry: PreHashMap<Address, Vec<(MassaTime, VestingInfo)>>,
    thread_count: u8,
    t0: MassaTime,
    genesis_timestamp: MassaTime,
    periods_per_cycle: u64,
    roll_price: Amount,
}

impl VestingManager {
    pub fn new(
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
        periods_per_cycle: u64,
        roll_price: Amount,
        file_path: PathBuf,
    ) -> Result<Self, ExecutionError> {
        let vesting = VestingManager::load_vesting_from_file(file_path)?;
        Ok(VestingManager {
            vesting_registry: vesting,
            thread_count,
            t0,
            genesis_timestamp,
            periods_per_cycle,
            roll_price,
        })
    }

    /// Check vesting max rolls for given address
    ///
    /// Call when roll_buy op
    ///
    /// * `buyer_addr` buyer address
    /// * `slot` current slot
    /// * `roll_count` roll count to buy
    pub fn check_vesting_rolls_buy(
        &self,
        rolls: (u64, u64),
        buyer_addr: &Address,
        slot: Slot,
        roll_count: u64,
    ) -> Result<(), ExecutionError> {
        if let Some(vesting) = self.get_addr_vesting_at_slot(buyer_addr, slot)? {
            // (candidate_rolls + amount to buy)
            let max_rolls = rolls.1.saturating_add(roll_count);
            if max_rolls > vesting.1.max_rolls {
                return Err(ExecutionError::VestingError(format!(
                    "trying to get to a total of {} rolls but only {} are allowed at that time by the vesting scheme",
                    max_rolls, vesting.1.max_rolls
                )));
            }
        }

        Ok(())
    }

    /// Check vesting minimal balance for given address
    ///
    /// Call on transfer_coins OP
    ///
    /// * `addr` sender address
    /// * `amount` amount of coins to transfer
    pub fn check_vesting_transfer_coins(
        &self,
        context: &ExecutionContext,
        addr: &Address,
        amount: Amount,
    ) -> Result<(), ExecutionError> {
        if amount == Amount::zero() {
            return Ok(());
        }

        // For the case of user sending coins to itself :
        // That implies spending the coins first, then receiving them.
        // So the spending part can fail in the case of vesting
        if let Some(vesting) = self.get_addr_vesting_at_slot(addr, context.slot)? {
            let new_balance = context
            .get_balance(addr)
            .ok_or_else(|| ExecutionError::RuntimeError(format!("spending address {} not found", addr)))?
            .checked_sub(amount)
            .ok_or_else(|| {
                ExecutionError::RuntimeError(format!("failed check transfer {} from spending address {} due to insufficient balance {}", amount, addr, context
                    .get_balance(addr).unwrap_or_default()))
            })?;

            let vec = context.get_address_cycle_infos(addr, self.periods_per_cycle);
            let Some(exec_info) = vec.last() else {
                return Err(ExecutionError::VestingError(format!("can not get address info cycle for {}", addr)));
            };

            let rolls_value = exec_info
                .active_rolls
                .map(|rolls| self.roll_price.saturating_mul_u64(rolls))
                .unwrap_or(Amount::zero());

            let deferred_map = context.get_address_deferred_credits(addr, context.slot);

            let deferred_credits = if deferred_map.is_empty() {
                Amount::zero()
            } else {
                Amount::from_raw(deferred_map.into_values().map(|a| a.to_raw()).sum())
            };

            // min_balance = (rolls * roll_price) + balance + deferred_credits
            let min_balance = new_balance
                .saturating_add(rolls_value)
                .saturating_add(deferred_credits);

            if min_balance < vesting.1.min_balance {
                return Err(ExecutionError::VestingError(format!(
                    "vesting_min_balance={} with value min_balance={} ",
                    vesting.1.min_balance, min_balance
                )));
            }
        }

        Ok(())
    }

    /// Retrieve vesting info for address at given time
    ///
    /// Return None if address is not vested
    pub(crate) fn get_addr_vesting_at_time(
        &self,
        addr: &Address,
        timestamp: &MassaTime,
    ) -> Option<&(MassaTime, VestingInfo)> {
        let Some(vest) = self.vesting_registry.get(addr) else {
            return None;
        };

        vest.get(
            vest.binary_search_by_key(timestamp, |(t, _)| *t)
                .unwrap_or_else(|i| i.saturating_sub(1)),
        )
    }

    /// Retrieve vesting info for address at given slot
    ///
    /// Return Ok(None) if address is not vested
    fn get_addr_vesting_at_slot(
        &self,
        addr: &Address,
        slot: Slot,
    ) -> Result<Option<&(MassaTime, VestingInfo)>, ExecutionError> {
        let timestamp = timeslots::get_block_slot_timestamp(
            self.thread_count,
            self.t0,
            self.genesis_timestamp,
            slot,
        )
        .map_err(|e| {
            ExecutionError::RuntimeError(format!(
                "error with get_block_slot_timestamp with slot : {}, error : {}",
                slot, e
            ))
        })?;

        Ok(self.get_addr_vesting_at_time(addr, &timestamp))
    }

    /// Initialize the hashmap of addresses from the vesting file
    fn load_vesting_from_file(
        path: PathBuf,
    ) -> Result<PreHashMap<Address, Vec<(MassaTime, VestingInfo)>>, ExecutionError> {
        let mut map: PreHashMap<Address, Vec<(MassaTime, VestingInfo)>> = PreHashMap::default();
        let data_load: PreHashMap<Address, Vec<TempFileVestingRange>> =
            serde_json::from_str(&std::fs::read_to_string(path).map_err(|err| {
                ExecutionError::InitVestingError(format!(
                    "error loading initial vesting file  {}",
                    err
                ))
            })?)
            .map_err(|err| {
                ExecutionError::InitVestingError(format!(
                    "error on deserialize initial vesting file  {}",
                    err
                ))
            })?;

        for (k, v) in data_load.into_iter() {
            let mut vec: Vec<(MassaTime, VestingInfo)> = v
                .into_iter()
                .map(|value| {
                    (
                        value.timestamp,
                        VestingInfo {
                            min_balance: value.min_balance,
                            max_rolls: value.max_rolls,
                        },
                    )
                })
                .collect();

            vec.sort_by(|a, b| a.0.cmp(&b.0));
            map.insert(k, vec);
        }

        Ok(map)
    }
}
