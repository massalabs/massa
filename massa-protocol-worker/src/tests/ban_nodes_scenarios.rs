// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::HashSet;
use std::time::Duration;

use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
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
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver,
              selector_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Create a block with bad public key.
            let mut block = tools::create_block(&node_a_keypair);
            block.content.header.content_creator_pub_key =
                KeyPair::generate(0).unwrap().get_public_key();
            //end setup

            //3. Send header to protocol.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header))),
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
            match consensus_event_receiver.wait_command(MassaTime::from_millis(500), |_| Some(())) {
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
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_bans_node_sending_operation_with_invalid_signature() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver,
              selector_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Create a operation with bad public key.
            let mut operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);
            operation.content_creator_pub_key = KeyPair::generate(0).unwrap().get_public_key();
            //end setup

            //3. Send operation to protocol.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation])),
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
            match pool_event_receiver.wait_command(MassaTime::from_millis(500), |_| Some(())) {
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
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_bans_node_sending_header_with_invalid_signature() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver,
              selector_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            //2. Creates 2 ops
            let operation_1 = tools::create_operation_with_expire_period(&node_a_keypair, 1);
            let operation_2 = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Create a block from the operation
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, 1),
                vec![operation_1],
            );

            //4. Node A send the block
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
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
                    Message::Block(Box::new(BlockMessage::DataResponse {
                        block_id: block.id,
                        block_info: BlockInfoReply::OperationIds(vec![operation_2.id]),
                    })),
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
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, _node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //9. Create a new block with the operation 2
            let block_2 = tools::create_block_with_operations(
                &node_b_keypair,
                Slot::new(1, 1),
                vec![operation_2],
            );

            //10. Node A tries to send it
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block_2.content.header))),
                )
                .expect_err("Node A should not be able to send a block");
            std::thread::sleep(std::time::Duration::from_millis(1000));
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_does_not_asks_for_block_from_banned_node_who_propagated_header() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver,
              selector_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Create a block.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Send header to protocol.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
                )
                .unwrap();

            //5. Check that protocol does send block to consensus.
            match consensus_event_receiver.wait_command(
                MassaTime::from_millis(500),
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
            let keypair = KeyPair::generate(0).unwrap();
            let mut block = tools::create_block(&keypair);
            block.content.header.content_creator_pub_key =
                KeyPair::generate(0).unwrap().get_public_key();
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
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
                    vec![(expected_hash, Some(block.content.header))]
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
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_bans_all_nodes_propagating_an_attack_attempt() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver,
              selector_event_receiver| {
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (node_b_peer_id, _node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Create a block.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Send header to protocol from the two nodes.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
                )
                .unwrap();

            //4. Check that protocol does send block to consensus the first time.
            match consensus_event_receiver.wait_command(
                MassaTime::from_millis(500),
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
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
                )
                .unwrap();
            //5. Check that protocol does send block to consensus the second time.
            match consensus_event_receiver.wait_command(MassaTime::from_millis(500), |_| Some(())) {
                Some(()) => panic!("Protocol should not send block to consensus"),
                None => {}
            }
            //6. Connect a new node that is not involved in the attack.
            let node_c_keypair = KeyPair::generate(0).unwrap();
            let (node_c_peer_id, _node_c) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_c_keypair.get_public_key()));

            //7. Notify protocol of the attack
            protocol_controller.notify_block_attack(block.id).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1000));

            //8. Check all nodes are banned except node C.
            assert_eq!(
                network_controller
                    .get_connections()
                    .get_peer_ids_connected(),
                [node_c_peer_id].into_iter().collect::<HashSet<PeerId>>()
            );
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
                selector_event_receiver,
            )
        },
    )
}
