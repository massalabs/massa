use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use crossbeam::{
    channel::{unbounded, Sender},
    select,
};
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use peernet::{
    config::PeerNetConfiguration, handlers::MessageHandlers, network_manager::PeerNetManager,
    peer_id::PeerId, transports::TransportType,
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

        let mut message_handlers: MessageHandlers = Default::default();
        message_handlers.add_handler(0, peer_manager_sender);

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
                default(Duration::from_millis(100)) => {
                    // Check if we need to connect to peers
                }
            }
            // TODO: add a way to stop the thread
        }
    });
    Ok((sender, handle))
}
