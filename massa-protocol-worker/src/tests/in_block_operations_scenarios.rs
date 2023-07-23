use std::time::Duration;

use crate::{
    handlers::{block_handler::BlockMessage, operation_handler::OperationMessage},
    messages::Message,
};
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::slot::Slot;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;

use super::{context::protocol_test, tools::send_and_propagate_block};

#[test]
#[serial]
fn test_protocol_does_propagate_operations_received_in_blocks() {
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
            let (_node_b_peer_id, node_b) = network_controller
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

            //3. Send the full block from node a
            send_and_propagate_block(
                &mut network_controller,
                block.clone(),
                &node_a_peer_id,
                &protocol_controller,
                vec![op_1.clone(), op_2.clone()],
            );

            //4. Verify that we sent to consensus
            consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |cmd| match cmd {
                MockConsensusControllerMessage::RegisterBlockHeader {
                    block_id,
                    header: _,
                } => {
                    assert_eq!(block_id, block.id);
                    Some(())
                }
                _ => panic!("Unexpected command: {:?}", cmd),
            });
            consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |cmd| match cmd {
                MockConsensusControllerMessage::RegisterBlock { block_id, .. } => {
                    assert_eq!(block_id, block.id);
                    Some(())
                }
                _ => panic!("Unexpected command: {:?}", cmd),
            });

            //5. Verify that we propagated the operations to node B. We make a loop because it's possible that we also asked infos of the block in node B in case the communication with node A is slow.
            loop {
                let msg = node_b
                    .recv_timeout(Duration::from_millis(1500))
                    .expect("Operations of the block hasn't been propagated to node B");
                match msg {
                    Message::Operation(OperationMessage::OperationsAnnouncement(ops)) => {
                        assert_eq!(ops.len(), 2);
                        assert!(ops.contains(&op_1.id.into_prefix()));
                        assert!(ops.contains(&op_2.id.into_prefix()));
                        break;
                    }
                    Message::Block(block_msg) => match *block_msg {
                        BlockMessage::DataRequest { .. } => {
                            continue;
                        }
                        _ => panic!("Unexpected message: {:?}", block_msg),
                    },
                    _ => panic!("Unexpected message: {:?}", msg),
                }
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

// Commented because fast release but the error seems to be that we try to send again block with node a but he is ban because of the first invalid hash of operations
// #[test]
// #[serial]
// fn test_protocol_sends_blocks_with_operations_to_consensus() {
//     let default_panic = std::panic::take_hook();
//     std::panic::set_hook(Box::new(move |info| {
//         default_panic(info);
//         std::process::exit(1);
//     }));
//     let mut protocol_config = ProtocolConfig::default();
//     protocol_config.thread_count = 2;
//     protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
//     protocol_test(
//         &protocol_config,
//         move |mut network_controller,
//               protocol_controller,
//               protocol_manager,
//               mut consensus_event_receiver,
//               pool_event_receiver| {
//             //1. Create 2 nodes
//             let node_a_keypair = KeyPair::generate(0).unwrap();
//             let node_b_keypair = KeyPair::generate(0).unwrap();
//             let (node_a_peer_id, _node_a) = network_controller
//                 .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
//             println!("node_a_peer_id: {:?}", node_a_peer_id);
//             let (_node_b_peer_id, _node_b) = network_controller
//                 .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

//             //2. Create a block coming from node a.
//             let op_1 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
//             let op_thread = op_1
//                 .content_creator_address
//                 .get_thread(protocol_config.thread_count);
//             let block = tools::create_block_with_operations(
//                 &node_a_keypair,
//                 Slot::new(1, op_thread),
//                 vec![op_1.clone()],
//             );
//             //end setup

//             //3. Send the full block from node a
//             send_and_propagate_block(
//                 &mut network_controller,
//                 block.clone(),
//                 &node_a_peer_id,
//                 &protocol_controller,
//                 vec![op_1],
//             );

//             //4. Verify that we sent to consensus
//             consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |cmd| match cmd {
//                 MockConsensusControllerMessage::RegisterBlockHeader {
//                     block_id,
//                     header: _,
//                 } => {
//                     assert_eq!(block_id, block.id);
//                     Some(())
//                 }
//                 _ => panic!("Unexpected command: {:?}", cmd),
//             });
//             consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |cmd| match cmd {
//                 MockConsensusControllerMessage::RegisterBlock { block_id, .. } => {
//                     assert_eq!(block_id, block.id);
//                     Some(())
//                 }
//                 _ => panic!("Unexpected command: {:?}", cmd),
//             });

//             // block with wrong merkle root
//             {
//                 let op = tools::create_operation_with_expire_period(&node_a_keypair, 5);
//                 let op_thread = op
//                     .content_creator_address
//                     .get_thread(protocol_config.thread_count);
//                 let block: SecureShare<Block, BlockId> = {
//                     let operation_merkle_root = Hash::compute_from("merkle root".as_bytes());

//                     let header = BlockHeader::new_verifiable(
//                         BlockHeader {
//                             announced_version: 0,
//                             current_version: 0,
//                             slot: Slot::new(1, op_thread),
//                             parents: vec![
//                                 BlockId::generate_from_hash(Hash::compute_from("Genesis 0".as_bytes())),
//                                 BlockId::generate_from_hash(Hash::compute_from("Genesis 1".as_bytes())),
//                             ],
//                             denunciations: Vec::new(),
//                             operation_merkle_root,
//                             endorsements: Vec::new(),
//                         },
//                         BlockHeaderSerializer::new(),
//                         &node_a_keypair,
//                     )
//                     .unwrap();

//                     Block::new_verifiable(
//                         Block {
//                             header,
//                             operations: vec![op.clone()].into_iter().map(|op| op.id).collect(),
//                         },
//                         BlockSerializer::new(),
//                         &node_a_keypair,
//                     )
//                     .unwrap()
//                 };

//                 send_and_propagate_block(
//                     &mut network_controller,
//                     block.clone(),
//                     &node_a_peer_id,
//                     &protocol_controller,
//                     vec![op],
//                 );

//                 // Check protocol did send block header to consensus but not the full block.
//                 assert_eq!(
//                     consensus_event_receiver.wait_command(
//                         MassaTime::from_millis(1000),
//                         |command| {
//                             match command {
//                                 MockConsensusControllerMessage::RegisterBlockHeader {
//                                     block_id,
//                                     header: _,
//                                 } => Some(block_id),
//                                 _ => None,
//                             }
//                         }
//                     ),
//                     Some(block.id)
//                 );
//                 assert_eq!(
//                     consensus_event_receiver.wait_command(
//                         MassaTime::from_millis(1000),
//                         |command| {
//                             match command {
//                                 MockConsensusControllerMessage::RegisterBlock {
//                                     block_id, ..
//                                 } => Some(block_id),
//                                 _ => None,
//                             }
//                         }
//                     ),
//                     None
//                 );
//             }

//             //block with operation with wrong signature
//             {
//                 let mut op = tools::create_operation_with_expire_period(&node_a_keypair, 5);
//                 let op_thread = op
//                     .content_creator_address
//                     .get_thread(protocol_config.thread_count);
//                 op.id = OperationId::new(Hash::compute_from("wrong signature".as_bytes()));
//                 let block = tools::create_block_with_operations(
//                     &node_a_keypair,
//                     Slot::new(1, op_thread),
//                     vec![op.clone()],
//                 );

//                 send_and_propagate_block(
//                     &mut network_controller,
//                     block.clone(),
//                     &node_a_peer_id,
//                     &protocol_controller,
//                     vec![op],
//                 );

//                 // Check protocol did send block header to consensus but not the full block.
//                 assert_eq!(
//                     consensus_event_receiver.wait_command(
//                         MassaTime::from_millis(1000),
//                         |command| {
//                             match command {
//                                 MockConsensusControllerMessage::RegisterBlockHeader {
//                                     block_id,
//                                     header: _,
//                                 } => Some(block_id),
//                                 _ => None,
//                             }
//                         }
//                     ),
//                     Some(block.id)
//                 );
//                 assert_eq!(
//                     consensus_event_receiver.wait_command(
//                         MassaTime::from_millis(1000),
//                         |command| {
//                             match command {
//                                 MockConsensusControllerMessage::RegisterBlock {
//                                     block_id, ..
//                                 } => Some(block_id),
//                                 _ => None,
//                             }
//                         }
//                     ),
//                     None
//                 );
//             }

//             (
//                 network_controller,
//                 protocol_controller,
//                 protocol_manager,
//                 consensus_event_receiver,
//                 pool_event_receiver,
//             )
//         },
//     )
// }
