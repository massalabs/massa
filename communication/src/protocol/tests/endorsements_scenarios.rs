// Copyright (c) 2021 MASSA LABS <info@massa.net>

//RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools;
use super::tools::protocol_test;
use crate::network::NetworkCommand;
use crate::protocol::ProtocolPoolEvent;
use models::{Amount, Slot};
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
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

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            let expected_endorsement_id = endorsement.verify_integrity().unwrap();

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

            // 1. Create an endorsement.
            let mut endorsement = tools::create_endorsement();

            // Change the slot, making the signature invalid.
            endorsement.content.slot = Slot::new(1, 1);

            // 3. Send operation to protocol.
            network_controller
                .send_endorsements(creator_node.id, vec![endorsement])
                .await;

            // Check protocol does not send endorsements to pool.
            match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedEndorsements { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedEndorsements { .. }) => {
                    panic!("Protocol send invalid endorsements.")
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
