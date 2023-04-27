// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::time::Duration;

use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_protocol_exports_2::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use peernet::peer_id::PeerId;
use serial_test::serial;

use crate::{
    handlers::{
        block_handler::{BlockInfoReply, BlockMessage},
        operation_handler::OperationMessage,
    },
    messages::Message,
    wrap_network::ActiveConnectionsTrait,
};

use super::{context::protocol_test, tools::assert_hash_asked_to_node};

#[test]
#[serial]
fn test_protocol_bans_node_sending_block_header_with_invalid_signature() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate();
            let (node_a_peer_id, _node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block with bad public key.
            let mut block = tools::create_block(&node_a_keypair);
            block.content.header.content_creator_pub_key = KeyPair::generate().get_public_key();
            //end setup

            //3. Send header to protocol.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            std::thread::sleep(std::time::Duration::from_millis(1000));
            //4. Check that node connection is closed (node should be banned)
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected()
                    .len(),
                0
            );

            //5. Check that protocol does not send block to consensus.
            match consensus_event_receiver.wait_command(MassaTime::from_millis(100), |_| Some(())) {
                Some(()) => {
                    panic!("Protocol sent block to consensus.");
                }
                None => {}
            }
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_bans_node_sending_operation_with_invalid_signature() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate();
            let (node_a_peer_id, _node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a operation with bad public key.
            let mut operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);
            operation.content_creator_pub_key = KeyPair::generate().get_public_key();
            //end setup

            //3. Send operation to protocol.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            std::thread::sleep(std::time::Duration::from_millis(1000));
            //4. Check that node connection is closed (node should be banned)
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected()
                    .len(),
                0
            );

            //5. Check that protocol does not send operation to pool.
            match pool_event_receiver.wait_command(MassaTime::from_millis(100), |_| Some(())) {
                Some(()) => {
                    panic!("Protocol sent block to consensus.");
                }
                None => {}
            }
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_bans_node_sending_header_with_invalid_signature() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            //2. Creates 2 ops
            let operation_1 = tools::create_operation_with_expire_period(&node_a_keypair, 1);
            let operation_2 = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Create a block from the operation
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, 1),
                vec![operation_1.clone()],
            );

            //4. Node A send the block
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            // 5. Send wishlist
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            assert_hash_asked_to_node(&node_a, &block.id);

            //6. Node A sends block info with bad ops list
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![operation_2.id].into_iter().collect()),
                    )]))),
                )
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1000));
            //7. Check that node connection is closed (node should be banned)
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected()
                    .len(),
                0
            );

            //8. Create a new node
            let node_b_keypair = KeyPair::generate();
            let (_node_b_peer_id, _node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //9. Create a new block with the operation 2
            let block_2 = tools::create_block_with_operations(
                &node_b_keypair,
                Slot::new(1, 1),
                vec![operation_2.clone()],
            );

            //10. Node A tries to send it
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block_2.content.header.clone(),
                    ))),
                )
                .expect_err("Node A should not be able to send a block");
            std::thread::sleep(std::time::Duration::from_millis(1000));
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_does_not_asks_for_block_from_banned_node_who_propagated_header() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Send header to protocol.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            //5. Check that protocol does send block to consensus.
            match consensus_event_receiver.wait_command(
                MassaTime::from_millis(100),
                |evt| match evt {
                    MockConsensusControllerMessage::RegisterBlockHeader {
                        block_id,
                        header: _,
                    } => {
                        assert_eq!(block_id, block.id);
                        Some(())
                    }
                    _ => None,
                },
            ) {
                Some(()) => {}
                None => {
                    panic!("Protocol should send block to consensus");
                }
            }
            let expected_hash = block.id;
            //6. Get node A banned
            // New keypair to avoid getting same block id
            let keypair = KeyPair::generate();
            let mut block = tools::create_block(&keypair);
            block.content.header.content_creator_pub_key = KeyPair::generate().get_public_key();
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();
            //7. Check that node connection is closed (node should be banned)
            std::thread::sleep(std::time::Duration::from_millis(1000));
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected()
                    .len(),
                0
            );
            //8. Send a wishlist that ask for the first block
            protocol_controller
                .send_wishlist_delta(
                    vec![(expected_hash, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();
            let _ = node_a
                .recv_timeout(Duration::from_millis(1000))
                .expect_err("Node A should not receive a ask block");
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_bans_all_nodes_propagating_an_attack_attempt() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let (node_a_peer_id, _node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, _node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Send header to protocol from the two nodes.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();
            //4. Check that protocol does send block to consensus the first time.
            match consensus_event_receiver.wait_command(
                MassaTime::from_millis(100),
                |evt| match evt {
                    MockConsensusControllerMessage::RegisterBlockHeader {
                        block_id,
                        header: _,
                    } => {
                        assert_eq!(block_id, block.id);
                        Some(())
                    }
                    _ => None,
                },
            ) {
                Some(()) => {}
                None => {
                    panic!("Protocol should send block to consensus");
                }
            }
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();
            //5. Check that protocol does send block to consensus the second time.
            match consensus_event_receiver.wait_command(MassaTime::from_millis(100), |_| Some(())) {
                Some(()) => panic!("Protocol should not send block to consensus"),
                None => {}
            }
            //6. Connect a new node that don't known about the attack.
            let node_c_keypair = KeyPair::generate();
            let (_node_c_peer_id, _node_c) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_c_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //7. Notify the attack
            protocol_controller.notify_block_attack(block.id).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1000));
            //8. Check that there is only node C not banned.
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected()
                    .len(),
                1
            );
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected()
                    .iter()
                    .next()
                    .unwrap(),
                &_node_c_peer_id
            );
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

