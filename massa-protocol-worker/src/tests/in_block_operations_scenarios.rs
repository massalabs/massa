// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::protocol_test;
use massa_hash::hash::Hash;
use massa_models::{
    get_serialization_context, Address, Amount, Block, BlockHeader, BlockHeaderContent, Slot,
};
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::tests::tools::{
    create_and_connect_nodes, create_block_with_operations, create_operation_with_expire_period,
    send_and_propagate_block,
};
use massa_signature::{derive_public_key, generate_random_private_key};
use serial_test::serial;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_protocol_sends_blocks_with_operations_to_consensus() {
    //         // setup logging
    // stderrlog::new()
    // .verbosity(4)
    // .timestamp(stderrlog::Timestamp::Millisecond)
    // .init()
    // .unwrap();
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let serialization_context = get_serialization_context();

            // Create 1 node.
            let mut nodes = create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            let mut private_key = generate_random_private_key();
            let mut public_key = derive_public_key(&private_key);
            let mut address = Address::from_public_key(&public_key);
            let mut thread = address.get_thread(serialization_context.thread_count);

            while thread != 0 {
                private_key = generate_random_private_key();
                public_key = derive_public_key(&private_key);
                address = Address::from_public_key(&public_key);
                thread = address.get_thread(serialization_context.thread_count);
            }

            let slot_a = Slot::new(1, 0);

            // block with ok operation
            {
                let op = create_operation_with_expire_period(&private_key, 5);

                let block = create_block_with_operations(
                    &creator_node.private_key,
                    &creator_node.id.0,
                    slot_a,
                    vec![op],
                );

                send_and_propagate_block(
                    &mut network_controller,
                    block,
                    true,
                    creator_node.id,
                    &mut protocol_event_receiver,
                )
                .await;
            }

            // block with operation too far in the future
            {
                let op = create_operation_with_expire_period(&private_key, 50);

                let block = create_block_with_operations(
                    &creator_node.private_key,
                    &creator_node.id.0,
                    slot_a,
                    vec![op],
                );

                send_and_propagate_block(
                    &mut network_controller,
                    block,
                    false,
                    creator_node.id,
                    &mut protocol_event_receiver,
                )
                .await;
            }
            // block with an operation twice
            {
                let op = create_operation_with_expire_period(&private_key, 5);

                let block = create_block_with_operations(
                    &creator_node.private_key,
                    &creator_node.id.0,
                    slot_a,
                    vec![op.clone(), op],
                );

                send_and_propagate_block(
                    &mut network_controller,
                    block,
                    false,
                    creator_node.id,
                    &mut protocol_event_receiver,
                )
                .await;
            }
            // block with wrong merkle root
            {
                let op = create_operation_with_expire_period(&private_key, 5);

                let block = {
                    let operation_merkle_root = Hash::compute_from("merkle root".as_bytes());

                    let (_, header) = BlockHeader::new_signed(
                        &creator_node.private_key,
                        BlockHeaderContent {
                            creator: derive_public_key(&creator_node.private_key),
                            slot: slot_a,
                            parents: Vec::new(),
                            operation_merkle_root,
                            endorsements: Vec::new(),
                        },
                    )
                    .unwrap();

                    Block {
                        header,
                        operations: vec![op.clone()],
                    }
                };

                send_and_propagate_block(
                    &mut network_controller,
                    block,
                    false,
                    creator_node.id,
                    &mut protocol_event_receiver,
                )
                .await;
            }

            // block with operation with wrong signature
            {
                let mut op = create_operation_with_expire_period(&private_key, 5);
                op.content.fee = Amount::from_str("10").unwrap();
                let block = create_block_with_operations(
                    &creator_node.private_key,
                    &creator_node.id.0,
                    slot_a,
                    vec![op],
                );

                send_and_propagate_block(
                    &mut network_controller,
                    block,
                    false,
                    creator_node.id,
                    &mut protocol_event_receiver,
                )
                .await;
            }

            // block with operation in wrong thread
            {
                let mut op = create_operation_with_expire_period(&private_key, 5);
                op.content.fee = Amount::from_str("10").unwrap();
                let block = create_block_with_operations(
                    &creator_node.private_key,
                    &creator_node.id.0,
                    Slot::new(1, 1),
                    vec![op],
                );

                send_and_propagate_block(
                    &mut network_controller,
                    block,
                    false,
                    creator_node.id,
                    &mut protocol_event_receiver,
                )
                .await;
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
