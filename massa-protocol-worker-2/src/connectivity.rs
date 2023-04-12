use crossbeam::{
    channel::{unbounded, Sender},
    select,
};
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
        block_handler::BlockHandler,
        endorsement_handler::EndorsementHandler,
        operation_handler::{cache::OperationCache, OperationHandler},
        peer_handler::{fallback_function, MassaHandshake, PeerManagementHandler},
    },
    messages::MessagesHandler,
};

pub enum ConnectivityCommand {
    Stop,
}

pub fn start_connectivity_thread(
    config: ProtocolConfig,
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

    let handle = std::thread::spawn({
        let sender_operations_ext = sender_operations_ext.clone();
        move || {
            let (mut peer_manager_handler, sender_peers) =
                PeerManagementHandler::new(initial_peers);
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
                sender_peers,
                id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            };

            let mut peernet_config =
                PeerNetConfiguration::default(MassaHandshake::new(), message_handlers);
            peernet_config.self_keypair = config.keypair.clone();
            peernet_config.fallback_function = Some(&fallback_function);
            //TODO: Add the rest of the config
            peernet_config.max_in_connections = config.max_in_connections;
            peernet_config.max_out_connections = config.max_out_connections;

            let mut manager = PeerNetManager::new(peernet_config);

            // Create cache outside of the op handler because it could be used by other handlers
            //TODO: Add real config values
            let operation_cache = Arc::new(RwLock::new(OperationCache::new(
                NonZeroUsize::new(usize::MAX).unwrap(),
                NonZeroUsize::new(usize::MAX).unwrap(),
            )));

            // Start handlers
            let mut operation_handler = OperationHandler::new(
                pool_controller.clone(),
                storage.clone_without_refs(),
                config.clone(),
                operation_cache,
                manager.active_connections.clone(),
                receiver_operations,
                sender_operations_ext,
                receiver_operations_ext,
            );
            let mut endorsement_handler = EndorsementHandler::new(
                pool_controller,
                storage,
                manager.active_connections.clone(),
                receiver_endorsements,
                receiver_endorsements_ext,
            );
            let mut block_handler = BlockHandler::new(
                manager.active_connections.clone(),
                receiver_blocks,
                receiver_blocks_ext,
            );

            for (addr, transport) in config.listeners {
                manager.start_listener(transport, addr).expect(&format!(
                    "Failed to start listener {:?} of transport {:?} in protocol",
                    addr, transport
                ));
            }
            //Try to connect to peers
            loop {
                select! {
                    recv(receiver) -> msg => {
                        if let Ok(ConnectivityCommand::Stop) = msg {
                            if let Some(handle) = peer_manager_handler.thread_join.take() {
                                handle.join().expect("Failed to join peer manager thread");
                            }
                            operation_handler.stop();
                            endorsement_handler.stop();
                            block_handler.stop();
                            break;
                        }
                    }
                    default(Duration::from_millis(2000)) => {
                        // Check if we need to connect to peers
                        let nb_connection_to_try = {
                            let active_connections = manager.active_connections.read();
                            let connection_to_try = active_connections.max_out_connections - active_connections.nb_out_connections;
                            if connection_to_try <= 0 {
                                continue;
                            }
                           connection_to_try
                        };
                        // Get the best peers
                        {
                            let peer_db_read = peer_manager_handler.peer_db.read();
                            let best_peers = peer_db_read.index_by_newest.iter().take(nb_connection_to_try as usize);
                            for (_timestamp, peer_id) in best_peers {
                                let peer_info = peer_db_read.peers.get(peer_id).unwrap();
                                if peer_info.last_announce.listeners.is_empty() {
                                    continue;
                                }
                                {
                                    let active_connections = manager.active_connections.read();
                                    if active_connections.connections.contains_key(peer_id) {
                                        continue;
                                    }
                                }
                                // We only manage TCP for now
                                let (addr, _transport) = peer_info.last_announce.listeners.iter().next().unwrap();
                                manager.try_connect(*addr, Duration::from_millis(200), &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {}))).unwrap();
                            };
                        };
                    }
                }
            }
        }
    });
    // Start controller
    let controller = ProtocolControllerImpl::new(
        sender_blocks_ext,
        sender_operations_ext,
        sender_endorsements_ext,
    );
    Ok(((sender, handle), controller))
}
