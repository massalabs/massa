use std::{
    collections::HashMap,
    net::SocketAddr,
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::messages::MessagesHandler;
use crossbeam::channel::Sender;
use massa_protocol_exports::ProtocolConfig;
use massa_serialization::{DeserializeError, Deserializer};
use peernet::{
    config::PeerNetConfiguration,
    error::{PeerNetError, PeerNetResult},
    messages::MessagesHandler as PeerNetMessagesHandler,
    network_manager::PeerNetManager,
    peer::InitConnectionHandler,
    peer_id::PeerId,
    transports::{endpoint::Endpoint, OutConnectionConfig, TcpOutConnectionConfig, TransportType},
    types::KeyPair,
};
use std::cmp::Reverse;
use tracing::info;

use super::{
    announcement::{AnnouncementDeserializer, AnnouncementDeserializerArgs},
    models::PeerInfo,
    SharedPeerDB,
};
use crate::wrap_network::ActiveConnectionsTrait;

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

impl InitConnectionHandler for TesterHandshake {
    fn perform_handshake<MassaMessagesHandler: PeerNetMessagesHandler>(
        &mut self,
        _: &KeyPair,
        endpoint: &mut Endpoint,
        _: &HashMap<SocketAddr, TransportType>,
        messages_handler: MassaMessagesHandler,
    ) -> PeerNetResult<PeerId> {
        let data = endpoint.receive()?;
        if data.is_empty() {
            return Err(PeerNetError::HandshakeError.error(
                "Tester Handshake",
                Some(String::from("Peer didn't accepted us")),
            ));
        }
        let peer_id = PeerId::from_bytes(&data[..32].try_into().map_err(|_| {
            PeerNetError::HandshakeError.error(
                "Massa Handshake",
                Some("Failed to deserialize PeerId".to_string()),
            )
        })?)?;
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
            let id = data.get(32).ok_or(
                PeerNetError::HandshakeError
                    .error("Massa Handshake", Some("Failed to get id".to_string())),
            )?;
            match id {
                0 => {
                    let (_, announcement) = self
                        .announcement_deserializer
                        .deserialize::<DeserializeError>(&data[33..])
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
                        //TODO: Hacky change it when better management ip/listeners
                        if !announcement.listeners.is_empty() {
                            peer_db_write
                                .index_by_newest
                                .retain(|_, peer_id_stored| peer_id_stored != &peer_id);
                            peer_db_write
                                .index_by_newest
                                .insert(Reverse(announcement.timestamp), peer_id.clone());
                        }
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
                }
                1 => {
                    let (received, id) = messages_handler.deserialize_id(&data[33..], &peer_id)?;
                    messages_handler.handle(id, received, &peer_id)?;
                    Err(PeerNetError::HandshakeError.error(
                            "Massa Handshake",
                            Some("Tester Handshake failed received a message that our connection has been refused".to_string()),
                        ))
                    //TODO: Add the peerdb but for now impossible as we don't have announcement and we need one to place in peerdb
                }
                _ => Err(PeerNetError::HandshakeError
                    .error("Massa handshake", Some("Invalid id".to_string()))),
            }
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

    fn fallback_function(
        &mut self,
        _keypair: &KeyPair,
        _endpoint: &mut Endpoint,
        _listeners: &HashMap<SocketAddr, TransportType>,
    ) -> PeerNetResult<()> {
        std::thread::sleep(Duration::from_millis(10000));
        Ok(())
    }
}

pub struct Tester {
    pub handler: Option<JoinHandle<()>>,
}

#[allow(clippy::type_complexity)]
impl Tester {
    pub fn run(
        config: &ProtocolConfig,
        active_connections: Box<dyn ActiveConnectionsTrait>,
        peer_db: SharedPeerDB,
        messages_handler: MessagesHandler,
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
                active_connections.clone(),
                config.clone(),
                test_receiver.clone(),
                messages_handler.clone(),
            ));
        }

        (test_sender, testers)
    }

    /// Create a new tester (spawn a thread)
    pub fn new(
        peer_db: SharedPeerDB,
        active_connections: Box<dyn ActiveConnectionsTrait>,
        protocol_config: ProtocolConfig,
        receiver: crossbeam::channel::Receiver<(PeerId, HashMap<SocketAddr, TransportType>)>,
        messages_handler: MessagesHandler,
    ) -> Self {
        tracing::log::debug!("running new tester");

        let handle = std::thread::Builder::new()
        .name("protocol-peer-handler-tester".to_string())
        .spawn(move || {
            let db = peer_db.clone();
            let active_connections = active_connections.clone();
            let mut config = PeerNetConfiguration::default(
                TesterHandshake::new(peer_db, protocol_config.clone()),
                messages_handler,
            );
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
                                    let elapsed_secs = (timestamp.saturating_sub(peer_info.last_announce.timestamp)) / 1000;
                                    if elapsed_secs < 60 {
                                        continue;
                                    }
                                }
                                // receive new listener to test
                                listener.1.iter().for_each(|(addr, _transport)| {
                                    // Don't launch test if peer is already connected to us as a normal connection.
                                    // Maybe we need to have a way to still update his last announce timestamp because he is a great peer
                                    if active_connections.check_addr_accepted(addr)  {
                                        //Don't test our local addresses
                                        for (local_addr, _transport) in protocol_config.listeners.iter() {
                                            if addr == local_addr {
                                                return;
                                            }
                                        }
                                        //Don't test our proper ip
                                        if let Some(ip) = protocol_config.routable_ip {
                                            if ip.to_canonical() == addr.ip().to_canonical() {
                                                return;
                                            }
                                        }
                                        info!("testing peer {} listener addr: {}", &listener.0, &addr);
                                        let _res =  network_manager.try_connect(
                                            *addr,
                                            Duration::from_millis(500),
                                            &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig::new(protocol_config.read_write_limit_bytes_per_second / 10, Duration::from_millis(100)))),
                                        );
                                    }
                                });
                            },
                            Err(_e) => break,
                        }
                    }
                    default(Duration::from_secs(2)) => {
                        // If no message in 2 seconds they will test a peer that hasn't been tested for long time

                        // we find the last peer that has been tested
                        let Some((peer_id, peer_info)) = db.read().get_oldest_peer() else {
                            continue;
                        };

                        let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("Time went backward")
                        .as_millis();
                        let elapsed_secs = (timestamp.saturating_sub(peer_info.last_announce.timestamp)) / 1000;
                        if elapsed_secs < 60 {
                            continue;
                        }

                        // we try to connect to all peer listener (For now we have only one listener)
                        peer_info.last_announce.listeners.iter().for_each(|listener| {
                            if !listener.0.ip().to_canonical().is_global() || !active_connections.check_addr_accepted(listener.0) {
                                return;
                            }
                            //Don't test our local addresses
                            for (local_addr, _transport) in protocol_config.listeners.iter() {
                                if listener.0 == local_addr {
                                    return;
                                }
                            }
                            //Don't test our proper ip
                            if let Some(ip) = protocol_config.routable_ip {
                                if ip.to_canonical() == listener.0.ip().to_canonical() {
                                    return;
                                }
                            }
                            info!("testing peer {} listener addr: {}", peer_id, &listener.0);
                            let _res =  network_manager.try_connect(
                                *listener.0,
                                Duration::from_millis(200),
                                &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig::new(protocol_config.read_write_limit_bytes_per_second / 10, Duration::from_millis(100)))),
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
