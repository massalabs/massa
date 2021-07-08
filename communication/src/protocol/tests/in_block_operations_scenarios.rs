use crypto::hash::Hash;
use models::{Address, Block, BlockHeader, BlockHeaderContent, Slot};

use crate::protocol::{
    start_protocol_controller,
    tests::{
        mock_network_controller::MockNetworkController,
        tools::{
            create_and_connect_nodes, create_block, create_block_with_operations,
            create_operation_with_expire_period, create_protocol_config, send_and_propagate_block,
            wait_protocol_event,
        },
    },
    ProtocolEvent,
};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_protocol_sends_blocks_with_operations_to_consensus() {
    //         // setup logging
    // stderrlog::new()
    // .verbosity(4)
    // .timestamp(stderrlog::Timestamp::Millisecond)
    // .init()
    // .unwrap();
    let (protocol_config, serialization_context) = create_protocol_config();

    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (_, mut protocol_event_receiver, protocol_pool_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            5u64,
            serialization_context.clone(),
            network_command_sender,
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    // Create 1 node.
    let mut nodes = create_and_connect_nodes(1, &mut network_controller).await;

    let creator_node = nodes.pop().expect("Failed to get node info.");

    let mut private_key = crypto::generate_random_private_key();
    let mut public_key = crypto::derive_public_key(&private_key);
    let mut address = Address::from_public_key(&public_key).unwrap();
    let mut thread = address.get_thread(serialization_context.parent_count);

    while thread != 0 {
        private_key = crypto::generate_random_private_key();
        public_key = crypto::derive_public_key(&private_key);
        address = Address::from_public_key(&public_key).unwrap();
        thread = address.get_thread(serialization_context.parent_count);
    }

    let slot_a = Slot::new(1, 0);

    // block with ok operation
    {
        let op =
            create_operation_with_expire_period(&serialization_context, private_key, public_key, 5);

        let block = create_block_with_operations(
            &creator_node.private_key,
            &creator_node.id.0,
            slot_a,
            vec![op],
            &serialization_context,
        );

        send_and_propagate_block(
            &mut network_controller,
            block,
            true,
            &serialization_context,
            creator_node.id,
            &mut protocol_event_receiver,
        )
        .await;
    }

    // block with operation too far in the future
    {
        let op = create_operation_with_expire_period(
            &serialization_context,
            private_key,
            public_key,
            50,
        );

        let block = create_block_with_operations(
            &creator_node.private_key,
            &creator_node.id.0,
            slot_a,
            vec![op],
            &serialization_context,
        );

        send_and_propagate_block(
            &mut network_controller,
            block,
            false,
            &serialization_context,
            creator_node.id,
            &mut protocol_event_receiver,
        )
        .await;
    }
    // block with an operation twice
    {
        let op =
            create_operation_with_expire_period(&serialization_context, private_key, public_key, 5);

        let block = create_block_with_operations(
            &creator_node.private_key,
            &creator_node.id.0,
            slot_a,
            vec![op.clone(), op],
            &serialization_context,
        );

        send_and_propagate_block(
            &mut network_controller,
            block,
            false,
            &serialization_context,
            creator_node.id,
            &mut protocol_event_receiver,
        )
        .await;
    }
    // block with wrong merkle root
    {
        let op =
            create_operation_with_expire_period(&serialization_context, private_key, public_key, 5);

        let block = {
            let operation_merkle_root = Hash::hash("merkle roor".as_bytes());

            let (_, header) = BlockHeader::new_signed(
                &creator_node.private_key,
                BlockHeaderContent {
                    creator: crypto::derive_public_key(&creator_node.private_key).clone(),
                    slot: slot_a,
                    parents: Vec::new(),
                    operation_merkle_root,
                },
                &serialization_context,
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
            &serialization_context,
            creator_node.id,
            &mut protocol_event_receiver,
        )
        .await;
    }

    // block with operation with wrong signature
    {
        let mut op =
            create_operation_with_expire_period(&serialization_context, private_key, public_key, 5);
        op.content.fee = 10;
        let block = create_block_with_operations(
            &creator_node.private_key,
            &creator_node.id.0,
            slot_a,
            vec![op],
            &serialization_context,
        );

        send_and_propagate_block(
            &mut network_controller,
            block,
            false,
            &serialization_context,
            creator_node.id,
            &mut protocol_event_receiver,
        )
        .await;
    }

    // block with operation in wrong thread
    {
        let mut op =
            create_operation_with_expire_period(&serialization_context, private_key, public_key, 5);
        op.content.fee = 10;
        let block = create_block_with_operations(
            &creator_node.private_key,
            &creator_node.id.0,
            Slot::new(1, 1),
            vec![op],
            &serialization_context,
        );

        send_and_propagate_block(
            &mut network_controller,
            block,
            false,
            &serialization_context,
            creator_node.id,
            &mut protocol_event_receiver,
        )
        .await;
    }

    protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}
