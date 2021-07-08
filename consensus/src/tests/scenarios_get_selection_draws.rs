use std::collections::{HashMap, HashSet};

use models::{Address, AddressRollState, Slot};
use pool::{PoolCommand, PoolCommandSender};
use serial_test::serial;
use time::UTime;

use crate::{
    start_consensus_controller,
    tests::{
        mock_pool_controller::{MockPoolController, PoolCommandSink},
        mock_protocol_controller::MockProtocolController,
        tools::{
            self, create_and_test_block, create_block_with_operations, create_roll_buy,
            create_roll_sell, create_transaction, generate_ledger_file, get_creator_for_draw,
            propagate_block, wait_pool_slot,
        },
    },
    LedgerData, LedgerExport,
};

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

    let mut ledger = HashMap::new();
    ledger.insert(address_2, LedgerData { balance: 10_000 });
    let ledger_file = generate_ledger_file(&ledger);

    let roll_counts_file = tools::generate_default_roll_counts_file(vec![priv_1]);

    let staking_file = tools::generate_staking_keys_file(&vec![priv_2]);
    let mut cfg = tools::default_consensus_config(
        1,
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
    cfg.block_reward = 0;
    cfg.roll_price = 1000;
    cfg.operation_validity_periods = 100;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();

    cfg.genesis_timestamp = UTime::now(0).unwrap().saturating_add(300.into());
    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let draws = consensus_command_sender
        .get_selection_draws(Slot::new(1, 0), Slot::new(2, 0))
        .await;
    assert!(draws.is_ok());

    // Too high end selection should return an error.
    let too_high_draws = consensus_command_sender
        .get_selection_draws(Slot::new(1, 0), Slot::new(200, 0))
        .await;
    assert!(too_high_draws.is_err());

    // stop controller while ignoring all commands
    let pool_sink = PoolCommandSink::new(pool_controller).await;
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
