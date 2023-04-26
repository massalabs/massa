use std::time::Duration;

use massa_hash::Hash;
use massa_models::{
    block_id::BlockId,
    endorsement::{Endorsement, EndorsementSerializer},
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::test_exports::MockPoolControllerMessage;
use massa_protocol_exports_2::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use peernet::peer_id::PeerId;
use serial_test::serial;

use crate::{
    handlers::{block_handler::BlockMessage, endorsement_handler::EndorsementMessage},
    messages::Message,
};

use super::context::protocol_test;

#[test]
#[serial]
fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
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
            //1. Create 1 nodes
            let node_a_keypair = KeyPair::generate();
            let (node_a_peer_id, _node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create an endorsement
            let endorsement = tools::create_endorsement();

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

            //3. Check protocol sends endorsements to pool.
            let received_endorsements =
                match pool_event_receiver.wait_command(1500.into(), |evt| match evt {
                    evt @ MockPoolControllerMessage::AddEndorsements { .. } => Some(evt),
                    _ => None,
                }) {
                    Some(MockPoolControllerMessage::AddEndorsements { endorsements, .. }) => {
                        endorsements
                    }
                    _ => panic!("Unexpected or no protocol pool event."),
                };
            assert!(received_endorsements
                .get_endorsement_refs()
                .contains(&endorsement.id));
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
fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
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
            //1. Create 1 nodes
            let node_a_keypair = KeyPair::generate();
            let (node_a_peer_id, _node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create an endorsement
            let mut endorsement = tools::create_endorsement();

            //3. Change endorsement to make signature is invalid
            endorsement.content_creator_pub_key = node_a_keypair.get_public_key().clone();

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

            //4. Check protocol does not send endorsements to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddEndorsements { .. } => {
                    panic!("Protocol send invalid endorsements.")
                }
                _ => Some(MockPoolControllerMessage::Any),
            });
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
fn test_protocol_propagates_endorsements_to_active_nodes() {
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
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            let (_node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create an endorsement
            let endorsement = tools::create_endorsement();

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

            //3. Check protocol sends endorsements to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddEndorsements { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            });

            //4. Check that we propagated the endorsement to the node B that don't know it but not to A that already know it
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("Node A should not receive the endorsement");
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B should receive the endorsement");
            match msg {
                Message::Endorsement(EndorsementMessage::Endorsements(endorsements)) => {
                    assert_eq!(endorsements.len(), 1);
                    assert_eq!(endorsements[0], endorsement);
                }
                _ => panic!("Unexpected message type"),
            }
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        }
    )
}

