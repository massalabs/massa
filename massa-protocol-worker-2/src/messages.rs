use crossbeam::channel::Sender;
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use peernet::{
    error::{PeerNetError, PeerNetResult},
    messages::{
        MessagesHandler as PeerNetMessagesHandler, MessagesSerializer as PeerNetMessagesSerializer,
    },
    peer_id::PeerId,
};

use crate::handlers::{
    block_handler::{BlockMessage, BlockMessageSerializer},
    endorsement_handler::{EndorsementMessage, EndorsementMessageSerializer},
    operation_handler::{OperationMessage, OperationMessageSerializer},
    peer_handler::{PeerManagementMessage, PeerManagementMessageSerializer},
};

pub enum Message {
    Block(BlockMessage),
    Endorsement(EndorsementMessage),
    Operation(OperationMessage),
    PeerManagement(PeerManagementMessage),
}

//TODO: Macroize this
impl From<BlockMessage> for Message {
    fn from(message: BlockMessage) -> Self {
        Self::Block(message)
    }
}

impl From<EndorsementMessage> for Message {
    fn from(message: EndorsementMessage) -> Self {
        Self::Endorsement(message)
    }
}

impl From<OperationMessage> for Message {
    fn from(message: OperationMessage) -> Self {
        Self::Operation(message)
    }
}

impl From<PeerManagementMessage> for Message {
    fn from(message: PeerManagementMessage) -> Self {
        Self::PeerManagement(message)
    }
}

impl Message {
    //TODO: Macroize get_id and max_id
    fn get_id(&self) -> u64 {
        match self {
            Message::Block(message) => message.get_id() as u64,
            Message::Endorsement(message) => {
                message.get_id() as u64 + BlockMessage::max_id()
            }
            Message::Operation(message) => {
                message.get_id() as u64 + BlockMessage::max_id() + EndorsementMessage::max_id()
            }
            Message::PeerManagement(message) => {
                message.get_id() as u64
                    + BlockMessage::max_id()
                    + EndorsementMessage::max_id()
                    + OperationMessage::max_id()
            }
        }
    }
}

pub struct MessagesSerializer {
    id_serializer: U64VarIntSerializer,
    block_message_serializer: Option<BlockMessageSerializer>,
    operation_message_serializer: Option<OperationMessageSerializer>,
    endorsement_message_serializer: Option<EndorsementMessageSerializer>,
    peer_management_message_serializer: Option<PeerManagementMessageSerializer>,
}

impl MessagesSerializer {
    pub fn new() -> Self {
        Self {
            id_serializer: U64VarIntSerializer::new(),
            block_message_serializer: None,
            operation_message_serializer: None,
            endorsement_message_serializer: None,
            peer_management_message_serializer: None,
        }
    }

    pub fn with_block_message_serializer(
        mut self,
        block_message_serializer: BlockMessageSerializer,
    ) -> Self {
        self.block_message_serializer = Some(block_message_serializer);
        self
    }

    pub fn with_operation_message_serializer(
        mut self,
        operation_message_serializer: OperationMessageSerializer,
    ) -> Self {
        self.operation_message_serializer = Some(operation_message_serializer);
        self
    }

    pub fn with_endorsement_message_serializer(
        mut self,
        endorsement_message_serializer: EndorsementMessageSerializer,
    ) -> Self {
        self.endorsement_message_serializer = Some(endorsement_message_serializer);
        self
    }

    pub fn with_peer_management_message_serializer(
        mut self,
        peer_management_message_serializer: PeerManagementMessageSerializer,
    ) -> Self {
        self.peer_management_message_serializer = Some(peer_management_message_serializer);
        self
    }
}

