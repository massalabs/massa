use std::{thread::JoinHandle, time::Duration};

use crossbeam::{channel::{unbounded, Sender}, select};
use massa_protocol_exports_2::{ProtocolError, ProtocolConfig};
use peernet::{network_manager::PeerNetManager, config::PeerNetConfiguration};

pub enum ConnectivityCommand {
    Stop,
}

pub fn start_connectivity_thread(config: ProtocolConfig) -> Result<(Sender<ConnectivityCommand>, JoinHandle<()>), ProtocolError> {
    let (sender, receiver) = unbounded();
    let handle = std::thread::spawn(move || {
            //TODO: when https://github.com/massalabs/PeerNet/issues/12 is done
        let peernet_config = PeerNetConfiguration::default();
        let mut manager = PeerNetManager::new(peernet_config);
        for (addr, transport) in config.listeners {
            manager.start_listener(transport, addr).expect(&format!("Failed to start listener {:?} of transport {:?} in protocol", addr, transport));
        }
        //Try to connect to peers
        loop {
            select! {
                recv(receiver) -> msg => {
                    if let Ok(ConnectivityCommand::Stop) = msg {
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