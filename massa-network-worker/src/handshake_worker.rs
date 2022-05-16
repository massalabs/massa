// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Here are happening handshakes.

use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
};
use futures::future::try_join;
use massa_hash::Hash;
use massa_logging::massa_trace;
use massa_models::node::NodeId;
use massa_models::SerializeCompact;
use massa_models::Version;
use massa_network_exports::{
    throw_handshake_error as throw, ConnectionId, HandshakeErrorType, NetworkError, ReadHalf,
    WriteHalf,
};
use massa_signature::{sign, verify_signature, PrivateKey};
use massa_time::MassaTime;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use tokio::{task::JoinHandle, time::timeout};
use tracing::debug;

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
    /// After `timeout_duration` milliseconds, the handshake attempt is dropped.
    timeout_duration: MassaTime,
    version: Version,
}

impl HandshakeWorker {
    /// Creates a new handshake worker.
    ///
    /// Manage a new connection and perform a normal handshake
    ///
    /// Used for incoming and outgoing connections.
    /// It will spawn a new future with an `HandshakeWorker` from the given `reader`
    /// and `writer` from your current node to the distant `connectionId`
    ///
    /// # Arguments
    /// * `socket_reader`: receives data.
    /// * `socket_writer`: sends data.
    /// * `self_node_id`: our node id.
    /// * `private_key`: our private key.
    /// * `timeout_duration`: after `timeout_duration` milliseconds, the handshake attempt is dropped.
    /// * `connection_id`: Node we are trying to connect for debugging
    /// * `version`: Node version used in handshake initialization (check peers compatibility)
    pub fn spawn(
        socket_reader: ReadHalf,
        socket_writer: WriteHalf,
        self_node_id: NodeId,
        private_key: PrivateKey,
        timeout_duration: MassaTime,
        version: Version,
        connection_id: ConnectionId,
    ) -> JoinHandle<(ConnectionId, HandshakeReturnType)> {
        debug!("starting handshake with connection_id={}", connection_id);
        massa_trace!("network_worker.new_connection", {
            "connection_id": connection_id
        });

        let connection_id_copy = connection_id;
        tokio::spawn(async move {
            (
                connection_id_copy,
                HandshakeWorker {
                    reader: ReadBinder::new(socket_reader),
                    writer: WriteBinder::new(socket_writer),
                    self_node_id,
                    private_key,
                    timeout_duration,
                    version,
                }
                .run()
                .await,
            )
        })
    }

    /// Manages one on going handshake.
    /// Consumes self.
    /// Returns a tuple `(ConnectionId, Result)`.
    /// Creates the binders to communicate with that node.
    async fn run(mut self) -> HandshakeReturnType {
        // generate random bytes
        let mut self_random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut self_random_bytes);
        let self_random_hash = Hash::compute_from(&self_random_bytes);
        // send handshake init future
        let send_init_msg = Message::HandshakeInitiation {
            public_key: self.self_node_id.0,
            random_bytes: self_random_bytes,
            version: self.version,
        };
        let bytes_vec: Vec<u8> = send_init_msg.to_bytes_compact().unwrap();
        let send_init_fut = self.writer.send(&bytes_vec);

        // receive handshake init future
        let recv_init_fut = self.reader.next();

        // join send_init_fut and recv_init_fut with a timeout, and match result
        let (other_node_id, other_random_bytes, other_version) = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_init_fut, recv_init_fut),
        )
        .await
        {
            Err(_) => throw!(HandshakeTimeout),
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => throw!(HandshakeInterruption, "init".into()),
            Ok(Ok((_, Some((_, msg, _))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                    version,
                } => (NodeId(pk), rb, version),
                Message::PeerList(list) => throw!(PeerListReceived, list),
                _ => throw!(HandshakeWrongMessage),
            },
        };

        // check if remote node ID is the same as ours
        if other_node_id == self.self_node_id {
            throw!(HandshakeKey)
        }

        // check if version is compatible with ours
        if !self.version.is_compatible(&other_version) {
            throw!(IncompatibleVersion)
        }

        // sign their random bytes
        let other_random_hash = Hash::compute_from(&other_random_bytes);
        let self_signature = sign(&other_random_hash, &self.private_key)?;

        // send handshake reply future
        let send_reply_msg = Message::HandshakeReply {
            signature: self_signature,
        };
        let bytes_vec: Vec<u8> = send_reply_msg.to_bytes_compact().unwrap();
        let send_reply_fut = self.writer.send(&bytes_vec);

        // receive handshake reply future
        let recv_reply_fut = self.reader.next();

        // join send_reply_fut and recv_reply_fut with a timeout, and match result
        let other_signature = match timeout(
            self.timeout_duration.to_duration(),
            try_join(send_reply_fut, recv_reply_fut),
        )
        .await
        {
            Err(_) => throw!(HandshakeTimeout),
            Ok(Err(e)) => return Err(e),
            Ok(Ok((_, None))) => throw!(HandshakeInterruption, "repl".into()),
            Ok(Ok((_, Some((_, msg, _))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => throw!(HandshakeWrongMessage),
            },
        };

        // check their signature
        verify_signature(&self_random_hash, &other_signature, &other_node_id.0).map_err(
            |_err| NetworkError::HandshakeError(HandshakeErrorType::HandshakeInvalidSignature),
        )?;

        Ok((other_node_id, self.reader, self.writer))
    }
}