#[test]
#[serial]
fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration() {
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
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate();
            let node_b_keypair = KeyPair::generate();
            let (node_a_peer_id, node_a) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_a_keypair.get_public_key().to_bytes()).unwrap(),
            );
            println!("Node A peer id: {:?}", node_a_peer_id);
            let (node_b_peer_id, node_b) = network_controller.create_fake_connection(
                PeerId::from_bytes(node_b_keypair.get_public_key().to_bytes()).unwrap(),
            );

            //2. Create an endorsement
            let content = Endorsement {
                slot: Slot::new(0, 1),
                index: 0,
                endorsed_block: BlockId(Hash::compute_from(&[])),
            };
            let endorsement =
                Endorsement::new_verifiable(content, EndorsementSerializer::new(), &node_a_keypair)
                    .unwrap();
            println!("Endorsement id: {:?}", endorsement.id);
            println!(
                "Endorsement content serialized: {:?}",
                endorsement.serialized_data
            );
            //3. Creates a block with the endorsement
            let block = tools::create_block_with_endorsements(
                &node_a_keypair,
                Slot::new(1, 1),
                vec![endorsement.clone()],
            );
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

            //3. Check protocol sends endorsements to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddEndorsements { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            });

            //4. Check that we propagated the endorsement to the node B that don't know it but not to A that already know it through the block
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("Node A should not receive the endorsement");
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B should receive the endorsement");
            match msg {
                Message::Endorsement(EndorsementMessage::Endorsements(endorsements)) => {
                    assert_eq!(endorsements.len(), 1);
                    assert_eq!(endorsements[0], endorsement);
                }
                _ => panic!("Unexpected message type"),
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

// #[tokio::test]
// #[serial]
// async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header(
// ) {
//     let protocol_config = &tools::PROTOCOL_CONFIG;
//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     protocol_command_sender,
//                     protocol_manager,
//                     mut protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             // Create 2 nodes.
//             let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

//             let address = Address::from_public_key(&nodes[0].id.get_public_key());
//             let thread = address.get_thread(2);

//             let endorsement = tools::create_endorsement();
//             let endorsement_id = endorsement.id;

//             let block = tools::create_block_with_endorsements(
//                 &nodes[0].keypair,
//                 Slot::new(1, thread),
//                 vec![endorsement.clone()],
//             );

//             // Node 2 sends block, resulting in endorsements noted in block info.
//             // TODO: rewrite

//             // Node 1 sends header, resulting in protocol using the block info to determine
//             // the node knows about the endorsements contained in the block header.
//             network_controller
//                 .send_header(nodes[0].id, block.content.header.clone())
//                 .await;

//             // Wait for the event to be sure that the node is connected,
//             // and noted as knowing the block and its endorsements.
//             let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
//                 protocol_consensus_event_receiver.wait_command(
//                     MassaTime::from_millis(1000),
//                     |command| match command {
//                         MockConsensusControllerMessage::RegisterBlockHeader { .. } => Some(()),
//                         _ => panic!("Node isn't connected or didn't mark block as known."),
//                     },
//                 );
//                 protocol_consensus_event_receiver
//             })
//             .await
//             .unwrap();

//             // Send the endorsement to protocol
//             // it should not propagate to the node that already knows about it
//             // because of the previously received header.
//             let mut sender = protocol_command_sender.clone();
//             thread::spawn(move || {
//                 let mut storage = Storage::create_root();
//                 storage.store_endorsements(vec![endorsement]);
//                 sender.propagate_endorsements(storage).unwrap();
//             });

//             match network_controller
//                 .wait_command(1000.into(), |cmd| match cmd {
//                     cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
//                     _ => None,
//                 })
//                 .await
//             {
//                 Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
//                     let id = endorsements[0].id;
//                     assert_eq!(id, endorsement_id);
//                     if nodes[0].id == node {
//                         panic!("Unexpected propagated of endorsement.");
//                     }
//                 }
//                 None => {}
//                 Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
//             };

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
// async fn test_protocol_does_not_propagates_endorsements_when_receiving_those_inside_a_header() {
//     let protocol_config = &tools::PROTOCOL_CONFIG;
//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     protocol_command_sender,
//                     protocol_manager,
//                     protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             // Create 2 nodes.
//             let mut nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

//             // 1. Create an endorsement
//             let endorsement = tools::create_endorsement();

//             let creator_node = nodes.pop().expect("Failed to get node info.");

//             // 2. Create a block coming from node creator_node.
//             let mut block = tools::create_block(&creator_node.keypair);

//             // 3. Add endorsement to block
//             block.content.header.content.endorsements = vec![endorsement.clone()];

//             // 4. Send header to protocol.
//             network_controller
//                 .send_header(creator_node.id, block.content.header.clone())
//                 .await;

//             let expected_endorsement_id = endorsement.id;

//             // 5. Check that the endorsements included in the header are propagated.
//             loop {
//                 match network_controller
//                     .wait_command(1000.into(), |cmd| match cmd {
//                         cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
//                         _ => None,
//                     })
//                     .await
//                 {
//                     Some(NetworkCommand::SendEndorsements {
//                         node: _node,
//                         endorsements,
//                     }) => {
//                         let id = endorsements[0].id;
//                         assert_eq!(id, expected_endorsement_id);
//                         panic!("Unexpected propagation of endorsement.");
//                     }
//                     Some(_) => panic!("Unexpected network command.."),
//                     None => break,
//                 };
//             }
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
