use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use massa_serialization::{DeserializeError, Deserializer};
use peernet::{
    config::PeerNetConfiguration,
    error::{PeerNetError, PeerNetResult},
    messages::MessagesHandler,
    network_manager::PeerNetManager,
    peer::HandshakeHandler,
    peer_id::PeerId,
    transports::{endpoint::Endpoint, OutConnectionConfig, TcpOutConnectionConfig, TransportType},
    types::KeyPair,
};

use super::{
    announcement::{AnnouncementDeserializer, AnnouncementDeserializerArgs},
    models::PeerInfo,
    SharedPeerDB,
};

#[derive(Clone)]
pub struct TesterHandshake {
    peer_db: SharedPeerDB,
    announcement_deserializer: AnnouncementDeserializer,
}

impl TesterHandshake {
    pub fn new(peer_db: SharedPeerDB) -> Self {
        Self {
            peer_db,
            announcement_deserializer: AnnouncementDeserializer::new(
                AnnouncementDeserializerArgs {
                    //TODO: Config
                    max_listeners: 100,
                },
            ),
        }
    }
}

#[derive(Clone)]
pub struct TesterMessagesHandler;

impl MessagesHandler for TesterMessagesHandler {
    fn deserialize_id<'a>(
        &self,
        data: &'a [u8],
        _peer_id: &PeerId,
    ) -> PeerNetResult<(&'a [u8], u64)> {
        Ok((data, 0))
    }

    fn handle(&self, _id: u64, _data: &[u8], _peer_id: &PeerId) -> PeerNetResult<()> {
        Ok(())
    }
}

impl HandshakeHandler for TesterHandshake {
    fn perform_handshake<TesterMessagesHandler>(
        &mut self,
        _: &KeyPair,
        endpoint: &mut Endpoint,
        _: &HashMap<SocketAddr, TransportType>,
        _: TesterMessagesHandler,
    ) -> PeerNetResult<PeerId> {
        println!("Before receive");
        let data = endpoint.receive()?;
        println!("Received handshake data: {:?}", data);
        let peer_id = PeerId::from_bytes(&data[..32].try_into().unwrap())?;

        let res = {
            {
                // check if peer is banned else set state to InHandshake
                let mut peer_db_write = self.peer_db.write();
                if let Some(info) = peer_db_write.peers.get_mut(&peer_id) {
                    if info.state == super::PeerState::Banned {
                        return Err(PeerNetError::HandshakeError
                            .error("Tester Handshake", Some(String::from("Peer is banned"))));
                    } else {
                        info.state = super::PeerState::InHandshake;
                    }
                }
            }

            let (_, announcement) = self
                .announcement_deserializer
                .deserialize::<DeserializeError>(&data[32..])
                .map_err(|err| {
                    PeerNetError::HandshakeError.error(
                        "Tester Handshake",
                        Some(format!("Failed to deserialize announcement: {}", err)),
                    )
                })?;
            if peer_id
                .verify_signature(&announcement.hash, &announcement.signature)
                .is_err()
            {
                return Err(PeerNetError::HandshakeError
                    .error("Tester Handshake", Some(String::from("Invalid signature"))));
            }
            //TODO: Check ip we are connected match one of the announced ips
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
                        info.state = super::PeerState::Trusted;
                    })
                    .or_insert(PeerInfo {
                        last_announce: announcement,
                        state: super::PeerState::Trusted,
                    });
            }

            Ok(peer_id.clone())
        };

        // if handshake failed, we set the peer state to HandshakeFailed
        if res.is_err() {
            let mut peer_db_write = self.peer_db.write();
            peer_db_write.peers.entry(peer_id).and_modify(|info| {
                info.state = super::PeerState::HandshakeFailed;
            });
        }

        res
    }
}

pub struct Tester {
    pub handler: Option<JoinHandle<()>>,
}

impl Tester {
    pub fn new(peer_db: SharedPeerDB, listener: (SocketAddr, TransportType)) -> Self {
        let handle = std::thread::spawn(move || {
            let mut config = PeerNetConfiguration::default(
                TesterHandshake::new(peer_db),
                TesterMessagesHandler {},
            );
            config.fallback_function = Some(&empty_fallback);
            config.max_out_connections = 1;
            let mut network_manager = PeerNetManager::new(config);
            network_manager
                .try_connect(
                    listener.0,
                    Duration::from_millis(200),
                    &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {})),
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
) -> PeerNetResult<()> {
    println!("Fallback function called");
    std::thread::sleep(Duration::from_millis(10000));
    Ok(())
}
