// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::context::protocol_test;
use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::prehash::PreHashSet;
use massa_models::{block_id::BlockId, slot::Slot};
use massa_protocol_exports_2::test_exports::tools;
use massa_protocol_exports_2::ProtocolConfig;
//use massa_protocol_exports_2::test_exports::tools::{asked_list, assert_hash_asked_to_node};
use massa_time::MassaTime;
use peernet::types::KeyPair;
use serial_test::serial;

#[test]
#[serial]
fn test_full_ask_block_workflow() {
    let mut protocol_config = ProtocolConfig::default();
    protocol_config.listeners.insert(
        "127.0.0.1:8080".parse().unwrap(),
        peernet::transports::TransportType::Tcp,
    );
    protocol_test(
        &protocol_config,
        move |protocol_manager, mut consensus_event_receiver, pool_event_receiver| {
            (
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

// #[test]
// #[serial]
// fn test_full_ask_block_workflow() {
//     let protocol_config = ProtocolConfig::default();

//     protocol_test(
//         &protocol_config,
//         move |protocol_manager,
//               mut protocol_consensus_event_receiver,
//               protocol_pool_event_receiver| {
//             let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let node_b = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();

//             // 2. Create a block coming from node 0.
//             let op_1 = tools::create_operation_with_expire_period(&node_a.keypair, 5);
//             let op_2 = tools::create_operation_with_expire_period(&node_a.keypair, 5);
//             let op_thread = op_1
//                 .content_creator_address
//                 .get_thread(protocol_config.thread_count);
//             let block = tools::create_block_with_operations(
//                 &node_a.keypair,
//                 Slot::new(1, op_thread),
//                 vec![op_1.clone(), op_2.clone()],
//             );
//             // end set up

//             // Send header via node_a
//             network_controller
//                 .send_header(node_a.id, block.content.header.clone())
//                 .await;

//             // Send wishlist
//             let header = block.content.header.clone();
//             let protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(
//                         vec![(block.id, Some(header))].into_iter().collect(),
//                         PreHashSet::<BlockId>::default(),
//                     )
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             // assert it was asked to node A, then B
//             assert_hash_asked_to_node(block.id, node_a.id, &mut network_controller).await;
//             assert_hash_asked_to_node(block.id, node_b.id, &mut network_controller).await;

//             // Node B replied with the block info.
//             network_controller
//                 .send_block_info(
//                     node_b.id,
//                     vec![(block.id, BlockInfoReply::Info(vec![op_1.id, op_2.id]))],
//                 )
//                 .await;

//             // 7. Make sure protocol did ask for the operations.
//             let ask_for_block_cmd_filter = |cmd| match cmd {
//                 NetworkCommand::AskForBlocks { list } => Some(list),
//                 _ => None,
//             };

//             let mut ask_list = network_controller
//                 .wait_command(100.into(), ask_for_block_cmd_filter)
//                 .await
//                 .unwrap();
//             let (hash, asked) = ask_list.get_mut(&node_b.id).unwrap().pop().unwrap();
//             assert_eq!(block.id, hash);
//             if let AskForBlocksInfo::Operations(ops) = asked {
//                 assert_eq!(ops.len(), 2);
//                 for op in ops {
//                     assert!(block.content.operations.contains(&op));
//                 }
//             } else {
//                 panic!("Unexpected ask for blocks.");
//             }

//             // Node B replied with the operations.
//             network_controller
//                 .send_block_info(
//                     node_b.id,
//                     vec![(block.id, BlockInfoReply::Operations(vec![op_1, op_2]))],
//                 )
//                 .await;

//             let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
//                 // Protocol sends expected block to consensus.
//                 loop {
//                     match protocol_consensus_event_receiver.wait_command(
//                         MassaTime::from_millis(100),
//                         |command| match command {
//                             MockConsensusControllerMessage::RegisterBlock {
//                                 slot,
//                                 block_id,
//                                 block_storage,
//                                 created: _,
//                             } => {
//                                 assert_eq!(slot, block.content.header.content.slot);
//                                 assert_eq!(block_id, block.id);
//                                 let received_block =
//                                     block_storage.read_blocks().get(&block_id).cloned().unwrap();
//                                 assert_eq!(
//                                     received_block.content.operations,
//                                     block.content.operations
//                                 );
//                                 Some(())
//                             }
//                             _evt => None,
//                         },
//                     ) {
//                         Some(()) => {
//                             break;
//                         }
//                         None => {
//                             continue;
//                         }
//                     }
//                 }
//                 return protocol_consensus_event_receiver;
//             })
//             .await
//             .unwrap();

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
// async fn test_empty_block() {
//     // start
//     let protocol_config = &tools::PROTOCOL_CONFIG;

//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     mut protocol_command_sender,
//                     protocol_manager,
//                     mut protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let node_b = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();

//             // 2. Create a block coming from node 0.
//             let block = tools::create_block(&node_a.keypair);
//             let hash_1 = block.id;
//             // end set up

//             // Send header via node_a
//             network_controller
//                 .send_header(node_a.id, block.content.header.clone())
//                 .await;

//             // send wishlist
//             let header = block.content.header.clone();
//             let protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(
//                         vec![(hash_1, Some(header))].into_iter().collect(),
//                         PreHashSet::<BlockId>::default(),
//                     )
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             // assert it was asked to node A, then B
//             assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;
//             assert_hash_asked_to_node(hash_1, node_b.id, &mut network_controller).await;

//             // node B replied with the block
//             network_controller
//                 .send_block_info(
//                     node_b.id,
//                     vec![(block.id, BlockInfoReply::Info(Default::default()))],
//                 )
//                 .await;

//             // 7. Make sure protocol did not send additional ask for block commands.
//             let ask_for_block_cmd_filter = |cmd| match cmd {
//                 cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
//                 _ => None,
//             };

//             let got_more_commands = network_controller
//                 .wait_command(100.into(), ask_for_block_cmd_filter)
//                 .await;
//             assert!(
//                 got_more_commands.is_none(),
//                 "unexpected command {:?}",
//                 got_more_commands
//             );

//             // Protocol sends expected block to consensus.
//             let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
//                 loop {
//                     match protocol_consensus_event_receiver.wait_command(
//                         MassaTime::from_millis(100),
//                         |command| match command {
//                             MockConsensusControllerMessage::RegisterBlock {
//                                 slot,
//                                 block_id,
//                                 block_storage,
//                                 created: _,
//                             } => {
//                                 assert_eq!(slot, block.content.header.content.slot);
//                                 assert_eq!(block_id, block.id);
//                                 let received_block =
//                                     block_storage.read_blocks().get(&block_id).cloned().unwrap();
//                                 assert_eq!(
//                                     received_block.content.operations,
//                                     block.content.operations
//                                 );
//                                 Some(())
//                             }
//                             _evt => None,
//                         },
//                     ) {
//                         Some(()) => {
//                             break;
//                         }
//                         None => {
//                             continue;
//                         }
//                     }
//                 }
//                 protocol_consensus_event_receiver
//             })
//             .await
//             .unwrap();
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
// async fn test_someone_knows_it() {
//     // start
//     let protocol_config = &tools::PROTOCOL_CONFIG;
//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     mut protocol_command_sender,
//                     protocol_manager,
//                     mut protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_b = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let node_c = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();

//             // 2. Create a block coming from node 0.
//             let op = tools::create_operation_with_expire_period(&node_a.keypair, 5);

//             let block = tools::create_block_with_operations(
//                 &node_a.keypair,
//                 Slot::new(1, 0),
//                 vec![op.clone()],
//             );
//             let hash_1 = block.id;
//             // end set up

//             // node c must know about block
//             network_controller
//                 .send_header(node_c.id, block.content.header.clone())
//                 .await;

//             let protocol_consensus_event_receiver = tokio::task::spawn_blocking(move || {
//                 protocol_consensus_event_receiver.wait_command(
//                     MassaTime::from_millis(100),
//                     |command| match command {
//                         MockConsensusControllerMessage::RegisterBlockHeader { .. } => Some(()),
//                         _ => panic!("unexpected protocol event"),
//                     },
//                 );
//                 protocol_consensus_event_receiver
//             })
//             .await
//             .unwrap();

//             // send wishlist
//             let protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(
//                         vec![(hash_1, Some(block.content.header.clone()))]
//                             .into_iter()
//                             .collect(),
//                         PreHashSet::<BlockId>::default(),
//                     )
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             assert_hash_asked_to_node(hash_1, node_c.id, &mut network_controller).await;

//             // node C replied with the block info containing the operation id.
//             network_controller
//                 .send_block_info(
//                     node_c.id,
//                     vec![(
//                         block.id,
//                         BlockInfoReply::Info(vec![op].into_iter().map(|op| op.id).collect()),
//                     )],
//                 )
//                 .await;

//             // 7. Make sure protocol ask for the operations next.
//             let ask_for_block_cmd_filter = |cmd| match cmd {
//                 NetworkCommand::AskForBlocks { list } => Some(list),
//                 _ => None,
//             };

//             let mut ask_list = network_controller
//                 .wait_command(100.into(), ask_for_block_cmd_filter)
//                 .await
//                 .unwrap();
//             let (hash, asked) = ask_list.get_mut(&node_c.id).unwrap().pop().unwrap();
//             assert_eq!(hash_1, hash);
//             if let AskForBlocksInfo::Operations(ops) = asked {
//                 for op in ops {
//                     assert!(block.content.operations.contains(&op));
//                 }
//             } else {
//                 panic!("Unexpected ask for blocks.");
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

// #[tokio::test]
// #[serial]
// async fn test_dont_want_it_anymore() {
//     // start
//     let protocol_config = &tools::PROTOCOL_CONFIG;
//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     mut protocol_command_sender,
//                     protocol_manager,
//                     protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_b = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();

//             // 2. Create a block coming from node 0.
//             let block = tools::create_block(&node_a.keypair);
//             let hash_1 = block.id;
//             // end set up

//             // send wishlist
//             protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(
//                         vec![(hash_1, Some(block.content.header.clone()))]
//                             .into_iter()
//                             .collect(),
//                         PreHashSet::<BlockId>::default(),
//                     )
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             // assert it was asked to node A
//             assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;

//             // we don't want it anymore
//             protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(Default::default(), vec![hash_1].into_iter().collect())
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             // 7. Make sure protocol did not send additional ask for block commands.
//             let ask_for_block_cmd_filter = |cmd| match cmd {
//                 cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
//                 _ => None,
//             };

//             let got_more_commands = network_controller
//                 .wait_command(100.into(), ask_for_block_cmd_filter)
//                 .await;
//             assert!(
//                 got_more_commands.is_none(),
//                 "unexpected command {:?}",
//                 got_more_commands
//             );
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
// async fn test_no_one_has_it() {
//     // start
//     let protocol_config = &tools::PROTOCOL_CONFIG;

//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     mut protocol_command_sender,
//                     protocol_manager,
//                     protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let node_b = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let node_c = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();

//             // 2. Create a block coming from node 0.
//             let block = tools::create_block(&node_a.keypair);
//             let hash_1 = block.id;
//             // end set up

//             // send wishlist
//             let protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(
//                         vec![(hash_1, Some(block.content.header.clone()))]
//                             .into_iter()
//                             .collect(),
//                         PreHashSet::<BlockId>::default(),
//                     )
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             // assert it was asked to node A
//             assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;

//             // node a replied is does not have it
//             network_controller
//                 .send_block_info(node_a.id, vec![(hash_1, BlockInfoReply::NotFound)])
//                 .await;

//             assert_hash_asked_to_node(hash_1, node_b.id, &mut network_controller).await;
//             assert_hash_asked_to_node(hash_1, node_c.id, &mut network_controller).await;
//             assert_hash_asked_to_node(hash_1, node_a.id, &mut network_controller).await;
//             assert_hash_asked_to_node(hash_1, node_b.id, &mut network_controller).await;
//             assert_hash_asked_to_node(hash_1, node_c.id, &mut network_controller).await;

//             // 7. Make sure protocol did not send additional ask for block commands.
//             let ask_for_block_cmd_filter = |cmd| match cmd {
//                 cmd @ NetworkCommand::AskForBlocks { .. } => Some(cmd),
//                 _ => None,
//             };

//             let got_more_commands = network_controller
//                 .wait_command(100.into(), ask_for_block_cmd_filter)
//                 .await;
//             assert!(
//                 got_more_commands.is_none(),
//                 "unexpected command {:?}",
//                 got_more_commands
//             );
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
// async fn test_multiple_blocks_without_a_priori() {
//     // start
//     let protocol_config = &tools::PROTOCOL_CONFIG;

//     protocol_test(
//         protocol_config,
//         async move |mut network_controller,
//                     mut protocol_command_sender,
//                     protocol_manager,
//                     protocol_consensus_event_receiver,
//                     protocol_pool_event_receiver| {
//             let node_a = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_b = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();
//             let _node_c = tools::create_and_connect_nodes(1, &mut network_controller)
//                 .await
//                 .pop()
//                 .unwrap();

//             // 2. Create two blocks coming from node 0.
//             let block_1 = tools::create_block(&node_a.keypair);
//             let hash_1 = block_1.id;

//             let block_2 = tools::create_block(&node_a.keypair);
//             let hash_2 = block_2.id;

//             // node a is disconnected so no node knows about wanted blocks
//             network_controller.close_connection(node_a.id).await;
//             // end set up
//             tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

//             // send wishlist
//             let protocol_command_sender = tokio::task::spawn_blocking(move || {
//                 protocol_command_sender
//                     .send_wishlist_delta(
//                         vec![
//                             (hash_1, Some(block_1.content.header.clone())),
//                             (hash_2, Some(block_2.content.header.clone())),
//                         ]
//                         .into_iter()
//                         .collect(),
//                         PreHashSet::<BlockId>::default(),
//                     )
//                     .unwrap();
//                 protocol_command_sender
//             })
//             .await
//             .unwrap();

//             let list = asked_list(&mut network_controller).await;
//             for (node_id, set) in list.into_iter() {
//                 // assert we ask one block per node
//                 assert_eq!(
//                     set.len(),
//                     1,
//                     "node {:?} was asked {:?} blocks",
//                     node_id,
//                     set.len()
//                 );
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
