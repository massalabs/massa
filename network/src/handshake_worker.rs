// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! Here are happening handshakes.
use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
};
use crate::{
    error::{HandshakeErrorType, NetworkError},
    ReadHalf, WriteHalf,
};
use futures::future::try_join;
use massa_hash::hash::Hash;
use models::node::NodeId;
use models::Version;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use signature::{sign, verify_signature, PrivateKey};
use time::UTime;
use tokio::time::timeout;

/// Type alias for more readability
pub type HandshakeReturnType = Result<(NodeId, ReadBinder, WriteBinder), NetworkError>;

/// Manages handshakes.
pub struct HandshakeWorker {
    /// Listens incoming data.
    reader: ReadBinder,
    /// Sends out data.
    writer: WriteBinder,
    /// Our node id.
    self_node_id: NodeId,
    /// Our private key.
    private_key: PrivateKey,
    /// After timeout_duration millis, the handshake attempt is dropped.
    timeout_duration: UTime,
    version: Version,
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
        socket_reader: ReadHalf,
        socket_writer: WriteHalf,
        self_node_id: NodeId,
        private_key: PrivateKey,
        timeout_duration: UTime,
        version: Version,
    ) -> HandshakeWorker {
        HandshakeWorker {
            reader: ReadBinder::new(socket_reader),
            writer: WriteBinder::new(socket_writer),
            self_node_id,
            private_key,
            timeout_duration,
            version,
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
        let self_random_hash = Hash::from(&self_random_bytes);
        // send handshake init future
        let send_init_msg = Message::HandshakeInitiation {
            public_key: self.self_node_id.0,
            random_bytes: self_random_bytes,
            version: self.version,
        };
        let send_init_fut = self.writer.send(&send_init_msg);

        // receive handshake init future
        let recv_init_fut = self.reader.next();

        // join send_init_fut and recv_init_fut with a timeout, and match result
        let (other_node_id, other_random_bytes, other_version) = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_init_fut, recv_init_fut),
        )
        .await
        {
            Err(_) => {
                return Err(NetworkError::HandshakeError(
                    HandshakeErrorType::HandshakeTimeoutError,
                ))
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => {
                return Err(NetworkError::HandshakeError(
                    HandshakeErrorType::HandshakeInterruptionError("init".into()),
                ))
            }
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                    version,
                } => (NodeId(pk), rb, version),
                _ => {
                    return Err(NetworkError::HandshakeError(
                        HandshakeErrorType::HandshakeWrongMessageError,
                    ))
                }
            },
        };

        // check if remote node ID is the same as ours
        if other_node_id == self.self_node_id {
            return Err(NetworkError::HandshakeError(
                HandshakeErrorType::HandshakeKeyError,
            ));
        }

        // check if version is compatible with ours
        if !self.version.is_compatible(&other_version) {
            return Err(NetworkError::HandshakeError(
                HandshakeErrorType::IncompatibleVersionError,
            ));
        }

        // sign their random bytes
        let other_random_hash = Hash::from(&other_random_bytes);
        let self_signature = sign(&other_random_hash, &self.private_key)?;

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
                return Err(NetworkError::HandshakeError(
                    HandshakeErrorType::HandshakeTimeoutError,
                ))
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => {
                return Err(NetworkError::HandshakeError(
                    HandshakeErrorType::HandshakeInterruptionError("repl".into()),
                ))
            }
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => {
                    return Err(NetworkError::HandshakeError(
                        HandshakeErrorType::HandshakeWrongMessageError,
                    ))
                }
            },
        };

        // check their signature
        verify_signature(&self_random_hash, &other_signature, &other_node_id.0).map_err(
            |_err| NetworkError::HandshakeError(HandshakeErrorType::HandshakeInvalidSignatureError),
        )?;

        Ok((other_node_id, self.reader, self.writer))
    }
}
