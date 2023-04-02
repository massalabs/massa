use crossbeam::channel::Sender;
use massa_serialization::{DeserializeError, Deserializer, U64VarIntDeserializer};
use peernet::{
    error::{PeerNetError, PeerNetResult},
    messages::MessagesHandler as PeerNetMessagesHandler,
    peer_id::PeerId,
};

#[derive(Clone)]
pub struct MessagesHandler {
    pub sender_blocks: Sender<(PeerId, u64, Vec<u8>)>,
    pub sender_endorsements: Sender<(PeerId, u64, Vec<u8>)>,
    pub sender_operations: Sender<(PeerId, u64, Vec<u8>)>,
    pub sender_peers: Sender<(PeerId, u64, Vec<u8>)>,
    pub id_deserializer: U64VarIntDeserializer,
}

impl PeerNetMessagesHandler for MessagesHandler {
    fn deserialize_and_handle(&self, data: &[u8], peer_id: &PeerId) -> PeerNetResult<()> {
        if data.is_empty() {
            return Err(PeerNetError::ReceiveError.error(
                "MessagesHandler",
                Some("Empty message received".to_string()),
            ));
        }
        let (rest, id) = self
            .id_deserializer
            .deserialize::<DeserializeError>(data)
            .map_err(|err| {
                PeerNetError::HandlerError.error(
                    "MessagesHandler",
                    Some(format!("Failed to deserialize message id: {}", err)),
                )
            })?;
        match id {
            0..=2 => self
                .sender_blocks
                .send((peer_id.clone(), id, rest.to_vec()))
                .map_err(|err| {
                    PeerNetError::HandlerError.error(
                        "MessagesHandler",
                        Some(format!("Failed to send block message to channel: {}", err)),
                    )
                }),
            3 => self
                .sender_endorsements
                .send((peer_id.clone(), id - 3, rest.to_vec()))
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
                .send((peer_id.clone(), id - 4, rest.to_vec()))
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
                .send((peer_id.clone(), id - 7, rest.to_vec()))
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
