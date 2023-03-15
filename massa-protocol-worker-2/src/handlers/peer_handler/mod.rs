use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    thread::JoinHandle,
    time::Duration,
};

use parking_lot::RwLock;
use rand::{rngs::StdRng, RngCore, SeedableRng};

use peernet::{
    error::PeerNetError,
    handlers::{MessageHandler, MessageHandlers},
    peer::HandshakeHandler,
    peer_id::PeerId,
    transports::{endpoint::Endpoint, TransportType},
    types::Hash,
    types::{KeyPair, Signature},
};

use crate::handlers::peer_handler::tester::Tester;

use self::announcement::Announcement;

/// This file contains the definition of the peer management handler
/// This handler is here to check that announcements we receive are valid and
/// that all the endpoints we received are active.
mod announcement;
mod tester;

pub type InitialPeers = HashMap<PeerId, HashMap<SocketAddr, TransportType>>;

#[derive(Default)]
pub struct PeerDB {
    //TODO: Add state of the peer (banned, trusted, ...)
    pub peers: HashMap<PeerId, PeerInfo>,
    pub index_by_newest: BTreeMap<u128, PeerId>,
}

pub type SharedPeerDB = Arc<RwLock<PeerDB>>;

pub struct PeerManagementHandler {
    pub peer_db: SharedPeerDB,
    pub thread_join: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub enum PeerManagementMessage {
    // Receive the ip addresses sent by a peer when connecting.
    NewPeerConnected((PeerId, HashMap<SocketAddr, TransportType>)),
    // Receive the ip addresses sent by a peer that is already connected.
    ListPeers(Vec<(PeerId, HashMap<SocketAddr, TransportType>)>),
}

//TODO: Use a proper serialization system like we have in massa.
impl PeerManagementMessage {
    fn from_bytes(bytes: &[u8]) -> Result<Self, PeerNetError> {
        match bytes[0] {
            0 => {
                let peer_id = PeerId::from_bytes(&bytes[1..33].try_into().unwrap())?;
                let nb_listeners = u64::from_be_bytes(bytes[33..41].try_into().unwrap());
                let mut listeners = HashMap::with_capacity(nb_listeners as usize);
                let mut offset = 41;
                for _ in 0..nb_listeners {
                    let ip = match bytes[offset] {
                        4 => {
                            offset += 1;
                            let bytes: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
                            offset += 4;
                            IpAddr::from(bytes)
                        }
                        6 => {
                            offset += 1;
                            let bytes: [u8; 16] = bytes[offset..offset + 16].try_into().unwrap();
                            offset += 16;
                            IpAddr::from(bytes)
                        }
                        _ => return Err(PeerNetError::InvalidMessage),
                    };
                    let port = u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap());
                    offset += 2;
                    let transport_type = match bytes[offset] {
                        0 => TransportType::Tcp,
                        1 => TransportType::Quic,
                        _ => return Err(PeerNetError::InvalidMessage),
                    };
                    offset += 1;
                    listeners.insert(SocketAddr::new(ip, port), transport_type);
                }
                Ok(PeerManagementMessage::NewPeerConnected((
                    peer_id, listeners,
                )))
            }
            1 => {
                let nb_peers = u64::from_le_bytes(bytes[1..9].try_into().unwrap());
                let mut peers = Vec::with_capacity(nb_peers as usize);
                let mut offset = 9;
                for _ in 0..nb_peers {
                    let peer_id =
                        PeerId::from_bytes(&bytes[offset..offset + 32].try_into().unwrap())?;
                    offset += 32;
                    let nb_listeners =
                        u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap());
                    offset += 8;
                    let mut listeners = HashMap::with_capacity(nb_listeners as usize);
                    for _ in 0..nb_listeners {
                        let ip = match bytes[offset] {
                            4 => {
                                offset += 1;
                                let bytes: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
                                IpAddr::from(bytes)
                            }
                            6 => {
                                offset += 1;
                                let bytes: [u8; 16] =
                                    bytes[offset..offset + 16].try_into().unwrap();
                                IpAddr::from(bytes)
                            }
                            _ => return Err(PeerNetError::InvalidMessage),
                        };
                        offset += 16;
                        let port =
                            u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap());
                        offset += 2;
                        let transport_type = match bytes[offset] {
                            0 => TransportType::Tcp,
                            1 => TransportType::Quic,
                            _ => return Err(PeerNetError::InvalidMessage),
                        };
                        offset += 1;
                        listeners.insert(SocketAddr::new(ip, port), transport_type);
                    }
                    peers.push((peer_id, listeners));
                }
                Ok(PeerManagementMessage::ListPeers(peers))
            }
            _ => Err(PeerNetError::InvalidMessage),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            PeerManagementMessage::NewPeerConnected((peer_id, listeners)) => {
                let mut bytes = vec![0];
                bytes.extend_from_slice(&peer_id.to_bytes());
                bytes.extend((listeners.len() as u64).to_be_bytes());
                for listener in listeners {
                    let ip_bytes = match listener.0.ip() {
                        IpAddr::V4(ip) => {
                            bytes.push(4);
                            ip.octets().to_vec()
                        }
                        IpAddr::V6(ip) => {
                            bytes.push(6);
                            ip.octets().to_vec()
                        }
                    };
                    bytes.extend_from_slice(&ip_bytes);
                    let port_bytes = listener.0.port().to_be_bytes();
                    bytes.extend_from_slice(&port_bytes);
                    bytes.push(*listener.1 as u8);
                }
                bytes
            }
            PeerManagementMessage::ListPeers(peers) => {
                let mut bytes = vec![1];
                let nb_peers = peers.len() as u64;
                bytes.extend_from_slice(&nb_peers.to_le_bytes());
                for (peer_id, listeners) in peers {
                    bytes.extend_from_slice(&peer_id.to_bytes());
                    bytes.extend(listeners.len().to_be_bytes());
                    for listener in listeners {
                        let ip_bytes = match listener.0.ip() {
                            IpAddr::V4(ip) => {
                                bytes.push(4);
                                ip.octets().to_vec()
                            }
                            IpAddr::V6(ip) => {
                                bytes.push(6);
                                ip.octets().to_vec()
                            }
                        };
                        bytes.extend_from_slice(&ip_bytes);
                        let port_bytes = listener.0.port().to_be_bytes();
                        bytes.extend_from_slice(&port_bytes);
                        bytes.push(*listener.1 as u8);
                    }
                }
                bytes
            }
        }
    }
}

