// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::protocol_test;
use massa_models::prehash::{Map, Set};
use massa_models::{BlockId, Slot};
use massa_network::NetworkCommand;
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::ProtocolEvent;
use massa_protocol_exports::ProtocolPoolEvent;
use serial_test::serial;
use std::collections::HashSet;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_block_with_invalid_signature() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let mut block = tools::create_block(&creator_node.private_key, &creator_node.id.0);

            // 2. Change the slot.
            block.header.content.slot = Slot::new(1, 1);

            // 3. Send block to protocol.
            network_controller.send_block(creator_node.id, block).await;

            // The node is banned.
            tools::assert_banned_node(creator_node.id, &mut network_controller).await;

            // Check protocol does not send block to consensus.
            match tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlock { .. } => Some(evt),
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                }
            })
            .await
            {
                None => {}
                _ => panic!("Protocol unexpectedly sent block or header."),
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

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_operation_with_invalid_signature() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation
            let mut operation =
                tools::create_operation_with_expire_period(&creator_node.private_key, 1);

            // 2. Change the validity period.
            operation.content.expire_period += 10;

            // 3. Send block to protocol.
            network_controller
                .send_operations(creator_node.id, vec![operation])
                .await;

            // The node is banned.
            tools::assert_banned_node(creator_node.id, &mut network_controller).await;

            // Check protocol does not send operation to pool.
            match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                None => {}
                _ => panic!("Protocol unexpectedly sent operation."),
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

#[tokio::test]
#[serial]
async fn test_protocol_bans_node_sending_header_with_invalid_signature() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let to_ban_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let mut block = tools::create_block(&to_ban_node.private_key, &to_ban_node.id.0);

            // 2. Change the slot.
            block.header.content.slot = Slot::new(1, 1);

            // 3. Send header to protocol.
            network_controller
                .send_header(to_ban_node.id, block.header)
                .await;

            // The node is banned.
            tools::assert_banned_node(to_ban_node.id, &mut network_controller).await;

            // Check protocol does not send block to consensus.
            match tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlock { .. } => Some(evt),
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                }
            })
            .await
            {
                None => {}
                _ => panic!("Protocol unexpectedly sent block or header."),
            }

            // Create another node.
            let not_banned = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .expect("Node not created.");

            // Create a valid block from the other node.
            let block = tools::create_block(&not_banned.private_key, &not_banned.id.0);

            // 3. Send header to protocol, via the banned node.
            network_controller
                .send_header(to_ban_node.id, block.header)
                .await;

            // Check protocol does not send block to consensus.
            match tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlock { .. } => Some(evt),
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                }
            })
            .await
            {
                None => {}
                _ => panic!("Protocol unexpectedly sent header coming from banned node."),
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

#[tokio::test]
#[serial]
async fn test_protocol_does_not_asks_for_block_from_banned_node_who_propagated_header() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let ask_for_block_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
                _ => None,
            };

            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from node creator_node.
            let block = tools::create_block(&creator_node.private_key, &creator_node.id.0);

            // 2. Send header to protocol.
            network_controller
                .send_header(creator_node.id, block.header.clone())
                .await;

            // Check protocol sends header to consensus.
            let received_hash =
                match tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                    match evt {
                        evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                        _ => None,
                    }
                })
                .await
                {
                    Some(ProtocolEvent::ReceivedBlockHeader { block_id, .. }) => block_id,
                    _ => panic!("Unexpected or no protocol event."),
                };

            // 3. Check that protocol sent the right header to consensus.
            let expected_hash = block
                .header
                .compute_block_id()
                .expect("Failed to compute hash.");
            assert_eq!(expected_hash, received_hash);

            // 4. Get the node banned.
            let mut block = tools::create_block(&creator_node.private_key, &creator_node.id.0);
            block.header.content.slot = Slot::new(1, 1);
            network_controller
                .send_header(creator_node.id, block.header)
                .await;
            tools::assert_banned_node(creator_node.id, &mut network_controller).await;

            // 5. Ask for block.
            protocol_command_sender
                .send_wishlist_delta(
                    vec![expected_hash].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .expect("Failed to ask for block.");

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
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_send_blocks_when_asked_for_by_banned_node() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let send_block_or_header_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::SendBlock { .. } => Some(cmd),
                cmd @ NetworkCommand::SendBlockHeader { .. } => Some(cmd),
                _ => None,
            };

            let mut nodes = tools::create_and_connect_nodes(4, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Close one connection.
            network_controller.close_connection(nodes[2].id).await;

            // 2. Create a block coming from creator_node.
            let block = tools::create_block(&creator_node.private_key, &creator_node.id.0);

            let expected_hash = block
                .header
                .compute_block_id()
                .expect("Failed to compute hash.");

            // 3. Simulate two nodes asking for a block.
            for node in nodes.iter().take(2) {
                network_controller
                    .send_ask_for_block(node.id, vec![expected_hash])
                    .await;

                // Check protocol sends get block event to consensus.
                let received_hash = match tools::wait_protocol_event(
                    &mut protocol_event_receiver,
                    1000.into(),
                    |evt| match evt {
                        evt @ ProtocolEvent::GetBlocks(..) => Some(evt),
                        _ => None,
                    },
                )
                .await
                {
                    Some(ProtocolEvent::GetBlocks(mut list)) => {
                        list.pop().expect("Received empty list of hashes.")
                    }
                    _ => panic!("Unexpected or no protocol event."),
                };

                // Check that protocol sent the right hash to consensus.
                assert_eq!(expected_hash, received_hash);
            }

            // Get one node banned.
            let mut bad_block = tools::create_block(&nodes[1].private_key, &nodes[1].id.0);
            bad_block.header.content.slot = Slot::new(1, 1);
            network_controller
                .send_header(nodes[1].id, bad_block.header.clone())
                .await;
            tools::assert_banned_node(nodes[1].id, &mut network_controller).await;

            // 4. Simulate consensus sending block.
            let mut results = Map::default();
            results.insert(expected_hash, Some((block, None, None)));
            protocol_command_sender
                .send_get_blocks_results(results)
                .await
                .expect("Failed to send get block results");

            // 5. Check that protocol sends the non-banned node the full block.
            let mut expecting_block = HashSet::new();
            expecting_block.insert(nodes[0].id);
            loop {
                match network_controller
                    .wait_command(1000.into(), send_block_or_header_cmd_filter)
                    .await
                {
                    Some(NetworkCommand::SendBlock { node, block }) => {
                        let hash = block
                            .header
                            .compute_block_id()
                            .expect("Failed to compute hash.");
                        assert_eq!(expected_hash, hash);
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
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_bans_all_nodes_propagating_an_attack_attempt() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 4 nodes.
            let nodes = tools::create_and_connect_nodes(4, &mut network_controller).await;

            // Create a block coming from one node.
            let block = tools::create_block(&nodes[0].private_key, &nodes[0].id.0);

            let expected_hash = block
                .header
                .compute_block_id()
                .expect("Failed to compute hash.");

            // Propagate the block via 4 nodes.
            for (idx, creator_node) in nodes.iter().enumerate() {
                // Send block to protocol.
                network_controller
                    .send_header(creator_node.id, block.header.clone())
                    .await;

                // Check protocol sends header to consensus (only the 1st time: later, there is caching).
                if idx == 0 {
                    let received_hash = match tools::wait_protocol_event(
                        &mut protocol_event_receiver,
                        1000.into(),
                        |evt| match evt {
                            evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                            _ => None,
                        },
                    )
                    .await
                    {
                        Some(ProtocolEvent::ReceivedBlockHeader { block_id, .. }) => block_id,
                        Some(evt) => panic!("Unexpected protocol event {:?}", evt),
                        None => panic!("no protocol event"),
                    };
                    // Check that protocol sent the right header to consensus.
                    assert_eq!(expected_hash, received_hash);
                } else {
                    assert!(
                        tools::wait_protocol_event(
                            &mut protocol_event_receiver,
                            150.into(),
                            |evt| match evt {
                                evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                                _ => None,
                            },
                        )
                        .await
                        .is_none(),
                        "caching was ignored"
                    );
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
            protocol_command_sender
                .notify_block_attack(expected_hash)
                .await
                .expect("Failed to ask for block.");

            // Make sure all initial nodes are banned.
            let node_ids = nodes.into_iter().map(|node_info| node_info.id).collect();
            tools::assert_banned_nodes(node_ids, &mut network_controller).await;

            // Make sure protocol did not ban the node that did not know about the block.
            let ban_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::Ban { .. } => Some(cmd),
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
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_removes_banned_node_on_disconnection() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // Get the node banned.
            let mut block = tools::create_block(&creator_node.private_key, &creator_node.id.0);
            block.header.content.slot = Slot::new(1, 1);
            network_controller
                .send_header(creator_node.id, block.header)
                .await;
            tools::assert_banned_node(creator_node.id, &mut network_controller).await;

            // Close the connection.
            network_controller.close_connection(creator_node.id).await;

            // Re-connect the node.
            network_controller.new_connection(creator_node.id).await;

            // The node is not banned anymore.
            let block = tools::create_block(&creator_node.private_key, &creator_node.id.0);
            network_controller
                .send_header(creator_node.id, block.header.clone())
                .await;

            // Check protocol sends header to consensus.
            let received_hash =
                match tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                    match evt {
                        evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                        _ => None,
                    }
                })
                .await
                {
                    Some(ProtocolEvent::ReceivedBlockHeader { block_id, .. }) => block_id,
                    _ => panic!("Unexpected or no protocol event."),
                };

            // Check that protocol sent the right header to consensus.
            let expected_hash = block
                .header
                .compute_block_id()
                .expect("Failed to compute hash.");
            assert_eq!(expected_hash, received_hash);
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
