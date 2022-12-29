// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test;
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::{address::Address, slot::Slot};
use massa_network_exports::NetworkCommand;
use massa_pool_exports::test_exports::MockPoolControllerMessage;
use massa_protocol_exports::tests::tools;
use massa_storage::Storage;
use massa_time::MassaTime;
use serial_test::serial;
use std::thread;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    mut pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            let expected_endorsement_id = endorsement.id;

            // 3. Send endorsement to protocol.
            network_controller
                .send_endorsements(creator_node.id, vec![endorsement])
                .await;

            // Check protocol sends endorsements to pool.
            let received_endorsements =
                match pool_event_receiver.wait_command(1000.into(), |evt| match evt {
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
                .contains(&expected_endorsement_id));

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    mut pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an endorsement.
            let mut endorsement = tools::create_endorsement();

            // Change the slot, making the signature invalid.
            endorsement.content.slot = Slot::new(1, 1);

            // 3. Send operation to protocol.
            network_controller
                .send_endorsements(creator_node.id, vec![endorsement])
                .await;

            // Check protocol does not send endorsements to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddEndorsements { .. } => {
                    panic!("Protocol send invalid endorsements.")
                }
                _ => Some(MockPoolControllerMessage::Any),
            });

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_protocol_propagates_endorsements_to_active_nodes() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    mut pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            // Send endorsement and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_endorsements(nodes[0].id, vec![endorsement.clone()])
                .await;
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddEndorsements { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            });

            let expected_endorsement_id = endorsement.id;

            let mut sender = protocol_command_sender.clone();
            thread::spawn(move || {
                let mut storage = Storage::create_root();
                storage.store_endorsements(vec![endorsement]);
                sender.propagate_endorsements(storage).unwrap();
            });

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                        let id = endorsements[0].id;
                        assert_eq!(id, expected_endorsement_id);
                        assert_eq!(nodes[1].id, node);
                        break;
                    }
                    _ => panic!("Unexpected or no network command."),
                };
            }
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    mut pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            // Send endorsement and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_endorsements(nodes[0].id, vec![endorsement.clone()])
                .await;
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddEndorsements { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            });

            // create and connect a node that does not know about the endorsement
            let new_nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            // wait for things to settle
            tokio::time::sleep(Duration::from_millis(250)).await;

            let expected_endorsement_id = endorsement.id;

            // send the endorsement to protocol
            // it should propagate it to nodes that don't know about it
            let mut sender = protocol_command_sender.clone();
            thread::spawn(move || {
                let mut storage = Storage::create_root();
                storage.store_endorsements(vec![endorsement]);
                sender.propagate_endorsements(storage).unwrap();
            });

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                        let id = endorsements[0].id;
                        assert_eq!(id, expected_endorsement_id);
                        assert_eq!(new_nodes[0].id, node);
                        break;
                    }
                    None => panic!("no network command"),
                    Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
                };
            }
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration(
) {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.get_public_key());
            let thread = address.get_thread(2);

            let endorsement = tools::create_endorsement();
            let endorsement_id = endorsement.id;

            let block = tools::create_block_with_endorsements(
                &nodes[0].keypair,
                Slot::new(1, thread),
                vec![endorsement.clone()],
            );

            // Send the header,
            // this should note the node as knowing about the endorsement.
            network_controller
                .send_header(nodes[0].id, block.content.header)
                .await;

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously received header.
            let mut sender = protocol_command_sender.clone();
            thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let mut storage = Storage::create_root();
                storage.store_endorsements(vec![endorsement]);
                sender.propagate_endorsements(storage).unwrap();
            });

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                    let id = endorsements[0].id;
                    assert_eq!(id, endorsement_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of endorsement.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
#[ignore]
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_get_block_results(
) {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.get_public_key());
            let thread = address.get_thread(2);

            let endorsement = tools::create_endorsement();
            let endorsement_id = endorsement.id;

            let block = tools::create_block_with_endorsements(
                &nodes[0].keypair,
                Slot::new(1, thread),
                vec![endorsement.clone()],
            );

            // Send the header,
            // this should note the node as knowing about the endorsement.
            network_controller
                .send_header(nodes[0].id, block.content.header)
                .await;

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously integrated block.
            let mut sender = protocol_command_sender.clone();
            thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let mut storage = Storage::create_root();
                storage.store_endorsements(vec![endorsement]);
                sender.propagate_endorsements(storage).unwrap();
            });

            match network_controller
                .wait_command(300.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                    let id = endorsements[0].id;
                    assert_eq!(id, endorsement_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagation of endorsement.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header(
) {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.get_public_key());
            let thread = address.get_thread(2);

            let endorsement = tools::create_endorsement();
            let endorsement_id = endorsement.id;

            let block = tools::create_block_with_endorsements(
                &nodes[0].keypair,
                Slot::new(1, thread),
                vec![endorsement.clone()],
            );

            // Node 2 sends block, resulting in endorsements noted in block info.
            // TODO: rewrite

            // Node 1 sends header, resulting in protocol using the block info to determine
            // the node knows about the endorsements contained in the block header.
            network_controller
                .send_header(nodes[0].id, block.content.header.clone())
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as knowing the block and its endorsements.
            let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
                protocol_consensus_event_receiver.wait_command(
                    MassaTime::from_millis(1000),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlockHeader { .. } => Some(()),
                        _ => panic!("Node isn't connected or didn't mark block as known."),
                    },
                );
                protocol_consensus_event_receiver
            })
            .await
            .unwrap();

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously received header.
            let mut sender = protocol_command_sender.clone();
            thread::spawn(move || {
                let mut storage = Storage::create_root();
                storage.store_endorsements(vec![endorsement]);
                sender.propagate_endorsements(storage).unwrap();
            });

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendEndorsements { node, endorsements }) => {
                    let id = endorsements[0].id;
                    assert_eq!(id, endorsement_id);
                    if nodes[0].id == node {
                        panic!("Unexpected propagated of endorsement.");
                    }
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_propagates_endorsements_when_receiving_those_inside_a_header() {
    let protocol_config = &tools::PROTOCOL_CONFIG;
    protocol_test(
        protocol_config,
        async move |mut network_controller,
                    protocol_command_sender,
                    protocol_manager,
                    protocol_consensus_event_receiver,
                    protocol_pool_event_receiver| {
            // Create 2 nodes.
            let mut nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            // 1. Create an endorsement
            let endorsement = tools::create_endorsement();

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 2. Create a block coming from node creator_node.
            let mut block = tools::create_block(&creator_node.keypair);

            // 3. Add endorsement to block
            block.content.header.content.endorsements = vec![endorsement.clone()];

            // 4. Send header to protocol.
            network_controller
                .send_header(creator_node.id, block.content.header.clone())
                .await;

            let expected_endorsement_id = endorsement.id;

            // 5. Check that the endorsements included in the header are propagated.
            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendEndorsements { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendEndorsements {
                        node: _node,
                        endorsements,
                    }) => {
                        let id = endorsements[0].id;
                        assert_eq!(id, expected_endorsement_id);
                        panic!("Unexpected propagation of endorsement.");
                    }
                    Some(_) => panic!("Unexpected network command.."),
                    None => break,
                };
            }
            (
                network_controller,
                protocol_command_sender,
                protocol_manager,
                protocol_consensus_event_receiver,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}
