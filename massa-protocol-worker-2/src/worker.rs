use crossbeam::channel::unbounded;
use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{
    ProtocolConfig, ProtocolController, ProtocolError, ProtocolManager,
};
use massa_serialization::U64VarIntDeserializer;
use massa_storage::Storage;
use peernet::{
    config::PeerNetConfiguration, network_manager::PeerNetManager, peer_id::PeerId,
    transports::TransportType,
};
use std::{collections::HashMap, net::SocketAddr, ops::Bound::Included};
use tracing::debug;

use crate::{
    connectivity::start_connectivity_thread,
    handlers::peer_handler::{fallback_function, MassaHandshake, PeerManagementHandler},
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
pub fn start_protocol_controller(
    config: ProtocolConfig,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<(Box<dyn ProtocolController>, Box<dyn ProtocolManager>), ProtocolError> {
    debug!("starting protocol controller");
    let initial_peers = serde_json::from_str::<HashMap<PeerId, HashMap<SocketAddr, TransportType>>>(
        &std::fs::read_to_string(&config.initial_peers)?,
    )?;
    let peer_management_handler = PeerManagementHandler::new(initial_peers, &config);

    let (sender_operations, receiver_operations) = unbounded();
    let (sender_endorsements, receiver_endorsements) = unbounded();
    let (sender_blocks, receiver_blocks) = unbounded();

    // Register channels for handlers
    let message_handlers: MessagesHandler = MessagesHandler {
        sender_blocks: sender_blocks.clone(),
        sender_endorsements: sender_endorsements.clone(),
        sender_operations: sender_operations.clone(),
        sender_peers: peer_management_handler.sender.msg_sender.clone(),
        id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
    };

    let mut peernet_config = PeerNetConfiguration::default(
        MassaHandshake::new(peer_management_handler.peer_db.clone()),
        message_handlers,
    );
    peernet_config.self_keypair = config.keypair.clone();
    peernet_config.fallback_function = Some(&fallback_function);
    //TODO: Add the rest of the config
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
        peer_management_handler,
        storage,
    )?;

    let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

    Ok((Box::new(controller), Box::new(manager)))
}