//TODO: Is this behavior still wanted ?
// #[tokio::test]
// #[serial]
// async fn test_protocol_removes_banned_node_on_disconnection() {
//     let protocol_config = &tools::PROTOCOL_CONFIG;
//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     protocol_command_sender,
//                     protocol_manager,
//                     mut protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

//             let creator_node = nodes.pop().expect("Failed to get node info.");

//             // Get the node banned.
//             let mut block = tools::create_block(&creator_node.keypair);
//             block.content.header.id = BlockId::new(Hash::compute_from("invalid".as_bytes()));
//             network_controller
//                 .send_header(creator_node.id, block.content.header)
//                 .await;
//             tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

//             // Close the connection.
//             network_controller.close_connection(creator_node.id).await;

//             // Re-connect the node.
//             network_controller.new_connection(creator_node.id).await;

//             // The node is not banned anymore.
//             let block = tools::create_block(&creator_node.keypair);
//             network_controller
//                 .send_header(creator_node.id, block.content.header.clone())
//                 .await;

//             // Check protocol sends header to consensus.
//             let (protocol_consensus_event_receiver, received_hash) =
//                 tokio::task::spawn_blocking(move || {
//                     let id = protocol_consensus_event_receiver
//                         .wait_command(MassaTime::from_millis(1000), |command| match command {
//                             MockConsensusControllerMessage::RegisterBlockHeader {
//                                 block_id,
//                                 header: _,
//                             } => Some(block_id),
//                             _ => panic!("unexpected protocol event"),
//                         })
//                         .unwrap();
//                     (protocol_consensus_event_receiver, id)
//                 })
//                 .await
//                 .unwrap();

//             // Check that protocol sent the right header to consensus.
//             let expected_hash = block.id;
//             assert_eq!(expected_hash, received_hash);
//             (
//                 network_controller,
//                 protocol_command_sender,
//                 protocol_manager,
//                 protocol_consensus_event_receiver,
//                 protocol_pool_event_receiver,
//             )
//         },
//     )
//     .await;
// }

// #[tokio::test]
// #[serial]
// async fn test_protocol_bans_node_sending_operation_with_size_bigger_than_max_block_size() {
//     let protocol_config = &tools::PROTOCOL_CONFIG;
//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     protocol_event_receiver,
//                     protocol_command_sender,
//                     protocol_manager,
//                     mut pool_event_receiver| {
//             // Create 1 node.
//             let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

//             let creator_node = nodes.pop().expect("Failed to get node info.");

//             // 1. Create an operation
//             let mut operation =
//                 tools::create_operation_with_expire_period(&creator_node.keypair, 1);

//             // 2. Change the serialized data
//             operation.serialized_data = vec![1; 500_001];

//             // 3. Send block to protocol.
//             network_controller
//                 .send_operations(creator_node.id, vec![operation])
//                 .await;

//             // The node is banned.
//             tools::assert_banned_nodes(vec![creator_node.id], &mut network_controller).await;

//             // Check protocol does not send operation to pool.
//             pool_event_receiver.wait_command(1000.into(), |evt| match evt {
//                 evt @ MockPoolControllerMessage::AddOperations { .. } => Some(evt),
//                 _ => None,
//             });
//             (
//                 network_controller,
//                 protocol_event_receiver,
//                 protocol_command_sender,
//                 protocol_manager,
//                 pool_event_receiver,
//             )
//         },
//     )
//     .await;
// }
