use std::time::Duration;

use massa_hash::Hash;
use massa_models::{
    block_id::BlockId,
    config::ENDORSEMENT_COUNT,
    endorsement::{Endorsement, EndorsementSerializer},
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::test_exports::MockPoolControllerMessage;
use massa_pos_exports::{test_exports::MockSelectorControllerMessage, Selection};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;

use crate::{
    handlers::{block_handler::BlockMessage, endorsement_handler::EndorsementMessage},
    messages::Message,
};

use super::context::{protocol_test, protocol_test_with_storage};

#[test]
#[serial]
fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
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
            //1. Create 1 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

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

            if let Ok(MockSelectorControllerMessage::GetSelection {
                slot: _,
                response_tx,
            }) = selector_event_receiver.recv_timeout(Duration::from_secs(2))
            {
                response_tx
                    .send(Ok(Selection {
                        endorsements: vec![
                            endorsement.content_creator_address;
                            ENDORSEMENT_COUNT as usize
                        ],
                        producer: endorsement.content_creator_address,
                    }))
                    .unwrap();
            } else {
                panic!("Unexpected or no selector selector event.");
            }
            //3. Check protocol sends endorsements to pool.
            let received_endorsements = match pool_event_receiver.wait_command(
                MassaTime::from_millis(1500),
                |evt| match evt {
                    evt @ MockPoolControllerMessage::AddEndorsements { .. } => Some(evt),
                    _ => None,
                },
            ) {
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
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
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
            //1. Create 1 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Create an endorsement
            let mut endorsement = tools::create_endorsement();

            //3. Change endorsement to make signature is invalid
            endorsement.content_creator_pub_key = node_a_keypair.get_public_key();

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

            //4. Check protocol does not send endorsements to pool.
            pool_event_receiver.wait_command(MassaTime::from_millis(1000), |evt| match evt {
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
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_propagates_endorsements_to_active_nodes() {
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
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

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
            if let Ok(MockSelectorControllerMessage::GetSelection {
                slot: _,
                response_tx,
            }) = selector_event_receiver.recv_timeout(Duration::from_secs(2))
            {
                response_tx
                    .send(Ok(Selection {
                        endorsements: vec![
                            endorsement.content_creator_address;
                            ENDORSEMENT_COUNT as usize
                        ],
                        producer: endorsement.content_creator_address,
                    }))
                    .unwrap();
            } else {
                panic!("Unexpected or no selector selector event.");
            }
            //3. Check protocol sends endorsements to pool.
            pool_event_receiver.wait_command(MassaTime::from_millis(1000), |evt| match evt {
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
                selector_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver,
              selector_event_receiver,
              mut storage| {
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Create an endorsement
            let content = Endorsement {
                slot: Slot::new(1, 1),
                index: 0,
                endorsed_block: BlockId(Hash::compute_from("Genesis 1".as_bytes())),
            };
            let endorsement =
                Endorsement::new_verifiable(content, EndorsementSerializer::new(), &node_a_keypair)
                    .unwrap();
            //3. Creates a block with the endorsement
            let block = tools::create_block_with_endorsements(
                &node_a_keypair,
                Slot::new(1, 1),
                vec![endorsement.clone()],
            );
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(block.content.header))),
                )
                .unwrap();

            if let Ok(MockSelectorControllerMessage::GetSelection {
                slot: _,
                response_tx,
            }) = selector_event_receiver.recv_timeout(Duration::from_secs(2))
            {
                response_tx
                    .send(Ok(Selection {
                        endorsements: vec![
                            endorsement.content_creator_address;
                            ENDORSEMENT_COUNT as usize
                        ],
                        producer: endorsement.content_creator_address,
                    }))
                    .unwrap();
            } else {
                panic!("Unexpected or no selector selector event.");
            }

            std::thread::sleep(Duration::from_millis(300));
            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously received header.
            storage.store_endorsements(vec![endorsement.clone()]);
            protocol_controller.propagate_endorsements(storage).unwrap();

            //3. Check protocol sends endorsements to pool.
            pool_event_receiver.wait_command(MassaTime::from_millis(1000), |evt| match evt {
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
                selector_event_receiver,
            )
        },
    )
}
