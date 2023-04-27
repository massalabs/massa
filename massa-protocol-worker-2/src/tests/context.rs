use std::sync::Arc;

use crate::{
    connectivity::start_connectivity_thread, handlers::peer_handler::models::PeerDB,
    manager::ProtocolManagerImpl, messages::MessagesHandler,
    tests::mock_network::MockNetworkController,
};
use crossbeam::channel::unbounded;
use massa_consensus_exports::{
    test_exports::{ConsensusControllerImpl, ConsensusEventReceiver},
    ConsensusController,
};
//use crate::handlers::block_handler::BlockInfoReply;
use massa_pool_exports::{
    test_exports::{MockPoolController, PoolEventReceiver},
    PoolController,
};
use massa_protocol_exports_2::{
    ProtocolConfig, ProtocolController, ProtocolError, ProtocolManager,
};
use massa_serialization::U64VarIntDeserializer;
use massa_storage::Storage;
use parking_lot::RwLock;
use std::ops::Bound::Included;
use tracing::debug;

/// start a new `ProtocolController` from a `ProtocolConfig`
///
/// # Arguments
/// * `config`: protocol settings
/// * `consensus_controller`: interact with consensus module
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub fn start_protocol_controller_with_mock_network(
    config: ProtocolConfig,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<
    (
        Box<MockNetworkController>,
        Box<dyn ProtocolController>,
        Box<dyn ProtocolManager>,
    ),
    ProtocolError,
> {
    debug!("starting protocol controller with mock network");
    let peer_db = Arc::new(RwLock::new(PeerDB::default()));

    let (sender_operations, receiver_operations) = unbounded();
    let (sender_endorsements, receiver_endorsements) = unbounded();
    let (sender_blocks, receiver_blocks) = unbounded();
    let (sender_peers, receiver_peers) = unbounded();

    // Register channels for handlers
    let message_handlers: MessagesHandler = MessagesHandler {
        sender_blocks: sender_blocks.clone(),
        sender_endorsements: sender_endorsements.clone(),
        sender_operations: sender_operations.clone(),
        sender_peers: sender_peers.clone(),
        id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
    };

    let network_controller = Box::new(MockNetworkController::new(message_handlers));

    let (connectivity_thread_handle, controller) = start_connectivity_thread(
        config,
        network_controller.clone(),
        consensus_controller,
        pool_controller,
        (sender_blocks, receiver_blocks),
        (sender_endorsements, receiver_endorsements),
        (sender_operations, receiver_operations),
        (sender_peers, receiver_peers),
        peer_db,
        storage,
    )?;

    let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

    Ok((network_controller, Box::new(controller), Box::new(manager)))
}

pub fn protocol_test<F>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        Box<MockNetworkController>,
        Box<dyn ProtocolController>,
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
    ) -> (
        Box<MockNetworkController>,
        Box<dyn ProtocolController>,
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
    ),
{
    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let (consensus_controller, consensus_event_receiver) =
        ConsensusControllerImpl::new_with_receiver();
    // start protocol controller
    let (network_controller, protocol_controller, protocol_manager) =
        start_protocol_controller_with_mock_network(
            protocol_config.clone(),
            consensus_controller,
            pool_controller,
            Storage::create_root(),
        )
        .expect("could not start protocol controller");

    let (
        _network_controller,
        _protocol_controller,
        mut protocol_manager,
        _consensus_event_receiver,
        _pool_event_receiver,
    ) = test(
        network_controller,
        protocol_controller,
        protocol_manager,
        consensus_event_receiver,
        pool_event_receiver,
    );

    protocol_manager.stop()
}

pub fn protocol_test_with_storage<F>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        Box<MockNetworkController>,
        Box<dyn ProtocolController>,
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
        Storage,
    ) -> (
        Box<MockNetworkController>,
        Box<dyn ProtocolController>,
        Box<dyn ProtocolManager>,
        ConsensusEventReceiver,
        PoolEventReceiver,
    ),
{
    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let (consensus_controller, consensus_event_receiver) =
        ConsensusControllerImpl::new_with_receiver();
    let storage = Storage::create_root();
    // start protocol controller
    let (network_controller, protocol_controller, protocol_manager) =
        start_protocol_controller_with_mock_network(
            protocol_config.clone(),
            consensus_controller,
            pool_controller,
            storage.clone_without_refs(),
        )
        .expect("could not start protocol controller");

    let (
        _protocol_controller,
        _network_controller,
        mut protocol_manager,
        _consensus_event_receiver,
        _pool_event_receiver,
    ) = test(
        network_controller,
        protocol_controller,
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
