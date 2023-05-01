use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use massa_consensus_exports::ConsensusController;
use massa_models::stats::NetworkStats;
use massa_pool_exports::PoolController;
use massa_protocol_exports::{BootstrapPeers, ProtocolConfig, ProtocolError};
use massa_storage::Storage;
use parking_lot::RwLock;
use peernet::{
    peer::PeerConnectionType,
    transports::{OutConnectionConfig, TransportType},
};
use peernet::{peer_id::PeerId, transports::TcpOutConnectionConfig};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::{num::NonZeroUsize, sync::Arc};
use std::{thread::JoinHandle, time::Duration};
use tracing::warn;

use crate::handlers::peer_handler::PeerManagementHandler;
use crate::{handlers::peer_handler::models::SharedPeerDB, worker::ProtocolChannels};
use crate::{
    handlers::{
        block_handler::{cache::BlockCache, BlockHandler},
        endorsement_handler::{cache::EndorsementCache, EndorsementHandler},
        operation_handler::{cache::OperationCache, OperationHandler},
        peer_handler::models::PeerMessageTuple,
    },
    wrap_network::NetworkController,
};

pub enum ConnectivityCommand {
    Stop,
    GetStats {
        #[allow(clippy::type_complexity)]
        responder: Sender<(
            NetworkStats,
            HashMap<PeerId, (SocketAddr, PeerConnectionType)>,
        )>,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_connectivity_thread(
    config: ProtocolConfig,
    mut network_controller: Box<dyn NetworkController>,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    channel_blocks: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    channel_endorsements: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    channel_operations: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    channel_peers: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    bootstrap_peers: Option<BootstrapPeers>,
    peer_db: SharedPeerDB,
    storage: Storage,
    protocol_channels: ProtocolChannels,
) -> Result<(Sender<ConnectivityCommand>, JoinHandle<()>), ProtocolError> {
    let initial_peers = if let Some(bootstrap_peers) = bootstrap_peers {
        bootstrap_peers.0.into_iter().collect()
    } else {
        serde_json::from_str::<HashMap<PeerId, HashMap<SocketAddr, TransportType>>>(
            &std::fs::read_to_string(&config.initial_peers)?,
        )?
    };

    let handle = std::thread::Builder::new()
    .name("protocol-connectivity".to_string())
    .spawn({
        let sender_endorsements_propagation_ext = protocol_channels.endorsement_handler_propagation.0.clone();
        let sender_blocks_retrieval_ext = protocol_channels.block_handler_retrieval.0.clone();
        let sender_blocks_propagation_ext = protocol_channels.block_handler_propagation.0.clone();
        let sender_operations_propagation_ext = protocol_channels.operation_handler_propagation.0.clone();
        move || {
            for (addr, transport) in &config.listeners {
                network_controller
                    .start_listener(*transport, *addr)
                    .unwrap_or_else(|_| panic!(
                        "Failed to start listener {:?} of transport {:?} in protocol",
                        addr, transport
                    ));
            }
            std::thread::sleep(Duration::from_millis(100));

            // Create cache outside of the op handler because it could be used by other handlers
            let operation_cache = Arc::new(RwLock::new(OperationCache::new(
                NonZeroUsize::new(config.max_known_ops_size).unwrap(),
                NonZeroUsize::new(config.max_in_connections + config.max_out_connections).unwrap(),
            )));
            let endorsement_cache = Arc::new(RwLock::new(EndorsementCache::new(
                NonZeroUsize::new(config.max_known_endorsements_size).unwrap(),
                NonZeroUsize::new(config.max_in_connections + config.max_out_connections).unwrap(),
            )));

            let block_cache = Arc::new(RwLock::new(BlockCache::new(
                NonZeroUsize::new(config.max_known_blocks_size).unwrap(),
                NonZeroUsize::new(config.max_in_connections + config.max_out_connections).unwrap(),
            )));

            // Start handlers
            let mut peer_management_handler = PeerManagementHandler::new(
                initial_peers,
                peer_db.clone(),
                channel_peers,
                protocol_channels.peer_management_handler,
                network_controller.get_active_connections(),
                &config,
            );

            let mut operation_handler = OperationHandler::new(
                pool_controller.clone(),
                storage.clone_without_refs(),
                config.clone(),
                operation_cache.clone(),
                network_controller.get_active_connections(),
                channel_operations.1,
                protocol_channels.operation_handler_retrieval.0.clone(),
                protocol_channels.operation_handler_retrieval.1.clone(),
                sender_operations_propagation_ext.clone(),
                protocol_channels.operation_handler_propagation.1.clone(),
                peer_management_handler.sender.command_sender.clone(),
            );
            let mut endorsement_handler = EndorsementHandler::new(
                pool_controller.clone(),
                endorsement_cache.clone(),
                storage.clone_without_refs(),
                config.clone(),
                network_controller.get_active_connections(),
                channel_endorsements.1,
                protocol_channels.endorsement_handler_retrieval.0.clone(),
                protocol_channels.endorsement_handler_retrieval.1.clone(),
                sender_endorsements_propagation_ext,
                protocol_channels.endorsement_handler_propagation.1.clone(),
                peer_management_handler.sender.command_sender.clone(),
            );
            let mut block_handler = BlockHandler::new(
                network_controller.get_active_connections(),
                consensus_controller,
                pool_controller,
                channel_blocks.1,
                sender_blocks_retrieval_ext,
                protocol_channels.block_handler_retrieval.1.clone(),
                protocol_channels.block_handler_propagation.1.clone(),
                sender_blocks_propagation_ext,
                sender_operations_propagation_ext,
                peer_management_handler.sender.command_sender.clone(),
                config.clone(),
                endorsement_cache,
                operation_cache,
                block_cache,
                storage.clone_without_refs(),
            );

            //Try to connect to peers
            loop {
                select! {
                        recv(protocol_channels.connectivity_thread.1) -> msg => {
                            match msg {
                                Ok(ConnectivityCommand::Stop) => {
                                    drop(network_controller);
                                    operation_handler.stop();
                                    endorsement_handler.stop();
                                    block_handler.stop();
                                    peer_management_handler.stop();
                                    break;
                                },
                                Ok(ConnectivityCommand::GetStats { responder }) => {
                                    let active_node_count = network_controller.get_active_connections().get_peer_ids_connected().len() as u64;
                                    let in_connection_count = network_controller.get_active_connections().get_nb_in_connections() as u64;
                                    let out_connection_count = network_controller.get_active_connections().get_nb_out_connections() as u64;
                                    let stats = NetworkStats {
                                        active_node_count,
                                        in_connection_count,
                                        out_connection_count,
                                        banned_peer_count: peer_db.read().get_banned_peer_count(),
                                        known_peer_count: peer_db.read().peers.len() as u64,
                                    };
                                    let peers: HashMap<PeerId, (SocketAddr, PeerConnectionType)> = network_controller.get_active_connections().get_peers_connected();
                                    responder.send((stats, peers)).unwrap_or_else(|_| warn!("Failed to send stats to responder"));
                                }
                                Err(_) => {
                                    warn!("Channel to connectivity thread is closed. Stopping the protocol");
                                    break;
                                }
                            }
                        }
                    default(Duration::from_millis(1000)) => {
                        println!("PeerDB: {:#?}", peer_db.read().peers);
                        if config.debug {
                            println!("nb peers connected: {}", network_controller.get_active_connections().get_peer_ids_connected().len());
                        }
                        // Check if we need to connect to peers
                        let nb_connection_to_try = {
                            let nb_connection_to_try = network_controller.get_active_connections().get_max_out_connections().saturating_sub(network_controller.get_active_connections().get_nb_out_connections());
                            if nb_connection_to_try == 0 {
                                continue;
                            }
                            nb_connection_to_try
                        };
                        if config.debug {
                            println!("Trying to connect to {} peers", nb_connection_to_try);
                        }
                        // Get the best peers
                        {
                            let peer_db_read = peer_management_handler.peer_db.read();
                            let best_peers = peer_db_read.get_best_peers(nb_connection_to_try);
                            for peer_id in best_peers {
                                let Some(peer_info) = peer_db_read.peers.get(&peer_id) else {
                                    warn!("Peer {} not found in peer_db", peer_id);
                                    continue;
                                };
                                //TODO: Adapt for multiple listeners
                                let (addr, _) = peer_info.last_announce.listeners.iter().next().unwrap();
                                if peer_info.last_announce.listeners.is_empty() {
                                    continue;
                                }
                                {
                                    {
                                        if !network_controller.get_active_connections().check_addr_accepted(addr) {
                                            println!("Address already connected");
                                            continue;
                                        }
                                    }
                                    if config.debug {
                                        println!("Trying to connect to peer {:?}", addr);
                                    }
                                    // We only manage TCP for now
                                    if let Err(err) = network_controller.try_connect(*addr, Duration::from_millis(200), &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig::new(config.read_write_limit_bytes_per_second / 10, Duration::from_millis(100))))) {
                                        warn!("Failed to connect to peer {:?}: {:?}", addr, err);
                                    }
                                };
                            };
                        }
                    }
                }
            }
        }
    }).expect("OS failed to start connectivity thread");

    // Start controller
    Ok((protocol_channels.connectivity_thread.0, handle))
}