impl PeerNetMessagesSerializer<Message> for MessagesSerializer {
    /// Serialize the id of a message
    fn serialize_id(&self, message: &Message, buffer: &mut Vec<u8>) -> PeerNetResult<()> {
        self.id_serializer
            .serialize(&message.get_id(), buffer)
            .map_err(|err| {
                PeerNetError::HandlerError.error(
                    "MessagesSerializer",
                    Some(format!("Failed to serialize message id: {}", err)),
                )
            })
    }
    /// Serialize the message
    fn serialize(&self, message: &Message, buffer: &mut Vec<u8>) -> PeerNetResult<()> {
        match message {
            Message::Block(message) => {
                if let Some(serializer) = &self.block_message_serializer {
                    serializer.serialize(message, buffer).map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message: {}", err)),
                        )
                    })
                } else {
                    Err(PeerNetError::HandlerError.error(
                        "MessagesSerializer",
                        Some("BlockMessageSerializer not initialized".to_string()),
                    ))
                }
            }
            Message::Endorsement(message) => {
                if let Some(serializer) = &self.endorsement_message_serializer {
                    serializer.serialize(message, buffer).map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message: {}", err)),
                        )
                    })
                } else {
                    Err(PeerNetError::HandlerError.error(
                        "MessagesSerializer",
                        Some("EndorsementMessageSerializer not initialized".to_string()),
                    ))
                }
            }
            Message::Operation(message) => {
                if let Some(serializer) = &self.operation_message_serializer {
                    serializer.serialize(message, buffer).map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message: {}", err)),
                        )
                    })
                } else {
                    Err(PeerNetError::HandlerError.error(
                        "MessagesSerializer",
                        Some("OperationMessageSerializer not initialized".to_string()),
                    ))
                }
            }
            Message::PeerManagement(message) => {
                if let Some(serializer) = &self.peer_management_message_serializer {
                    serializer.serialize(message, buffer).map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message: {}", err)),
                        )
                    })
                } else {
                    Err(PeerNetError::HandlerError.error(
                        "MessagesSerializer",
                        Some("PeerManagementMessageSerializer not initialized".to_string()),
                    ))
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct MessagesHandler {
    pub sender_blocks: Sender<(PeerId, u64, Vec<u8>)>,
    pub sender_endorsements: Sender<(PeerId, u64, Vec<u8>)>,
    pub sender_operations: Sender<(PeerId, u64, Vec<u8>)>,
    pub sender_peers: Sender<(PeerId, u64, Vec<u8>)>,
    pub id_deserializer: U64VarIntDeserializer,
}

impl PeerNetMessagesHandler for MessagesHandler {
    fn deserialize_id<'a>(
        &self,
        data: &'a [u8],
        _peer_id: &PeerId,
    ) -> PeerNetResult<(&'a [u8], u64)> {
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

    fn handle(&self, id: u64, data: &[u8], peer_id: &PeerId) -> PeerNetResult<()> {
        let block_max_id = BlockMessage::max_id();
        let endorsement_max_id = EndorsementMessage::max_id();
        let operation_max_id = OperationMessage::max_id();
        let peer_management_max_id = PeerManagementMessage::max_id();
        if id < block_max_id {
            self.sender_blocks
                .send((peer_id.clone(), id, data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                })
        } else if id < endorsement_max_id {
            self.sender_endorsements
                .send((peer_id.clone(), id - block_max_id, data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!(
                            "Failed to send endorsement message to channel: {}",
                            err
                        )),
                    )
                })
        } else if id < operation_max_id {
            self.sender_operations
                .send((
                    peer_id.clone(),
                    id - (block_max_id + endorsement_max_id),
                    data.to_vec(),
                ))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!(
                            "Failed to send operation message to channel: {}",
                            err
                        )),
                    )
                })
        } else if id < peer_management_max_id {
            self.sender_peers
                .send((
                    peer_id.clone(),
                    id - (block_max_id + endorsement_max_id + operation_max_id),
                    data.to_vec(),
                ))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!(
                            "Failed to send peer management message to channel: {}",
                            err
                        )),
                    )
                })
        } else {
            Err(PeerNetError::HandlerError.error(
                "MessagesHandler",
                Some(format!("Unknown message id: {}", id)),
            ))
        }
    }
}
