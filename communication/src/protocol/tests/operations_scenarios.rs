//RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::{mock_network_controller::MockNetworkController, tools};
use crate::network::NetworkCommand;
use crate::protocol::start_protocol_controller;
use crate::protocol::ProtocolPoolEvent;
use models::get_serialization_context;
use serial_test::serial;
use std::collections::HashMap;

#[tokio::test]
#[serial]
async fn test_protocol_sends_valid_operations_it_receives_to_consensus() {
    let protocol_config = tools::create_protocol_config();
    let serialization_context = get_serialization_context();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (_, protocol_event_receiver, mut protocol_pool_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            5u64,
            network_command_sender,
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    // Create 1 node.
    let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Create an operation
    let operation = tools::create_operation();

    let expected_operation_id = operation.verify_integrity(&serialization_context).unwrap();

    // 3. Send operation to protocol.
    network_controller
        .send_operations(creator_node.id, vec![operation])
        .await;

    // Check protocol sends operations to consensus.
    let received_operations = match tools::wait_protocol_pool_event(
        &mut protocol_pool_event_receiver,
        1000.into(),
        |evt| match evt {
            evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
        },
    )
    .await
    {
        Some(ProtocolPoolEvent::ReceivedOperations(operations)) => operations,
        _ => panic!("Unexpected or no protocol pool event."),
    };
    assert!(received_operations.contains_key(&expected_operation_id));

    protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_send_invalid_operations_it_receives_to_consensus() {
    let protocol_config = tools::create_protocol_config();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (_, protocol_event_receiver, mut protocol_pool_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            5u64,
            network_command_sender,
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    // Create 1 node.
    let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    // 1. Create an operation.
    let mut operation = tools::create_operation();

    // Change the fee, making the signature invalid.
    operation.content.fee = 111;

    // 3. Send operation to protocol.
    network_controller
        .send_operations(creator_node.id, vec![operation])
        .await;

    // Check protocol does not send operations to consensus.
    match tools::wait_protocol_pool_event(&mut protocol_pool_event_receiver, 1000.into(), |evt| {
        match evt {
            evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
        }
    })
    .await
    {
        Some(ProtocolPoolEvent::ReceivedOperations(_)) => {
            panic!("Protocol send invalid operations.")
        }
        _ => {}
    };

    protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_to_active_nodes() {
    let protocol_config = tools::create_protocol_config();
    let serialization_context = get_serialization_context();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (
        mut protocol_command_sender,
        protocol_event_receiver,
        mut protocol_pool_event_receiver,
        protocol_manager,
    ) = start_protocol_controller(
        protocol_config.clone(),
        5u64,
        network_command_sender,
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // Create 2 nodes.
    let mut nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

    // 1. Create an operation
    let operation = tools::create_operation();

    // Send operation and wait for the protocol event,
    // just to be sure the nodes are connected before sendind the propagate command.
    network_controller
        .send_operations(nodes[0].id, vec![operation.clone()])
        .await;
    let _received_operations = match tools::wait_protocol_pool_event(
        &mut protocol_pool_event_receiver,
        1000.into(),
        |evt| match evt {
            evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
        },
    )
    .await
    {
        Some(ProtocolPoolEvent::ReceivedOperations(operations)) => operations,
        _ => panic!("Unexpected or no protocol pool event."),
    };

    let expected_operation_id = operation.verify_integrity(&serialization_context).unwrap();

    let mut ops = HashMap::new();
    ops.insert(expected_operation_id.clone(), operation);
    protocol_command_sender
        .propagate_operations(ops)
        .await
        .unwrap();

    loop {
        match network_controller
            .wait_command(1000.into(), |cmd| match cmd {
                cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                _ => None,
            })
            .await
        {
            Some(NetworkCommand::SendOperations { node, operations }) => {
                let id = operations[0]
                    .verify_integrity(&serialization_context)
                    .unwrap();
                assert_eq!(id, expected_operation_id);
                nodes.retain(|node_info| node != node_info.id);
                if nodes.is_empty() {
                    break;
                }
            }
            _ => panic!("Unexpected or no network command."),
        };
    }

    protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}
