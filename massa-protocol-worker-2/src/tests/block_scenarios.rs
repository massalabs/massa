// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::HashSet;
use std::time::Duration;

use crate::handlers::block_handler::{AskForBlocksInfo, BlockInfoReply, BlockMessage};
use crate::messages::Message;

use super::context::{protocol_test, protocol_test_with_storage};
use super::tools::{assert_block_info_sent_to_node, assert_hash_asked_to_node};
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::prehash::PreHashSet;
use massa_models::{block_id::BlockId, slot::Slot};
use massa_protocol_exports_2::test_exports::tools;
use massa_protocol_exports_2::ProtocolConfig;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use peernet::peer_id::PeerId;
use serial_test::serial;

#[test]
#[serial]
fn test_full_ask_block_workflow() {
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
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block coming from node a.
            let op_1 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
            let op_2 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
            let op_thread = op_1
                .content_creator_address
                .get_thread(protocol_config.thread_count);
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, op_thread),
                vec![op_1.clone(), op_2.clone()],
            );
            //end setup

            //3. Send the block header from node a
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            //4. Assert that we register the block header to the consensus
            loop {
                match consensus_event_receiver.wait_command(
                    MassaTime::from_millis(100),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlockHeader {
                            header,
                            block_id,
                        } => {
                            assert_eq!(header.id, block.content.header.id);
                            assert_eq!(block_id, block.id);
                            Some(())
                        }
                        _evt => None,
                    },
                ) {
                    Some(()) => {
                        break;
                    }
                    None => {
                        continue;
                    }
                }
            }

            //5. Send a wishlist that ask for the block
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            //6. Assert that we asked the block to node a then node b
            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);

            //7. Node B answer with the infos
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![op_1.id, op_2.id]),
                    )]))),
                )
                .unwrap();

            //8. Assert that we asked the operations to node b
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B didn't receive the ask for operations message");
            match msg {
                Message::Block(message) => {
                    if let BlockMessage::AskForBlocks(asked) = *message {
                        assert_eq!(asked.len(), 1);
                        assert_eq!(asked[0].0, block.id);
                        assert_eq!(
                            asked[0].1,
                            AskForBlocksInfo::Operations(vec![op_1.id, op_2.id])
                        );
                    } else {
                        panic!("Node B didn't receive the ask for operations message");
                    }
                }
                _ => panic!("Node B didn't receive the ask for operations message"),
            }

            //9. Node B answer with the operations
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Operations(vec![op_1.clone(), op_2.clone()]),
                    )]))),
                )
                .unwrap();

            //10. Assert that we send the block to consensus
            loop {
                match consensus_event_receiver.wait_command(
                    MassaTime::from_millis(100),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlock {
                            slot,
                            block_id,
                            block_storage,
                            created: _,
                        } => {
                            assert_eq!(slot, block.content.header.content.slot);
                            assert_eq!(block_id, block.id);
                            let received_block =
                                block_storage.read_blocks().get(&block_id).cloned().unwrap();
                            assert_eq!(received_block.content.operations, block.content.operations);
                            Some(())
                        }
                        _evt => None,
                    },
                ) {
                    Some(()) => {
                        break;
                    }
                    None => {
                        continue;
                    }
                }
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
fn test_empty_block() {
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
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block coming from node a.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Send the block header from node a
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            //4. Send a wishlist that ask for the block
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            //5. Assert that we asked the block to node a then node b
            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);

            //6. Node B answer with the infos
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![]),
                    )]))),
                )
                .unwrap();

            //7. Assert that we didn't asked any other infos
            let _ = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("A new ask has been sent to node B when we shouldn't send any.");

            //8. Assert that we send the block to consensus
            loop {
                match consensus_event_receiver.wait_command(
                    MassaTime::from_millis(100),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlock {
                            slot,
                            block_id,
                            block_storage,
                            created: _,
                        } => {
                            assert_eq!(slot, block.content.header.content.slot);
                            assert_eq!(block_id, block.id);
                            let received_block =
                                block_storage.read_blocks().get(&block_id).cloned().unwrap();
                            assert_eq!(received_block.content.operations, block.content.operations);
                            Some(())
                        }
                        _evt => None,
                    },
                ) {
                    Some(()) => {
                        break;
                    }
                    None => {
                        continue;
                    }
                }
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
fn test_dont_want_it_anymore() {
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
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block coming from node a.
            let op_1 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
            let op_2 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
            let op_thread = op_1
                .content_creator_address
                .get_thread(protocol_config.thread_count);
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, op_thread),
                vec![op_1.clone(), op_2.clone()],
            );
            //end setup

            //3. Send the block header from node a
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            //4. Send a wishlist that ask for the block
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            //5. Assert that we asked the block to node a then node b
            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);

            //6. Consensus say that it doesn't want the block anymore
            protocol_controller
                .send_wishlist_delta(Default::default(), vec![block.id].into_iter().collect())
                .unwrap();

            //7. Answer the infos from node b
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![op_1.id, op_2.id]),
                    )]))),
                )
                .unwrap();

            //8. Assert that we didn't asked to any other node
            let _ = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("A new ask has been sent to node B when we shouldn't send any.");
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("A new ask has been sent to node B when we shouldn't send any.");

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
fn test_no_one_has_it() {
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
            //1. Create 3 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let node_c_keypair = KeyPair::generate();
            let (_node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (_node_c_peer_id, node_c) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_c_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block coming from node a.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Send a wishlist that ask for the block
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            //4. Assert that we asked the block to node a
            assert_hash_asked_to_node(&node_a, &block.id);

            //5. Node A answer with the not found message
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::NotFound,
                    )]))),
                )
                .unwrap();

            //6. Assert that we asked the block to other nodes
            assert_hash_asked_to_node(&node_b, &block.id);
            assert_hash_asked_to_node(&node_c, &block.id);
            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);
            assert_hash_asked_to_node(&node_c, &block.id);

            //7. Assert that we didn't asked any other infos
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("A new ask has been sent to node B when we shouldn't send any.");
            let _ = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("A new ask has been sent to node B when we shouldn't send any.");
            let _ = node_c
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("A new ask has been sent to node B when we shouldn't send any.");

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
fn test_multiple_blocks_without_a_priori() {
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
            //1. Create 3 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let node_c_keypair = KeyPair::generate();
            let (node_a_peer_id, _node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (_node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (_node_c_peer_id, node_c) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_c_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create 2 block coming from node a.
            let block_1 = tools::create_block(&node_a_keypair);
            let block_2 = tools::create_block(&node_a_keypair);
            //end setup

            network_controller.remove_fake_connection(&node_a_peer_id);

            std::thread::sleep(Duration::from_millis(100));

            //3. Send a wishlist that ask for the two blocks
            protocol_controller
                .send_wishlist_delta(
                    vec![
                        (block_1.id, Some(block_1.content.header.clone())),
                        (block_2.id, Some(block_2.content.header.clone())),
                    ]
                    .into_iter()
                    .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            //4. Assert that we asked a block to node b and c in random order
            let mut to_be_asked_blocks: HashSet<BlockId> =
                vec![block_1.id, block_2.id].into_iter().collect();
            let message = node_b.recv_timeout(Duration::from_millis(1500)).unwrap();
            match message {
                Message::Block(message) => {
                    if let BlockMessage::AskForBlocks(asked) = *message {
                        assert_eq!(asked.len(), 1);
                        to_be_asked_blocks.remove(&asked[0].0);
                    } else {
                        panic!("Node didn't receive the ask for block message");
                    }
                }
                _ => panic!("Node didn't receive the ask for block message"),
            }
            let message = node_c.recv_timeout(Duration::from_millis(1500)).unwrap();
            match message {
                Message::Block(message) => {
                    if let BlockMessage::AskForBlocks(asked) = *message {
                        assert_eq!(asked.len(), 1);
                        to_be_asked_blocks.remove(&asked[0].0);
                    } else {
                        panic!("Node didn't receive the ask for block message");
                    }
                }
                _ => panic!("Node didn't receive the ask for block message"),
            }
            assert_eq!(to_be_asked_blocks.len(), 0);
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
fn test_protocol_sends_blocks_when_asked_for() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver,
              mut storage| {
            //1. Create 3 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let node_c_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (_node_c_peer_id, node_c) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_c_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block coming from node a.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Consensus inform us that a block has been integrated
            storage.store_block(block.clone());
            protocol_controller
                .integrated_block(block.id, storage)
                .unwrap();

            std::thread::sleep(Duration::from_millis(100));
            //4. Two nodes are asking for the block
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::AskForBlocks(vec![(
                        block.id,
                        AskForBlocksInfo::Info,
                    )]))),
                )
                .unwrap();
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::AskForBlocks(vec![(
                        block.id,
                        AskForBlocksInfo::Info,
                    )]))),
                )
                .unwrap();

            //5. Check that protocol send the block to the two nodes
            assert_block_info_sent_to_node(&node_a, &block.id);
            assert_block_info_sent_to_node(&node_b, &block.id);

            //6. Make sure we didn't sent the block info to node c
            let _ = node_c
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node c should receive the header");
            let _ = node_c
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("Node c shouldn't receive the block info");
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
fn test_protocol_propagates_block_to_node_who_asked_for_operations_and_only_header_to_others() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver,
              mut storage| {
            //1. Create 3 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let node_c_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (_node_c_peer_id, node_c) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_c_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create a block coming from node a.
            let block = tools::create_block(&node_a_keypair);
            //end setup

            //3. Node A send the block to us
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            //4. Check that we sent the block header to consensus
            loop {
                match consensus_event_receiver.wait_command(
                    MassaTime::from_millis(100),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlockHeader {
                            header,
                            block_id,
                        } => {
                            assert_eq!(header.id, block.content.header.id);
                            assert_eq!(block_id, block.id);
                            Some(())
                        }
                        _evt => None,
                    },
                ) {
                    Some(()) => {
                        break;
                    }
                    None => {
                        continue;
                    }
                }
            }

            //5. Consensus inform us that a block has been integrated and so we propagate it
            storage.store_block(block.clone());
            protocol_controller
                .integrated_block(block.id, storage)
                .unwrap();

            std::thread::sleep(Duration::from_millis(100));

            //6. Node B is asking for the block
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::AskForBlocks(vec![(
                        block.id,
                        AskForBlocksInfo::Info,
                    )]))),
                )
                .unwrap();

            //7. Verify that we sent the right informations to each node :
            // - node a should receive nothing because he sent the block
            // - node b should receive the block header and the infos as asked
            // - node c should receive the block header only
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("Node a shouldn't receive the block");
            assert_block_info_sent_to_node(&node_b, &block.id);
            let msg = node_c
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node c should receive the block header");
            match msg {
                Message::Block(block_msg) => match *block_msg {
                    BlockMessage::BlockHeader(header) => {
                        assert_eq!(header.id, block.content.header.id);
                    }
                    _ => {
                        panic!("Node c should receive the block header");
                    }
                },
                _ => {
                    panic!("Node c should receive the block header");
                }
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
