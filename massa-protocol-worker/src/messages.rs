use massa_metrics::channels::MassaSender;
use massa_protocol_exports::PeerId;
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use peernet::{
    error::{PeerNetError, PeerNetResult},
    messages::{
        MessagesHandler as PeerNetMessagesHandler, MessagesSerializer as PeerNetMessagesSerializer,
    },
};

use crate::handlers::{
    block_handler::{BlockMessage, BlockMessageSerializer},
    endorsement_handler::{EndorsementMessage, EndorsementMessageSerializer},
    operation_handler::{OperationMessage, OperationMessageSerializer},
    peer_handler::{
        models::PeerMessageTuple, PeerManagementMessage, PeerManagementMessageSerializer,
    },
};

#[derive(Debug)]
pub enum Message {
    Block(Box<BlockMessage>),
    Endorsement(EndorsementMessage),
    Operation(OperationMessage),
    PeerManagement(Box<PeerManagementMessage>),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum MessageTypeId {
    Block = 0,
    Endorsement = 1,
    Operation = 2,
    PeerManagement = 3,
}

impl From<&Message> for MessageTypeId {
    fn from(value: &Message) -> Self {
        match value {
            Message::Block(_) => MessageTypeId::Block,
            Message::Endorsement(_) => MessageTypeId::Endorsement,
            Message::Operation(_) => MessageTypeId::Operation,
            Message::PeerManagement(_) => MessageTypeId::PeerManagement,
        }
    }
}

//TODO: Macroize this
impl From<BlockMessage> for Message {
    fn from(message: BlockMessage) -> Self {
        Self::Block(Box::from(message))
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
        Self::PeerManagement(Box::from(message))
    }
}

#[derive(Clone)]
pub struct MessagesSerializer {
    id_serializer: U64VarIntSerializer,
    block_message_serializer: Option<BlockMessageSerializer>,
    operation_message_serializer: Option<OperationMessageSerializer>,
    endorsement_message_serializer: Option<EndorsementMessageSerializer>,
    peer_management_message_serializer: Option<PeerManagementMessageSerializer>,
}

impl Default for MessagesSerializer {
    fn default() -> Self {
        Self::new()
    }
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
    /// Serialize the message
    fn serialize(&self, message: &Message, buffer: &mut Vec<u8>) -> PeerNetResult<()> {
        self.id_serializer
            .serialize(
                &MessageTypeId::from(message).try_into().map_err(|_| {
                    PeerNetError::HandlerError.error(
                        "MessagesSerializer",
                        Some(String::from("Failed to serialize id")),
                    )
                })?,
                buffer,
            )
            .map_err(|err| {
                PeerNetError::HandlerError.error(
                    "MessagesHandler",
                    Some(format!("Failed to serialize id {}", err)),
                )
            })?;
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
    pub id_deserializer: U64VarIntDeserializer,
    pub sender_blocks: MassaSender<PeerMessageTuple>,
    pub sender_endorsements: MassaSender<PeerMessageTuple>,
    pub sender_operations: MassaSender<PeerMessageTuple>,
    pub sender_peers: MassaSender<PeerMessageTuple>,
}

impl PeerNetMessagesHandler<PeerId> for MessagesHandler {
    fn handle(&self, data: &[u8], peer_id: &PeerId) -> PeerNetResult<()> {
        let (data, raw_id) = self
            .id_deserializer
            .deserialize::<DeserializeError>(data)
            .map_err(|err| {
                PeerNetError::HandlerError.error(
                    "MessagesHandler",
                    Some(format!("Failed to deserialize id: {}", err)),
                )
            })?;
        let id = MessageTypeId::try_from(raw_id).map_err(|_| {
            PeerNetError::HandlerError.error(
                "MessagesHandler",
                Some(String::from("Failed to deserialize id")),
            )
        })?;
        match id {
            MessageTypeId::Block => self
                .sender_blocks
                .send((peer_id.clone(), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                }),
            MessageTypeId::Endorsement => self
                .sender_endorsements
                .try_send((peer_id.clone(), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                }),
            MessageTypeId::Operation => self
                .sender_operations
                .try_send((peer_id.clone(), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                }),
            MessageTypeId::PeerManagement => self
                .sender_peers
                .try_send((peer_id.clone(), data.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                }),
        }
    }
}
