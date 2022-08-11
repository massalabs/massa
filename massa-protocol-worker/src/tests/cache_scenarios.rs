// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test;
use massa_models::constants::THREAD_COUNT;
use massa_models::{self, Address, Slot};
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::{ProtocolConfig, ProtocolSettings};
use serial_test::serial;

lazy_static::lazy_static! {
    pub static ref CUSTOM_PROTOCOL_CONFIG: ProtocolConfig = {
        let mut protocol_config = *tools::PROTOCOL_CONFIG;

        // Set max_node_known_blocks_size to zero.
        protocol_config.max_node_known_blocks_size = 0;

        protocol_config
    };
}

#[tokio::test]
#[serial]
#[ignore]
async fn test_noting_block_does_not_panic_with_zero_max_node_known_blocks_size() {
    let protocol_config = &CUSTOM_PROTOCOL_CONFIG;

    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let thread = address.get_thread(THREAD_COUNT);

            let operation = tools::create_operation_with_expire_period(&nodes[0].keypair, 1);

            let block = tools::create_block_with_operations(
                &nodes[0].keypair,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            // TODO: rewrite with block info.

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
