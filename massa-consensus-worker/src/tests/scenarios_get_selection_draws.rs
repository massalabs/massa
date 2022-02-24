// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::{tools::*, ConsensusConfig};

use massa_models::{init_serialization_context, ledger_models::LedgerData, SerializationContext};
use massa_models::{Amount, Slot};
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
    init_serialization_context(SerializationContext::default());
    let mut cfg = ConsensusConfig {
        periods_per_cycle: 2,
        t0: 500.into(),
        delta_f0: 3,
        block_reward: Amount::default(),
        roll_price: Amount::from_raw(1000),
        operation_validity_periods: 100,
        genesis_timestamp: MassaTime::now().unwrap().saturating_add(300.into()),
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
    let initial_ledger_file = generate_ledger_file(&ledger);
    let initial_rolls_file = generate_default_roll_counts_file(vec![addr_1.private_key]);
    let staking_keys_file = generate_staking_keys_file(&[addr_2.private_key]);

    cfg.initial_ledger_path = initial_ledger_file.path().to_path_buf();
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();
    cfg.staking_keys_path = staking_keys_file.path().to_path_buf();
    consensus_without_pool_test(
        cfg.clone(),
        async move |protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let draws = consensus_command_sender
                .get_selection_draws(Slot::new(1, 0), Slot::new(2, 0))
                .await;
            assert!(draws.is_ok());

            // Too high end selection should return an error.
            let too_high_draws = consensus_command_sender
                .get_selection_draws(Slot::new(1, 0), Slot::new(200, 0))
                .await;
            assert!(too_high_draws.is_err());
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
