use crossbeam::channel::bounded;
use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{
    ProtocolConfig, ProtocolController, ProtocolError, ProtocolManager,
};
use massa_serialization::U64VarIntDeserializer;
use massa_storage::Storage;
use parking_lot::RwLock;
use peernet::{config::PeerNetConfiguration, network_manager::PeerNetManager};
use std::{ops::Bound::Included, sync::Arc};
use tracing::debug;

use crate::{
    connectivity::start_connectivity_thread,
    handlers::peer_handler::{fallback_function, models::PeerDB, MassaHandshake},
    manager::ProtocolManagerImpl,
    messages::MessagesHandler,
    wrap_network::NetworkControllerImpl,
};

/// start a new `ProtocolController` from a `ProtocolConfig`
///
/// # Arguments
/// * `config`: protocol settings
/// * `consensus_controller`: interact with consensus module
/// * `storage`: Shared storage to fetch data that are fetch across all modules
#[allow(clippy::type_complexity)]
pub fn start_protocol_controller(
    config: ProtocolConfig,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<(Box<dyn ProtocolController>, Box<dyn ProtocolManager>), ProtocolError> {
    debug!("starting protocol controller");
    let peer_db = Arc::new(RwLock::new(PeerDB::default()));

    let (sender_operations, receiver_operations) =
        bounded(config.max_size_channel_network_to_operation_handler);
    let (sender_endorsements, receiver_endorsements) =
        bounded(config.max_size_channel_network_to_endorsement_handler);
    let (sender_blocks, receiver_blocks) =
        bounded(config.max_size_channel_network_to_block_handler);
    let (sender_peers, receiver_peers) = bounded(config.max_size_channel_network_to_peer_handler);

    // Register channels for handlers
    let message_handlers: MessagesHandler = MessagesHandler {
        sender_blocks: sender_blocks.clone(),
        sender_endorsements: sender_endorsements.clone(),
        sender_operations: sender_operations.clone(),
        sender_peers: sender_peers.clone(),
        id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
    };

    let mut peernet_config = PeerNetConfiguration::default(
        MassaHandshake::new(peer_db.clone(), config.clone()),
        message_handlers,
    );
    peernet_config.self_keypair = config.keypair.clone();
    peernet_config.fallback_function = Some(&fallback_function);
    peernet_config.max_in_connections = config.max_in_connections;
    peernet_config.max_out_connections = config.max_out_connections;

    let network_controller = Box::new(NetworkControllerImpl::new(PeerNetManager::new(
        peernet_config,
    )));

    let (connectivity_thread_handle, controller) = start_connectivity_thread(
        config,
        network_controller,
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

    Ok((Box::new(controller), Box::new(manager)))
}
