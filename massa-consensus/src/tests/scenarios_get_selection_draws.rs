// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::tests::tools::{self, generate_ledger_file};
use massa_models::ledger::LedgerData;
use massa_models::{Address, Amount, Slot};
use massa_signature::{derive_public_key, generate_random_private_key};
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
    let thread_count = 2;
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let mut priv_1 = generate_random_private_key();
    let mut pubkey_1 = derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1);
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = generate_random_private_key();
        pubkey_1 = derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1);
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = generate_random_private_key();
    let mut pubkey_2 = derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2);
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = generate_random_private_key();
        pubkey_2 = derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2);
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let mut ledger = HashMap::new();
    ledger.insert(
        address_2,
        LedgerData::new(Amount::from_str("10000").unwrap()),
    );
    let ledger_file = generate_ledger_file(&ledger);

    let roll_counts_file = tools::generate_default_roll_counts_file(vec![priv_1]);

    let staking_file = tools::generate_staking_keys_file(&vec![priv_2]);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.periods_per_cycle = 2;
    cfg.pos_lookback_cycles = 2;
    cfg.pos_lock_cycles = 1;
    cfg.t0 = 500.into();
    cfg.delta_f0 = 3;
    cfg.disable_block_creation = true;
    cfg.thread_count = thread_count;
    cfg.block_reward = Amount::default();
    cfg.roll_price = Amount::from_str("1000").unwrap();
    cfg.operation_validity_periods = 100;
    cfg.genesis_timestamp = MassaTime::now().unwrap().saturating_add(300.into());

    tools::consensus_without_pool_test(
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
