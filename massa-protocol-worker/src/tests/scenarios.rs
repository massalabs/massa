// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test;
use massa_models::prehash::{Map, Set};
use massa_models::signed::Signable;
use massa_models::BlockId;
use massa_network::NetworkCommand;
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::{
    tests::tools::{create_and_connect_nodes, create_block, wait_protocol_event},
    ProtocolEvent,
};
use serial_test::serial;
use std::collections::HashSet;

#[tokio::test]
#[serial]
async fn test_protocol_asks_for_block_from_node_who_propagated_header() {
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
            let mut nodes = create_and_connect_nodes(3, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Close one connection.
            network_controller.close_connection(nodes[0].id).await;

            // 2. Create a block coming from node creator_node.
            let block = create_block(&creator_node.private_key, &creator_node.id.0);

            // 3. Send header to protocol.
            network_controller
                .send_header(creator_node.id, block.header.clone())
                .await;

            // Check protocol sends header to consensus.
            let received_hash = match wait_protocol_event(
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
                _ => panic!("Unexpected or no protocol event."),
            };

            // 4. Check that protocol sent the right header to consensus.
            let expected_hash = block.header.content.compute_id().unwrap();
            assert_eq!(expected_hash, received_hash);

            // 5. Ask for block.
            protocol_command_sender
                .send_wishlist_delta(
                    vec![expected_hash].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .expect("Failed to ask for block.");

            // 6. Check that protocol asks the node for the full block.
            match network_controller
                .wait_command(1000.into(), ask_for_block_cmd_filter)
                .await
                .expect("Protocol didn't send network command.")
            {
                NetworkCommand::AskForBlocks { list } => {
                    assert!(list.get(&creator_node.id).unwrap().contains(&expected_hash));
                }
                _ => panic!("Unexpected network command."),
            };

            // 7. Make sure protocol did not ask for the block again.
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
async fn test_protocol_sends_blocks_when_asked_for() {
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

            let mut nodes = create_and_connect_nodes(4, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Close one connection.
            network_controller.close_connection(nodes[2].id).await;

            // 2. Create a block coming from creator_node.
            let block = create_block(&creator_node.private_key, &creator_node.id.0);

            let expected_hash = block.header.content.compute_id().unwrap();

            // 3. Simulate two nodes asking for a block.
            for node in nodes.iter().take(2) {
                network_controller
                    .send_ask_for_block(node.id, vec![expected_hash])
                    .await;

                // Check protocol sends get block event to consensus.
                let received_hash =
                    match wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                        match evt {
                            evt @ ProtocolEvent::GetBlocks(..) => Some(evt),
                            _ => None,
                        }
                    })
                    .await
                    {
                        Some(ProtocolEvent::GetBlocks(mut list)) => {
                            list.pop().expect("Empty list of hashes.")
                        }
                        _ => panic!("Unexpected or no protocol event."),
                    };

                // Check that protocol sent the right hash to consensus.
                assert_eq!(expected_hash, received_hash);
            }

            // 4. Simulate consensus sending block.
            let mut results = Map::default();
            results.insert(expected_hash, Some((block, None, None)));
            protocol_command_sender
                .send_get_blocks_results(results)
                .await
                .expect("Failed to send get block results");

            // 5. Check that protocol sends the nodes the full block.
            let mut expecting_block = HashSet::new();
            expecting_block.insert(nodes[0].id);
            expecting_block.insert(nodes[1].id);
            loop {
                match network_controller
                    .wait_command(1000.into(), send_block_or_header_cmd_filter)
                    .await
                {
                    Some(NetworkCommand::SendBlock { node, block }) => {
                        let hash = block.header.content.compute_id().unwrap();
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

            // 7. Make sure protocol did not send block or header to other nodes.
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
async fn test_protocol_propagates_block_to_node_who_asked_for_it_and_only_header_to_others() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 4 nodes.
            let nodes = create_and_connect_nodes(4, &mut network_controller).await;
            let (node_a, node_b, node_c, node_d) = (
                nodes[0].clone(),
                nodes[1].clone(),
                nodes[2].clone(),
                nodes[3].clone(),
            );

            let creator_node = node_a.clone();

            // 1. Close one connection.
            network_controller.close_connection(node_d.id).await;

            // 2. Create a block coming from one node.
            let ref_block = create_block(&creator_node.private_key, &creator_node.id.0);

            // 3. Send header to protocol.
            network_controller
                .send_header(creator_node.id, ref_block.header.clone())
                .await;

            // node[1] asks for that block

            // Check protocol sends header to consensus.
            let (ref_hash, _) = match wait_protocol_event(
                &mut protocol_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolEvent::ReceivedBlockHeader { block_id, header }) => (block_id, header),
                _ => panic!("Unexpected or no protocol event."),
            };

            network_controller
                .send_ask_for_block(node_b.id, vec![ref_hash])
                .await;

            match wait_protocol_event(&mut protocol_event_receiver, 200.into(), |evt| match evt {
                evt @ ProtocolEvent::GetBlocks(..) => Some(evt),
                _ => None,
            })
            .await
            {
                Some(ProtocolEvent::GetBlocks(mut list)) => {
                    assert_eq!(list.pop().expect("Empty list of hashes."), ref_hash)
                }
                _ => panic!("timeout reached while sending get block"),
            }

            // 5. Propagate header.
            let op_ids = ref_block
                .operations
                .iter()
                .map(|op| op.content.compute_id().unwrap())
                .collect();
            let endo_ids = ref_block
                .header
                .content
                .endorsements
                .iter()
                .map(|endo| endo.content.compute_id().unwrap())
                .collect();
            protocol_command_sender
                .integrated_block(ref_hash, ref_block, op_ids, endo_ids)
                .await
                .expect("Failed to ask for block.");

            // 6. Check that protocol propagates the header to the right nodes.
            // node_a created the block and should receive nothing
            // node_b asked for the block and should receive the full block
            // node_c did nothing, it should receive the header
            // node_d was disconnected, so nothing should be send to it
            let mut expected_headers = HashSet::new();
            expected_headers.insert(node_c.id);

            let mut expected_full_blocks = HashSet::new();
            expected_full_blocks.insert(node_b.id);

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendBlockHeader { .. } => Some(cmd),
                        cmd @ NetworkCommand::SendBlock { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendBlockHeader { node, header }) => {
                        assert!(expected_headers.remove(&node));
                        let sent_header_hash = header.content.compute_id().unwrap();
                        assert_eq!(sent_header_hash, ref_hash);
                    }
                    Some(NetworkCommand::SendBlock { node, block }) => {
                        assert!(expected_full_blocks.remove(&node));
                        let sent_header_hash = block.header.content.compute_id().unwrap();
                        assert_eq!(sent_header_hash, ref_hash);
                    }
                    _ => panic!("Unexpected or no network command."),
                };

                if expected_headers.is_empty() && expected_full_blocks.is_empty() {
                    break;
                }
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
async fn test_protocol_sends_full_blocks_it_receives_to_consensus() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;

    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let block = create_block(&creator_node.private_key, &creator_node.id.0);

            let expected_hash = block.header.content.compute_id().unwrap();

            // 3. Send block to protocol.
            network_controller.send_block(creator_node.id, block).await;

            // Check protocol sends block to consensus.
            let hash =
                match wait_protocol_event(
                    &mut protocol_event_receiver,
                    1000.into(),
                    |evt| match evt {
                        evt @ ProtocolEvent::ReceivedBlock { .. } => Some(evt),
                        _ => None,
                    },
                )
                .await
                {
                    Some(ProtocolEvent::ReceivedBlock { block_id, .. }) => block_id,
                    _ => panic!("Unexpected or no protocol event."),
                };
            assert_eq!(expected_hash, hash);

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
async fn test_protocol_block_not_found() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let block = create_block(&creator_node.private_key, &creator_node.id.0);

            let expected_hash = block.header.content.compute_id().unwrap();

            // 3. Ask block to protocol.
            network_controller
                .send_ask_for_block(creator_node.id, vec![expected_hash])
                .await;

            // Check protocol sends ask block to consensus.
            let hash =
                match wait_protocol_event(
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
                        list.pop().expect("Empty list of hashes.")
                    }
                    _ => panic!("Unexpected or no protocol event."),
                };
            assert_eq!(expected_hash, hash);

            // consensus didn't found block
            let mut results = Map::default();
            results.insert(expected_hash, None);
            protocol_command_sender
                .send_get_blocks_results(results)
                .await
                .unwrap();

            // protocol transmits blockNotFound
            let (node, hash) = match network_controller
                .wait_command(100.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::BlockNotFound { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::BlockNotFound { node, block_id }) => (node, block_id),
                _ => panic!("Unexpected or no network command."),
            };

            assert_eq!(expected_hash, hash);
            assert_eq!(creator_node.id, node);

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
