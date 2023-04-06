use crossbeam::channel::Sender;
use massa_serialization::{DeserializeError, Serializer, Deserializer, U64VarIntDeserializer, U64VarIntSerializer};
use num_enum::{TryFromPrimitive, IntoPrimitive};
use peernet::{
    error::{PeerNetError, PeerNetResult},
    messages::{MessagesHandler as PeerNetMessagesHandler, MessagesSerializer as PeerNetMessagesSerializer},
    peer_id::PeerId,
};

use crate::handlers::{peer_handler::{PeerManagementMessage, PeerManagementMessageSerializer}, block_handler::{BlockMessage, BlockMessageSerializer}, endorsement_handler::{EndorsementMessage, EndorsementMessageSerializer}, operation_handler::{OperationMessage, OperationMessageSerializer}};

pub enum Message {
    BlockMessage(BlockMessage),
    EndorsementMessage(EndorsementMessage),
    OperationMessage(OperationMessage),
    PeerManagementMessage(PeerManagementMessage),
}

impl From<BlockMessage> for Message {
    fn from(message: BlockMessage) -> Self {
        Self::BlockMessage(message)
    }
}

impl From<EndorsementMessage> for Message {
    fn from(message: EndorsementMessage) -> Self {
        Self::EndorsementMessage(message)
    }
}

impl From<OperationMessage> for Message {
    fn from(message: OperationMessage) -> Self {
        Self::OperationMessage(message)
    }
}

impl From<PeerManagementMessage> for Message {
    fn from(message: PeerManagementMessage) -> Self {
        Self::PeerManagementMessage(message)
    }
}

#[derive(IntoPrimitive, TryFromPrimitive)]
#[repr(u64)]
pub enum MessageCategoryId {
    BlockHandler = 0,
    EndorsementHandler = 3,
    OperationHandler = 4,
    PeerHandler = 7,
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
        match message {
            Message::BlockMessage(message) => {
                self.id_serializer
                    .serialize(
                        &(MessageCategoryId::BlockHandler as u64 + message.get_id() as u64),
                        buffer,
                    )
                    .map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message id: {}", err)),
                        )
                    })
            },
            Message::EndorsementMessage(message) => {
                self.id_serializer
                    .serialize(
                        &(MessageCategoryId::EndorsementHandler as u64 + message.get_id() as u64),
                        buffer,
                    )
                    .map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message id: {}", err)),
                        )
                    })
            },
            Message::OperationMessage(message) => {
                self.id_serializer
                    .serialize(
                        &(MessageCategoryId::OperationHandler as u64 + message.get_id() as u64),
                        buffer,
                    )
                    .map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message id: {}", err)),
                        )
                    })
            },
            Message::PeerManagementMessage(message) => {
                self.id_serializer
                    .serialize(
                        &(MessageCategoryId::PeerHandler as u64 + message.get_id() as u64),
                        buffer,
                    )
                    .map_err(|err| {
                        PeerNetError::HandlerError.error(
                            "MessagesSerializer",
                            Some(format!("Failed to serialize message id: {}", err)),
                        )
                    })
            },
        }
    }
    /// Serialize the message
    fn serialize(&self, message: &Message, buffer: &mut Vec<u8>) -> PeerNetResult<()> {
        match message {
            Message::BlockMessage(message) => {
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
            },
            Message::EndorsementMessage(message) => {
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
            },
            Message::OperationMessage(message) => {
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
            },
            Message::PeerManagementMessage(message) => {
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
            },
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
        match id {
            0..=2 => self
                .sender_blocks
                .send((peer_id.clone(), id - <MessageCategoryId as Into<u64>>::into(MessageCategoryId::BlockHandler.into()), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                }),
            3 => self
                .sender_endorsements
                .send((peer_id.clone(), id - <MessageCategoryId as Into<u64>>::into(MessageCategoryId::EndorsementHandler.into()), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!(
                            "Failed to send endorsement message to channel: {}",
                            err
                        )),
                    )
                }),
            4..=6 => self
                .sender_operations
                .send((peer_id.clone(), id - <MessageCategoryId as Into<u64>>::into(MessageCategoryId::OperationHandler.into()), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!(
                            "Failed to send operation message to channel: {}",
                            err
                        )),
                    )
                }),
            7..=8 => self
                .sender_peers
                .send((peer_id.clone(), id - <MessageCategoryId as Into<u64>>::into(MessageCategoryId::PeerHandler.into()), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send peer message to channel: {}", err)),
                    )
                }),
            _ => Err(PeerNetError::HandlerError.error(
                "MessagesHandler",
                Some(format!("Invalid message id: {}", id)),
            )),
        }
    }
}