pub struct PeerInfo {
    pub last_announce: Announcement,
}

impl PeerManagementHandler {
    pub fn new(initial_peers: InitialPeers) -> (Self, MessageHandler) {
        let (sender, receiver) = crossbeam::channel::unbounded();
        for (peer_id, listeners) in &initial_peers {
            println!("Sending initial peer: {:?}", peer_id);
            sender
                .send((
                    peer_id.clone(),
                    PeerManagementMessage::NewPeerConnected((peer_id.clone(), listeners.clone()))
                        .to_bytes(),
                ))
                .unwrap();
        }
        let peer_db: SharedPeerDB = Arc::new(RwLock::new(Default::default()));
        let thread_join = std::thread::spawn({
            let peer_db = peer_db.clone();
            move || {
                loop {
                    let (peer_id, message) = receiver.recv().unwrap();
                    println!("Received message len: {}", message.len());
                    let message = PeerManagementMessage::from_bytes(&message).unwrap();
                    println!("Received message from peer: {:?}", peer_id);
                    println!("Message: {:?}", message);
                    // TODO: Bufferize launch of test thread
                    // TODO: Add wait group or something like that to wait for all threads to finish when stop
                    match message {
                        PeerManagementMessage::NewPeerConnected((_peer_id, listeners)) => {
                            for listener in listeners.into_iter() {
                                let _tester = Tester::new(peer_db.clone(), listener.clone());
                            }
                        }
                        PeerManagementMessage::ListPeers(peers) => {
                            for (_peer_id, listeners) in peers.into_iter() {
                                for listener in listeners.into_iter() {
                                    let _tester = Tester::new(peer_db.clone(), listener.clone());
                                }
                            }
                        }
                    }
                }
            }
        });
        (
            Self {
                peer_db,
                thread_join: Some(thread_join),
            },
            MessageHandler::new(sender),
        )
    }
}

