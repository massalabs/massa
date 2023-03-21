// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test_with_storage;
use massa_models::{self, address::Address, slot::Slot};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NetworkCommand};
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::ProtocolConfig;
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
async fn test_noting_block_does_not_panic_with_zero_max_node_known_blocks_size() {
    let protocol_config = &CUSTOM_PROTOCOL_CONFIG;

    protocol_test_with_storage(
        protocol_config,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver,
                    mut storage| {
            // Create 2 node.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.get_public_key());
            let thread = address.get_thread(2.try_into().unwrap());

            let operation = tools::create_operation_with_expire_period(&nodes[0].keypair, 1);

            let block = tools::create_block_with_operations(
                &nodes[0].keypair,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            // Add block to storage.
            storage.store_block(block.clone());

            // Add Operation to storage.
            storage.store_operations(vec![operation.clone()]);

            // Have the second node ask for the operations.
            let needed_ops = vec![operation.id].into_iter().collect();
            network_controller
                .send_ask_for_block(
                    nodes[1].id,
                    vec![(block.id, AskForBlocksInfo::Operations(needed_ops))],
                )
                .await;

            // Check that protocol processes the message without panicking.
            let cmd_filter = |cmd| match cmd {
                NetworkCommand::SendBlockInfo { node, info } => {
                    assert_eq!(node, nodes[1].id);
                    Some(info)
                }
                _ => None,
            };

            let (block_id, info) = network_controller
                .wait_command(100.into(), cmd_filter)
                .await
                .unwrap()
                .pop()
                .unwrap();

            assert_eq!(block_id, block.id);
            if let BlockInfoReply::Operations(mut ops) = info {
                assert_eq!(ops.pop().unwrap().id, operation.id);
                assert!(ops.is_empty());
            } else {
                panic!("Unexpected block info.");
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
