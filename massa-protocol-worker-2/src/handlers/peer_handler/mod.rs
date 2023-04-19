use std::{collections::HashMap, net::SocketAddr, sync::Arc, thread::JoinHandle, time::Duration};

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
use tracing::log::{error, warn};

use crate::handlers::peer_handler::models::PeerState;

use self::{
    models::{
        InitialPeers, PeerManagementChannel, PeerManagementCmd, PeerMessageTuple, SharedPeerDB,
    },
    tester::Tester,
};

use self::{
    announcement::{
        Announcement, AnnouncementDeserializer, AnnouncementDeserializerArgs,
        AnnouncementSerializer,
    },
    messages::{PeerManagementMessageDeserializer, PeerManagementMessageDeserializerArgs},
};

/// This file contains the definition of the peer management handler
/// This handler is here to check that announcements we receive are valid and
/// that all the endpoints we received are active.
mod announcement;
mod messages;
pub mod models;
mod tester;

pub(crate) use messages::{PeerManagementMessage, PeerManagementMessageSerializer};

pub struct PeerManagementHandler {
    pub peer_db: SharedPeerDB,
    pub thread_join: Option<JoinHandle<()>>,
    pub sender: PeerManagementChannel,
}

impl PeerManagementHandler {
    pub fn new(initial_peers: InitialPeers) -> Self {
        let (sender, receiver): (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>) =
            crossbeam::channel::unbounded();
        let (sender_cmd, receiver_cmd): (Sender<PeerManagementCmd>, Receiver<PeerManagementCmd>) =
            crossbeam::channel::unbounded();
        let message_serializer = PeerManagementMessageSerializer::new();

        let peer_db: SharedPeerDB = Arc::new(RwLock::new(Default::default()));
        let thread_join = std::thread::spawn({
            let peer_db = peer_db.clone();
            let mut message_deserializer =
                PeerManagementMessageDeserializer::new(PeerManagementMessageDeserializerArgs {
                    //TODO: Real config values
                    max_peers_per_announcement: 1000,
                    max_listeners_per_peer: 100,
                });
            move || {
                loop {
                    select! {
                        // internal command
                        recv(receiver_cmd) -> cmd => {
                           match cmd {
                             Ok(PeerManagementCmd::Ban(peer_id)) => {

                                // remove runing handshake ?

                                // close peer connection ?

                                // update peer_db
                                peer_db.write().ban_peer(&peer_id);
                             },
                            Err(e) => {
                                error!("error receiving command: {:?}", e);
                            }
                           }
                        },
                        recv(receiver) -> msg => {
                            let (peer_id, message_id, message) = match msg {
                                Ok((peer_id, message_id, message)) => (peer_id, message_id, message),
                                Err(_) => {
                                    return;
                                }
                            };

                            // check if peer is banned
                            if let Some(peer) = peer_db.read().peers.get(&peer_id) {
                                if peer.state == PeerState::Banned {
                                    warn!("Banned peer sent us a message: {:?}", peer_id);
                                    continue;
                                }
                            }

                            println!("Received message len: {}", message.len());
                            message_deserializer.set_message(message_id);
                            let (_, message) = message_deserializer
                                .deserialize::<DeserializeError>(&message)
                                .unwrap();
                            println!("Received message from peer: {:?}", peer_id);
                            println!("Message: {:?}", message);
                            // TODO: Bufferize launch of test thread
                            // TODO: Add wait group or something like that to wait for all threads to finish when stop
                            match message {
                                PeerManagementMessage::NewPeerConnected((_peer_id, listeners)) => {
                                    for listener in listeners.into_iter() {
                                        let _tester = Tester::new(peer_db.clone(), listener);
                                    }
                                }
                                PeerManagementMessage::ListPeers(peers) => {
                                    for (_peer_id, listeners) in peers.into_iter() {
                                        for listener in listeners.into_iter() {
                                            let _tester = Tester::new(peer_db.clone(), listener);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        for (peer_id, listeners) in &initial_peers {
            println!("Sending initial peer: {:?}", peer_id);
            let mut message = Vec::new();
            message_serializer
                .serialize(
                    &PeerManagementMessage::NewPeerConnected((peer_id.clone(), listeners.clone())),
                    &mut message,
                )
                .unwrap();
            sender.send((peer_id.clone(), 0, message)).unwrap();
        }

        Self {
            peer_db,
            thread_join: Some(thread_join),
            sender: PeerManagementChannel {
                msg_sender: sender,
                command_sender: sender_cmd,
            },
        }
    }

    pub fn stop(mut self) {
        drop(self.sender);
        if let Some(handle) = self.thread_join.take() {
            handle.join().expect("Failed to join peer manager thread");
        }
    }
}

#[derive(Clone)]
pub struct MassaHandshake {
    pub announcement_serializer: AnnouncementSerializer,
    pub announcement_deserializer: AnnouncementDeserializer,
    pub peer_db: SharedPeerDB,
}

impl MassaHandshake {
    pub fn new(peer_db: SharedPeerDB) -> Self {
        Self {
            peer_db,
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
        let peer_id = PeerId::from_bytes(&received[offset..offset + 32].try_into().unwrap())?;

        let res = {
            {
                let mut peer_db_write = self.peer_db.write();
                peer_db_write
                    .peers
                    .entry(peer_id.clone())
                    .and_modify(|info| {
                        info.state = PeerState::InHandshake;
                    });
            }

            //TODO: We use this to verify the signature before sending it to the handler.
            //This will be done also in the handler but as we are in the handshake we want to do it to invalid the handshake in case it fails.

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
            let message =
                PeerManagementMessage::NewPeerConnected((peer_id.clone(), announcement.listeners));
            let mut bytes = Vec::new();
            let peer_management_message_serializer = PeerManagementMessageSerializer::new();
            peer_management_message_serializer
                .serialize(&message, &mut bytes)
                .map_err(|err| {
                    PeerNetError::HandshakeError.error(
                        "Massa Handshake",
                        Some(format!("Failed to serialize announcement: {}", err)),
                    )
                })?;
            messages_handler.handle(7, &bytes, &peer_id)?;

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
            Ok(peer_id.clone())
        };

        let mut peer_db_write = self.peer_db.write();
        // if handshake failed, we set the peer state to HandshakeFailed
        if res.is_err() {
            peer_db_write.peers.entry(peer_id).and_modify(|info| {
                info.state = PeerState::HandshakeFailed;
            });
        } else {
            peer_db_write.peers.entry(peer_id).and_modify(|info| {
                info.state = PeerState::Trusted;
            });
        }

        res
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
