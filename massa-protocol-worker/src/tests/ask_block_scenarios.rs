// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::protocol_test;
use massa_models::prehash::Set;
use massa_models::signed::Signable;
use massa_models::BlockId;
use massa_network::NetworkCommand;
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::tests::tools::{asked_list, assert_hash_asked_to_node};
use massa_protocol_exports::ProtocolEvent;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_without_a_priori() {
    // start
    let protocol_settings = &tools::PROTOCOL_SETTINGS;

    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let node_b = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();

            // 2. Create a block coming from node 0.
            let block = tools::create_block(&node_a.private_key, &node_a.id.0);
            let hash_1 = block.header.content.compute_id().unwrap();
            // end set up

            // send wishlist
            protocol_command_sender
                .send_wishlist_delta(
                    vec![hash_1].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .unwrap();

            // assert it was asked to node A, then B
            assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;
            assert_hash_asked_to_node(hash_1, node_b.id, &mut network_controller).await;

            // node B replied with the block
            network_controller.send_block(node_b.id, block).await;

            // 7. Make sure protocol did not send additional ask for block commands.
            let ask_for_block_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
                _ => None,
            };

            let got_more_commands = network_controller
                .wait_command(100.into(), ask_for_block_cmd_filter)
                .await;
            assert!(
                got_more_commands.is_none(),
                "unexpected command {:?}",
                got_more_commands
            );
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
async fn test_someone_knows_it() {
    // start
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let _node_b = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let node_c = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();

            // 2. Create a block coming from node 0.
            let block = tools::create_block(&node_a.private_key, &node_a.id.0);
            let hash_1 = block.header.content.compute_id().unwrap();
            // end set up

            // node c must know about block
            network_controller
                .send_header(node_c.id, block.header.clone())
                .await;

            match protocol_event_receiver.wait_event().await.unwrap() {
                ProtocolEvent::ReceivedBlockHeader { .. } => {}
                _ => panic!("unexpected protocol event"),
            };

            // send wishlist
            protocol_command_sender
                .send_wishlist_delta(
                    vec![hash_1].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .unwrap();

            assert_hash_asked_to_node(hash_1, node_c.id, &mut network_controller).await;

            // node C replied with the block
            network_controller.send_block(node_c.id, block).await;

            // 7. Make sure protocol did not send additional ask for block commands.
            let ask_for_block_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
                _ => None,
            };

            let got_more_commands = network_controller
                .wait_command(100.into(), ask_for_block_cmd_filter)
                .await;
            assert!(
                got_more_commands.is_none(),
                "unexpected command {:?}",
                got_more_commands
            );
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
async fn test_dont_want_it_anymore() {
    // start
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let _node_b = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();

            // 2. Create a block coming from node 0.
            let block = tools::create_block(&node_a.private_key, &node_a.id.0);
            let hash_1 = block.header.content.compute_id().unwrap();
            // end set up

            // send wishlist
            protocol_command_sender
                .send_wishlist_delta(
                    vec![hash_1].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .unwrap();

            // assert it was asked to node A
            assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;

            // we don't want it anymore
            protocol_command_sender
                .send_wishlist_delta(
                    Set::<BlockId>::default(),
                    vec![hash_1].into_iter().collect(),
                )
                .await
                .unwrap();

            // 7. Make sure protocol did not send additional ask for block commands.
            let ask_for_block_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
                _ => None,
            };

            let got_more_commands = network_controller
                .wait_command(100.into(), ask_for_block_cmd_filter)
                .await;
            assert!(
                got_more_commands.is_none(),
                "unexpected command {:?}",
                got_more_commands
            );
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
async fn test_no_one_has_it() {
    // start
    let protocol_settings = &tools::PROTOCOL_SETTINGS;

    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let node_b = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let node_c = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();

            // 2. Create a block coming from node 0.
            let block = tools::create_block(&node_a.private_key, &node_a.id.0);
            let hash_1 = block.header.content.compute_id().unwrap();
            // end set up

            // send wishlist
            protocol_command_sender
                .send_wishlist_delta(
                    vec![hash_1].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .unwrap();

            // assert it was asked to node A
            assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;

            // node a replied is does not have it
            network_controller
                .send_block_not_found(node_a.id, hash_1)
                .await;

            assert_hash_asked_to_node(hash_1, node_b.id, &mut network_controller).await;
            assert_hash_asked_to_node(hash_1, node_c.id, &mut network_controller).await;
            assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;
            assert_hash_asked_to_node(hash_1, node_b.id, &mut network_controller).await;
            assert_hash_asked_to_node(hash_1, node_c.id, &mut network_controller).await;

            // 7. Make sure protocol did not send additional ask for block commands.
            let ask_for_block_cmd_filter = |cmd| match cmd {
                cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
                _ => None,
            };

            let got_more_commands = network_controller
                .wait_command(100.into(), ask_for_block_cmd_filter)
                .await;
            assert!(
                got_more_commands.is_none(),
                "unexpected command {:?}",
                got_more_commands
            );
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
async fn test_multiple_blocks_without_a_priori() {
    // start
    let protocol_settings = &tools::PROTOCOL_SETTINGS;

    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let _node_b = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();
            let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
                .await
                .pop()
                .unwrap();

            // 2. Create two blocks coming from node 0.
            let block_1 = tools::create_block(&node_a.private_key, &node_a.id.0);
            let hash_1 = block_1.header.content.compute_id().unwrap();

            let block_2 = tools::create_block(&node_a.private_key, &node_a.id.0);
            let hash_2 = block_2.header.content.compute_id().unwrap();

            // node a is disconnected so no node knows about wanted blocks
            network_controller.close_connection(node_a.id).await;
            // end set up

            // send wishlist
            protocol_command_sender
                .send_wishlist_delta(
                    vec![hash_1, hash_2].into_iter().collect(),
                    Set::<BlockId>::default(),
                )
                .await
                .unwrap();

            let list = asked_list(&mut network_controller).await;
            for (node_id, set) in list.into_iter() {
                // assert we ask one block per node
                assert_eq!(
                    set.len(),
                    1,
                    "node {:?} was asked {:?} blocks",
                    node_id,
                    set.len()
                );
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
