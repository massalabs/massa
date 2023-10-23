use std::time::Duration;

use massa_consensus_exports::MockConsensusController;
use massa_hash::Hash;
use massa_models::{
    address::Address,
    block_id::BlockId,
    endorsement::{Endorsement, EndorsementSerializer},
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::{MockSelectorController, Selection};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;

use crate::{
    handlers::{block_handler::BlockMessage, endorsement_handler::EndorsementMessage},
    messages::Message,
};

use super::context::protocol_test;

#[test]
fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let protocol_config = ProtocolConfig {
        thread_count: 2,
        initial_peers: "./src/tests/empty_initial_peers.json".to_string().into(),
        ..Default::default()
    };
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let endorsement = tools::create_endorsement();
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_endorsements()
            .return_once(move |endorsements_storage| {
                let stored_endorsements = endorsements_storage.get_endorsement_refs();
                assert_eq!(stored_endorsements.len(), 1);
                assert!(stored_endorsements.contains(&endorsement.id));
            });
        pool_controller
    });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller.expect_clone_box().returning(move || {
        let mut selector_controller = Box::new(MockSelectorController::new());
        selector_controller
            .expect_get_selection()
            .return_once(move |slot| {
                assert_eq!(slot, endorsement.content.slot);
                Ok(Selection {
                    endorsements: vec![endorsement.content_creator_address; 1],
                    producer: endorsement.content_creator_address,
                })
            });
        selector_controller
    });
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 1 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

            std::thread::sleep(std::time::Duration::from_millis(1000));
        },
    )
}

#[test]
fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        initial_peers: "./src/tests/empty_initial_peers.json".to_string().into(),
        ..Default::default()
    };
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut endorsement = tools::create_endorsement();
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_endorsements()
            .return_once(move |endorsements_storage| {
                let stored_endorsements = endorsements_storage.get_endorsement_refs();
                assert_eq!(stored_endorsements.len(), 1);
                assert!(stored_endorsements.contains(&endorsement.id));
            });
        pool_controller
    });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller.expect_clone_box().returning(move || {
        let mut selector_controller = Box::new(MockSelectorController::new());
        selector_controller
            .expect_get_selection()
            .return_once(move |slot| {
                assert_eq!(slot, endorsement.content.slot);
                Ok(Selection {
                    endorsements: vec![endorsement.content_creator_address; 1],
                    producer: endorsement.content_creator_address,
                })
            });
        selector_controller
    });
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 1 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Change endorsement to make signature is invalid
            endorsement.content_creator_pub_key = node_a_keypair.get_public_key();

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement])),
                )
                .unwrap();
        },
    )
}

#[test]
fn test_protocol_propagates_endorsements_to_active_nodes() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let protocol_config = ProtocolConfig {
        thread_count: 2,
        initial_peers: "./src/tests/empty_initial_peers.json".to_string().into(),
        ..Default::default()
    };
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let endorsement = tools::create_endorsement();
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_endorsements()
            .return_once(move |endorsements_storage| {
                let stored_endorsements = endorsements_storage.get_endorsement_refs();
                assert_eq!(stored_endorsements.len(), 1);
                assert!(stored_endorsements.contains(&endorsement.id));
            });
        pool_controller
    });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller.expect_clone_box().returning(move || {
        let mut selector_controller = Box::new(MockSelectorController::new());
        selector_controller
            .expect_get_selection()
            .return_once(move |slot| {
                assert_eq!(slot, endorsement.content.slot);
                Ok(Selection {
                    endorsements: vec![endorsement.content_creator_address; 1],
                    producer: endorsement.content_creator_address,
                })
            });
        selector_controller
    });
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Endorsement(EndorsementMessage::Endorsements(vec![
                        endorsement.clone()
                    ])),
                )
                .unwrap();

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
        },
    )
}

#[test]
fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let protocol_config = ProtocolConfig {
        thread_count: 2,
        initial_peers: "./src/tests/empty_initial_peers.json".to_string().into(),
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let address_block_creator = Address::from_public_key(&block_creator.get_public_key());
    let content = Endorsement {
        slot: Slot::new(1, 1),
        index: 0,
        endorsed_block: BlockId::generate_from_hash(Hash::compute_from("Genesis 1".as_bytes())),
    };
    let endorsement =
        Endorsement::new_verifiable(content, EndorsementSerializer::new(), &block_creator).unwrap();
    //3. Creates a block with the endorsement
    let block = tools::create_block_with_endorsements(
        &block_creator,
        Slot::new(1, 1),
        vec![endorsement.clone()],
    );
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    consensus_controller
        .expect_register_block_header()
        .returning(move |block_id, block| {
            assert_eq!(block_id, block.id);
        });
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_endorsements()
            .return_once(move |endorsements_storage| {
                let stored_endorsements = endorsements_storage.get_endorsement_refs();
                assert_eq!(stored_endorsements.len(), 1);
                assert!(stored_endorsements.contains(&endorsement.id));
            });
        pool_controller
    });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller.expect_clone_box().returning(move || {
        let mut selector_controller = Box::new(MockSelectorController::new());
        selector_controller
            .expect_get_selection()
            .return_once(move |slot| {
                assert_eq!(slot, endorsement.content.slot);
                Ok(Selection {
                    endorsements: vec![address_block_creator; 1],
                    producer: address_block_creator,
                })
            });
        selector_controller
    });
    selector_controller
        .expect_get_selection()
        .return_once(move |slot| {
            assert_eq!(slot, block.content.header.content.slot);
            Ok(Selection {
                endorsements: vec![address_block_creator; 1],
                producer: address_block_creator,
            })
        });
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, mut storage, protocol_controller| {
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header))),
                )
                .unwrap();

            std::thread::sleep(Duration::from_millis(300));
            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously received header.
            storage.store_endorsements(vec![endorsement.clone()]);
            protocol_controller.propagate_endorsements(storage).unwrap();

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
        },
    )
}
