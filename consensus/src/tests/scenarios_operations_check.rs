use std::{collections::HashMap, usize};

use models::{Address, Slot};

use crate::{
    ledger::LedgerData,
    start_consensus_controller,
    tests::{
        mock_pool_controller::{MockPoolController, PoolCommandSink},
        mock_protocol_controller::MockProtocolController,
        tools::{
            self, create_block_with_operations, create_transaction, generate_ledger_file,
            propagate_block,
        },
    },
};

#[tokio::test]
async fn test_operations_check() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let thread_count = 2;

    let private_key_1 = crypto::generate_random_private_key();
    let public_key_1 = crypto::derive_public_key(&private_key_1);
    let address_1 = Address::from_public_key(&public_key_1).unwrap();
    let thread_1 = address_1.get_thread(thread_count);
    let mut private_key_2 = crypto::generate_random_private_key();
    let mut public_key_2 = crypto::derive_public_key(&private_key_2);
    let mut address_2 = Address::from_public_key(&public_key_2).unwrap();
    let mut thread_2 = address_2.get_thread(thread_count);

    // make sure that both threads are different
    while thread_1 == thread_2 {
        private_key_2 = crypto::generate_random_private_key();
        public_key_2 = crypto::derive_public_key(&private_key_2);
        address_2 = Address::from_public_key(&public_key_2).unwrap();
        thread_2 = address_2.get_thread(thread_count);
    }

    let mut ledger = HashMap::new();
    ledger.insert(address_1, LedgerData { balance: 5 });

    let ledger_file = generate_ledger_file(&ledger);
    let (mut cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;
    cfg.block_reward = 1;
    cfg.thread_count = thread_count;
    cfg.operation_validity_periods = 10;
    cfg.nodes = vec![(public_key_1, private_key_1)];
    cfg.genesis_timestamp = cfg.genesis_timestamp.saturating_sub(2000.into());

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());
    let (pool_controller, pool_command_sender) =
        MockPoolController::new(serialization_context.clone());
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let genesis_ids = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    let operation_1 = create_transaction(
        private_key_1,
        public_key_1,
        address_2,
        5,
        &serialization_context,
        5,
    );
    let operation_2 = create_transaction(
        private_key_2,
        public_key_2,
        address_1,
        10,
        &serialization_context,
        8,
    );
    let creator = (public_key_1, private_key_1);

    let slot_a = Slot::new(1, thread_1);
    let parents_a = genesis_ids.clone();

    let slot_b = Slot::new(1, thread_2);

    let slot_c = Slot::new(2, thread_1);

    // receiving block A with ok validity period
    let (id_a, block_a, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        slot_a,
        &parents_a,
        creator,
        vec![operation_1.clone()],
    );
    propagate_block(
        &serialization_context,
        &mut protocol_controller,
        block_a,
        true,
    )
    .await;

    // todo assert address 1 has 1 coin at block A (see #269)

    let mut parents_b = genesis_ids.clone();
    parents_b[thread_1 as usize] = id_a;

    // receive block b with invalid operation (not enough coins)
    let (_, block_2b, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        slot_b,
        &parents_b,
        creator,
        vec![operation_2],
    );
    propagate_block(
        &serialization_context,
        &mut protocol_controller,
        block_2b,
        false,
    )
    .await;

    // receive empty block b
    let (id_b, block_b, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        slot_b,
        &parents_b,
        creator,
        vec![],
    );
    propagate_block(
        &serialization_context,
        &mut protocol_controller,
        block_b,
        true,
    )
    .await;

    // todo assert address 2 has 5 coins at block B (see #269)
    // (block A taken in account in thread 2)

    let mut parents_c = parents_b.clone();
    parents_c[thread_2 as usize] = id_b;

    // receive block with reused operation
    let (_, block_1c, _) = create_block_with_operations(
        &cfg,
        &serialization_context,
        slot_c,
        &parents_c,
        creator,
        vec![operation_1.clone()],
    );
    propagate_block(
        &serialization_context,
        &mut protocol_controller,
        block_1c,
        false,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
