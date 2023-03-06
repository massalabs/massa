// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use std::collections::HashSet;

use super::tools::protocol_test_with_storage;
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_hash::Hash;
use massa_models::block_id::BlockId;
use massa_models::prehash::PreHashMap;
use massa_models::{self, address::Address, slot::Slot};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NetworkCommand};
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::ProtocolConfig;
use massa_time::MassaTime;
use serial_test::serial;

lazy_static::lazy_static! {
    pub static ref CUSTOM_PROTOCOL_CONFIG: ProtocolConfig = {
        let mut protocol_config = *tools::PROTOCOL_CONFIG;

        // Set max_node_known_blocks_size to zero.
        protocol_config.max_node_known_blocks_size = 0;

        protocol_config
    };
}

#[tokio::test]
#[serial]
async fn test_noting_block_does_not_panic_with_zero_max_node_known_blocks_size() {
    let protocol_config = &CUSTOM_PROTOCOL_CONFIG;

    protocol_test_with_storage(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver,
                    mut storage| {
            // Create 2 node.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.get_public_key());
            let thread = address.get_thread(2);

            let operation = tools::create_operation_with_expire_period(&nodes[0].keypair, 1);

            let block = tools::create_block_with_operations(
                &nodes[0].keypair,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            // Add block to storage.
            storage.store_block(block.clone());

            // Add Operation to storage.
            storage.store_operations(vec![operation.clone()]);

            // Have the second node ask for the operations.
            let needed_ops = vec![operation.id].into_iter().collect();
            network_controller
                .send_ask_for_block(
                    nodes[1].id,
                    vec![(block.id, AskForBlocksInfo::Operations(needed_ops))],
                )
                .await;

            // Check that protocol processes the message without panicking.
            let cmd_filter = |cmd| match cmd {
                NetworkCommand::SendBlockInfo { node, info } => {
                    assert_eq!(node, nodes[1].id);
                    Some(info)
                }
                _ => None,
            };

            let (block_id, info) = network_controller
                .wait_command(100.into(), cmd_filter)
                .await
                .unwrap()
                .pop()
                .unwrap();

            assert_eq!(block_id, block.id);
            if let BlockInfoReply::Operations(mut ops) = info {
                assert_eq!(ops.pop().unwrap().id, operation.id);
                assert!(ops.is_empty());
            } else {
                panic!("Unexpected block info.");
            }

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

// This test will try to ask for a block that we already received as a separate header before.
// Tests steps:
// 1. Create 2 nodes.
// 2. Create 3 block for the same slot.
// 3. Simulate that the consensus as a dependency on the third block.
// 4. Verify that protocol asks the header and all the others steps even if we already received the header during propagation.
// 5. Verify that we send the block to consensus.
#[tokio::test]
#[serial]
async fn test_ask_info_already_received_header() {
    let protocol_config = &CUSTOM_PROTOCOL_CONFIG;

    protocol_test_with_storage(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut consensus_event_receiver,
                    pool_event_receiver,
                    mut storage| {
            // Create 2 node.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.get_public_key());
            let thread = address.get_thread(2);
            let mut last_block_id = BlockId(Hash::compute_from(&[0u8; 32]));
            let mut last_header = None;
            let mut last_operations = None;
            for i in 0..3 {
                // Create a block for that slot
                let operation = tools::create_operation_with_expire_period(&nodes[0].keypair, i);

                let block = tools::create_block_with_operations(
                    &nodes[0].keypair,
                    Slot::new(1, thread),
                    vec![operation.clone()],
                );

                last_block_id = block.id;
                last_header = Some(block.content.header.clone());
                last_operations = Some(vec![operation.clone()]);

                // Add block to storage.
                storage.store_block(block.clone());

                // Add Operation to storage.
                storage.store_operations(vec![operation.clone()]);

                // Node 0 propagates the header to ourself.
                network_controller
                    .send_header(nodes[0].id, block.content.header.clone())
                    .await;

                // Verify that we notify consensus that we received a block header
                let consensus_event_receiver_2 = tokio::task::spawn_blocking(move || {
                    consensus_event_receiver.wait_command(MassaTime::from_millis(10000), |event| {
                        match event {
                            MockConsensusControllerMessage::RegisterBlockHeader { .. } => Some(()),
                            _ => panic!("Unexpected consensus event"),
                        }
                    });
                    consensus_event_receiver
                })
                .await
                .unwrap();
                consensus_event_receiver = consensus_event_receiver_2;
            }

            // Let's imagine a block arrive referencing the last block we received as parent and so the consensus will ask for it in the wishlist
            let mut add = PreHashMap::default();
            add.insert(last_block_id, None);
            let protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .send_wishlist_delta(add, HashSet::default())
                    .unwrap();
                protocol_command_sender
            })
            .await
            .unwrap();
            // Receiving the ask through the network
            network_controller
                .wait_command(MassaTime::from_millis(10000), |event| match event {
                    NetworkCommand::AskForBlocks { list } => {
                        assert_eq!(list.len(), 1);
                        for elem in list {
                            for (block_id, info) in elem.1 {
                                assert_eq!(block_id, last_block_id);
                                match info {
                                    AskForBlocksInfo::Header => {}
                                    _ => panic!("Unexpected block info"),
                                }
                            }
                        }
                        Some(())
                    }
                    _ => panic!("Unexpected network event"),
                })
                .await
                .unwrap();

            //Answering the ask
            network_controller
                .send_block_info(
                    nodes[0].id,
                    vec![(last_block_id, BlockInfoReply::Header(last_header.unwrap()))],
                )
                .await;

            // Receiving the ask for the second step through the network
            network_controller
                .wait_command(MassaTime::from_millis(10000), |event| match event {
                    NetworkCommand::AskForBlocks { list } => {
                        assert_eq!(list.len(), 1);
                        for elem in list {
                            for (block_id, info) in elem.1 {
                                assert_eq!(block_id, last_block_id);
                                match info {
                                    AskForBlocksInfo::Info => {}
                                    _ => panic!("Unexpected block info"),
                                }
                            }
                        }
                        Some(())
                    }
                    _ => panic!("Unexpected network event"),
                })
                .await
                .unwrap();

            //Answering the ask
            network_controller
                .send_block_info(
                    nodes[0].id,
                    vec![(
                        last_block_id,
                        BlockInfoReply::Info(
                            last_operations
                                .clone()
                                .unwrap()
                                .iter()
                                .map(|op| op.id)
                                .collect(),
                        ),
                    )],
                )
                .await;

            // Receiving the ask for the third step through the network
            network_controller
                .wait_command(MassaTime::from_millis(10000), |event| match event {
                    NetworkCommand::AskForBlocks { list } => {
                        assert_eq!(list.len(), 1);
                        for elem in list {
                            for (block_id, info) in elem.1 {
                                assert_eq!(block_id, last_block_id);
                                match info {
                                    AskForBlocksInfo::Operations(_) => {}
                                    _ => panic!("Unexpected block info"),
                                }
                            }
                        }
                        Some(())
                    }
                    _ => panic!("Unexpected network event"),
                })
                .await
                .unwrap();

            //Answering the ask
            network_controller
                .send_block_info(
                    nodes[0].id,
                    vec![(
                        last_block_id,
                        BlockInfoReply::Operations(last_operations.unwrap()),
                    )],
                )
                .await;

            // Verify that we notify consensus that we received the block header asked block header
            let consensus_event_receiver = tokio::task::spawn_blocking(move || {
                consensus_event_receiver.wait_command(MassaTime::from_millis(10000), |event| {
                    match event {
                        MockConsensusControllerMessage::RegisterBlock { .. } => Some(()),
                        _ => panic!("Unexpected consensus event"),
                    }
                });
                consensus_event_receiver
            })
            .await
            .unwrap();
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
    .await;
}
