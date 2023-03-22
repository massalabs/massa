use massa_execution_exports::ExecutionError;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::prehash::PreHashMap;
use massa_models::slot::Slot;
use massa_models::timeslots;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::error;

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
}

/// Used for Deserialize
#[derive(Clone, Copy, Deserialize, Serialize, Debug)]
pub(crate) struct TempFileVestingRange {
    /// start timestamp
    pub timestamp: MassaTime,

    /// minimal balance
    pub min_balance: Amount,

    /// max rolls
    pub max_rolls: u64,
}

impl VestingManager {
    pub fn new(
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
        file_path: PathBuf,
    ) -> Result<Self, ExecutionError> {
        let vesting = match VestingManager::load_vesting_from_file(file_path) {
            Ok(data) => data,
            Err(e) => {
                error!("error on vesting file load : {}", e);
                PreHashMap::default()
            }
        };

        Ok(VestingManager {
            vesting_registry: vesting,
            thread_count,
            t0,
            genesis_timestamp,
        })
    }

    /// Retrieve vesting info for address at given time
    ///
    /// Return None if address is not vested
    pub fn get_addr_vesting_at_time(
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
    pub fn get_addr_vesting_at_slot(
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
