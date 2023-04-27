use crossbeam::channel::{bounded, Receiver, Sender};
use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports::{ProtocolConfig, ProtocolController, ProtocolError, ProtocolManager};
use massa_serialization::U64VarIntDeserializer;
use massa_storage::Storage;
use parking_lot::RwLock;
use peernet::{config::PeerNetConfiguration, network_manager::PeerNetManager, types::KeyPair};
use std::{fs::read_to_string, ops::Bound::Included, sync::Arc};
use tracing::{debug, log::warn};

use crate::{
    connectivity::start_connectivity_thread,
    controller::ProtocolControllerImpl,
    handlers::{
        block_handler::{
            commands_propagation::BlockHandlerPropagationCommand,
            commands_retrieval::BlockHandlerRetrievalCommand,
        },
        endorsement_handler::{
            commands_propagation::EndorsementHandlerPropagationCommand,
            commands_retrieval::EndorsementHandlerRetrievalCommand,
        },
        operation_handler::{
            commands_propagation::OperationHandlerPropagationCommand,
            commands_retrieval::OperationHandlerRetrievalCommand,
        },
        peer_handler::{models::PeerDB, MassaHandshake},
    },
    manager::ProtocolManagerImpl,
    messages::MessagesHandler,
    wrap_network::NetworkControllerImpl,
};

pub struct ProtocolChannels {
    pub operation_handler_retrieval: (
        Sender<OperationHandlerRetrievalCommand>,
        Receiver<OperationHandlerRetrievalCommand>,
    ),
    pub operation_handler_propagation: (
        Sender<OperationHandlerPropagationCommand>,
        Receiver<OperationHandlerPropagationCommand>,
    ),
    pub endorsement_handler_retrieval: (
        Sender<EndorsementHandlerRetrievalCommand>,
        Receiver<EndorsementHandlerRetrievalCommand>,
    ),
    pub endorsement_handler_propagation: (
        Sender<EndorsementHandlerPropagationCommand>,
        Receiver<EndorsementHandlerPropagationCommand>,
    ),
    pub block_handler_retrieval: (
        Sender<BlockHandlerRetrievalCommand>,
        Receiver<BlockHandlerRetrievalCommand>,
    ),
    pub block_handler_propagation: (
        Sender<BlockHandlerPropagationCommand>,
        Receiver<BlockHandlerPropagationCommand>,
    ),
}

/// This function exists because consensus need the protocol controller and we need consensus controller.
/// Someone has to be created first.
pub fn create_protocol_controller(
    config: ProtocolConfig,
) -> (Box<dyn ProtocolController>, ProtocolChannels) {
    let (sender_operations_retrieval_ext, receiver_operations_retrieval_ext) =
        bounded(config.max_size_channel_commands_retrieval_operations);
    let (sender_operations_propagation_ext, receiver_operations_propagation_ext) =
        bounded(config.max_size_channel_commands_propagation_operations);
    let (sender_endorsements_retrieval_ext, receiver_endorsements_retrieval_ext) =
        bounded(config.max_size_channel_commands_retrieval_endorsements);
    let (sender_endorsements_propagation_ext, receiver_endorsements_propagation_ext) =
        bounded(config.max_size_channel_commands_propagation_endorsements);
    let (sender_blocks_propagation_ext, receiver_blocks_propagation_ext) =
        bounded(config.max_size_channel_commands_retrieval_blocks);
    let (sender_blocks_retrieval_ext, receiver_blocks_retrieval_ext) =
        bounded(config.max_size_channel_commands_propagation_blocks);
    (
        Box::new(ProtocolControllerImpl::new(
            sender_blocks_retrieval_ext.clone(),
            sender_blocks_propagation_ext.clone(),
            sender_operations_propagation_ext.clone(),
            sender_endorsements_propagation_ext.clone(),
        )),
        ProtocolChannels {
            operation_handler_retrieval: (
                sender_operations_retrieval_ext,
                receiver_operations_retrieval_ext,
            ),
            operation_handler_propagation: (
                sender_operations_propagation_ext,
                receiver_operations_propagation_ext,
            ),
            endorsement_handler_retrieval: (
                sender_endorsements_retrieval_ext,
                receiver_endorsements_retrieval_ext,
            ),
            endorsement_handler_propagation: (
                sender_endorsements_propagation_ext,
                receiver_endorsements_propagation_ext,
            ),
            block_handler_retrieval: (sender_blocks_retrieval_ext, receiver_blocks_retrieval_ext),
            block_handler_propagation: (
                sender_blocks_propagation_ext,
                receiver_blocks_propagation_ext,
            ),
        },
    )
}

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
    protocol_channels: ProtocolChannels,
) -> Result<Box<dyn ProtocolManager>, ProtocolError> {
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

    // try to read node keypair from file, otherwise generate it & write to file. Then derive nodeId
    let keypair = if std::path::Path::is_file(&config.keypair_file) {
        // file exists: try to load it
        let keypair_bs58_check_encoded = read_to_string(&config.keypair_file).map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })?;
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes())?
    } else {
        // node file does not exist: generate the key and save it
        let keypair = KeyPair::generate();
        if let Err(e) = std::fs::write(&config.keypair_file, serde_json::to_string(&keypair)?) {
            warn!("could not generate node key file: {}", e);
        }
        keypair
    };

    peernet_config.self_keypair = keypair.clone();
    //TODO: Add the rest of the config
    peernet_config.max_in_connections = config.max_in_connections;
    peernet_config.max_out_connections = config.max_out_connections;

    let network_controller = Box::new(NetworkControllerImpl::new(PeerNetManager::new(
        peernet_config,
    )));

    let connectivity_thread_handle = start_connectivity_thread(
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
        protocol_channels,
    )?;

    let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

    Ok(Box::new(manager))
}
