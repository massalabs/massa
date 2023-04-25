use crate::{
    handlers::{
        block_handler::{
            BlockMessage, BlockMessageDeserializer, BlockMessageDeserializerArgs,
            BlockMessageSerializer,
        },
        endorsement_handler::EndorsementMessage,
        operation_handler::OperationMessage,
        peer_handler::{fallback_function, models::PeerDB, MassaHandshake, PeerManagementMessage},
    },
    messages::Message,
};
use crossbeam::channel::{Receiver, Sender};
use massa_protocol_exports_2::ProtocolConfig;
use massa_serialization::{DeserializeError, Deserializer, U64VarIntDeserializer};
use parking_lot::RwLock;
use peernet::{
    config::PeerNetConfiguration, error::PeerNetError, messages::MessagesHandler,
    network_manager::PeerNetManager, transports::TcpOutConnectionConfig, types::KeyPair,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};

/// This file create a mocked peer that can be used to test the protocol

pub struct MockPeer {
    pub stream_message_received: Receiver<Message>,
}

impl MockPeer {
    pub fn new(keypair: KeyPair, remote_addr: SocketAddr) -> Self {
        let (sender_message, stream_message_received) = crossbeam::channel::unbounded();
        let mut protocol_config = ProtocolConfig::default();
        protocol_config.keypair = keypair.clone();
        protocol_config.max_in_connections = 0;
        let peer_db = Arc::new(RwLock::new(PeerDB::default()));
        let mut peernet_config = PeerNetConfiguration::default(
            MassaHandshake::new(peer_db),
            MockMessageHandler {
                sender: sender_message,
                id_deserializer: U64VarIntDeserializer::new(
                    std::ops::Bound::Included(0),
                    std::ops::Bound::Included(u64::MAX),
                ),
            },
        );
        peernet_config.self_keypair = protocol_config.keypair.clone();
        peernet_config.fallback_function = Some(&fallback_function);
        peernet_config.max_in_connections = 0;
        peernet_config.max_out_connections = 10;
        let mut manager = PeerNetManager::new(peernet_config);
        manager.try_connect(
            remote_addr,
            Duration::from_secs(1),
            &peernet::transports::OutConnectionConfig::Tcp(Box::new(TcpOutConnectionConfig {})),
        );
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(manager.active_connections.read().connections.len(), 1);
        Self {
            stream_message_received,
        }
    }
}

#[derive(Clone)]
pub struct MockMessageHandler {
    pub sender: Sender<Message>,
    pub id_deserializer: U64VarIntDeserializer,
}

impl MessagesHandler for MockMessageHandler {
    fn deserialize_id<'a>(
        &self,
        data: &'a [u8],
        peer_id: &peernet::peer_id::PeerId,
    ) -> peernet::error::PeerNetResult<(&'a [u8], u64)> {
        if data.is_empty() {
            return Err(PeerNetError::ReceiveError.error(
                "MessagesHandler",
                Some("Empty message received".to_string()),
            ));
        }
        self.id_deserializer
            .deserialize::<DeserializeError>(data)
            .map_err(|err| {
                PeerNetError::HandlerError.error(
                    "MessagesHandler",
                    Some(format!("Failed to deserialize message id: {}", err)),
                )
            })
    }

    fn handle(
        &self,
        id: u64,
        data: &[u8],
        peer_id: &peernet::peer_id::PeerId,
    ) -> peernet::error::PeerNetResult<()> {
        let block_max_id = BlockMessage::max_id();
        let endorsement_max_id = EndorsementMessage::max_id();
        let operation_max_id = OperationMessage::max_id();
        let peer_management_max_id = PeerManagementMessage::max_id();
        if id < block_max_id {
            let mut block_message_deserializer =
                BlockMessageDeserializer::new(BlockMessageDeserializerArgs {
                    thread_count: 32,
                    endorsement_count: 10000,
                    block_infos_length_max: 10000,
                    max_operations_per_block: 10000,
                    max_datastore_value_length: 10000,
                    max_function_name_length: 10000,
                    max_parameters_size: 10000,
                    max_op_datastore_entry_count: 10000,
                    max_op_datastore_key_length: 100,
                    max_op_datastore_value_length: 10000,
                });
            block_message_deserializer.set_message_id(id);
            block_message_deserializer
                .deserialize::<DeserializeError>(data)
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to deserialize block message: {}", err)),
                    )
                })
                .and_then(|(_, block_message)| {
                    self.sender
                        .send(Message::Block(Box::new(block_message)))
                        .map_err(|err| {
                            PeerNetError::HandlerError.error(
                                "MessagesHandler",
                                Some(format!("Failed to send block message to channel: {}", err)),
                            )
                        })
                })
        } else if id < endorsement_max_id + block_max_id {
            // self.sender_endorsements
            //     .send((peer_id.clone(), id - block_max_id, data.to_vec()))
            //     .map_err(|err| {
            //         PeerNetError::HandlerError.error(
            //             "MessagesHandler",
            //             Some(format!(
            //                 "Failed to send endorsement message to channel: {}",
            //                 err
            //             )),
            //         )
            //     })
            Ok(())
        } else if id < operation_max_id + block_max_id + endorsement_max_id {
            // self.sender_operations
            //     .send((
            //         peer_id.clone(),
            //         id - (block_max_id + endorsement_max_id),
            //         data.to_vec(),
            //     ))
            //     .map_err(|err| {
            //         PeerNetError::HandlerError.error(
            //             "MessagesHandler",
            //             Some(format!(
            //                 "Failed to send operation message to channel: {}",
            //                 err
            //             )),
            //         )
            //     })
            Ok(())
        } else if id < peer_management_max_id + block_max_id + endorsement_max_id + operation_max_id
        {
            // self.sender_peers
            //     .send((
            //         peer_id.clone(),
            //         id - (block_max_id + endorsement_max_id + operation_max_id),
            //         data.to_vec(),
            //     ))
            //     .map_err(|err| {
            //         PeerNetError::HandlerError.error(
            //             "MessagesHandler",
            //             Some(format!(
            //                 "Failed to send peer management message to channel: {}",
            //                 err
            //             )),
            //         )
            //     })
            Ok(())
        } else {
            Err(PeerNetError::HandlerError.error(
                "MessagesHandler",
                Some(format!("Unknown message id: {}", id)),
            ))
        }
    }
}
