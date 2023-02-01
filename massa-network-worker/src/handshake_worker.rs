// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Here are happening handshakes.

use crate::messages::MessageDeserializer;

use super::{
    binders::{ReadBinder, WriteBinder},
    messages::Message,
};
use crossbeam_channel::{bounded, select, Receiver, Sender};
use futures::future::try_join;
use massa_hash::Hash;
use massa_models::{
    config::{
        constants::{MAX_DATASTORE_VALUE_LENGTH, MAX_FUNCTION_NAME_LENGTH, MAX_PARAMETERS_SIZE},
        ENDORSEMENT_COUNT, MAX_ADVERTISE_LENGTH, MAX_ENDORSEMENTS_PER_MESSAGE, MAX_MESSAGE_SIZE,
        MAX_OPERATIONS_PER_BLOCK, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH, THREAD_COUNT,
    },
    version::Version,
};
use massa_models::{
    config::{MAX_ASK_BLOCKS_PER_MESSAGE, MAX_OPERATIONS_PER_MESSAGE},
    node::NodeId,
};
use massa_network_exports::{
    throw_handshake_error as throw, ConnectionId, HandshakeErrorType, NetworkError, ReadHalf,
    WriteHalf,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use std::collections::HashMap;
use std::thread::{self, JoinHandle};
use tokio::runtime::Handle;
use tokio::time::timeout;

/// Type alias for more readability
pub type HandshakeReturnType = Result<(NodeId, ReadBinder, WriteBinder), NetworkError>;

/// Start a thread, that will be responsible for running handshake workers,
/// and sending the results back to the network worker.
pub fn start_handshake_manager(
    worker_rx: Receiver<(ConnectionId, HandshakeWorker)>,
    connection_sender: Sender<(ConnectionId, HandshakeReturnType)>,
    runtime_handle: Handle,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut task_handles = HashMap::new();
        let (result_tx, result_rx) = bounded::<(ConnectionId, HandshakeReturnType)>(1);
        loop {
            select! {
                recv(worker_rx) -> msg => {
                    match msg {
                        Err(_) => break,
                        Ok((connection_id, new_handshake_worker)) => {
                            let result_tx = result_tx.clone();
                            // Spawn an async task to run the handshake.
                            let handle = runtime_handle.spawn(async move {
                                let result = new_handshake_worker.run().await;
                                // Spawn a blocking task to send the result back on the channel.
                                Handle::current()
                                    .spawn_blocking(move || {
                                        result_tx
                                            .send((connection_id, result))
                                            .expect("Failed to send new connection message to network worker.")
                                    })
                                    .await.expect("Failed to spawn blocking call.");
                            });
                            task_handles.insert(connection_id, handle);
                        }
                    }
                },
                recv(result_rx) -> msg => {
                    match msg {
                        Err(_) => break,
                        Ok((connection_id, result)) => {
                            connection_sender
                                .send((connection_id, result))
                                .expect("Failed to send new connection message to network worker.");
                            task_handles.remove(&connection_id);
                        }
                    }
                }
            }
        }
        // Abort all outstanding handshakes.
        for handle in task_handles.values() {
            handle.abort();
        }
    })
}

/// Manages handshakes.
pub struct HandshakeWorker {
    /// Listens incoming data.
    reader: ReadBinder,
    /// Sends out data.
    writer: WriteBinder,
    /// Our node id.
    self_node_id: NodeId,
    /// Our keypair.
    keypair: KeyPair,
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
    /// * `keypair`: our keypair.
    /// * `timeout_duration`: after `timeout_duration` milliseconds, the handshake attempt is dropped.
    /// * `connection_id`: Node we are trying to connect for debugging
    /// * `version`: Node version used in handshake initialization (check peers compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        socket_reader: ReadHalf,
        socket_writer: WriteHalf,
        self_node_id: NodeId,
        keypair: KeyPair,
        timeout_duration: MassaTime,
        version: Version,
        max_bytes_read: f64,
        max_bytes_write: f64,
    ) -> Self {
        HandshakeWorker {
            reader: ReadBinder::new(
                socket_reader,
                max_bytes_read,
                MAX_MESSAGE_SIZE,
                MessageDeserializer::new(
                    THREAD_COUNT,
                    ENDORSEMENT_COUNT,
                    MAX_ADVERTISE_LENGTH,
                    MAX_ASK_BLOCKS_PER_MESSAGE,
                    MAX_OPERATIONS_PER_BLOCK,
                    MAX_OPERATIONS_PER_MESSAGE,
                    MAX_ENDORSEMENTS_PER_MESSAGE,
                    MAX_DATASTORE_VALUE_LENGTH,
                    MAX_FUNCTION_NAME_LENGTH,
                    MAX_PARAMETERS_SIZE,
                    MAX_OPERATION_DATASTORE_ENTRY_COUNT,
                    MAX_OPERATION_DATASTORE_KEY_LENGTH,
                    MAX_OPERATION_DATASTORE_VALUE_LENGTH,
                ),
            ),
            writer: WriteBinder::new(socket_writer, max_bytes_write, MAX_MESSAGE_SIZE),
            self_node_id,
            keypair,
            timeout_duration,
            version,
        }
    }

    /// Manages one on going handshake.
    /// Consumes self.
    /// Returns a tuple `(ConnectionId, Result)`.
    /// Creates the binders to communicate with that node.
    pub async fn run(mut self) -> HandshakeReturnType {
        // generate random bytes
        let mut self_random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut self_random_bytes);
        let self_random_hash = Hash::compute_from(&self_random_bytes);
        // send handshake init future
        let msg = Message::HandshakeInitiation {
            public_key: self.self_node_id.get_public_key(),
            random_bytes: self_random_bytes,
            version: self.version,
        };
        let send_init_fut = self.writer.send(&msg);

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
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeInitiation {
                    public_key: pk,
                    random_bytes: rb,
                    version,
                } => (NodeId::new(pk), rb, version),
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
        let self_signature = self.keypair.sign(&other_random_hash)?;

        // send handshake reply future
        let msg = Message::HandshakeReply {
            signature: self_signature,
        };
        let send_reply_fut = self.writer.send(&msg);

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
            Ok(Ok((_, Some((_, msg))))) => match msg {
                Message::HandshakeReply { signature: sig } => sig,
                _ => throw!(HandshakeWrongMessage),
            },
        };

        // check their signature
        other_node_id
            .get_public_key()
            .verify_signature(&self_random_hash, &other_signature)
            .map_err(|_err| {
                NetworkError::HandshakeError(HandshakeErrorType::HandshakeInvalidSignature)
            })?;

        Ok((other_node_id, self.reader, self.writer))
    }
}
