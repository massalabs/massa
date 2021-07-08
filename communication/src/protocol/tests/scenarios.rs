//RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::{mock_network_controller::MockNetworkController, tools};
use crate::error::CommunicationError;
use crate::network::NetworkCommand;
use crate::protocol::start_protocol_controller;
use crate::protocol::ProtocolEvent;
use crypto::signature::SignatureEngine;
use models::Slot;
use std::collections::HashSet;

#[tokio::test]
async fn test_protocol_asks_for_block_from_node_who_propagated_header() {
    let (protocol_config, serialization_context) = tools::create_protocol_config();

    let mut signature_engine = SignatureEngine::new();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (mut protocol_command_sender, mut protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            serialization_context.clone(),
            network_command_sender,
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    let mut nodes =
        tools::create_and_connect_nodes(3, &signature_engine, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Close one connection.
    network_controller.close_connection(&nodes[0].id).await;

    // 2. Create a block coming from node 0.
    let block = tools::create_block(
        &creator_node.private_key,
        &creator_node.id.0,
        &serialization_context,
        &mut signature_engine,
    );

    // 3. Send header to protocol.
    network_controller
        .send_header(&creator_node.id, &block.header)
        .await;

    // Check protocol sends header to consensus.
    let received_hash = match protocol_event_receiver
        .wait_event()
        .await
        .expect("Protocol didn't send event.")
    {
        ProtocolEvent::ReceivedBlockHeader {
            hash,
            header: _header,
        } => hash,
        _ => panic!("Unexpected protocol event."),
    };

    // 4. Check that protocol sent the right header to consensus.
    let expected_hash = block
        .header
        .content
        .compute_hash(&serialization_context)
        .expect("Failed to compute hash.");
    assert_eq!(expected_hash, received_hash);

    // 5. Ask for block.
    protocol_command_sender
        .ask_for_block(expected_hash)
        .await
        .expect("Failed to ask for block.");

    // 6. Check that protocol asks the node for the full block.
    let (ask_to_node_id, asked_for_hash) = match network_controller
        .wait_command()
        .await
        .expect("Protocol didn't send network command.")
    {
        NetworkCommand::AskForBlock(node_id, hash) => (node_id, hash),
        _ => panic!("Unexpected network command."),
    };
    assert_eq!(expected_hash, asked_for_hash);
    assert_eq!(ask_to_node_id, creator_node.id);

    protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");

    // 7. Make sure protocol did not ask the other nodes for the block.
    let got_more_commands = network_controller.wait_command().await;
    assert!(got_more_commands.is_none());
}

#[tokio::test]
async fn test_protocol_sends_blocks_when_asked_for() {
    let (protocol_config, serialization_context) = tools::create_protocol_config();

    let mut signature_engine = SignatureEngine::new();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (mut protocol_command_sender, mut protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            serialization_context.clone(),
            network_command_sender,
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    let mut nodes =
        tools::create_and_connect_nodes(4, &signature_engine, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Close one connection.
    network_controller.close_connection(&nodes[2].id).await;

    // 2. Create a block coming from node 0.
    let block = tools::create_block(
        &creator_node.private_key,
        &creator_node.id.0,
        &serialization_context,
        &mut signature_engine,
    );

    let expected_hash = block
        .header
        .content
        .compute_hash(&serialization_context)
        .expect("Failed to compute hash.");

    // 3. Simulate two nodes asking for a block.
    for n in 0..2 {
        network_controller
            .send_ask_for_block(&nodes[n].id, &expected_hash)
            .await;

        // Check protocol sends get block event to consensus.
        let received_hash = match protocol_event_receiver
            .wait_event()
            .await
            .expect("Protocol didn't send event.")
        {
            ProtocolEvent::GetBlock(hash) => hash,
            _ => panic!("Unexpected protocol event."),
        };

        // Check that protocol sent the right hash to consensus.
        assert_eq!(expected_hash, received_hash);
    }

    // 4. Simulate consensing sending block.
    protocol_command_sender
        .send_block(expected_hash.clone(), block)
        .await
        .expect("Failed to ask for block.");

    // 5. Check that protocol send the nodes the full block.
    let mut expected = HashSet::new();
    expected.insert(nodes[0].id.clone());
    expected.insert(nodes[1].id.clone());
    loop {
        let (sent_to_node_id, sent_hash) = match network_controller
            .wait_command()
            .await
            .expect("Protocol didn't send network command.")
        {
            NetworkCommand::SendBlock(node_id, block) => {
                let hash = block
                    .header
                    .content
                    .compute_hash(&serialization_context)
                    .expect("Failed to compute hash.");
                (node_id, hash)
            }
            _ => panic!("Unexpected network command."),
        };
        assert_eq!(expected_hash, sent_hash);

        // Check that the header was propagated to a node that didn't know about it.
        assert!(expected.remove(&sent_to_node_id));

        if expected.is_empty() {
            break;
        }
    }

    protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");

    // 7. Make sure protocol did not send the block to other nodes.
    let got_more_commands = network_controller.wait_command().await;
    assert!(got_more_commands.is_none());
}

#[tokio::test]
async fn test_protocol_propagates_headers_to_all_node_who_do_not_know_about_it() {
    let (protocol_config, serialization_context) = tools::create_protocol_config();

    let mut signature_engine = SignatureEngine::new();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (mut protocol_command_sender, mut protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            serialization_context.clone(),
            network_command_sender,
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    // Create 4 nodes.
    let mut nodes =
        tools::create_and_connect_nodes(4, &signature_engine, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Close one connection.
    network_controller.close_connection(&nodes[0].id).await;

    // 2. Create a block coming from one node.
    let block = tools::create_block(
        &creator_node.private_key,
        &creator_node.id.0,
        &serialization_context,
        &mut signature_engine,
    );

    // 3. Send header to protocol.
    network_controller
        .send_header(&creator_node.id, &block.header)
        .await;

    // Check protocol sends header to consensus.
    let (hash, header) = match protocol_event_receiver
        .wait_event()
        .await
        .expect("Protocol didn't send event.")
    {
        ProtocolEvent::ReceivedBlockHeader { hash, header } => (hash, header),
        _ => panic!("Unexpected protocol event."),
    };

    // 5. Propagate header.
    protocol_command_sender
        .propagate_block_header(hash, header)
        .await
        .expect("Failed to ask for block.");

    // 6. Check that protocol propagates the header to the rigth nodes.
    let mut expected = HashSet::new();
    expected.insert(nodes[1].id.clone());
    expected.insert(nodes[2].id.clone());
    loop {
        let (sent_to_node_id, sent_header) = match network_controller
            .wait_command()
            .await
            .expect("Protocol didn't send network command.")
        {
            NetworkCommand::SendBlockHeader { node, header } => (node, header),
            _ => panic!("Unexpected network command."),
        };

        // Check that the header was propagated to a node that didn't know about it.
        assert!(expected.remove(&sent_to_node_id));

        // Check that it was the expected header.
        let sent_header_hash = sent_header
            .content
            .compute_hash(&serialization_context)
            .expect("Couldn't compute hash.");
        assert_eq!(sent_header_hash, hash);

        if expected.is_empty() {
            break;
        }
    }

    protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}

#[tokio::test]
async fn test_protocol_sends_full_blocks_it_receives_to_consensus() {
    let (protocol_config, serialization_context) = tools::create_protocol_config();

    let mut signature_engine = SignatureEngine::new();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (_, mut protocol_event_receiver, protocol_manager) = start_protocol_controller(
        protocol_config.clone(),
        serialization_context.clone(),
        network_command_sender,
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // Create 1 node.
    let mut nodes =
        tools::create_and_connect_nodes(1, &signature_engine, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Create a block coming from one node.
    let block = tools::create_block(
        &creator_node.private_key,
        &creator_node.id.0,
        &serialization_context,
        &mut signature_engine,
    );

    let expected_hash = block
        .header
        .content
        .compute_hash(&serialization_context)
        .expect("Couldn't compute hash.");

    // 3. Send block to protocol.
    network_controller
        .send_block(&creator_node.id, &block)
        .await;

    // Check protocol sends block to consensus.
    let hash = match protocol_event_receiver
        .wait_event()
        .await
        .expect("Protocol didn't send event.")
    {
        ProtocolEvent::ReceivedBlock { hash, block: _ } => hash,
        _ => panic!("Unexpected protocol event."),
    };
    assert_eq!(expected_hash, hash);

    protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}

#[tokio::test]
async fn test_protocol_does_not_send_full_blocks_it_receives_with_invalid_signature() {
    let (protocol_config, serialization_context) = tools::create_protocol_config();

    let mut signature_engine = SignatureEngine::new();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (_, mut protocol_event_receiver, protocol_manager) = start_protocol_controller(
        protocol_config.clone(),
        serialization_context.clone(),
        network_command_sender,
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // Create 1 node.
    let mut nodes =
        tools::create_and_connect_nodes(1, &signature_engine, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Create a block coming from one node.
    let mut block = tools::create_block(
        &creator_node.private_key,
        &creator_node.id.0,
        &serialization_context,
        &mut signature_engine,
    );

    // 2. Change the slot.
    block.header.content.slot = Slot::new(1, 1);

    // 3. Send block to protocol.
    network_controller
        .send_block(&creator_node.id, &block)
        .await;

    // Check protocol does not send block to consensus.
    match protocol_event_receiver.wait_event().await {
        Err(_) => {}
        _ => panic!("Unexpected protocol event."),
    }

    match protocol_manager.stop(protocol_event_receiver).await {
        Err(CommunicationError::WrongSignature) => {}
        _ => panic!("Unexpected protocol shutdown."),
    }
}
