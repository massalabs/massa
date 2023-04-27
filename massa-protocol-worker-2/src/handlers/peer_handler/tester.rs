use std::{
    collections::HashMap,
    net::SocketAddr,
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossbeam::channel::Sender;
use massa_protocol_exports_2::ProtocolConfig;
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
use std::cmp::Reverse;

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
    pub fn new(peer_db: SharedPeerDB, config: ProtocolConfig) -> Self {
        Self {
            peer_db,
            announcement_deserializer: AnnouncementDeserializer::new(
                AnnouncementDeserializerArgs {
                    max_listeners: config.max_size_listeners_per_peer,
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
        let data = endpoint.receive()?;
        if data.is_empty() {
            return Err(PeerNetError::HandshakeError.error(
                "Tester Handshake",
                Some(String::from("Peer didn't accepted us")),
            ));
        }
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
                    .insert(Reverse(announcement.timestamp), peer_id.clone());
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
        endpoint.shutdown();
        res
    }
}

pub struct Tester {
    pub handler: Option<JoinHandle<()>>,
}

impl Tester {
    pub fn run(
        config: &ProtocolConfig,
        peer_db: SharedPeerDB,
    ) -> (
        Sender<(PeerId, HashMap<SocketAddr, TransportType>)>,
        Vec<Tester>,
    ) {
        let mut testers = Vec::new();

        // create shared channel between thread for launching test
        let (test_sender, test_receiver) =
            crossbeam::channel::bounded(config.max_size_channel_commands_peer_testers);

        for _ in 0..config.thread_tester_count {
            testers.push(Tester::new(
                peer_db.clone(),
                config.clone(),
                test_receiver.clone(),
            ));
        }

        (test_sender, testers)
    }

    /// Create a new tester (spawn a thread)
    pub fn new(
        peer_db: SharedPeerDB,
        config: ProtocolConfig,
        receiver: crossbeam::channel::Receiver<(PeerId, HashMap<SocketAddr, TransportType>)>,
    ) -> Self {
        tracing::log::debug!("running new tester");

        let handle = std::thread::Builder::new()
        .name("protocol-peer-handler-tester".to_string())
        .spawn(move || {
            let db = peer_db.clone();
            let mut config = PeerNetConfiguration::default(
                TesterHandshake::new(peer_db, config),
                TesterMessagesHandler {},
            );
            config.fallback_function = Some(&empty_fallback);
            config.max_out_connections = 1;

            let mut network_manager = PeerNetManager::new(config);
            loop {
                crossbeam::select! {
                    recv(receiver) -> res => {
                        match res {
                            Ok(listener) => {
                                if let Some(peer_info) = db.read().peers.get(&listener.0) {
                                    let timestamp = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .expect("Time went backward")
                                    .as_millis();
                                    let elapsed_secs = (timestamp - peer_info.last_announce.timestamp) / 1000000;
                                    if elapsed_secs < 60 {
                                        continue;
                                    }
                                }
                                //TODO: Don't launch test if peer is already connected to us as a normal connection.
                                // Maybe we need to have a way to still update his last announce timestamp because he is a great peer

                                // receive new listener to test
                                listener.1.iter().for_each(|(addr, _transport)| {
                                    let _res =  network_manager.try_connect(
                                        *addr,
                                        Duration::from_millis(500),
                                        &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {})),
                                    );
                                });
                            },
                            Err(_e) => break,
                        }
                    }
                    default(Duration::from_secs(2)) => {
                        // If no message in 2 seconds they will test a peer that hasn't been tested for long time

                        // we find the last peer that has been tested
                        let Some((peer_id, peer_info)) = db.read().get_oldest_peer() else {
                            dbg!("No peer to test");
                            continue;
                        };

                        let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("Time went backward")
                        .as_millis();
                        let elapsed_secs = (timestamp - peer_info.last_announce.timestamp) / 1000000;
                        if elapsed_secs < 60 {
                            continue;
                        }

                        dbg!("Testing peer {}", peer_id);
                        // we try to connect to all peer listener (For now we have only one listener)
                        peer_info.last_announce.listeners.iter().for_each(|listener| {
                           let _res =  network_manager.try_connect(
                                *listener.0,
                                Duration::from_millis(200),
                                &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {})),
                            );
                        });
                    }
                }
            }
        }).expect("OS failed to start peer tester thread");

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
    std::thread::sleep(Duration::from_millis(10000));
    Ok(())
}
