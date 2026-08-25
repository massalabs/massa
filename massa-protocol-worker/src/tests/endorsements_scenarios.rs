use massa_hash::Hash;
use massa_models::block_id::BlockId;
use massa_models::config::MAX_ENDORSEMENTS_PER_SLOT_INDEX;
use massa_models::endorsement::{Endorsement, EndorsementSerializer};
use massa_models::secure_share::SecureShareContent;
use massa_models::slot::Slot;
use massa_pos_exports::Selection;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::ProtocolConfig;
use massa_signature::KeyPair;
use massa_test_framework::{TestUniverse, WaitPoint};
use massa_time::MassaTime;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::{
    handlers::{block_handler::BlockMessage, endorsement_handler::EndorsementMessage},
    messages::Message,
    wrap_network::MockActiveConnectionsTraitWrapper,
};

use super::universe::{ProtocolForeignControllers, ProtocolTestUniverse};

#[test]
fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let peer_ids = [node_a_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&endorsement.id));
                    waitpoint_trigger_handle.trigger();
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement.clone()])),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let peer_ids = [node_a_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let mut endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));
    endorsement.content_creator_pub_key = node_a_keypair.get_public_key();

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections
            .expect_shutdown_connection()
            .returning(move |_| ());
    });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            waitpoint_trigger_handle.trigger();
        });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller.expect_add_endorsements().never();
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement.clone()])),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_propagates_endorsements_to_active_nodes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let peer_ids = [node_a_peer_id, node_b_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));
    let endorsement_clone = endorsement.clone();

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections.expect_send_to_peer().returning(
            move |peer_id, _message_serializer, message, _high_priority| {
                assert_eq!(peer_id, &node_b_peer_id);
                match message {
                    Message::Endorsement(EndorsementMessage::Endorsements(endorsements)) => {
                        assert_eq!(endorsements.len(), 1);
                        assert_eq!(endorsements[0], endorsement_clone);
                        waitpoint_trigger_handle.trigger();
                    }
                    _ => panic!("Unexpected message type"),
                }
                Ok(())
            },
        );
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&endorsement.id));
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement.clone()])),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let peer_ids = [node_a_peer_id, node_b_peer_id];
    let block_creator = KeyPair::generate(0).unwrap();
    let endorsement = ProtocolTestUniverse::create_endorsement(&block_creator, Slot::new(1, 1));
    let endorsement_clone = endorsement.clone();
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, 1),
        vec![],
        vec![endorsement.clone()],
        vec![],
    );

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections.expect_send_to_peer().times(1).returning(
            move |peer_id, _message_serializer, message, _high_priority| {
                assert_eq!(peer_id, &node_b_peer_id);
                match message {
                    Message::Endorsement(EndorsementMessage::Endorsements(endorsements)) => {
                        assert_eq!(endorsements.len(), 1);
                        assert_eq!(endorsements[0], endorsement_clone);
                        waitpoint_trigger_handle.trigger();
                    }
                    _ => panic!("Unexpected message type"),
                }
                Ok(())
            },
        );
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&endorsement.id));
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, block| {
            assert_eq!(block_id, block.id);
        });
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header))),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_does_not_check_stale_endorsements_it_receives() {
    // genesis is far enough in the past for slot (1, 1) to be stale
    // while slot (100, 1) is still fresh
    let t0 = MassaTime::from_millis(16000);
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        t0,
        genesis_timestamp: MassaTime::now().saturating_sub(t0.saturating_mul(100)),
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let peer_ids = [node_a_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let stale_endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));
    let fresh_slot = Slot::new(100, 1);
    let fresh_endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, fresh_slot);

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&fresh_endorsement.id));
                    waitpoint_trigger_handle.trigger();
                });
        });
    // record the slots the PoS draws have been looked up for
    let queried_slots = Arc::new(Mutex::new(Vec::new()));
    let queried_slots_handle = queried_slots.clone();
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .returning(move |slot| {
                    queried_slots_handle.lock().push(slot);
                    Ok(Selection {
                        endorsements: vec![fresh_endorsement.content_creator_address; 1],
                        producer: fresh_endorsement.content_creator_address,
                    })
                });
        });
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![
            stale_endorsement.clone(),
            fresh_endorsement.clone(),
        ])),
    );
    waitpoint.wait();
    // the stale endorsement must never have reached the PoS draws check
    assert_eq!(*queried_slots.lock(), vec![fresh_slot]);
}

#[test]
fn test_protocol_bounds_conflicting_endorsements_for_the_same_draw() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let peer_ids = [node_a_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();

    // the drawn endorser equivocates: same (slot, index), one endorsement per endorsed block
    let endorsements: Vec<_> = (0..(MAX_ENDORSEMENTS_PER_SLOT_INDEX + 2))
        .map(|i| {
            let mut endorsement =
                ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));
            endorsement.content.endorsed_block = BlockId::generate_from_hash(Hash::compute_from(
                format!("conflicting parent {}", i).as_bytes(),
            ));
            Endorsement::new_verifiable(
                endorsement.content,
                EndorsementSerializer::new(),
                &endorsement_creator,
                0,
            )
            .unwrap()
        })
        .collect();
    let endorser_address = endorsements[0].content_creator_address;

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    // only the per-draw bound worth of variants make it through
                    assert_eq!(
                        endorsements_storage.get_endorsement_refs().len(),
                        MAX_ENDORSEMENTS_PER_SLOT_INDEX
                    );
                    waitpoint_trigger_handle.trigger();
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(move |selector_controller| {
            selector_controller
                .expect_get_selection()
                .returning(move |_| {
                    Ok(Selection {
                        endorsements: vec![endorser_address; 1],
                        producer: endorser_address,
                    })
                });
        });
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(endorsements)),
    );
    waitpoint.wait();
}
