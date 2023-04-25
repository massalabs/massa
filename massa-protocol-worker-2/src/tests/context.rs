use crate::start_protocol_controller;
use massa_consensus_exports::test_exports::{ConsensusEventReceiver, MockConsensusController};
use massa_models::{
    block::SecureShareBlock, block_id::BlockId, node::NodeId, operation::SecureShareOperation,
    prehash::PreHashSet,
};
//use crate::handlers::block_handler::BlockInfoReply;
use massa_pool_exports::test_exports::{MockPoolController, PoolEventReceiver};
use massa_protocol_exports_2::{ProtocolConfig, ProtocolManager};
use massa_storage::Storage;

pub fn protocol_test<F>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
    ) -> (
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
    ),
{
    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let (consensus_controller, consensus_event_receiver) =
        MockConsensusController::new_with_receiver();
    // start protocol controller
    let (protocol_controller, protocol_manager) = start_protocol_controller(
        protocol_config.clone(),
        consensus_controller,
        pool_controller,
        Storage::create_root(),
    )
    .expect("could not start protocol controller");

    let (mut protocol_manager, _consensus_event_receiver, _pool_event_receiver) = test(
        protocol_manager,
        consensus_event_receiver,
        pool_event_receiver,
    );

    protocol_manager.stop()
}

pub fn protocol_test_with_storage<F>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
        Storage,
    ) -> (
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
    ),
{
    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let (consensus_controller, consensus_event_receiver) =
        MockConsensusController::new_with_receiver();
    let storage = Storage::create_root();
    // start protocol controller
    let (protocol_controller, protocol_manager) = start_protocol_controller(
        protocol_config.clone(),
        consensus_controller,
        pool_controller,
        storage.clone_without_refs(),
    )
    .expect("could not start protocol controller");

    let (mut protocol_manager, _consensus_event_receiver, _pool_event_receiver) = test(
        protocol_manager,
        consensus_event_receiver,
        pool_event_receiver,
        storage,
    );

    protocol_manager.stop()
}

// /// send a block and assert it has been propagate (or not)
// pub async fn send_and_propagate_block(
//     network_controller: &mut MockNetworkController,
//     block: SecureShareBlock,
//     source_node_id: NodeId,
//     protocol_command_sender: &mut ProtocolCommandSender,
//     operations: Vec<SecureShareOperation>,
// ) {
//     network_controller
//         .send_header(source_node_id, block.content.header.clone())
//         .await;

//     let mut protocol_sender = protocol_command_sender.clone();
//     tokio::task::spawn_blocking(move || {
//         protocol_sender
//             .send_wishlist_delta(
//                 vec![(block.id, Some(block.content.header.clone()))]
//                     .into_iter()
//                     .collect(),
//                 PreHashSet::<BlockId>::default(),
//             )
//             .unwrap();
//     })
//     .await
//     .unwrap();

//     // Send block info to protocol.
//     let info = vec![(
//         block.id,
//         BlockInfoReply::Info(block.content.operations.clone()),
//     )];
//     network_controller
//         .send_block_info(source_node_id, info)
//         .await;

//     // Send full ops.
//     let info = vec![(block.id, BlockInfoReply::Operations(operations))];
//     network_controller
//         .send_block_info(source_node_id, info)
//         .await;
// }
