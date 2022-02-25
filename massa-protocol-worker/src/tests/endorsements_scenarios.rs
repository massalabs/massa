// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test;
use massa_models::prehash::Map;
use massa_models::{Address, Slot};
use massa_network::NetworkCommand;
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::{ProtocolEvent, ProtocolPoolEvent};
use serial_test::serial;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
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

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            let expected_endorsement_id = endorsement.compute_endorsement_id().unwrap();

            // 3. Send endorsement to protocol.
            network_controller
                .send_endorsements(creator_node.id, vec![endorsement])
                .await;

            // Check protocol sends endorsements to pool.
            let received_endorsements = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedEndorsements { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedEndorsements { endorsements, .. }) => endorsements,
                _ => panic!("Unexpected or no protocol pool event."),
            };
            assert!(received_endorsements.contains_key(&expected_endorsement_id));

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
async fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
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

            // 1. Create an endorsement.
            let mut endorsement = tools::create_endorsement();

            // Change the slot, making the signature invalid.
            endorsement.content.slot = Slot::new(1, 1);

            // 3. Send operation to protocol.
            network_controller
                .send_endorsements(creator_node.id, vec![endorsement])
                .await;

            // Check protocol does not send endorsements to pool.
            if let Some(ProtocolPoolEvent::ReceivedEndorsements { .. }) =
                tools::wait_protocol_pool_event(
                    &mut protocol_pool_event_receiver,
                    1000.into(),
                    |evt| match evt {
                        evt @ ProtocolPoolEvent::ReceivedEndorsements { .. } => Some(evt),
                        _ => None,
                    },
                )
                .await
            {
                panic!("Protocol send invalid endorsements.")
            };

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
async fn test_protocol_propagates_endorsements_to_active_nodes() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            // Send endorsement and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_endorsements(nodes[0].id, vec![endorsement.clone()])
                .await;
            let _received_endorsements = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedEndorsements { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedEndorsements { endorsements, .. }) => endorsements,
                _ => panic!("Unexpected or no protocol pool event."),
            };

            let expected_endorsement_id = endorsement.compute_endorsement_id().unwrap();

            let mut ends = Map::default();
            ends.insert(expected_endorsement_id, endorsement);
            protocol_command_sender
                .propagate_endorsements(ends)
                .await
                .unwrap();

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                        let id = endorsements[0].compute_endorsement_id().unwrap();
                        assert_eq!(id, expected_endorsement_id);
                        assert_eq!(nodes[1].id, node);
                        break;
                    }
                    _ => panic!("Unexpected or no network command."),
                };
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
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            // Send endorsement and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_endorsements(nodes[0].id, vec![endorsement.clone()])
                .await;
            let _received_endorsements = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedEndorsements { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedEndorsements { endorsements, .. }) => endorsements,
                _ => panic!("Unexpected or no protocol pool event."),
            };

            // create and connect a node that does not know about the endorsement
            let new_nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            // wait for things to settle
            tokio::time::sleep(Duration::from_millis(250)).await;

            let expected_endorsement_id = endorsement.compute_endorsement_id().unwrap();

            // send the endorsement to protocol
            // it should propagate it to nodes that don't know about it
            let mut ops = Map::default();
            ops.insert(expected_endorsement_id, endorsement);
            protocol_command_sender
                .propagate_endorsements(ops)
                .await
                .unwrap();

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                        let id = endorsements[0].compute_endorsement_id().unwrap();
                        assert_eq!(id, expected_endorsement_id);
                        assert_eq!(new_nodes[0].id, node);
                        break;
                    }
                    None => panic!("no network command"),
                    Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
                };
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
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let endorsement = tools::create_endorsement();
            let endorsement_id = endorsement.compute_endorsement_id().unwrap();

            let block = tools::create_block_with_endorsements(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![endorsement.clone()],
            );
            let block_id = block.header.compute_block_id().unwrap();

            network_controller
                .send_ask_for_block(nodes[0].id, vec![block_id])
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as interested in the block.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::GetBlocks { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Integrate the block,
            // this should note the node as knowning about the endorsement.
            protocol_command_sender
                .integrated_block(block_id, block, Default::default(), vec![endorsement_id])
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendBlock { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendBlock { node, block }) => {
                    assert_eq!(node, nodes[0].id);
                    assert_eq!(block.header.compute_block_id().unwrap(), block_id);
                }
                Some(_) => panic!("Unexpected network command.."),
                None => panic!("Block not sent."),
            };

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously integrated block.
            let mut ops = Map::default();
            ops.insert(endorsement_id, endorsement);
            protocol_command_sender
                .propagate_endorsements(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                    let id = endorsements[0].compute_endorsement_id().unwrap();
                    assert_eq!(id, endorsement_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of endorsement.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

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
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_get_block_results(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let endorsement = tools::create_endorsement();
            let endorsement_id = endorsement.compute_endorsement_id().unwrap();

            let block = tools::create_block_with_endorsements(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![endorsement.clone()],
            );
            let block_id = block.header.compute_block_id().unwrap();

            network_controller
                .send_ask_for_block(nodes[0].id, vec![block_id])
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as interested in the block.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::GetBlocks { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Send the block as search results.
            let mut results = Map::default();
            results.insert(
                block_id,
                Some((block.clone(), None, Some(vec![endorsement_id]))),
            );

            protocol_command_sender
                .send_get_blocks_results(results)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendBlock { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendBlock { node, block }) => {
                    assert_eq!(node, nodes[0].id);
                    assert_eq!(block.header.compute_block_id().unwrap(), block_id);
                }
                Some(_) => panic!("Unexpected network command.."),
                None => panic!("Block not sent."),
            };

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously integrated block.
            let mut ops = Map::default();
            ops.insert(endorsement_id, endorsement);
            protocol_command_sender
                .propagate_endorsements(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                    let id = endorsements[0].compute_endorsement_id().unwrap();
                    assert_eq!(id, endorsement_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of endorsement.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

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
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let endorsement = tools::create_endorsement();
            let endorsement_id = endorsement.compute_endorsement_id().unwrap();

            let block = tools::create_block_with_endorsements(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![endorsement.clone()],
            );

            // Node 2 sends block, resulting in endorsements noted in block info.
            network_controller
                .send_block(nodes[1].id, block.clone())
                .await;

            // Node 1 sends header, resulting in protocol using the block info to determine
            // the node knows about the endorsements contained in the block header.
            network_controller
                .send_header(nodes[0].id, block.header.clone())
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as knowing the block and its endorsements.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously received header.
            let mut ops = Map::default();
            ops.insert(endorsement_id, endorsement);
            protocol_command_sender
                .propagate_endorsements(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                    let id = endorsements[0].compute_endorsement_id().unwrap();
                    assert_eq!(id, endorsement_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of endorsement.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

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
async fn test_protocol_does_not_propagates_endorsements_when_receiving_those_inside_a_header() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 2 nodes.
            let mut nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 2. Create a block coming from node creator_node.
            let mut block = tools::create_block(&creator_node.private_key, &creator_node.id.0);

            // 3. Add endorsement to block
            block.header.content.endorsements = vec![endorsement.clone()];

            // 4. Send header to protocol.
            network_controller
                .send_header(creator_node.id, block.header.clone())
                .await;

            let expected_endorsement_id = endorsement.compute_endorsement_id().unwrap();

            // 5. Check that the endorsements included in the header are not propagated.
            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendEndorsements {
                        node: _node,
                        endorsements,
                    }) => {
                        let id = endorsements[0].compute_endorsement_id().unwrap();
                        assert_eq!(id, expected_endorsement_id);
                        panic!("Unexpected propagation of endorsement received inside header.")
                    }
                    Some(_) => panic!("Unexpected network command.."),
                    None => break,
                };
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
