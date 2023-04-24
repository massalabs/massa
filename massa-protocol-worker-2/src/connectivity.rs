use crossbeam::{
    channel::{unbounded, Sender},
    select,
};
use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_serialization::U64VarIntDeserializer;
use massa_storage::Storage;
use parking_lot::RwLock;
use peernet::{
    config::PeerNetConfiguration,
    network_manager::PeerNetManager,
    peer_id::PeerId,
    transports::{OutConnectionConfig, TcpOutConnectionConfig, TransportType},
};
use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};
use std::{num::NonZeroUsize, ops::Bound::Included, sync::Arc};

use crate::{
    controller::ProtocolControllerImpl,
    handlers::{
        block_handler::{cache::BlockCache, BlockHandler},
        endorsement_handler::{cache::EndorsementCache, EndorsementHandler},
        operation_handler::{cache::OperationCache, OperationHandler},
        peer_handler::{self, fallback_function, MassaHandshake, PeerManagementHandler},
    },
    messages::MessagesHandler,
};

pub enum ConnectivityCommand {
    Stop,
}

pub fn start_connectivity_thread(
    config: ProtocolConfig,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<
    (
        (Sender<ConnectivityCommand>, JoinHandle<()>),
        ProtocolControllerImpl,
    ),
    ProtocolError,
> {
    let (sender, receiver) = unbounded();
    let initial_peers = serde_json::from_str::<HashMap<PeerId, HashMap<SocketAddr, TransportType>>>(
        &std::fs::read_to_string(&config.initial_peers)?,
    )?;
    //TODO: Bound the channel
    // Channels handlers <-> outside world
    let (sender_operations_ext, receiver_operations_ext) = unbounded();
    let (sender_endorsements_ext, receiver_endorsements_ext) = unbounded();
    let (sender_blocks_ext, receiver_blocks_ext) = unbounded();
    let (sender_blocks_retrieval_ext, receiver_blocks_retrieval_ext) = unbounded();

    let handle = std::thread::spawn({
        let sender_endorsements_ext = sender_endorsements_ext.clone();
        let sender_blocks_ext = sender_blocks_ext.clone();
        let sender_operations_ext = sender_operations_ext.clone();
        move || {
            let mut peer_management_handler = PeerManagementHandler::new(initial_peers, &config);
            //TODO: Bound the channel
            // Channels network <-> handlers
            let (sender_operations, receiver_operations) = unbounded();
            let (sender_endorsements, receiver_endorsements) = unbounded();
            let (sender_blocks, receiver_blocks) = unbounded();

            // Register channels for handlers
            let message_handlers: MessagesHandler = MessagesHandler {
                sender_blocks,
                sender_endorsements,
                sender_operations,
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

            let mut manager = PeerNetManager::new(peernet_config);

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
            let mut operation_handler = OperationHandler::new(
                pool_controller.clone(),
                storage.clone_without_refs(),
                config.clone(),
                operation_cache.clone(),
                manager.active_connections.clone(),
                receiver_operations,
                sender_operations_ext,
                receiver_operations_ext,
                peer_management_handler.sender.command_sender.clone(),
            );
            let mut endorsement_handler = EndorsementHandler::new(
                pool_controller.clone(),
                endorsement_cache.clone(),
                storage.clone_without_refs(),
                config.clone(),
                manager.active_connections.clone(),
                receiver_endorsements,
                sender_endorsements_ext,
                receiver_endorsements_ext,
                peer_management_handler.sender.command_sender.clone(),
            );
            let mut block_handler = BlockHandler::new(
                manager.active_connections.clone(),
                consensus_controller,
                pool_controller,
                receiver_blocks,
                receiver_blocks_retrieval_ext,
                receiver_blocks_ext,
                sender_blocks_ext,
                peer_management_handler.sender.command_sender.clone(),
                config.clone(),
                endorsement_cache,
                operation_cache,
                block_cache,
                storage.clone_without_refs(),
            );

            for (addr, transport) in config.listeners {
                manager.start_listener(transport, addr).expect(&format!(
                    "Failed to start listener {:?} of transport {:?} in protocol",
                    addr, transport
                ));
            }

            println!("Protocol started");
            //Try to connect to peers
            loop {
                select! {
                        recv(receiver) -> msg => {
                            if let Ok(ConnectivityCommand::Stop) = msg {
                                drop(manager);
                                operation_handler.stop();
                                drop(operation_handler);
                                println!("Operation stopped");
                                endorsement_handler.stop();
                                block_handler.stop();
                                peer_management_handler.stop();
                                break;
                            }
                        }
                    default(Duration::from_millis(500)) => {
                        // Check if we need to connect to peers
                        let nb_connection_to_try = {
                            let active_connections = manager.active_connections.read();
                            if config.debug {
                                dbg!(&manager.active_connections.read().connections);
                                dbg!(&manager.active_connections.read().connection_queue);
                            }
                            let nb_connection_to_try = active_connections.max_out_connections - active_connections.nb_out_connections;
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
                                        let active_connections = manager.active_connections.read();
                                        if config.debug {
                                            println!("Checking addr: {:?}", addr);
                                            println!("Connections queue = {:#?}", active_connections.connection_queue);
                                            println!("Connections = {:#?}", active_connections.connections);
                                        }
                                        if !active_connections.check_addr_accepted(&addr) {
                                            println!("Address already connected");
                                            continue;
                                        }
                                    }
                                    if config.debug {
                                        println!("Trying to connect to peer {:?}", addr);
                                    }
                                    // We only manage TCP for now
                                    manager.try_connect(*addr, Duration::from_millis(200), &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {}))).unwrap();
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
        sender_blocks_ext,
        sender_operations_ext,
        sender_endorsements_ext,
    );
    Ok(((sender, handle), controller))
}
