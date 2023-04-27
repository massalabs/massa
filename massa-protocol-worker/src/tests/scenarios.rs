// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::{protocol_test, protocol_test_with_storage};
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::block_id::BlockId;
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_network_exports::{AskForBlocksInfo, NetworkCommand};
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::{
    tests::tools::{create_and_connect_nodes, create_block},
    BlocksResults,
};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashSet;

#[tokio::test]
#[serial]
async fn test_protocol_asks_for_block_from_node_who_propagated_header() {
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
            let mut nodes = create_and_connect_nodes(3, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Close one connection.
            network_controller.close_connection(nodes[0].id).await;

            // 2. Create a block coming from node creator_node.
            let block = create_block(&creator_node.keypair);

            // 3. Send header to protocol.
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

            // 4. Check that protocol sent the right header to consensus.
            let expected_hash = block.id;
            assert_eq!(expected_hash, received_hash);

            // 5. Ask for block.
            protocol_command_sender = tokio::task::spawn_blocking(move || {
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

            // 6. Check that protocol asks the node for the full block.
            match network_controller
                .wait_command(1000.into(), ask_for_block_cmd_filter)
                .await
                .expect("Protocol didn't send network command.")
            {
                NetworkCommand::AskForBlocks { list } => {
                    assert_eq!(
                        list.get(&creator_node.id).unwrap().clone().pop().unwrap().0,
                        expected_hash
                    );
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
async fn test_protocol_sends_blocks_when_asked_for() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test_with_storage(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver,
                    mut storage| {
            let send_block_info_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::SendBlockInfo { .. } => Some(cmd),
                _ => None,
            };

            let mut nodes = create_and_connect_nodes(4, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Close one connection.
            network_controller.close_connection(nodes[2].id).await;

            // 2. Create a block coming from creator_node.
            let block = create_block(&creator_node.keypair);
            let expected_hash = block.id;

            // Add to storage, integrate.
            storage.store_block(block.clone());
            protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .integrated_block(expected_hash, storage.clone())
                    .unwrap();
                protocol_command_sender
            })
            .await
            .unwrap();

            // 3. Simulate two nodes asking for a block.
            for node in nodes.iter().take(2) {
                network_controller
                    .send_ask_for_block(node.id, vec![(expected_hash, Default::default())])
                    .await;
            }

            // 4. Simulate consensus sending block.
            let mut results: BlocksResults = PreHashMap::default();
            results.insert(expected_hash, Some((None, None)));

            // 5. Check that protocol sends the nodes the full block.
            let mut expecting_block = HashSet::new();
            expecting_block.insert(nodes[0].id);
            expecting_block.insert(nodes[1].id);
            loop {
                match network_controller
                    .wait_command(1000.into(), send_block_info_cmd_filter)
                    .await
                {
                    Some(NetworkCommand::SendBlockInfo { node, .. }) => {
                        expecting_block.remove(&node);
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
                .wait_command(100.into(), send_block_info_cmd_filter)
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
async fn test_protocol_propagates_block_to_all_nodes_including_those_who_asked_for_info() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test_with_storage(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver,
                    mut storage| {
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
            let ref_block = create_block(&creator_node.keypair);

            // 3. Send header to protocol.
            network_controller
                .send_header(creator_node.id, ref_block.content.header.clone())
                .await;

            // node[1] asks for that block

            // Check protocol sends header to consensus.
            let (protocol_consensus_event_receiver, ref_hash) =
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

            storage.store_block(ref_block.clone());

            // Now ask for the info on the block.
            network_controller
                .send_ask_for_block(node_b.id, vec![(ref_hash, AskForBlocksInfo::Info)])
                .await;

            // 5. Propagate header.
            let _op_ids = ref_block.content.operations.clone();
            protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .integrated_block(ref_hash, storage.clone())
                    .unwrap();
                protocol_command_sender
            })
            .await
            .unwrap();

            // 6. Check that protocol propagates the header to the right nodes.
            // node_a created the block and should receive nothing
            // node_b asked for the info and should receive the header
            // node_c did nothing, it should receive the header
            // node_d was disconnected, so nothing should be send to it
            let mut expected_headers = HashSet::new();
            expected_headers.insert(node_c.id);
            expected_headers.insert(node_b.id);

            let mut expected_info = HashSet::new();
            expected_info.insert(node_b.id);

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendBlockHeader { .. } => Some(cmd),
                        cmd @ NetworkCommand::SendBlockInfo { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendBlockHeader { node, header }) => {
                        assert!(expected_headers.remove(&node));
                        assert_eq!(header.id, ref_hash);
                    }
                    Some(NetworkCommand::SendBlockInfo { node, mut info }) => {
                        assert!(expected_info.remove(&node));
                        let asked = info.pop().unwrap();
                        assert_eq!(asked.0, ref_hash);
                    }
                    _ => panic!("Unexpected or network command."),
                };

                if expected_headers.is_empty() && expected_info.is_empty() {
                    break;
                }
            }
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
#[ignore]
async fn test_protocol_propagates_block_to_node_who_asked_for_operations_and_only_header_to_others()
{
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test_with_storage(
        protocol_config,
        async move |mut network_controller,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver,
                    mut storage| {
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
            let ref_block = create_block(&creator_node.keypair);

            // 3. Send header to protocol.
            network_controller
                .send_header(creator_node.id, ref_block.content.header.clone())
                .await;

            // node[1] asks for that block

            // Check protocol sends header to consensus.
            let (protocol_consensus_event_receiver, ref_hash) =
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

            storage.store_block(ref_block.clone());
            // 5. Propagate header.
            let _op_ids = ref_block.content.operations.clone();
            protocol_command_sender = tokio::task::spawn_blocking(move || {
                protocol_command_sender
                    .integrated_block(ref_hash, storage.clone())
                    .unwrap();
                protocol_command_sender
            })
            .await
            .unwrap();

            // 6. Check that protocol propagates the header to the right nodes.
            // node_a created the block and should receive nothing
            // node_b asked for the block and should receive the full block
            // node_c did nothing, it should receive the header
            // node_d was disconnected, so nothing should be send to it
            let mut expected_headers = HashSet::new();
            expected_headers.insert(node_a.id);
            expected_headers.insert(node_c.id);
            expected_headers.insert(node_b.id);

            let mut expected_full_blocks = HashSet::new();
            expected_full_blocks.insert(node_b.id);

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendBlockHeader { .. } => Some(cmd),
                        cmd @ NetworkCommand::SendBlockInfo { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendBlockHeader { node, header }) => {
                        assert!(expected_headers.remove(&node));
                        assert_eq!(header.id, ref_hash);
                    }
                    Some(NetworkCommand::SendBlockInfo { .. }) => {
                        panic!("Unpexpected sending of block info");
                    }
                    _ => panic!("Unexpected or network command."),
                };

                if expected_headers.is_empty() {
                    break;
                }
            }

            // Now ask for the full block.
            network_controller
                .send_ask_for_block(
                    node_b.id,
                    vec![(ref_hash, AskForBlocksInfo::Operations(vec![]))],
                )
                .await;

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendBlockHeader { .. } => Some(cmd),
                        cmd @ NetworkCommand::SendBlockInfo { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendBlockHeader { .. }) => {
                        panic!("Unexpected sending of header.");
                    }
                    Some(NetworkCommand::SendBlockInfo { node, mut info }) => {
                        assert!(expected_full_blocks.remove(&node));
                        let asked = info.pop().unwrap();
                        assert_eq!(asked.0, ref_hash);
                    }
                    _ => panic!("Unexpected or network command."),
                };

                if expected_full_blocks.is_empty() {
                    break;
                }
            }
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
async fn test_protocol_sends_full_blocks_it_receives_to_consensus() {
    let protocol_config = &tools::PROTOCOL_CONFIG;

    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let block = create_block(&creator_node.keypair);

            let _expected_hash = block.id;

            // TODO: rewrite with block info.

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
async fn test_protocol_block_not_found() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create a block coming from one node.
            let block = create_block(&creator_node.keypair);

            let expected_hash = block.id;

            // 3. Ask block to protocol.
            network_controller
                .send_ask_for_block(creator_node.id, vec![(expected_hash, Default::default())])
                .await;

            // protocol transmits blockNotFound
            let (node, mut info) = match network_controller
                .wait_command(100.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendBlockInfo { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendBlockInfo { node, info }) => (node, info),
                _ => panic!("Unexpected or no network command."),
            };

            let (block_id, _block_info) = info.pop().unwrap();

            assert_eq!(expected_hash, block_id);
            assert_eq!(creator_node.id, node);

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
