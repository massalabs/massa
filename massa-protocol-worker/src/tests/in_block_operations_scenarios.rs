use std::{collections::HashMap, fs, sync::Arc, time::Duration};

use crate::{
    connectivity::start_connectivity_thread,
    create_protocol_controller,
    handlers::{
        block_handler::BlockMessage, operation_handler::OperationMessage,
        peer_handler::models::PeerDB,
    },
    manager::ProtocolManagerImpl,
    messages::{Message, MessagesHandler},
};
use massa_channel::MassaChannel;
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_consensus_exports::AutoMockConsensusController;
use massa_hash::Hash;
use massa_metrics::MassaMetrics;
use massa_models::{
    block::{Block, BlockSerializer},
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    config::{MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX},
    operation::OperationId,
    secure_share::{Id, SecureShare, SecureShareContent},
    slot::Slot,
};
use massa_pool_exports::AutoMockPoolController;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig, ProtocolManager};
use massa_protocol_exports::{PeerCategoryInfo, PeerId};
use massa_serialization::U64VarIntDeserializer;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use mockall::Sequence;
use parking_lot::RwLock;
use serial_test::serial;
use std::ops::Bound::Included;
use tracing::{debug, log::warn};

use super::{
    context::protocol_test, mock_network::MockNetworkController, tools::send_and_propagate_block,
};

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
              pool_event_receiver| {
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
                        BlockMessage::AskForBlocks(_) => {
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
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_sends_blocks_with_operations_to_consensus() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let protocol_config = &protocol_config;
    let mut mock_pool_controller = Box::new(AutoMockPoolController::new());
    mock_pool_controller
        .expect_clone_box()
        .returning(|| Box::new(AutoMockPoolController::new()));
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let mut consensus_controller = Box::new(AutoMockConsensusController::new());
    let op_1 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
    let op_thread = op_1
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = tools::create_block_with_operations(
        &node_a_keypair,
        Slot::new(1, op_thread),
        vec![op_1.clone()],
    );
    let block_id_reference = block.id.clone();
    let block_id_reference2 = block.id.clone();
    let mut seq = Sequence::new();
    consensus_controller
        .expect_register_block_header()
        .times(1)
        .returning(move |block_id, _| {
            assert_eq!(block_id, block_id_reference);
            ()
        })
        .in_sequence(&mut seq);
    consensus_controller
        .expect_register_block()
        .times(1)
        .returning(move |block_id, _, _, _| {
            assert_eq!(block_id, block_id_reference2);
            ()
        })
        .in_sequence(&mut seq);

    // start protocol controller
    let (network_controller, protocol_controller, protocol_manager) = {
        let config = protocol_config.clone();
        let storage = Storage::create_root();
        // try to read node keypair from file, otherwise generate it & write to file. Then derive nodeId
        let keypair = if std::path::Path::is_file(&config.keypair_file) {
            // file exists: try to load it
            let keypair_bs58_check_encoded = fs::read_to_string(&config.keypair_file)
                .map_err(|err| {
                    std::io::Error::new(
                        err.kind(),
                        format!("could not load node key file: {}", err),
                    )
                })
                .unwrap();
            serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes()).unwrap()
        } else {
            // node file does not exist: generate the key and save it
            let keypair = KeyPair::generate(0).unwrap();
            if let Err(e) = std::fs::write(
                &config.keypair_file,
                serde_json::to_string(&keypair).unwrap(),
            ) {
                warn!("could not generate node key file: {}", e);
            }
            keypair
        };
        debug!("starting protocol controller with mock network");
        let peer_db = Arc::new(RwLock::new(PeerDB::default()));

        let (sender_operations, receiver_operations) = MassaChannel::new(
            "operations".to_string(),
            Some(config.max_size_channel_network_to_operation_handler),
        );
        let (sender_endorsements, receiver_endorsements) = MassaChannel::new(
            "endorsements".to_string(),
            Some(config.max_size_channel_network_to_endorsement_handler),
        );
        let (sender_blocks, receiver_blocks) = MassaChannel::new(
            "blocks".to_string(),
            Some(config.max_size_channel_network_to_block_handler),
        );
        let (sender_peers, receiver_peers) = MassaChannel::new(
            "peers".to_string(),
            Some(config.max_size_channel_network_to_peer_handler),
        );

        // Register channels for handlers
        let message_handlers: MessagesHandler = MessagesHandler {
            sender_blocks: sender_blocks.clone(),
            sender_endorsements: sender_endorsements.clone(),
            sender_operations: sender_operations.clone(),
            sender_peers: sender_peers.clone(),
            id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        };

        let (controller, channels) = create_protocol_controller(config.clone());

        let network_controller = Box::new(MockNetworkController::new(message_handlers.clone()));

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

        let connectivity_thread_handle = start_connectivity_thread(
            PeerId::from_public_key(keypair.get_public_key()),
            network_controller.clone(),
            consensus_controller,
            mock_pool_controller,
            (sender_blocks, receiver_blocks),
            (sender_endorsements, receiver_endorsements),
            (sender_operations, receiver_operations),
            (sender_peers, receiver_peers),
            HashMap::default(),
            peer_db,
            storage,
            channels,
            message_handlers,
            HashMap::default(),
            PeerCategoryInfo {
                max_in_connections_pre_handshake: 10,
                max_in_connections_post_handshake: 10,
                target_out_connections: 10,
                max_in_connections_per_ip: 10,
            },
            config,
            mip_store,
            MassaMetrics::new(false, 32),
        )
        .unwrap();

        let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

        (network_controller, controller, Box::new(manager))
    };

    let (mut protocol_manager,) = (move |mut network_controller: Box<MockNetworkController>,
                                         protocol_controller,
                                         protocol_manager| {
        //1. Create 2 nodes
        let node_b_keypair = KeyPair::generate(0).unwrap();
        let (node_a_peer_id, _node_a) = network_controller
            .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
        println!("node_a_peer_id: {:?}", node_a_peer_id);
        let (_node_b_peer_id, _node_b) = network_controller
            .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

        //2. Create a block coming from node a.
        // let op_1 = tools::create_operation_with_expire_period(&node_a_keypair, 5);
        // let op_thread = op_1
        //     .content_creator_address
        //     .get_thread(protocol_config.thread_count);
        // let block = tools::create_block_with_operations(
        //     &node_a_keypair,
        //     Slot::new(1, op_thread),
        //     vec![op_1.clone()],
        // );
        //end setup

        //3. Send the full block from node a
        send_and_propagate_block(
            &mut network_controller,
            block.clone(),
            &node_a_peer_id,
            &protocol_controller,
            vec![op_1],
        );

        //4. Mock consensus controller expectations here:
        // - register_block_header command invoked, with block_id matching the one we crated
        // - register block command invoked, with same block_id again

        // block with wrong merkle root
        {
            let op = tools::create_operation_with_expire_period(&node_a_keypair, 5);
            let op_thread = op
                .content_creator_address
                .get_thread(protocol_config.thread_count);
            let block: SecureShare<Block, BlockId> = {
                let operation_merkle_root = Hash::compute_from("merkle root".as_bytes());

                let header = BlockHeader::new_verifiable(
                    BlockHeader {
                        announced_version: 0,
                        current_version: 0,
                        slot: Slot::new(1, op_thread),
                        parents: vec![
                            BlockId(Hash::compute_from("Genesis 0".as_bytes())),
                            BlockId(Hash::compute_from("Genesis 1".as_bytes())),
                        ],
                        denunciations: Vec::new(),
                        operation_merkle_root,
                        endorsements: Vec::new(),
                    },
                    BlockHeaderSerializer::new(),
                    &node_a_keypair,
                )
                .unwrap();

                Block::new_verifiable(
                    Block {
                        header,
                        operations: vec![op.clone()].into_iter().map(|op| op.id).collect(),
                    },
                    BlockSerializer::new(),
                    &node_a_keypair,
                )
                .unwrap()
            };

            send_and_propagate_block(
                &mut network_controller,
                block.clone(),
                &node_a_peer_id,
                &protocol_controller,
                vec![op],
            );

            // // Check protocol did send block header to consensus but not the full block.
            // assert_eq!(
            //     consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |command| {
            //         match command {
            //             MockConsensusControllerMessage::RegisterBlockHeader {
            //                 block_id,
            //                 header: _,
            //             } => Some(block_id),
            //             _ => None,
            //         }
            //     }),
            //     Some(block.id)
            // );
            // assert_eq!(
            //     consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |command| {
            //         match command {
            //             MockConsensusControllerMessage::RegisterBlock { block_id, .. } => {
            //                 Some(block_id)
            //             }
            //             _ => None,
            //         }
            //     }),
            //     None
            // );
        }

        //block with operation with wrong signature
        {
            let mut op = tools::create_operation_with_expire_period(&node_a_keypair, 5);
            let op_thread = op
                .content_creator_address
                .get_thread(protocol_config.thread_count);
            op.id = OperationId::new(Hash::compute_from("wrong signature".as_bytes()));
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, op_thread),
                vec![op.clone()],
            );

            send_and_propagate_block(
                &mut network_controller,
                block.clone(),
                &node_a_peer_id,
                &protocol_controller,
                vec![op],
            );

            // // Check protocol did send block header to consensus but not the full block.
            // assert_eq!(
            //     consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |command| {
            //         match command {
            //             MockConsensusControllerMessage::RegisterBlockHeader {
            //                 block_id,
            //                 header: _,
            //             } => Some(block_id),
            //             _ => None,
            //         }
            //     }),
            //     Some(block.id)
            // );
            // assert_eq!(
            //     consensus_event_receiver.wait_command(MassaTime::from_millis(1000), |command| {
            //         match command {
            //             MockConsensusControllerMessage::RegisterBlock { block_id, .. } => {
            //                 Some(block_id)
            //             }
            //             _ => None,
            //         }
            //     }),
            //     None
            // );
        }

        (protocol_manager,)
    })(network_controller, protocol_controller, protocol_manager);

    protocol_manager.stop()
}
