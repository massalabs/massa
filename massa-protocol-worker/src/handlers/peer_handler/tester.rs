use std::{
    collections::HashMap,
    io::Read,
    net::{IpAddr, SocketAddr},
    thread::JoinHandle,
    time::Duration,
};

use crate::messages::MessagesHandler;
use massa_channel::{receiver::MassaReceiver, sender::MassaSender, MassaChannel};
use massa_models::version::{Version, VersionDeserializer};
use massa_protocol_exports::{PeerConnectionType, PeerId, PeerIdDeserializer, ProtocolConfig};
use massa_serialization::{DeserializeError, Deserializer};
use massa_time::MassaTime;
use peernet::{
    error::{PeerNetError, PeerNetResult},
    messages::MessagesHandler as PeerNetMessagesHandler,
    transports::TransportType,
};
use std::cmp::Reverse;
use tracing::info;

use super::{
    announcement::{AnnouncementDeserializer, AnnouncementDeserializerArgs},
    models::PeerInfo,
    SharedPeerDB,
};
use crate::wrap_network::ActiveConnectionsTrait;
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
        target_out_connections: HashMap<String, (Vec<IpAddr>, usize)>,
        default_target_out_connections: usize,
    ) -> (
        (
            MassaSender<(PeerId, HashMap<SocketAddr, TransportType>)>,
            MassaReceiver<(PeerId, HashMap<SocketAddr, TransportType>)>,
        ),
        Vec<Tester>,
    ) {
        let mut testers = Vec::new();

        // create shared channel between thread for launching test
        let (test_sender, test_receiver) = MassaChannel::new(
            "test_sender".to_string(),
            Some(config.max_size_channel_commands_peer_testers),
        );

        for _ in 0..config.thread_tester_count {
            testers.push(Tester::new(
                peer_db.clone(),
                active_connections.clone(),
                config.clone(),
                test_receiver.clone(),
                messages_handler.clone(),
                target_out_connections.clone(),
                default_target_out_connections,
            ));
        }

        ((test_sender, test_receiver), testers)
    }

    pub fn tcp_handshake(
        messages_handler: MessagesHandler,
        peer_db: SharedPeerDB,
        announcement_deserializer: AnnouncementDeserializer,
        version_deserializer: VersionDeserializer,
        peer_id_deserializer: PeerIdDeserializer,
        addr: SocketAddr,
        our_version: Version,
    ) -> PeerNetResult<PeerId> {
        let result = {
            let mut socket =
                std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500))
                    .map_err(|e| PeerNetError::PeerConnectionError.new("connect", e, None))?;

            // data.receive() from Endpoint
            let mut len_bytes = vec![0u8; 4];
            socket
                .read_exact(&mut len_bytes)
                .map_err(|err| PeerNetError::PeerConnectionError.new("recv len", err, None))?;

            let res_size = u32::from_be_bytes(len_bytes.try_into().map_err(|err| {
                PeerNetError::PeerConnectionError.error("recv len", Some(format!("{:?}", err)))
            })?);
            if res_size > 1048576000 {
                return Err(PeerNetError::InvalidMessage
                    .error("len too long", Some(format!("{:?}", res_size))));
            }
            let mut data = vec![0u8; res_size as usize];
            socket
                .read_exact(&mut data)
                .map_err(|err| PeerNetError::PeerConnectionError.new("recv data", err, None))?;

            // handshake
            if data.is_empty() {
                return Err(PeerNetError::HandshakeError.error(
                    "Tester Handshake",
                    Some(String::from("Peer didn't accepted us")),
                ));
            }
            let (data, peer_id) = peer_id_deserializer
                .deserialize::<DeserializeError>(&data)
                .map_err(|_| {
                    PeerNetError::HandshakeError.error(
                        "Massa Handshake",
                        Some("Failed to deserialize PeerId".to_string()),
                    )
                })?;
            let res = {
                {
                    // check if peer is banned
                    let peer_db_read = peer_db.read();
                    if let Some(info) = peer_db_read.peers.get(&peer_id) {
                        if info.state == super::PeerState::Banned {
                            return Err(PeerNetError::HandshakeError
                                .error("Tester Handshake", Some(String::from("Peer is banned"))));
                        }
                    }
                }

                let (data, version) = version_deserializer
                    .deserialize::<DeserializeError>(data)
                    .map_err(|err| {
                        PeerNetError::HandshakeError.error(
                            "Tester Handshake",
                            Some(format!("Failed to deserialize version: {}", err)),
                        )
                    })?;
                if !our_version.is_compatible(&version) {
                    return Err(PeerNetError::HandshakeError.error(
                        "Massa Handshake",
                        Some(format!("Received version incompatible: {}", version)),
                    ));
                }
                let id = data.first().ok_or(
                    PeerNetError::HandshakeError
                        .error("Massa Handshake", Some("Failed to get id".to_string())),
                )?;
                match id {
                    0 => {
                        let (_, announcement) = announcement_deserializer
                            .deserialize::<DeserializeError>(data.get(1..).ok_or(
                                PeerNetError::HandshakeError.error(
                                    "Massa Handshake",
                                    Some("Failed to get buffer".to_string()),
                                ),
                            )?)
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
                            return Err(PeerNetError::HandshakeError.error(
                                "Tester Handshake",
                                Some(String::from("Invalid signature")),
                            ));
                        }
                        //TODO: Check ip we are connected match one of the announced ips
                        {
                            let mut peer_db_write = peer_db.write();
                            //TODO: Hacky change it when better management ip/listeners
                            if !announcement.listeners.is_empty() {
                                peer_db_write
                                    .index_by_newest
                                    .retain(|(_, peer_id_stored)| peer_id_stored != &peer_id);
                                peer_db_write
                                    .index_by_newest
                                    .insert((Reverse(announcement.timestamp), peer_id.clone()));
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
                        messages_handler.handle(
                            data.get(1..).ok_or(PeerNetError::HandshakeError.error(
                                "Massa Handshake",
                                Some("Failed to get buffer".to_string()),
                            ))?,
                            &peer_id,
                        )?;
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
                let mut peer_db_write = peer_db.write();
                peer_db_write.peers.entry(peer_id).and_modify(|info| {
                    info.state = super::PeerState::HandshakeFailed;
                });
            }
            if let Err(e) = socket.shutdown(std::net::Shutdown::Both) {
                tracing::log::error!("Failed to shutdown socket: {}", e);
            }
            res
        };

        result
    }

    /// Create a new tester (spawn a thread)
    pub fn new(
        peer_db: SharedPeerDB,
        active_connections: Box<dyn ActiveConnectionsTrait>,
        protocol_config: ProtocolConfig,
        receiver: MassaReceiver<(PeerId, HashMap<SocketAddr, TransportType>)>,
        messages_handler: MessagesHandler,
        target_out_connections: HashMap<String, (Vec<IpAddr>, usize)>,
        default_target_out_connections: usize,
    ) -> Self {
        tracing::log::debug!("running new tester");

        let handle = std::thread::Builder::new()
        .name("protocol-peer-handler-tester".to_string())
        .spawn(move || {
            let db = peer_db;
            let active_connections = active_connections.clone();

            let announcement_deser = AnnouncementDeserializer::new(
                AnnouncementDeserializerArgs {
                    max_listeners: protocol_config.max_size_listeners_per_peer,
                },
            );


            //let mut network_manager = PeerNetManager::new(config);
            let protocol_config = protocol_config.clone();
            loop {
                crossbeam::select! {
                    recv(receiver) -> res => {
                        match res {
                            Ok(listener) => {
                                if listener.1.is_empty() {
                                    continue;
                                }
                                //Test
                                let peers_connected = active_connections.get_peers_connected();
                                let slots_out_connections: HashMap<String, (Vec<IpAddr>, usize)> = target_out_connections
                                    .iter()
                                    .map(|(key, value)| {
                                        let mut value = value.clone();
                                        value.1 = value.1.saturating_sub(peers_connected.iter().filter(|(_, (_, ty, category))| {
                                            if ty == &PeerConnectionType::IN {
                                                return false;
                                            }
                                            if let Some(category) = category {
                                                category == key
                                            } else {
                                                false
                                            }
                                        }).count());
                                        (key.clone(), value)
                                    })
                                    .collect();
                                let slot_default_category = default_target_out_connections.saturating_sub(peers_connected.iter().filter(|(_, (_, ty, category))| {
                                    if ty == &PeerConnectionType::IN {
                                        return false;
                                    }
                                    if category.is_some() {
                                        return false;
                                    }
                                    true
                                }).count());
                                {
                                    let now = MassaTime::now().unwrap();
                                    let db = db.clone();
                                    // receive new listener to test
                                    for (addr, _) in listener.1.iter() {
                                        //Find category of that address
                                        let ip_canonical = addr.ip().to_canonical();
                                        let cooldown = 'cooldown: {
                                            for category in &slots_out_connections {
                                                if category.1.0.contains(&ip_canonical) {
                                                    if category.1.1 == 0 {
                                                        break 'cooldown Duration::from_secs(60 * 60 * 2);
                                                    } else {
                                                        break 'cooldown Duration::from_secs(30);
                                                    }
                                                }
                                            }
                                            if slot_default_category == 0 {
                                                Duration::from_secs(60 * 60 * 2)
                                            } else {
                                                Duration::from_secs(30)
                                            }
                                        };
                                        //TODO: Change it to manage multiple listeners SAFETY: Check above
                                        {
                                            let mut db_write = db.write();
                                            if let Some(last_tested_time) = db_write.tested_addresses.get(addr) {
                                                let last_tested_time = last_tested_time.estimate_instant().expect("Time went backward");
                                                if last_tested_time.elapsed() < cooldown {
                                                    continue;
                                                }
                                            }
                                            db_write.tested_addresses.insert(*addr, now);
                                        }
                                        // TODO:  Don't launch test if peer is already connected to us as a normal connection.
                                        // Maybe we need to have a way to still update his last announce timestamp because he is a great peer
                                        if ip_canonical.is_global() && !active_connections.get_peers_connected().iter().any(|(_, (addr, _, _))| addr.ip().to_canonical() == ip_canonical) {
                                            //Don't test our local addresses
                                            for (local_addr, _transport) in protocol_config.listeners.iter() {
                                                if addr == local_addr {
                                                    continue;
                                                }
                                            }
                                            //Don't test our proper ip
                                            if let Some(ip) = protocol_config.routable_ip {
                                                if ip.to_canonical() == ip_canonical {
                                                    continue;
                                                }
                                            }
                                            info!("testing peer {} listener addr: {}", &listener.0, &addr);


                                            let res = Tester::tcp_handshake(
                                                messages_handler.clone(),
                                                db.clone(),
                                                announcement_deser.clone(),
                                                VersionDeserializer::new(),
                                                PeerIdDeserializer::new(),
                                                *addr,
                                                protocol_config.version,
                                            );

                                            // let _res =  network_manager.try_connect(
                                            //     *addr,
                                            //     protocol_config.timeout_connection.to_duration(),
                                            //     &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig::new(protocol_config.read_write_limit_bytes_per_second / 10, Duration::from_millis(100)))),
                                            // );

                                            tracing::log::debug!("{:?}", res);
                                        }
                                    };
                                }
                            },
                            Err(_e) => break,
                        }
                    }
                    default(Duration::from_secs(2)) => {
                        // If no message in 2 seconds they will test a peer that hasn't been tested for long time

                        let Some(listener) = db.read().get_oldest_peer(Duration::from_secs(60 * 60 * 2)) else {
                            continue;
                        };

                        {
                            let mut db = db.write();
                            db.tested_addresses.insert(listener, MassaTime::now().unwrap());
                        }

                        // we try to connect to all peer listener (For now we have only one listener)
                        let ip_canonical = listener.ip().to_canonical();
                        if !ip_canonical.is_global() || active_connections.get_peers_connected().iter().any(|(_, (addr, _, _))| addr.ip().to_canonical() == ip_canonical) {
                            continue;
                        }
                        //Don't test our local addresses
                        for (local_addr, _transport) in protocol_config.listeners.iter() {
                            if listener == *local_addr {
                                continue;
                            }
                        }
                        //Don't test our proper ip
                        if let Some(ip) = protocol_config.routable_ip {
                            if ip.to_canonical() == ip_canonical {
                                continue;
                            }
                        }
                        info!("testing listener addr: {}", &listener);

                        let res = Tester::tcp_handshake(
                            messages_handler.clone(),
                            db.clone(),
                            announcement_deser.clone(),
                            VersionDeserializer::new(),
                            PeerIdDeserializer::new(),
                            listener,
                            protocol_config.version,
                        );
                        // let res =  network_manager.try_connect(
                        //     listener,
                        //     protocol_config.timeout_connection.to_duration(),
                        //     &OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig::new(protocol_config.read_write_limit_bytes_per_second / 10, Duration::from_millis(100)))),
                        // );
                        tracing::log::debug!("{:?}", res);
                    }
                }
            }
        }).expect("OS failed to start peer tester thread");

        Self {
            handler: Some(handle),
        }
    }
}
