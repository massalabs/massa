#[cfg(test)]
mod test {
    use crate::tests::mock::get_initials_vesting;
    use crate::vesting_manager::{VestingInfo, VestingManager};
    use massa_models::address::Address;
    use massa_models::amount::Amount;
    use massa_models::config::{
        GENESIS_TIMESTAMP, PERIODS_PER_CYCLE, ROLL_PRICE, T0, THREAD_COUNT,
    };
    use massa_time::MassaTime;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn mock_manager(with_data: bool) -> VestingManager {
        let file = get_initials_vesting(with_data);

        let manager = VestingManager::new(
            THREAD_COUNT,
            T0,
            *GENESIS_TIMESTAMP,
            PERIODS_PER_CYCLE,
            ROLL_PRICE,
            file.path().to_path_buf(),
        )
        .unwrap();

        manager
    }

    #[test]
    fn test_get_addr_vesting_at_time() {
        let manager = mock_manager(true);

        let keypair_0 = massa_signature::KeyPair::from_str(
            "S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ",
        )
        .unwrap();
        let addr = Address::from_public_key(&keypair_0.get_public_key());

        {
            // addr not vested
            let addr2 =
                Address::from_str("AU1DHJY6zd6oKJPos8gQ6KYqmsTR669wes4ZhttLD9gE7PYUF3Rs").unwrap();
            let timestamp = &MassaTime::from(1678193291000); // 07/03/2023 13h48
            let vesting = manager.get_addr_vesting_at_time(&addr2, timestamp);
            assert!(vesting.min_balance.is_none());
            assert!(vesting.max_rolls.is_none());
        }

        {
            let timestamp = &MassaTime::from(1677675988000); // 01/03/2023 14h06
            let result = manager.get_addr_vesting_at_time(&addr, timestamp);
            assert_eq!(
                result,
                VestingInfo {
                    min_balance: Some(Amount::from_str("150000").unwrap()),
                    max_rolls: Some(30),
                }
            )
        }

        {
            let timestamp = &MassaTime::from(1678193291000); // 07/03/2023 13h48
            let result = manager.get_addr_vesting_at_time(&addr, timestamp);
            assert_eq!(
                result,
                VestingInfo {
                    min_balance: Some(Amount::from_str("100000").unwrap()),
                    max_rolls: Some(50),
                }
            );
        }

        {
            let timestamp = &MassaTime::from(1734786585000); // 21/12/2024 14h09
            let result = manager.get_addr_vesting_at_time(&addr, timestamp);
            assert_eq!(
                result,
                VestingInfo {
                    min_balance: Some(Amount::from_str("80000").unwrap()),
                    max_rolls: None,
                }
            )
        }
    }

    #[test]
    fn test_load_initial_file() {
        {
            // Whitout file, error is returned
            let result = VestingManager::new(
                THREAD_COUNT,
                T0,
                *GENESIS_TIMESTAMP,
                PERIODS_PER_CYCLE,
                ROLL_PRICE,
                PathBuf::new(),
            );
            assert!(result.is_err());
            let err = result.err().unwrap();
            assert!(err
                .to_string()
                .contains("error loading initial vesting file"));
        }

        {
            // mock file
            let file = get_initials_vesting(false);
            let result = VestingManager::new(
                THREAD_COUNT,
                T0,
                *GENESIS_TIMESTAMP,
                PERIODS_PER_CYCLE,
                ROLL_PRICE,
                file.path().to_path_buf(),
            );
            assert!(result.is_ok());
        }
    }
}
