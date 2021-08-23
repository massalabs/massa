// Copyright (c) 2021 MASSA LABS <info@massa.net>

use models::{Address, Amount, Slot};
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;
use time::UTime;

use crate::tests::tools::{self, generate_ledger_file};

#[tokio::test]
#[serial]
async fn test_endorsement_check() {
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
    let mut priv_1 = crypto::generate_random_private_key();
    let mut pubkey_1 = crypto::derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1).unwrap();
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = crypto::generate_random_private_key();
        pubkey_1 = crypto::derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1).unwrap();
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = crypto::generate_random_private_key();
    let mut pubkey_2 = crypto::derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2).unwrap();
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = crypto::generate_random_private_key();
        pubkey_2 = crypto::derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2).unwrap();
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let ledger_file = generate_ledger_file(&HashMap::new());

    let roll_counts_file = tools::generate_default_roll_counts_file(vec![priv_1, priv_2]);

    let staking_file = tools::generate_staking_keys_file(&vec![]);
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
    cfg.genesis_timestamp = UTime::now(0).unwrap().saturating_add(300.into());
    cfg.endorsement_count = 1;

    tools::consensus_without_pool_test(
        cfg.clone(),
        None,
        async move |protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(Slot::new(1, 0), Slot::new(2, 0))
                .await
                .unwrap()
                .into_iter()
                .collect();

            let staker_a = draws.get(&Slot::new(1, 0)).unwrap().0;
            let staker_b = draws.get(&Slot::new(1, 0)).unwrap().1[0];
            let staker_c = draws.get(&Slot::new(1, 1)).unwrap().1[0];

            // todo : create an otherwise valid endorsement with another address, include it in valid block(1,0), assert it is not propagated
            // todo : create an otherwise valid endorsement at slot (1,1), include it in valid block(1,0), assert it is not propagated
            // todo : create an otherwise valid endorsement with index = 1, include it in valid block(1,0), assert it is not propagated
            // todo : create an otherwise valid endorsement with genesis 1 as endorsed block, include it in valid block(1,0), assert it is not propagated
            // todo : create a valid endorsement, include it in valid block(1,1), assert it is propagated

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
