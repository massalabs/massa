// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test;
use massa_models::{self, Address, Slot};
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::ProtocolEvent;
use massa_protocol_exports::ProtocolSettings;
use serial_test::serial;

lazy_static::lazy_static! {
    pub static ref CUSTOM_PROTOCOL_SETTINGS: ProtocolSettings = {
        let mut protocol_settings = *tools::PROTOCOL_SETTINGS;

        // Set max_node_known_blocks_size to zero.
        protocol_settings.max_node_known_blocks_size = 0;

        protocol_settings
    };
}

#[tokio::test]
#[serial]
async fn test_noting_block_does_not_panic_with_zero_max_node_known_blocks_size() {
    let protocol_settings = &CUSTOM_PROTOCOL_SETTINGS;

    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);

            let block = tools::create_block_with_operations(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            // Send a block, ensuring the processing of it,
            // and of its header,
            // does not panic.
            network_controller.send_block(nodes[0].id, block).await;

            // Wait for the event, should not panic.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlock { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

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
