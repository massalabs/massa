#[cfg(test)]
mod test {
    // todo move to tests module
    use crate::vesting_manager::{VestingInfo, VestingManager};
    use massa_execution_exports::ExecutionConfig;
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

        let cfg = ExecutionConfig::default();

        let mut manager =
            VestingManager::new(cfg.thread_count, cfg.t0, cfg.genesis_timestamp).unwrap();

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

        manager.vesting_registry = map;
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
