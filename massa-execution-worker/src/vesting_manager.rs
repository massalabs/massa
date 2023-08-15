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

#[derive(Debug, Eq, Default, Clone, PartialEq)]
pub struct VestingInfo {
    pub(crate) min_balance: Option<Amount>,
    pub(crate) max_rolls: Option<u64>,
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
        if let Some(vesting_max_rolls) = self.get_addr_vesting_at_slot(buyer_addr, slot).max_rolls {
            // (candidate_rolls + amount to buy)
            let max_rolls = rolls.1.saturating_add(roll_count);
            if max_rolls > vesting_max_rolls {
                return Err(ExecutionError::VestingError(format!(
                        "trying to get to a total of {} rolls but only {} are allowed at that time by the vesting scheme",
                        max_rolls, vesting_max_rolls
                    )));
            }
        }

        Ok(())
    }

    /// Gets the minimal coin balance this address is allowed to have given the vesting scheme.
    /// Takes into account rolls, deferred credits etc...
    /// * `addr` sender address
    /// * `balance` sender balance
    pub fn check_coin_vesting(
        &self,
        context: &ExecutionContext,
        addr: &Address,
        balance: Amount,
    ) -> Result<(), ExecutionError> {
        // For the case of user sending coins to itself :
        // That implies spending the coins first, then receiving them.
        // So the spending part can fail in the case of vesting
        if let Some(vesting_min_balance) = self
            .get_addr_vesting_at_slot(addr, context.slot)
            .min_balance
        {
            let slot_cycle = context.slot.get_cycle(self.periods_per_cycle);

            let rolls_value = context
                .get_address_cycle_infos(addr, self.periods_per_cycle)
                .into_iter()
                .find_map(|nfo| {
                    if nfo.cycle == slot_cycle {
                        Some(
                            self.roll_price
                                .saturating_mul_u64(nfo.active_rolls.unwrap_or_default()),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_else(Amount::zero);

            let deferred_credits_value = context
                .get_address_deferred_credits(addr, context.slot)
                .into_values()
                .reduce(|acc, amount| acc.saturating_add(amount))
                .unwrap_or_else(Amount::zero);

            // min_balance = (rolls * roll_price) + balance + deferred_credits
            let total_coin_value = balance
                .saturating_add(rolls_value)
                .saturating_add(deferred_credits_value);

            if total_coin_value < vesting_min_balance {
                return Err(ExecutionError::VestingError(format!(
                    "total coin value {} fell under the minimal vesting value {}",
                    total_coin_value, vesting_min_balance
                )));
            }
        }

        Ok(())
    }

    /// Retrieve vesting info for address at given time
    pub(crate) fn get_addr_vesting_at_time(
        &self,
        addr: &Address,
        timestamp: &MassaTime,
    ) -> VestingInfo {
        if let Some(vec) = self.vesting_registry.get(addr) {
            if let Some(vesting) = vec.get(
                vec.binary_search_by_key(timestamp, |(t, _)| *t)
                    .unwrap_or_else(|i| i.saturating_sub(1)),
            ) {
                return vesting.1.clone();
            }
        }

        VestingInfo::default()
    }

    /// Retrieve vesting info for address at given slot
    fn get_addr_vesting_at_slot(&self, addr: &Address, slot: Slot) -> VestingInfo {
        let timestamp = timeslots::get_block_slot_timestamp(
            self.thread_count,
            self.t0,
            self.genesis_timestamp,
            slot,
        )
        .unwrap_or_else(|_| MassaTime::max());
        self.get_addr_vesting_at_time(addr, &timestamp)
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
