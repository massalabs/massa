// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::BootstrapError;
use crate::messages::{
    BootstrapClientMessage, BootstrapClientMessageDeserializer, BootstrapServerMessage,
    BootstrapServerMessageSerializer,
};
use crate::settings::BootstrapSrvBindCfg;
use massa_hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_models::serialization::{DeserializeMinBEInt, SerializeMinBEInt};
use massa_models::version::{Version, VersionDeserializer, VersionSerializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use mio::net::TcpStream;
use mio::{Events, Interest, Poll, Token};
use std::convert::TryInto;
use std::io::{ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;
use tracing::error;

/// Bootstrap server binder
pub struct BootstrapServerBinder {
    max_bootstrap_message_size: u32,
    max_consensus_block_ids: u64,
    thread_count: u8,
    max_datastore_key_length: u8,
    randomness_size_bytes: usize,
    size_field_len: usize,
    local_keypair: KeyPair,
    prev_message: Option<Hash>,
    version_serializer: VersionSerializer,
    version_deserializer: VersionDeserializer,
    write_error_timeout: MassaTime,
    poll: Poll,
    events: Events,
    duplex: TcpStream,
}

const BINDING_EVENT: Token = Token(0);

impl BootstrapServerBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * `duplex`: duplex stream.
    /// * `local_keypair`: local node user keypair
    /// * `limit`: limit max bytes per second (up and down)
    #[allow(clippy::too_many_arguments)]
    pub fn new(mut duplex: TcpStream, local_keypair: KeyPair, cfg: BootstrapSrvBindCfg) -> Self {
        let poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(1024);
        poll.registry()
            .register(&mut duplex, BINDING_EVENT, Interest::READABLE)
            .unwrap();
        let BootstrapSrvBindCfg {
            max_bytes_read_write: _limit,
            max_bootstrap_message_size,
            thread_count,
            max_datastore_key_length,
            randomness_size_bytes,
            consensus_bootstrap_part_size,
            write_error_timeout,
        } = cfg;
        let size_field_len = u32::be_bytes_min_length(max_bootstrap_message_size);
        BootstrapServerBinder {
            max_bootstrap_message_size,
            max_consensus_block_ids: consensus_bootstrap_part_size,
            size_field_len,
            local_keypair,
            prev_message: None,
            thread_count,
            max_datastore_key_length,
            randomness_size_bytes,
            version_serializer: VersionSerializer::new(),
            version_deserializer: VersionDeserializer::new(),
            write_error_timeout,
            poll,
            events,
            duplex,
        }
    }
    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    /// MUST always be followed by a send of the `BootstrapMessage::BootstrapTime`
    pub fn handshake_timeout(
        &mut self,
        version: Version,
        duration: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        // read version and random bytes, send signature
        let msg_hash = {
            let mut version_bytes = Vec::new();
            self.version_serializer
                .serialize(&version, &mut version_bytes)?;
            let mut msg_bytes = vec![0u8; version_bytes.len() + self.randomness_size_bytes];
            // self.duplex.set_read_timeout(duration)?;
            self.duplex.read_exact(&mut msg_bytes)?;
            let (_, received_version) = self
                .version_deserializer
                .deserialize::<DeserializeError>(&msg_bytes[..version_bytes.len()])
                .map_err(|err| BootstrapError::GeneralError(format!("{}", &err)))?;
            if !received_version.is_compatible(&version) {
                return Err(BootstrapError::IncompatibleVersionError(format!("Received a bad incompatible version in handshake. (excepted: {}, received: {})", version, received_version)));
            }
            Hash::compute_from(&msg_bytes)
        };

        // save prev sig
        self.prev_message = Some(msg_hash);

        Ok(())
    }

    pub fn send_msg(
        &mut self,
        timeout: Duration,
        msg: BootstrapServerMessage,
    ) -> Result<(), BootstrapError> {
        let to_str = msg.to_string();
        self.send_timeout(msg, Some(timeout)).map_err(|e| match e {
            BootstrapError::IoError(e)
            // On some systems, a timed out send returns WouldBlock
                if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock =>
            {
                BootstrapError::TimedOut(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("BootstrapServerMessage::{} send timed out", to_str),
                ))
            }
            _ => e,
        })
    }

    /// 1. Spawns a thread
    /// 2. blocks on the passed in runtime
    /// 3. uses passed in handle to send a message to the client
    /// 4. logs an error if the send times out
    /// 5. runs the passed in closure (typically a custom logging msg)
    ///
    /// consumes the binding in the process
    pub(crate) fn close_and_send_error<F>(mut self, msg: String, addr: SocketAddr, close_fn: F)
    where
        F: FnOnce() + Send + 'static,
    {
        thread::Builder::new()
            .name("bootstrap-error-send".to_string())
            .spawn(move || {
                let msg_cloned = msg.clone();
                let err_send = self.send_error_timeout(msg_cloned);
                match err_send {
                    Err(BootstrapError::IoError(e)) if e.kind() == ErrorKind::TimedOut => error!(
                        "bootstrap server timed out sending error '{}' to addr {}",
                        msg, addr
                    ),
                    Err(e) => error!("{}", e),
                    Ok(_) => {}
                }
                close_fn();
            })
            // the non-builder spawn doesn't return a Result, and documentation states that
            // it's an error at the OS level.
            .unwrap();
    }
    pub fn send_error_timeout(&mut self, error: String) -> Result<(), BootstrapError> {
        self.send_timeout(
            BootstrapServerMessage::BootstrapError { error },
            Some(self.write_error_timeout.to_duration()),
        )
        .map_err(|e| match e {
            BootstrapError::IoError(e) if e.kind() == ErrorKind::WouldBlock => {
                BootstrapError::TimedOut(ErrorKind::TimedOut.into())
            }
            e => e,
        })
    }

    /// Writes the next message. NOT cancel-safe
    pub fn send_timeout(
        &mut self,
        msg: BootstrapServerMessage,
        duration: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        // serialize message
        let mut msg_bytes = Vec::new();
        BootstrapServerMessageSerializer::new().serialize(&msg, &mut msg_bytes)?;
        let msg_len: u32 = msg_bytes.len().try_into().map_err(|e| {
            BootstrapError::GeneralError(format!("bootstrap message too large to encode: {}", e))
        })?;

        // compute signature
        let sig = {
            if let Some(prev_message) = self.prev_message {
                // there was a previous message: sign(prev_msg_hash + msg)
                let mut signed_data =
                    Vec::with_capacity(HASH_SIZE_BYTES.saturating_add(msg_len as usize));
                signed_data.extend(prev_message.to_bytes());
                signed_data.extend(&msg_bytes);
                self.local_keypair.sign(&Hash::compute_from(&signed_data))?
            } else {
                // there was no previous message: sign(msg)
                self.local_keypair.sign(&Hash::compute_from(&msg_bytes))?
            }
        };

        // send signature
        // self.duplex.set_write_timeout(duration)?;
        self.duplex.write_all(&sig.to_bytes())?;

        // send message length
        {
            let msg_len_bytes = msg_len.to_be_bytes_min(self.max_bootstrap_message_size)?;
            self.duplex.write_all(&msg_len_bytes)?;
        }

        // send message
        self.duplex.write_all(&msg_bytes)?;

        // save prev sig
        self.prev_message = Some(Hash::compute_from(&sig.to_bytes()));

        Ok(())
    }

    #[allow(dead_code)]
    /// Read a message sent from the client (not signed). NOT cancel-safe
    pub fn next_timeout(
        &mut self,
        duration: Option<Duration>,
    ) -> Result<BootstrapClientMessage, BootstrapError> {
        // self.duplex.set_read_timeout(duration)?;
        // read prev hash
        let received_prev_hash = {
            if self.prev_message.is_some() {
                let mut hash_bytes = [0u8; HASH_SIZE_BYTES];
                self.duplex.read_exact(&mut hash_bytes)?;
                Some(Hash::from_bytes(&hash_bytes))
            } else {
                None
            }
        };

        // read message length
        let msg_len = {
            let mut msg_len_bytes = vec![0u8; self.size_field_len];
            self.duplex.read_exact(&mut msg_len_bytes[..])?;
            u32::from_be_bytes_min(&msg_len_bytes, self.max_bootstrap_message_size)?.0
        };

        // read message
        let mut msg_bytes = vec![0u8; msg_len as usize];
        self.duplex.read_exact(&mut msg_bytes)?;

        // check previous hash
        if received_prev_hash != self.prev_message {
            return Err(BootstrapError::GeneralError(
                "Message sequencing has been broken".to_string(),
            ));
        }

        // update previous hash
        if let Some(prev_hash) = received_prev_hash {
            // there was a previous message: hash(prev_hash + message)
            let mut hashed_bytes =
                Vec::with_capacity(HASH_SIZE_BYTES.saturating_add(msg_bytes.len()));
            hashed_bytes.extend(prev_hash.to_bytes());
            hashed_bytes.extend(&msg_bytes);
            self.prev_message = Some(Hash::compute_from(&hashed_bytes));
        } else {
            // no previous message: hash message only
            self.prev_message = Some(Hash::compute_from(&msg_bytes));
        }

        // deserialize message
        let (_, msg) = BootstrapClientMessageDeserializer::new(
            self.thread_count,
            self.max_datastore_key_length,
            self.max_consensus_block_ids,
        )
        .deserialize::<DeserializeError>(&msg_bytes)
        .map_err(|err| BootstrapError::GeneralError(format!("{}", err)))?;

        Ok(msg)
    }
}
