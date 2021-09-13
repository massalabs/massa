// Copyright (c) 2021 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools;
use super::tools::protocol_test;
use crate::network::NetworkCommand;
use crate::protocol::ProtocolPoolEvent;
use models::{Amount, OperationHashMap};
use serial_test::serial;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_protocol_sends_valid_operations_it_receives_to_consensus() {
    let protocol_config = tools::create_protocol_config();
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation
            let operation = tools::create_operation();

            let expected_operation_id = operation.verify_integrity().unwrap();

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
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedOperations { operations, .. }) => operations,
                _ => panic!("Unexpected or no protocol pool event."),
            };
            assert!(received_operations.contains_key(&expected_operation_id));

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
async fn test_protocol_does_not_send_invalid_operations_it_receives_to_consensus() {
    let protocol_config = tools::create_protocol_config();
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation.
            let mut operation = tools::create_operation();

            // Change the fee, making the signature invalid.
            operation.content.fee = Amount::from_str("111").unwrap();

            // 3. Send operation to protocol.
            network_controller
                .send_operations(creator_node.id, vec![operation])
                .await;

            // Check protocol does not send operations to consensus.
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
                Some(ProtocolPoolEvent::ReceivedOperations { .. }) => {
                    panic!("Protocol send invalid operations.")
                }
                _ => {}
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
async fn test_protocol_propagates_operations_to_active_nodes() {
    let protocol_config = tools::create_protocol_config();
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            // 1. Create an operation
            let operation = tools::create_operation();

            // Send operation and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_operations(nodes[0].id, vec![operation.clone()])
                .await;
            let _received_operations = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedOperations { operations, .. }) => operations,
                _ => panic!("Unexpected or no protocol pool event."),
            };

            let expected_operation_id = operation.verify_integrity().unwrap();

            let mut ops = OperationHashMap::default();
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
                        let id = operations[0].verify_integrity().unwrap();
                        assert_eq!(id, expected_operation_id);
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
