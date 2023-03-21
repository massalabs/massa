use massa_execution_exports::{ExecutionConfig, ExecutionError};
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::prehash::PreHashMap;
use massa_models::slot::Slot;
use massa_models::vesting_range::VestingRange;
use massa_time::MassaTime;

#[derive(Debug, Eq, PartialEq)]
struct VestingInfo {
    min_balance: Amount,
    max_rolls: u64,
}

#[derive(Debug)]
struct VestingManager(PreHashMap<Address, Vec<(MassaTime, VestingInfo)>>);

impl VestingManager {
    pub fn new() -> Result<Self, ExecutionError> {
        // todo init with file
        Ok(VestingManager(Default::default()))
    }

    /// Retrieve vesting info for address at given time
    ///
    /// Return None if address is not vested
    pub fn get_addr_vesting_at_time(
        &self,
        addr: &Address,
        timestamp: &MassaTime,
    ) -> Option<&(MassaTime, VestingInfo)> {
        let Some(vest) = self.0.get(addr) else {
            return None;
        };

        vest.get(
            vest.binary_search_by_key(timestamp, |(t, _)| *t)
                .unwrap_or_else(|i| i.saturating_sub(1)),
        )
    }

    /// Initialize the hashmap of addresses from the vesting file
    fn init_vesting_registry(
        config: &ExecutionConfig,
    ) -> Result<PreHashMap<Address, Vec<VestingRange>>, ExecutionError> {
        // todo rework
        let mut hashmap: PreHashMap<Address, Vec<VestingRange>> = serde_json::from_str(
            &std::fs::read_to_string(&config.initial_vesting_path).map_err(|err| {
                ExecutionError::InitVestingError(format!(
                    "error loading initial vesting file  {}",
                    err
                ))
            })?,
        )
        .map_err(|err| {
            ExecutionError::InitVestingError(format!(
                "error on deserialize initial vesting file  {}",
                err
            ))
        })?;

        let get_slot_at_timestamp = |config: &ExecutionConfig, timestamp: MassaTime| {
            massa_models::timeslots::get_latest_block_slot_at_timestamp(
                config.thread_count,
                config.t0,
                config.genesis_timestamp,
                timestamp,
            )
            .map_err(|e| {
                ExecutionError::InitVestingError(format!(
                    "can no get the slot at timestamp {} : {}",
                    timestamp, e
                ))
            })
        };

        for v in hashmap.values_mut() {
            if v.len().eq(&1) {
                return Err(ExecutionError::InitVestingError(
                    "vesting file should have more than one element".to_string(),
                ));
            } else {
                *v = v
                    .windows(2)
                    .map(|elements| {
                        let (mut prev, next) = (elements[0], elements[1]);

                        // retrieve the start_slot
                        if prev.timestamp.eq(&MassaTime::from(0)) {
                            // first range with timestamp = 0
                            prev.start_slot = Slot::min();
                        } else if let Some(slot) = get_slot_at_timestamp(config, prev.timestamp)? {
                            prev.start_slot = slot;
                        }

                        // retrieve the end_slot
                        if let Some(next_range_slot) =
                            get_slot_at_timestamp(config, next.timestamp)?
                        {
                            prev.end_slot = next_range_slot.get_prev_slot(config.thread_count)?;
                        }

                        Ok(prev)
                    })
                    // we don't need range with StartSlot(0,0) && EndSlot(0,0)
                    // this can happen when the timestamp is passed
                    .filter(|a| {
                        !(a.as_ref().unwrap().end_slot == Slot::min()
                            && a.as_ref().unwrap().start_slot == Slot::min())
                    })
                    .collect::<Result<Vec<VestingRange>, ExecutionError>>()?;
            }
        }

        Ok(hashmap)
    }
}

#[cfg(test)]
mod test {
    // todo move to tests module
    use crate::vesting_manager::{VestingInfo, VestingManager};
    use massa_models::address::Address;
    use massa_models::amount::Amount;
    use massa_models::prehash::PreHashMap;
    use massa_models::vesting_range::VestingRange;
    use massa_time::MassaTime;
    use std::str::FromStr;

    fn mock_manager(with_data: bool) -> VestingManager {
        const PAST_TIMESTAMP: u64 = 1675356692000; // 02/02/2023 17h51
        const SEC_TIMESTAMP: u64 = 1677775892000; // 02/03/2023 17h51;
        const FUTURE_TIMESTAMP: u64 = 1731257385000; // 10/11/2024 17h49;

        let mut manager = VestingManager::new().unwrap();

        let map = if with_data {
            let addr =
                Address::from_str("AU1LQrXPJ3DVL8SFRqACk31E9MVxBcmCATFiRdpEmgztGxWAx48D").unwrap();
            let mut map: PreHashMap<Address, Vec<(MassaTime, VestingInfo)>> = PreHashMap::default();
            let mut vec = vec![
                (
                    MassaTime::from(PAST_TIMESTAMP),
                    VestingInfo {
                        min_balance: Amount::from_str("150000").unwrap(),
                        max_rolls: 10,
                    },
                ),
                (
                    MassaTime::from(SEC_TIMESTAMP),
                    VestingInfo {
                        min_balance: Amount::from_str("100000").unwrap(),
                        max_rolls: 15,
                    },
                ),
                (
                    MassaTime::from(FUTURE_TIMESTAMP),
                    VestingInfo {
                        min_balance: Amount::from_str("80000").unwrap(),
                        max_rolls: 20,
                    },
                ),
            ];

            // force ordering vec by timestamp
            vec.sort_by(|a, b| a.0.cmp(&b.0));
            map.insert(addr, vec);
            map
        } else {
            PreHashMap::default()
        };

        manager.0 = map;
        manager
    }

    #[test]
    fn test_get_addr_vesting_at_time() {
        let manager = mock_manager(true);

        let addr =
            Address::from_str("AU1LQrXPJ3DVL8SFRqACk31E9MVxBcmCATFiRdpEmgztGxWAx48D").unwrap();

        {
            // addr not vested
            let addr2 =
                Address::from_str("AU1DHJY6zd6oKJPos8gQ6KYqmsTR669wes4ZhttLD9gE7PYUF3Rs").unwrap();
            let timestamp = &MassaTime::from(1678193291000); // 07/03/2023 13h48
            let opt = manager.get_addr_vesting_at_time(&addr2, timestamp);
            assert!(opt.is_none());
        }

        {
            let timestamp = &MassaTime::from(1677675988000); // 01/03/2023 14h06
            let result = manager.get_addr_vesting_at_time(&addr, timestamp).unwrap();
            assert_eq!(
                result.1,
                VestingInfo {
                    min_balance: Amount::from_str("150000").unwrap(),
                    max_rolls: 10,
                }
            )
        }

        {
            let timestamp = &MassaTime::from(1678193291000); // 07/03/2023 13h48
            let result = manager.get_addr_vesting_at_time(&addr, timestamp).unwrap();
            assert_eq!(
                result.1,
                VestingInfo {
                    min_balance: Amount::from_str("100000").unwrap(),
                    max_rolls: 15,
                }
            );
        }

        {
            let timestamp = &MassaTime::from(1734786585000); // 21/12/2024 14h09
            let result = manager.get_addr_vesting_at_time(&addr, timestamp).unwrap();
            assert_eq!(
                result.1,
                VestingInfo {
                    min_balance: Amount::from_str("80000").unwrap(),
                    max_rolls: 20,
                }
            )
        }
    }
}