#[derive(Clone)]
pub struct MassaHandshake {}

impl HandshakeHandler for MassaHandshake {
    fn perform_handshake(
        &mut self,
        keypair: &KeyPair,
        endpoint: &mut Endpoint,
        listeners: &HashMap<SocketAddr, TransportType>,
        message_handlers: &MessageHandlers,
    ) -> Result<PeerId, PeerNetError> {
        let mut bytes = PeerId::from_public_key(keypair.get_public_key()).to_bytes();
        //TODO: Add version in announce
        let listeners_announcement = Announcement::new(listeners.clone(), keypair).unwrap();
        bytes.extend_from_slice(&listeners_announcement.to_bytes());
        endpoint.send(&bytes)?;

        let received = endpoint.receive()?;
        if received.is_empty() {
            return Err(PeerNetError::InvalidMessage);
        }
        let mut offset = 0;
        //TODO: We use this to verify the signature before sending it to the handler.
        //This will be done also in the handler but as we are in the handshake we want to do it to invalid the handshake in case it fails.
        let peer_id = PeerId::from_bytes(&received[offset..offset + 32].try_into().unwrap())?;
        offset += 32;
        let announcement = Announcement::from_bytes(&received[offset..], &peer_id)?;
        //TODO: Verify that the handler is defined
        message_handlers
            .get_handler(0)
            .unwrap()
            .send_message((
                peer_id.clone(),
                PeerManagementMessage::NewPeerConnected((
                    peer_id.clone(),
                    announcement.listeners.clone(),
                ))
                .to_bytes(),
            ))
            .unwrap();

        let mut self_random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut self_random_bytes);
        let self_random_hash = Hash::compute_from(&self_random_bytes);
        let mut bytes = [0u8; 32];
        bytes[..32].copy_from_slice(&self_random_bytes);

        endpoint.send(&bytes)?;
        let received = endpoint.receive()?;
        let other_random_bytes: &[u8; 32] = received.as_slice()[..32].try_into().unwrap();

        // sign their random bytes
        let other_random_hash = Hash::compute_from(other_random_bytes);
        let self_signature = keypair.sign(&other_random_hash).unwrap();

        let mut bytes = [0u8; 64];
        bytes.copy_from_slice(&self_signature.to_bytes());

        endpoint.send(&bytes)?;
        let received = endpoint.receive()?;

        let other_signature =
            Signature::from_bytes(received.as_slice().try_into().unwrap()).unwrap();

        // check their signature
        peer_id.verify_signature(&self_random_hash, &other_signature)?;

        println!("Handshake finished");
        Ok(peer_id)
    }
}

pub fn fallback_function(
    keypair: &KeyPair,
    endpoint: &mut Endpoint,
    listeners: &HashMap<SocketAddr, TransportType>,
    _message_handlers: &MessageHandlers,
) -> Result<(), PeerNetError> {
    //TODO: Fix this clone
    let keypair = keypair.clone();
    let mut endpoint = endpoint.clone();
    let listeners = listeners.clone();
    std::thread::spawn(move || {
        let mut bytes = PeerId::from_public_key(keypair.get_public_key()).to_bytes();
        //TODO: Add version in announce
        let listeners_announcement = Announcement::new(listeners.clone(), &keypair).unwrap();
        bytes.extend_from_slice(&listeners_announcement.to_bytes());
        endpoint.send(&bytes).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        endpoint.shutdown();
    });
    Ok(())
}
