use crossbeam::channel::{bounded, Receiver, Sender};
use massa_consensus_exports::ConsensusController;
use massa_models::node::NodeId;
use massa_pool_exports::PoolController;
use massa_protocol_exports::{
    BootstrapPeers, PeerData, PeerId, ProtocolConfig, ProtocolController, ProtocolError,
    ProtocolManager,
};
use massa_serialization::U64VarIntDeserializer;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_versioning::{
    keypair_factory::KeyPairFactory, versioning::MipStore, versioning_factory::VersioningFactory,
};
use parking_lot::RwLock;
use peernet::{
    config::{PeerNetCategoryInfo, PeerNetConfiguration},
    network_manager::PeerNetManager,
    types::KeyPair as PeerNetKeyPair,
};
use std::{
    collections::HashMap, fs::read_to_string, ops::Bound::Included, str::FromStr, sync::Arc,
};
use tracing::{debug, log::warn};

use crate::{
    connectivity::{start_connectivity_thread, ConnectivityCommand},
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
        peer_handler::{
            models::{PeerDB, PeerManagementCmd},
            MassaHandshake,
        },
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
    pub connectivity_thread: (Sender<ConnectivityCommand>, Receiver<ConnectivityCommand>),
    pub peer_management_handler: (Sender<PeerManagementCmd>, Receiver<PeerManagementCmd>),
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
    let (sender_connectivity_ext, receiver_connectivity_ext) =
        bounded(config.max_size_channel_commands_connectivity);
    let (sender_peer_management_ext, receiver_peer_management_ext): (
        Sender<PeerManagementCmd>,
        Receiver<PeerManagementCmd>,
    ) = crossbeam::channel::bounded(config.max_size_channel_commands_peers);
    (
        Box::new(ProtocolControllerImpl::new(
            sender_blocks_retrieval_ext.clone(),
            sender_blocks_propagation_ext.clone(),
            sender_operations_propagation_ext.clone(),
            sender_endorsements_propagation_ext.clone(),
            sender_connectivity_ext.clone(),
            sender_peer_management_ext.clone(),
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
            connectivity_thread: (sender_connectivity_ext, receiver_connectivity_ext),
            peer_management_handler: (sender_peer_management_ext, receiver_peer_management_ext),
        },
    )
}

/// start a new `ProtocolController` from a `ProtocolConfig`
///
/// # Arguments
/// * `config`: protocol settings
/// * `consensus_controller`: interact with consensus module
/// * `bootstrap_peers`: list of peers to connect to retrieved from the bootstrap
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub fn start_protocol_controller(
    config: ProtocolConfig,
    consensus_controller: Box<dyn ConsensusController>,
    bootstrap_peers: Option<BootstrapPeers>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    protocol_channels: ProtocolChannels,
    mip_store: MipStore,
) -> Result<(Box<dyn ProtocolManager>, KeyPair, NodeId), ProtocolError> {
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
        MassaHandshake::new(peer_db.clone(), config.clone(), message_handlers.clone()),
        message_handlers.clone(),
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
        // MERGE TODO
        let keypair_factory = KeyPairFactory {
            mip_store: mip_store.clone(),
        };
        let keypair = keypair_factory.create(&(), None)?;
        if let Err(e) = std::fs::write(&config.keypair_file, serde_json::to_string(&keypair)?) {
            warn!("could not generate node key file: {}", e);
        }
        keypair
    };

    let initial_peers_infos = serde_json::from_str::<HashMap<PeerId, PeerData>>(
        &std::fs::read_to_string(&config.initial_peers)?,
    )?;

    let initial_peers = if let Some(bootstrap_peers) = bootstrap_peers {
        bootstrap_peers.0.into_iter().collect()
    } else {
        initial_peers_infos
            .iter()
            .map(|(peer_id, data)| (peer_id.clone(), data.listeners.clone()))
            .collect()
    };

    let peernet_keypair = PeerNetKeyPair::from_str(&keypair.to_string()).unwrap();
    peernet_config.self_keypair = peernet_keypair.clone();
    let peernet_categories = config
        .peers_categories
        .iter()
        .map(|(category_name, infos)| {
            (
                category_name.clone(),
                (
                    initial_peers_infos
                        .iter()
                        .filter_map(|info| {
                            if info.1.category == *category_name {
                                //TODO: Adapt for multiple listeners
                                Some(
                                    info.1
                                        .listeners
                                        .iter()
                                        .next()
                                        .map(|addr| addr.0.ip().to_canonical())
                                        .unwrap(),
                                )
                            } else {
                                None
                            }
                        })
                        .collect(),
                    PeerNetCategoryInfo {
                        max_in_connections_post_handshake: infos.max_in_connections_post_handshake,
                        max_in_connections_pre_handshake: infos.max_in_connections_pre_handshake,
                        max_in_connections_per_ip: infos.max_in_connections_per_ip,
                    },
                ),
            )
        })
        .collect();
    peernet_config.peers_categories = peernet_categories;
    peernet_config.default_category_info = PeerNetCategoryInfo {
        max_in_connections_pre_handshake: config
            .default_category_info
            .max_in_connections_pre_handshake,
        max_in_connections_post_handshake: config
            .default_category_info
            .max_in_connections_post_handshake,
        max_in_connections_per_ip: config.default_category_info.max_in_connections_per_ip,
    };

    let network_controller = Box::new(NetworkControllerImpl::new(PeerNetManager::new(
        peernet_config,
    )));

    let connectivity_thread_handle = start_connectivity_thread(
        PeerId::from_public_key(peernet_keypair.get_public_key()),
        network_controller,
        consensus_controller,
        pool_controller,
        (sender_blocks, receiver_blocks),
        (sender_endorsements, receiver_endorsements),
        (sender_operations, receiver_operations),
        (sender_peers, receiver_peers),
        initial_peers,
        peer_db,
        storage,
        protocol_channels,
        message_handlers,
        config
            .peers_categories
            .iter()
            .map(|(category_name, infos)| {
                (
                    category_name.clone(),
                    (
                        initial_peers_infos
                            .iter()
                            .filter_map(|info| {
                                if info.1.category == *category_name {
                                    //TODO: Adapt for multiple listeners
                                    Some(
                                        info.1
                                            .listeners
                                            .iter()
                                            .next()
                                            .map(|addr| addr.0.ip().to_canonical())
                                            .unwrap(),
                                    )
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        *infos,
                    ),
                )
            })
            .collect(),
        config.default_category_info,
        config,
        mip_store,
    )?;

    let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

    Ok((
        Box::new(manager),
        keypair.clone(),
        NodeId::new(keypair.get_public_key()),
    ))
}
