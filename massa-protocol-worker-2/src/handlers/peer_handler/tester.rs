use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use crossbeam::channel::Sender;
use massa_signature::KeyPair;

use crate::{
    config::PeerNetConfiguration,
    error::PeerNetError,
    handlers::{MessageHandler, MessageHandlers},
    internal_handlers::peer_management::{announcement::Announcement, PeerInfo},
    network_manager::PeerNetManager,
    peer::HandshakeHandler,
    peer_id::PeerId,
    transports::{endpoint::Endpoint, OutConnectionConfig, TcpOutConnectionConfig, TransportType},
};

use super::{PeerDB, SharedPeerDB};

#[derive(Clone)]
pub struct EmptyHandshake {
    peer_db: SharedPeerDB,
}

impl HandshakeHandler for EmptyHandshake {
    fn perform_handshake(
        &mut self,
        _: &KeyPair,
        endpoint: &mut Endpoint,
        _: &HashMap<SocketAddr, TransportType>,
        _: &MessageHandlers,
    ) -> Result<PeerId, PeerNetError> {
        let data = endpoint.receive()?;
        let peer_id = PeerId::from_bytes(&data[..32].try_into().unwrap())?;
        let announcement = Announcement::from_bytes(&data[32..], &peer_id)?;
        self.peer_db.write().peers.insert(
            peer_id.clone(),
            PeerInfo {
                last_announce: announcement,
            },
        );
        Ok(peer_id)
    }
}

pub struct Tester {
    handler: Option<JoinHandle<()>>,
}

impl Tester {
    pub fn new(peer_db: SharedPeerDB, listener: (SocketAddr, TransportType)) -> Self {
        let handle = std::thread::spawn(move || {
            let mut config = PeerNetConfiguration::default(EmptyHandshake { peer_db });
            config.fallback_function = Some(&empty_fallback);
            config.max_out_connections = 1;
            let mut network_manager = PeerNetManager::new(config);
            network_manager
                .try_connect(
                    listener.0,
                    Duration::from_millis(200),
                    &OutConnectionConfig::Tcp(TcpOutConnectionConfig {}),
                )
                .unwrap();
            std::thread::sleep(Duration::from_millis(10));
        });
        Self {
            handler: Some(handle),
        }
    }
}

pub fn empty_fallback(
    _keypair: &KeyPair,
    _endpoint: &mut Endpoint,
    _listeners: &HashMap<SocketAddr, TransportType>,
    _message_handlers: &MessageHandlers,
) -> Result<(), PeerNetError> {
    println!("Fallback function called");
    std::thread::sleep(Duration::from_millis(10000));
    Ok(())
}
