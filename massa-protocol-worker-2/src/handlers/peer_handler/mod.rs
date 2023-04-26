use std::{collections::HashMap, net::SocketAddr, thread::JoinHandle, time::Duration};

use crossbeam::channel::tick;
use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use massa_protocol_exports_2::ProtocolConfig;
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use rand::{rngs::StdRng, RngCore, SeedableRng};

use peernet::messages::MessagesSerializer;
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
use crate::wrap_network::ActiveConnectionsTrait;

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

use crate::messages::Message;
pub(crate) use messages::{PeerManagementMessage, PeerManagementMessageSerializer};

pub struct PeerManagementHandler {
    pub peer_db: SharedPeerDB,
    pub thread_join: Option<JoinHandle<()>>,
    pub sender: PeerManagementChannel,
    testers: Vec<Tester>,
}

impl PeerManagementHandler {
    pub fn new(
        initial_peers: InitialPeers,
        peer_db: SharedPeerDB,
        (sender_msg, receiver_msg): (Sender<PeerMessageTuple>, Receiver<PeerMessageTuple>),
        active_connections: Box<dyn ActiveConnectionsTrait>,
        config: &ProtocolConfig,
    ) -> Self {
        let (sender_cmd, receiver_cmd): (Sender<PeerManagementCmd>, Receiver<PeerManagementCmd>) =
            crossbeam::channel::unbounded();
        let message_serializer = PeerManagementMessageSerializer::new();

        let (test_sender, testers) = Tester::run(config, peer_db.clone());

        let thread_join = std::thread::spawn({
            let peer_db = peer_db.clone();
            let ticker = tick(Duration::from_secs(10));

            let message_serializer = crate::messages::MessagesSerializer::new()
                .with_peer_management_message_serializer(PeerManagementMessageSerializer::new());
            let mut message_deserializer =
                PeerManagementMessageDeserializer::new(PeerManagementMessageDeserializerArgs {
                    //TODO: Real config values
                    max_peers_per_announcement: 1000,
                    max_listeners_per_peer: 100,
                });
            move || {
                loop {
                    select! {
                        recv(ticker) -> _ => {
                            let peers_to_send = peer_db.read().get_rand_peers_to_send(100);
                            if peers_to_send.is_empty() {
                                continue;
                            }

                            let msg = PeerManagementMessage::ListPeers(peers_to_send);

                            for peer_id in &active_connections.get_peer_ids_connected() {
                               if let Err(e) = active_connections
                                    .send_to_peer(peer_id, &message_serializer, msg.clone().into(), false) {
                                    error!("error sending ListPeers message to peer: {:?}", e);
                               }
                            }
                        }
                        recv(receiver_cmd) -> cmd => {
                            // internal command
                           match cmd {
                             Ok(PeerManagementCmd::Ban(peer_id)) => {

                                // remove running handshake ?

                                // close peer connection ?

                                // update peer_db
                                peer_db.write().ban_peer(&peer_id);
                             },
                             Ok(PeerManagementCmd::Stop) => {
                                return;
                             },
                            Err(e) => {
                                error!("error receiving command: {:?}", e);
                            }
                           }
                        },
                        recv(receiver_msg) -> msg => {
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

                            message_deserializer.set_message(message_id);
                            let (_, message) = message_deserializer
                                .deserialize::<DeserializeError>(&message)
                                .unwrap();
                            // TODO: Bufferize launch of test thread
                            // TODO: Add wait group or something like that to wait for all threads to finish when stop
                            match message {
                                PeerManagementMessage::NewPeerConnected((peer_id, listeners)) => {
                                   if let Err(e) = test_sender.send((peer_id, listeners)) {
                                        error!("error when sending msg to peer tester : {}", e);
                                   }
                                }
                                PeerManagementMessage::ListPeers(peers) => {
                                    for (peer_id, listeners) in peers.into_iter() {
                                        if let Err(e) = test_sender.send((peer_id, listeners)) {
                                            error!("error when sending msg to peer tester : {}", e);
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
            let mut message = Vec::new();
            message_serializer
                .serialize(
                    &PeerManagementMessage::NewPeerConnected((peer_id.clone(), listeners.clone())),
                    &mut message,
                )
                .unwrap();
            sender_msg.send((peer_id.clone(), 0, message)).unwrap();
        }

        Self {
            peer_db,
            thread_join: Some(thread_join),
            sender: PeerManagementChannel {
                msg_sender: sender_msg,
                command_sender: sender_cmd,
            },
            testers,
        }
    }

    pub fn stop(&mut self) {
        self.sender
            .command_sender
            .send(PeerManagementCmd::Stop)
            .unwrap();

        // waiting for all threads to finish
        self.testers.iter_mut().for_each(|tester| {
            if let Some(join_handle) = tester.handler.take() {
                join_handle.join().expect("Failed to join tester thread");
            }
        });
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

        // Send 100 peers to the other peer
        let peers_to_send = peer_db_write.get_rand_peers_to_send(100);

        let message_serializer = crate::messages::MessagesSerializer::new()
            .with_peer_management_message_serializer(PeerManagementMessageSerializer::new());
        let mut buf = Vec::new();
        let msg = PeerManagementMessage::ListPeers(peers_to_send);
        let message = Message::PeerManagement(Box::from(msg));

        message_serializer.serialize(&message, &mut buf)?;
        endpoint.send(buf.as_slice())?;

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
