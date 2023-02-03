// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::protocol_test;
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_hash::Hash;
use massa_models::operation::OperationId;
use massa_models::prehash::PreHashSet;
use massa_models::secure_share::Id;
use massa_models::{block_id::BlockId, slot::Slot};
use massa_network_exports::{BlockInfoReply, NetworkCommand};
use massa_pool_exports::test_exports::MockPoolControllerMessage;
use massa_protocol_exports::tests::tools;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashSet;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_block_header_with_invalid_signature() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let mut block = tools::create_block(&creator_node.keypair);

            // 2. Change the id.
            block.content.header.id = BlockId::new(Hash::compute_from("invalid".as_bytes()));

            // 3. Send header to protocol.
            network_controller
                .send_header(creator_node.id, block.content.header.clone())
                .await;

            // The node is banned.
            tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

            // Check protocol does not send block to consensus.
            let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
                protocol_consensus_event_receiver.wait_command(
                    MassaTime::from_millis(1000),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlock { .. } => {
                            panic!("Protocol unexpectedly sent block.")
                        }
                        MockConsensusControllerMessage::RegisterBlockHeader { .. } => {
                            panic!("Protocol unexpectedly sent header.")
                        }
                        MockConsensusControllerMessage::MarkInvalidBlock { .. } => {
                            panic!("Protocol unexpectedly sent invalid block.")
                        }
                        _ => Some(()),
                    },
                );
                protocol_consensus_event_receiver
            })
            .await
            .unwrap();
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_operation_with_invalid_signature() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation
            let mut operation =
                tools::create_operation_with_expire_period(&creator_node.keypair, 1);

            // 2. Change the id
            operation.id = OperationId::new(Hash::compute_from("invalid".as_bytes()));

            // 3. Send block to protocol.
            network_controller
                .send_operations(creator_node.id, vec![operation])
                .await;

            // The node is banned.
            tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

            // Check protocol does not send operation to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                evt @ MockPoolControllerMessage::AddOperations { .. } => Some(evt),
                _ => None,
            });
            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_operation_with_size_bigger_than_max_block_size() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation
            let mut operation =
                tools::create_operation_with_expire_period(&creator_node.keypair, 1);

            // 2. Change the serialized data
            operation.serialized_data = vec![1; 500_001];

            // 3. Send block to protocol.
            network_controller
                .send_operations(creator_node.id, vec![operation])
                .await;

            // The node is banned.
            tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

            // Check protocol does not send operation to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                evt @ MockPoolControllerMessage::AddOperations { .. } => Some(evt),
                _ => None,
            });
            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_header_with_invalid_signature() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let to_ban_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create two operations.
            let op_1 = tools::create_operation_with_expire_period(&to_ban_node.keypair, 5);
            let op_2 = tools::create_operation_with_expire_period(&to_ban_node.keypair, 5);

            // 2. Add one op to the block.
            let block = tools::create_block_with_operations(
                &to_ban_node.keypair,
                Slot::new(1, 0),
                vec![op_1],
            );

            // 3. Send header to protocol.
            network_controller
                .send_header(to_ban_node.id, block.content.header.clone())
                .await;

            let mut protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
                protocol_consensus_event_receiver.wait_command(
                    MassaTime::from_millis(1000),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlockHeader { .. } => Some(()),
                        _ => panic!("unexpected protocol event"),
                    },
                );
                protocol_consensus_event_receiver
            })
            .await
            .unwrap();

            // send wishlist
            let protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .send_wishlist_delta(
                        vec![(block.id, Some(block.content.header))]
                            .into_iter()
                            .collect(),
                        PreHashSet::<BlockId>::default(),
                    )
                    .unwrap();
                protocol_command_sender
            })
            .await
            .unwrap();

            tools::assert_hash_asked_to_node(block.id, to_ban_node.id, &mut network_controller)
                .await;

            // Reply with the other operation.
            network_controller
                .send_block_info(
                    to_ban_node.id,
                    vec![(
                        block.id,
                        BlockInfoReply::Info(vec![op_2].into_iter().map(|op| op.id).collect()),
                    )],
                )
                .await;

            // The node is banned.
            tools::assert_banned_nodes(vec![to_ban_node.id], &mut network_controller).await;

            // Create another node.
            let not_banned = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .expect("Node not created.");

            // Create a valid block from the other node.
            let block = tools::create_block(&not_banned.keypair);

            // 3. Send header to protocol, via the banned node.
            network_controller
                .send_header(to_ban_node.id, block.content.header)
                .await;

            // Check protocol does not send block to consensus.
            let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
                protocol_consensus_event_receiver.wait_command(
                    MassaTime::from_millis(1000),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlock { .. } => {
                            panic!("Protocol unexpectedly sent block.")
                        }
                        MockConsensusControllerMessage::RegisterBlockHeader { .. } => {
                            panic!("Protocol unexpectedly sent header.")
                        }
                        MockConsensusControllerMessage::MarkInvalidBlock { .. } => {
                            panic!("Protocol unexpectedly sent invalid block.")
                        }
                        _ => Some(()),
                    },
                );
                protocol_consensus_event_receiver
            })
            .await
            .unwrap();
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_asks_for_block_from_banned_node_who_propagated_header() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            let ask_for_block_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
                _ => None,
            };

            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from node creator_node.
            let block = tools::create_block(&creator_node.keypair);

            // 2. Send header to protocol.
            network_controller
                .send_header(creator_node.id, block.content.header.clone())
                .await;

            // Check protocol sends header to consensus.
            let (protocol_consensus_event_receiver, received_hash) =
                tokio::task::spawn_blocking(move || {
                    let id = protocol_consensus_event_receiver
                        .wait_command(MassaTime::from_millis(1000), |command| match command {
                            MockConsensusControllerMessage::RegisterBlockHeader {
                                block_id,
                                header: _,
                            } => Some(block_id),
                            _ => panic!("unexpected protocol event"),
                        })
                        .unwrap();
                    (protocol_consensus_event_receiver, id)
                })
                .await
                .unwrap();

            // 3. Check that protocol sent the right header to consensus.
            let expected_hash = block.id;

            assert_eq!(expected_hash, received_hash);

            // 4. Get the node banned.
            // New keypair to avoid getting same block id
            let keypair = KeyPair::generate(0).unwrap();
            let mut block = tools::create_block(&keypair);
            block.content.header.id = BlockId::new(Hash::compute_from("invalid".as_bytes()));
            network_controller
                .send_header(creator_node.id, block.content.header.clone())
                .await;
            tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

            // 5. Ask for block.
            let protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .send_wishlist_delta(
                        vec![(expected_hash, Some(block.content.header.clone()))]
                            .into_iter()
                            .collect(),
                        PreHashSet::<BlockId>::default(),
                    )
                    .expect("Failed to ask for block.");
                protocol_command_sender
            })
            .await
            .unwrap();

            // 6. Make sure protocol did not ask for the block from the banned node.
            let got_more_commands = network_controller
                .wait_command(100.into(), ask_for_block_cmd_filter)
                .await;
            assert!(
                got_more_commands.is_none(),
                "unexpected command {:?}",
                got_more_commands
            );
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_send_blocks_when_asked_for_by_banned_node() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            let send_block_or_header_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::SendBlockInfo { .. } => Some(cmd),
                cmd @ NetworkCommand::SendBlockHeader { .. } => Some(cmd),
                _ => None,
            };

            let mut nodes = tools::create_and_connect_nodes(4, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Close one connection.
            network_controller.close_connection(nodes[2].id).await;

            // 2. Create a block coming from creator_node.
            let block = tools::create_block(&creator_node.keypair);

            let expected_hash = block.id;

            // 3. Get one node banned.
            let mut bad_block = tools::create_block(&nodes[1].keypair);
            bad_block.content.header.id = BlockId::new(Hash::compute_from("invalid".as_bytes()));
            network_controller
                .send_header(nodes[1].id, bad_block.content.header.clone())
                .await;
            tools::assert_banned_nodes(vec![nodes[1].id], &mut network_controller).await;

            // 4. Simulate two nodes asking for a block.
            for node in nodes.iter().take(2) {
                network_controller
                    .send_ask_for_block(node.id, vec![(expected_hash, Default::default())])
                    .await;
            }

            // 5. Check that protocol sends the non-banned node the full block.
            let mut expecting_block = HashSet::new();
            expecting_block.insert(nodes[0].id);
            loop {
                match network_controller
                    .wait_command(1000.into(), send_block_or_header_cmd_filter)
                    .await
                {
                    Some(NetworkCommand::SendBlockInfo { node, info: _ }) => {
                        //assert_eq!(expected_hash, block_id);
                        assert!(expecting_block.remove(&node));
                    }
                    Some(NetworkCommand::SendBlockHeader { .. }) => {
                        panic!("unexpected header sent");
                    }
                    None => {
                        if expecting_block.is_empty() {
                            break;
                        } else {
                            panic!("expecting a block to be sent");
                        }
                    }
                    _ => panic!("Unexpected network command."),
                }
            }

            // 7. Make sure protocol did not send block to the banned node.
            let got_more_commands = network_controller
                .wait_command(100.into(), send_block_or_header_cmd_filter)
                .await;
            assert!(got_more_commands.is_none());

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_bans_all_nodes_propagating_an_attack_attempt() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 4 nodes.
            let nodes = tools::create_and_connect_nodes(4, &mut network_controller).await;

            // Create a block coming from one node.
            let block = tools::create_block(&nodes[0].keypair);

            let expected_hash = block.id;

            // Propagate the block via 4 nodes.
            for (idx, creator_node) in nodes.iter().enumerate() {
                // Send block to protocol.
                network_controller
                    .send_header(creator_node.id, block.content.header.clone())
                    .await;

                let (old_protocol_consensus_event_receiver, optional_block_id) =
                    tokio::task::spawn_blocking(move || {
                        let id = protocol_consensus_event_receiver.wait_command(
                            MassaTime::from_millis(1000),
                            |command| match command {
                                MockConsensusControllerMessage::RegisterBlockHeader {
                                    block_id,
                                    header: _,
                                } => Some(block_id),
                                _ => panic!("unexpected protocol event"),
                            },
                        );
                        (protocol_consensus_event_receiver, id)
                    })
                    .await
                    .unwrap();
                protocol_consensus_event_receiver = old_protocol_consensus_event_receiver;
                // Check protocol sends header to consensus (only the 1st time: later, there is caching).
                if idx == 0 {
                    let received_hash = optional_block_id.unwrap();
                    // Check that protocol sent the right header to consensus.
                    assert_eq!(expected_hash, received_hash);
                } else {
                    assert!(optional_block_id.is_none(), "caching was ignored");
                }
            }

            // Have one node send that they don't know about the block.
            let _not_banned_nodes =
                tools::create_and_connect_nodes(1, &mut network_controller).await;
            // network_controller
            //     .send_block_not_found(not_banned_nodes[0].id, expected_hash)
            //     .await;

            // wait for things to settle
            tokio::time::sleep(Duration::from_millis(250)).await;

            // Simulate consensus notifying an attack attempt.
            let protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .notify_block_attack(expected_hash)
                    .expect("Failed to ask for block.");
                protocol_command_sender
            })
            .await
            .unwrap();

            // Make sure all initial nodes are banned.
            let node_ids = nodes.into_iter().map(|node_info| node_info.id).collect();
            tools::assert_banned_nodes(node_ids, &mut network_controller).await;

            // Make sure protocol did not ban the node that did not know about the block.
            let ban_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::NodeBanByIds { .. } => Some(cmd),
                _ => None,
            };
            let got_more_commands = network_controller
                .wait_command(100.into(), ban_cmd_filter)
                .await;
            assert!(
                got_more_commands.is_none(),
                "unexpected command {:?}",
                got_more_commands
            );

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_removes_banned_node_on_disconnection() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // Get the node banned.
            let mut block = tools::create_block(&creator_node.keypair);
            block.content.header.id = BlockId::new(Hash::compute_from("invalid".as_bytes()));
            network_controller
                .send_header(creator_node.id, block.content.header)
                .await;
            tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

            // Close the connection.
            network_controller.close_connection(creator_node.id).await;

            // Re-connect the node.
            network_controller.new_connection(creator_node.id).await;

            // The node is not banned anymore.
            let block = tools::create_block(&creator_node.keypair);
            network_controller
                .send_header(creator_node.id, block.content.header.clone())
                .await;

            // Check protocol sends header to consensus.
            let (protocol_consensus_event_receiver, received_hash) =
                tokio::task::spawn_blocking(move || {
                    let id = protocol_consensus_event_receiver
                        .wait_command(MassaTime::from_millis(1000), |command| match command {
                            MockConsensusControllerMessage::RegisterBlockHeader {
                                block_id,
                                header: _,
                            } => Some(block_id),
                            _ => panic!("unexpected protocol event"),
                        })
                        .unwrap();
                    (protocol_consensus_event_receiver, id)
                })
                .await
                .unwrap();

            // Check that protocol sent the right header to consensus.
            let expected_hash = block.id;
            assert_eq!(expected_hash, received_hash);
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}
