use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use crossbeam::{
    channel::{unbounded, Sender},
    select,
};
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use peernet::{
    config::PeerNetConfiguration,
    handlers::MessageHandlers,
    network_manager::PeerNetManager,
    peer_id::PeerId,
    transports::{OutConnectionConfig, TcpOutConnectionConfig, TransportType},
};

use crate::handlers::peer_handler::{fallback_function, MassaHandshake, PeerManagementHandler};

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
        let (mut peer_manager, peer_manager_sender) = PeerManagementHandler::new(initial_peers);

        let mut peernet_config = PeerNetConfiguration::default(MassaHandshake {});
        peernet_config.self_keypair = config.keypair;
        peernet_config.fallback_function = Some(&fallback_function);
        //TODO: Add the rest of the config
        peernet_config.max_in_connections = config.max_in_connections;
        peernet_config.max_out_connections = config.max_out_connections;

        let mut message_handlers: MessageHandlers = Default::default();
        message_handlers.add_handler(0, peer_manager_sender);
        peernet_config.message_handlers = message_handlers;

        let mut manager = PeerNetManager::new(peernet_config);
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
                        if let Some(handle) = peer_manager.thread_join.take() {
                            handle.join().expect("Failed to join peer manager thread");
                        }
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
                        let peer_db_read = peer_manager.peer_db.read();
                        peer_db_read.index_by_newest.iter().take(nb_connection_to_try as usize).for_each(|(_timestamp, peer_id)| {
                            let peer_info = peer_db_read.peers.get(peer_id).unwrap();
                            if peer_info.last_announce.listeners.is_empty() {
                                return;
                            }
                            // We only manage TCP for now
                            let (addr, _transport) = peer_info.last_announce.listeners.iter().next().unwrap();
                            manager.try_connect(*addr, Duration::from_millis(200), &OutConnectionConfig::Tcp(TcpOutConnectionConfig {})).unwrap();
                        });
                    }
                }
            }
            // TODO: add a way to stop the thread
        }
    });
    Ok((sender, handle))
}
