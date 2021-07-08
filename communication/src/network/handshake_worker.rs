//! Here are happening hanshakes.
use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
};
use crate::{
    common::NodeId,
    error::{CommunicationError, HandshakeErrorType},
    network::{ReadHalf, WriteHalf},
};
use crypto::{
    hash::Hash,
    signature::{PrivateKey, SignatureEngine},
};
use futures::future::try_join;
use models::SerializationContext;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use time::UTime;
use tokio::time::timeout;

/// Type alias for more readability
pub type HandshakeReturnType = Result<(NodeId, ReadBinder, WriteBinder), CommunicationError>;

/// Manages handshakes.
pub struct HandshakeWorker {
    /// Listens incomming data.
    reader: ReadBinder,
    /// Sends out data.
    writer: WriteBinder,
    /// Our node id.
    self_node_id: NodeId,
    /// Our private key.
    private_key: PrivateKey,
    /// After timeout_duration millis, the handshake attempt is dropped.
    timeout_duration: UTime,
}

impl HandshakeWorker {
    /// Creates a new handshake worker.
    ///
    /// # Arguments
    /// * socket_reader: receives data.
    /// * socket_writer: sends data.
    /// * self_node_id: our node id.
    /// * private_key : our private key.
    /// * timeout_duration: after timeout_duration millis, the handshake attempt is dropped.
    pub fn new(
        serialization_context: SerializationContext,
        socket_reader: ReadHalf,
        socket_writer: WriteHalf,
        self_node_id: NodeId,
        private_key: PrivateKey,
        timeout_duration: UTime,
    ) -> HandshakeWorker {
        HandshakeWorker {
            reader: ReadBinder::new(socket_reader, serialization_context.clone()),
            writer: WriteBinder::new(socket_writer, serialization_context),
            self_node_id,
            private_key,
            timeout_duration,
        }
    }

    /// Manages one on going handshake.
    /// Consumes self.
    /// Returns a tuple (ConnectionId, Result).
    /// Creates the binders to communicate with that node.
    pub async fn run(mut self) -> HandshakeReturnType {
        // generate random bytes
        let mut self_random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut self_random_bytes);
        let self_random_hash = Hash::hash(&self_random_bytes);
        // send handshake init future
        let send_init_msg = Message::HandshakeInitiation {
            public_key: self.self_node_id.0,
            random_bytes: self_random_bytes.clone(),
        };
        let send_init_fut = self.writer.send(&send_init_msg);

        // receive handshake init future
        let recv_init_fut = self.reader.next();

        // join send_init_fut and recv_init_fut with a timeout, and match result
        let (other_node_id, other_random_bytes) = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_init_fut, recv_init_fut),
        )
        .await
        {
            Err(_) => {
                return Err(CommunicationError::HandshakeError(
                    HandshakeErrorType::HandshakeTimeoutError,
                ))
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => {
                return Err(CommunicationError::HandshakeError(
                    HandshakeErrorType::HandshakeInterruptionError("init".into()),
                ))
            }
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                } => (NodeId(pk), rb),
                _ => {
                    return Err(CommunicationError::HandshakeError(
                        HandshakeErrorType::HandshakeWrongMessageError,
                    ))
                }
            },
        };

        // check if remote node ID is the same as ours
        if other_node_id == self.self_node_id {
            return Err(CommunicationError::HandshakeError(
                HandshakeErrorType::HandshakeKeyError,
            ));
        }

        // sign their random bytes
        let signature_engine = SignatureEngine::new();
        let other_random_hash = Hash::hash(&other_random_bytes);
        let self_signature = signature_engine.sign(&other_random_hash, &self.private_key)?;

        // send handshake reply future
        let send_reply_msg = Message::HandshakeReply {
            signature: self_signature,
        };
        let send_reply_fut = self.writer.send(&send_reply_msg);

        // receive handshake reply future
        let recv_reply_fut = self.reader.next();

        // join send_reply_fut and recv_reply_fut with a timeout, and match result
        let other_signature = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_reply_fut, recv_reply_fut),
        )
        .await
        {
            Err(_) => {
                return Err(CommunicationError::HandshakeError(
                    HandshakeErrorType::HandshakeTimeoutError,
                ))
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => {
                return Err(CommunicationError::HandshakeError(
                    HandshakeErrorType::HandshakeInterruptionError("repl".into()),
                ))
            }
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => {
                    return Err(CommunicationError::HandshakeError(
                        HandshakeErrorType::HandshakeWrongMessageError,
                    ))
                }
            },
        };

        // check their signature
        signature_engine
            .verify(&self_random_hash, &other_signature, &other_node_id.0)
            .map_err(|_err| {
                CommunicationError::HandshakeError(
                    HandshakeErrorType::HandshakeInvalidSignatureError,
                )
            })?;

        Ok((other_node_id, self.reader, self.writer))
    }
}
