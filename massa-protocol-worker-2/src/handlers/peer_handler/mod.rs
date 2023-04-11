use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::Arc,
    thread::JoinHandle,
    time::Duration,
};

use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use parking_lot::RwLock;
use rand::{rngs::StdRng, RngCore, SeedableRng};

use peernet::{
    error::{PeerNetError, PeerNetResult},
    messages::MessagesHandler as PeerNetMessagesHandler,
    peer::HandshakeHandler,
    peer_id::PeerId,
    transports::{endpoint::Endpoint, TransportType},
    types::Hash,
    types::{KeyPair, Signature},
};

use crate::handlers::peer_handler::{messages::PeerManagementMessage, tester::Tester};

use self::announcement::{
    Announcement, AnnouncementDeserializer, AnnouncementDeserializerArgs, AnnouncementSerializer,
};

/// This file contains the definition of the peer management handler
/// This handler is here to check that announcements we receive are valid and
/// that all the endpoints we received are active.
mod announcement;
mod messages;
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

pub struct PeerInfo {
    pub last_announce: Announcement,
}

#[warn(dead_code)]
pub enum PeerManagementCmd {
    BAN(PeerId),
}

impl PeerManagementHandler {
    pub fn new(
        initial_peers: InitialPeers,
    ) -> (
        Self,
        Sender<(PeerId, u64, Vec<u8>)>,
        Sender<PeerManagementCmd>,
    ) {
        let (sender, receiver) = crossbeam::channel::unbounded();
        let (sender_cmd, receiver_cmd): (Sender<PeerManagementCmd>, Receiver<PeerManagementCmd>) =
            crossbeam::channel::unbounded();
        for (peer_id, listeners) in &initial_peers {
            println!("Sending initial peer: {:?}", peer_id);
            sender
                .send((
                    peer_id.clone(),
                    0,
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
                    select! {
                        // internal command
                        recv(receiver_cmd) -> cmd => {
                           match cmd {
                             Ok(PeerManagementCmd::BAN(peer_id)) => {
                                // TODO ban peer
                                 println!("Banning peer: {:?}", peer_id);
                             },
                            Err(_) => {
                                    println!("Error");
                            }
                           }
                        },
                        recv(receiver) -> msg => {
                            let (peer_id, message_id, message) = msg.unwrap();
                            println!("Received message len: {}", message.len());
                            let message = PeerManagementMessage::from_bytes(message_id, &message).unwrap();
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
                }
            }
        });
        (
            Self {
                peer_db,
                thread_join: Some(thread_join),
            },
            sender,
            sender_cmd,
        )
    }
}

#[derive(Clone)]
pub struct MassaHandshake {
    pub announcement_serializer: AnnouncementSerializer,
    pub announcement_deserializer: AnnouncementDeserializer,
}

impl MassaHandshake {
    pub fn new() -> Self {
        Self {
            announcement_serializer: AnnouncementSerializer::new(),
            announcement_deserializer: AnnouncementDeserializer::new(
                AnnouncementDeserializerArgs {
                    //TODO: Config
                    max_listeners: 1000,
                },
            ),
        }
    }
}

impl HandshakeHandler for MassaHandshake {
    fn perform_handshake<MassaMessagesHandler: PeerNetMessagesHandler>(
        &mut self,
        keypair: &KeyPair,
        endpoint: &mut Endpoint,
        listeners: &HashMap<SocketAddr, TransportType>,
        messages_handler: MassaMessagesHandler,
    ) -> PeerNetResult<PeerId> {
        let mut bytes = PeerId::from_public_key(keypair.get_public_key()).to_bytes();
        //TODO: Add version in announce
        let listeners_announcement = Announcement::new(listeners.clone(), keypair).unwrap();
        self.announcement_serializer
            .serialize(&listeners_announcement, &mut bytes)
            .map_err(|err| {
                PeerNetError::HandshakeError.error(
                    "Massa Handshake",
                    Some(format!("Failed to serialize announcement: {}", err)),
                )
            })?;
        endpoint.send(&bytes)?;

        let received = endpoint.receive()?;
        if received.is_empty() {
            return Err(PeerNetError::HandshakeError.error(
                "Massa Handshake",
                Some(String::from("Received empty message")),
            ));
        }
        let mut offset = 0;
        //TODO: We use this to verify the signature before sending it to the handler.
        //This will be done also in the handler but as we are in the handshake we want to do it to invalid the handshake in case it fails.
        let peer_id = PeerId::from_bytes(&received[offset..offset + 32].try_into().unwrap())?;
        offset += 32;
        let (_, announcement) = self
            .announcement_deserializer
            .deserialize::<DeserializeError>(&received[offset..])
            .map_err(|err| {
                PeerNetError::HandshakeError.error(
                    "Massa Handshake",
                    Some(format!("Failed to serialize announcement: {}", err)),
                )
            })?;
        if peer_id
            .verify_signature(&announcement.hash, &announcement.signature)
            .is_err()
        {
            return Err(PeerNetError::HandshakeError
                .error("Massa Handshake", Some("Invalid signature".to_string())));
        }
        //TODO: Avoid re-serializing
        let peer_message =
            PeerManagementMessage::NewPeerConnected((peer_id.clone(), announcement.listeners));
        let peer_message_serialized = peer_message.to_bytes();
        let (rest, id) = messages_handler.deserialize_id(&peer_message_serialized, &peer_id)?;
        messages_handler.handle(id, rest, &peer_id)?;

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
) -> PeerNetResult<()> {
    //TODO: Fix this clone
    let keypair = keypair.clone();
    let mut endpoint = endpoint.clone();
    let listeners = listeners.clone();
    std::thread::spawn(move || {
        let mut bytes = PeerId::from_public_key(keypair.get_public_key()).to_bytes();
        //TODO: Add version in announce
        let listeners_announcement = Announcement::new(listeners.clone(), &keypair).unwrap();
        let announcement_serializer = AnnouncementSerializer::new();
        announcement_serializer
            .serialize(&listeners_announcement, &mut bytes)
            .unwrap();
        endpoint.send(&bytes).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        endpoint.shutdown();
    });
    Ok(())
}
