// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::time::Duration;

use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;

use crate::{
    handlers::block_handler::{AskForBlocksInfo, BlockInfoReply, BlockMessage},
    messages::Message,
};

use super::{context::protocol_test, tools::assert_hash_asked_to_node};

#[test]
#[serial]
fn test_noting_block_does_not_panic_with_one_max_node_known_blocks_size() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.max_node_known_blocks_size = 1;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

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
                        BlockInfoReply::Operations(vec![op_1, op_2]),
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
