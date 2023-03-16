use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use crossbeam::{
    channel::{unbounded, Sender},
    select,
};
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use peernet::{
    config::PeerNetConfiguration,
    handlers::{MessageHandler, MessageHandlers},
    network_manager::PeerNetManager,
    peer_id::PeerId,
    transports::{OutConnectionConfig, TcpOutConnectionConfig, TransportType},
};

use crate::handlers::{
    endorsement_handler::EndorsementHandler,
    operation_handler::OperationHandler,
    peer_handler::{fallback_function, MassaHandshake, PeerManagementHandler}, block_handler::BlockHandler,
};

pub enum ConnectivityCommand {
    Stop,
}

pub fn start_connectivity_thread(
    config: ProtocolConfig,
) -> Result<(Sender<ConnectivityCommand>, JoinHandle<()>), ProtocolError> {
    let (sender, receiver) = unbounded();
    let initial_peers = serde_json::from_str::<HashMap<PeerId, HashMap<SocketAddr, TransportType>>>(
        &std::fs::read_to_string(&config.initial_peers)?,
    )?;
    let handle = std::thread::spawn(move || {
        let (mut peer_manager_handler, peer_manager_handler_sender) =
            PeerManagementHandler::new(initial_peers);
        //TODO: Bound the channel
        let (sender_operations, receiver_operations) = unbounded();
        let (sender_endorsements, receiver_endorsements) = unbounded();
        let (sender_blocks, receiver_blocks) = unbounded();

        let mut peernet_config = PeerNetConfiguration::default(MassaHandshake {});
        peernet_config.self_keypair = config.keypair;
        peernet_config.fallback_function = Some(&fallback_function);
        //TODO: Add the rest of the config
        peernet_config.max_in_connections = config.max_in_connections;
        peernet_config.max_out_connections = config.max_out_connections;

        let mut message_handlers: MessageHandlers = Default::default();
        message_handlers.add_handler(0, peer_manager_handler_sender);
        message_handlers.add_handler(1, MessageHandler::new(sender_operations));
        message_handlers.add_handler(2, MessageHandler::new(sender_endorsements));
        message_handlers.add_handler(3, MessageHandler::new(sender_blocks));
        peernet_config.message_handlers = message_handlers;

        let mut manager = PeerNetManager::new(peernet_config);

        let mut operation_handler =
            OperationHandler::new(manager.active_connections.clone(), receiver_operations);
        let mut endorsement_handler =
            EndorsementHandler::new(manager.active_connections.clone(), receiver_endorsements);
        let mut block_handler = BlockHandler::new(manager.active_connections.clone(), receiver_blocks);

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
                            manager.try_connect(*addr, Duration::from_millis(200), &OutConnectionConfig::Tcp(TcpOutConnectionConfig {})).unwrap();
                        };
                    };
                }
            }
            // TODO: add a way to stop the thread
        }
    });
    Ok((sender, handle))
}
