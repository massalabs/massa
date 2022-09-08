// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::test_exports::{
    generate_default_roll_counts_file, generate_ledger_file, generate_staking_keys_file,
};
use massa_consensus_exports::ConsensusConfig;

use massa_models::ledger_models::LedgerData;
use massa_models::{amount::Amount, slot::Slot};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_get_selection_draws_high_end_slot() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */
    let mut cfg = ConsensusConfig {
        periods_per_cycle: 2,
        t0: 500.into(),
        delta_f0: 3,
        operation_validity_periods: 100,
        genesis_timestamp: MassaTime::now(0).unwrap().saturating_add(300.into()),
        ..Default::default()
    };
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let addr_1 = random_address_on_thread(0, cfg.thread_count);
    let addr_2 = random_address_on_thread(0, cfg.thread_count);

    let mut ledger = HashMap::new();
    ledger.insert(
        addr_2.address,
        LedgerData::new(Amount::from_str("10000").unwrap()),
    );

    consensus_without_pool_test(
        cfg.clone(),
        async move |protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    selector_controller| {
            let draws = selector_controller.get_selection(Slot::new(1, 0));
            assert!(draws.is_ok());

            // Too high end selection should return an error.
            let too_high_draws = selector_controller.get_selection(Slot::new(200, 0));
            assert!(too_high_draws.is_err());
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
                selector_controller,
            )
        },
    )
    .await;
}
