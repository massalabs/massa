use crossbeam::{
    channel::{unbounded, Receiver, Sender},
    select,
};
use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_storage::Storage;
use parking_lot::RwLock;
use peernet::peer_id::PeerId;
use peernet::transports::{OutConnectionConfig, TcpOutConnectionConfig, TransportType};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::{num::NonZeroUsize, sync::Arc};
use std::{thread::JoinHandle, time::Duration};

use crate::handlers::peer_handler::models::SharedPeerDB;
use crate::handlers::peer_handler::PeerManagementHandler;
use crate::{
    controller::ProtocolControllerImpl,
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
}

pub fn start_connectivity_thread(
    config: ProtocolConfig,
    mut network_controller: Box<dyn NetworkController>,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    channel_blocks: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    channel_endorsements: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    channel_operations: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    channel_peers: (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
    peer_db: SharedPeerDB,
    storage: Storage,
) -> Result<
    (
        (Sender<ConnectivityCommand>, JoinHandle<()>),
        ProtocolControllerImpl,
    ),
    ProtocolError,
> {
    let initial_peers = serde_json::from_str::<HashMap<PeerId, HashMap<SocketAddr, TransportType>>>(
        &std::fs::read_to_string(&config.initial_peers)?,
    )?;
    let (sender, receiver) = unbounded();
    //TODO: Bound the channel
    // Channels handlers <-> outside world
    let (sender_operations_retrieval_ext, receiver_operations_retrieval_ext) = unbounded();
    let (sender_operations_propagation_ext, receiver_operations_propagation_ext) = unbounded();
    let (sender_endorsements_retrieval_ext, receiver_endorsements_retrieval_ext) = unbounded();
    let (sender_endorsements_propagation_ext, receiver_endorsements_propagation_ext) = unbounded();
    let (sender_blocks_propagation_ext, receiver_blocks_propagation_ext) = unbounded();
    let (sender_blocks_retrieval_ext, receiver_blocks_retrieval_ext) = unbounded();

    let handle = std::thread::spawn({
        let sender_endorsements_propagation_ext = sender_endorsements_propagation_ext.clone();
        let sender_blocks_retrieval_ext = sender_blocks_retrieval_ext.clone();
        let sender_blocks_propagation_ext = sender_blocks_propagation_ext.clone();
        let sender_operations_propagation_ext = sender_operations_propagation_ext.clone();
        move || {
            for (addr, transport) in &config.listeners {
                network_controller
                    .start_listener(*transport, *addr)
                    .expect(&format!(
                        "Failed to start listener {:?} of transport {:?} in protocol",
                        addr, transport
                    ));
            }

            // Create cache outside of the op handler because it could be used by other handlers
            //TODO: Add real config values
            let operation_cache = Arc::new(RwLock::new(OperationCache::new(
                NonZeroUsize::new(10000).unwrap(),
                NonZeroUsize::new(10000).unwrap(),
            )));
            let endorsement_cache = Arc::new(RwLock::new(EndorsementCache::new(
                NonZeroUsize::new(10000).unwrap(),
                NonZeroUsize::new(10000).unwrap(),
            )));

            let block_cache = Arc::new(RwLock::new(BlockCache::new(
                NonZeroUsize::new(10000).unwrap(),
                NonZeroUsize::new(10000).unwrap(),
            )));

            // Start handlers
            let mut peer_management_handler = PeerManagementHandler::new(
                initial_peers,
                peer_db,
                channel_peers,
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
                sender_operations_retrieval_ext,
                receiver_operations_retrieval_ext,
                sender_operations_propagation_ext,
                receiver_operations_propagation_ext,
                peer_management_handler.sender.command_sender.clone(),
            );
            let mut endorsement_handler = EndorsementHandler::new(
                pool_controller.clone(),
                endorsement_cache.clone(),
                storage.clone_without_refs(),
                config.clone(),
                network_controller.get_active_connections(),
                channel_endorsements.1,
                sender_endorsements_retrieval_ext,
                receiver_endorsements_retrieval_ext,
                sender_endorsements_propagation_ext,
                receiver_endorsements_propagation_ext,
                peer_management_handler.sender.command_sender.clone(),
            );
            let mut block_handler = BlockHandler::new(
                network_controller.get_active_connections(),
                consensus_controller,
                pool_controller,
                channel_blocks.1,
                sender_blocks_retrieval_ext,
                receiver_blocks_retrieval_ext,
                receiver_blocks_propagation_ext,
                sender_blocks_propagation_ext,
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
                        recv(receiver) -> msg => {
                            if let Ok(ConnectivityCommand::Stop) = msg {
                                drop(network_controller);
                                operation_handler.stop();
                                endorsement_handler.stop();
                                block_handler.stop();
                                peer_management_handler.stop();
                                break;
                            }
                        }
                    default(Duration::from_millis(1000)) => {
                        // Check if we need to connect to peers
                        let nb_connection_to_try = {
                            let nb_connection_to_try = network_controller.get_active_connections().get_max_out_connections() - network_controller.get_active_connections().get_nb_out_connections();
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
                                let peer_info = peer_db_read.peers.get(&peer_id).unwrap();
                                //TODO: Adapt for multiple listeners
                                let (addr, _) = peer_info.last_announce.listeners.iter().next().unwrap();
                                if peer_info.last_announce.listeners.is_empty() {
                                    continue;
                                }
                                {
                                    {
                                        if !network_controller.get_active_connections().check_addr_accepted(&addr) {
                                            println!("Address already connected");
                                            continue;
                                        }
                                    }
                                    if config.debug {
                                        println!("Trying to connect to peer {:?}", addr);
                                    }
                                    // We only manage TCP for now
                                    network_controller.try_connect(*addr, Duration::from_millis(200), &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {}))).unwrap();
                                };
                            };
                        }
                    }
                }
            }
        }
    });
    // Start controller
    let controller = ProtocolControllerImpl::new(
        sender_blocks_retrieval_ext,
        sender_blocks_propagation_ext,
        sender_operations_propagation_ext,
        sender_endorsements_propagation_ext,
    );
    Ok(((sender, handle), controller))
}
