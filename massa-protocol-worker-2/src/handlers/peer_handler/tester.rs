use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use peernet::{
    config::PeerNetConfiguration,
    error::PeerNetError,
    handlers::MessageHandlers,
    network_manager::PeerNetManager,
    peer::HandshakeHandler,
    peer_id::PeerId,
    transports::{endpoint::Endpoint, OutConnectionConfig, TcpOutConnectionConfig, TransportType},
    types::KeyPair,
};

use super::{announcement::Announcement, PeerInfo, SharedPeerDB};

#[derive(Clone)]
pub struct TesterHandshake {
    peer_db: SharedPeerDB,
}

impl HandshakeHandler for TesterHandshake {
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
        {
            let mut peer_db_write = self.peer_db.write();
            peer_db_write
                .index_by_newest
                .insert(announcement.timestamp, peer_id.clone());
            peer_db_write
                .peers
                .entry(peer_id.clone())
                .and_modify(|info| {
                    if info.last_announce.timestamp < announcement.timestamp {
                        info.last_announce = announcement.clone();
                    }
                })
                .or_insert(PeerInfo {
                    last_announce: announcement,
                });
        }
        Ok(peer_id)
    }
}

pub struct Tester {
    pub handler: Option<JoinHandle<()>>,
}

impl Tester {
    pub fn new(peer_db: SharedPeerDB, listener: (SocketAddr, TransportType)) -> Self {
        let handle = std::thread::spawn(move || {
            let mut config = PeerNetConfiguration::default(TesterHandshake { peer_db });
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
