// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::bindings::BindingReadExact;
use crate::error::BootstrapError;
use crate::messages::{
    BootstrapClientMessage, BootstrapClientMessageDeserializer, BootstrapServerMessage,
    BootstrapServerMessageSerializer,
};
use crate::settings::BootstrapSrvBindCfg;
use massa_hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_models::config::{MAX_BOOTSTRAP_MESSAGE_SIZE, MAX_BOOTSTRAP_MESSAGE_SIZE_BYTES};
use massa_models::serialization::{DeserializeMinBEInt, SerializeMinBEInt};
use massa_models::version::{Version, VersionDeserializer, VersionSerializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use std::io;
use std::time::Instant;
use std::{
    convert::TryInto,
    io::ErrorKind,
    net::{SocketAddr, TcpStream},
    thread,
    time::Duration,
};
use stream_limiter::{Limiter, LimiterOptions};
use tracing::error;

use super::BindingWriteExact;

const KNOWN_PREFIX_LEN: usize = HASH_SIZE_BYTES + MAX_BOOTSTRAP_MESSAGE_SIZE_BYTES;
/// The known-length component of a message to be received.
struct ClientMessageLeader {
    received_prev_hash: Option<Hash>,
    msg_len: u32,
}

/// Bootstrap server binder
pub struct BootstrapServerBinder {
    max_consensus_block_ids: u64,
    thread_count: u8,
    max_datastore_key_length: u8,
    randomness_size_bytes: usize,
    local_keypair: KeyPair,
    duplex: Limiter<TcpStream>,
    prev_message: Option<Hash>,
    version_serializer: VersionSerializer,
    version_deserializer: VersionDeserializer,
    write_error_timeout: MassaTime,
}

impl BootstrapServerBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * `duplex`: duplex stream.
    /// * `local_keypair`: local node user keypair
    /// * `limit`: limit max bytes per second (up and down)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        duplex: TcpStream,
        local_keypair: KeyPair,
        cfg: BootstrapSrvBindCfg,
        rw_limit: Option<u64>,
    ) -> Self {
        let BootstrapSrvBindCfg {
            max_bytes_read_write: _limit,
            thread_count,
            max_datastore_key_length,
            randomness_size_bytes,
            consensus_bootstrap_part_size,
            write_error_timeout,
        } = cfg;

        // A 1s window breaks anything requiring a 1s window
        let limit_opts = rw_limit.map(|limit| -> LimiterOptions {
            LimiterOptions::new(limit / 100, Duration::from_millis(10), limit / 10)
        });
        let duplex = Limiter::new(duplex, limit_opts.clone(), limit_opts);
        BootstrapServerBinder {
            max_consensus_block_ids: consensus_bootstrap_part_size,
            local_keypair,
            duplex,
            prev_message: None,
            thread_count,
            max_datastore_key_length,
            randomness_size_bytes,
            version_serializer: VersionSerializer::new(),
            version_deserializer: VersionDeserializer::new(),
            write_error_timeout,
        }
    }
    /// Performs a handshake. Should be called after connection
    /// MUST always be followed by a send of the `BootstrapMessage::BootstrapTime`
    pub fn handshake_timeout(
        &mut self,
        version: Version,
        duration: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        let deadline = duration.map(|d| Instant::now() + d);
        // read version and random bytes, send signature
        let msg_hash = {
            let mut version_bytes = Vec::new();
            self.version_serializer
                .serialize(&version, &mut version_bytes)?;
            let mut msg_bytes = vec![0u8; version_bytes.len() + self.randomness_size_bytes];
            self.read_exact_timeout(&mut msg_bytes, deadline)
                .map_err(|(e, _)| e)?;
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
                    Err(e) => error!(
                        "bootstrap server encountered error '{}' sending error '{}' to addr '{}'",
                        e, msg, addr
                    ),
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

    /// Writes the next message.
    pub fn send_timeout(
        &mut self,
        msg: BootstrapServerMessage,
        duration: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        let deadline = duration.map(|d| Instant::now() + d);
        // serialize the message to bytes
        let mut msg_bytes = Vec::new();
        BootstrapServerMessageSerializer::new().serialize(&msg, &mut msg_bytes)?;
        let msg_len: u32 = msg_bytes.len().try_into().map_err(|e| {
            BootstrapError::GeneralError(format!("bootstrap message too large to encode: {}", e))
        })?;

        // compute signature, and extract the bytes
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

        // construct msg length, and convert to bytes
        let msg_len_bytes = msg_len.to_be_bytes_min(MAX_BOOTSTRAP_MESSAGE_SIZE)?;

        // organize the bytes into a sendable array
        let stream_data = [sig.to_bytes().as_slice(), &msg_len_bytes, &msg_bytes].concat();

        // send the data
        self.write_all_timeout(&stream_data, deadline)
            .map_err(|(e, _)| e)?;

        // update prev sig
        self.prev_message = Some(Hash::compute_from(&sig.to_bytes()));

        Ok(())
    }

    // TODO: use a proper (de)serializer: https://github.com/massalabs/massa/pull/3745#discussion_r1169733161
    /// Read a message sent from the client (not signed).
    pub fn next_timeout(
        &mut self,
        duration: Option<Duration>,
    ) -> Result<BootstrapClientMessage, BootstrapError> {
        let deadline = duration.map(|d| Instant::now() + d);

        let mut known_len_buf = vec![0; KNOWN_PREFIX_LEN];
        // TODO: handle a partial read
        self.read_exact_timeout(&mut known_len_buf, deadline)
            .map_err(|(err, _consumed)| err)?;

        let ClientMessageLeader {
            received_prev_hash,
            msg_len,
        } = self.decode_message_leader(&known_len_buf)?;

        // read the rest of the message
        let mut msg_bytes = vec![0u8; msg_len as usize];
        self.read_exact_timeout(&mut msg_bytes, deadline)
            .map_err(|(err, _consumed)| err)?;

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

    /// We are using this instead of of our library deserializer as the process is relatively straight forward
    /// and makes error-type management cleaner
    fn decode_message_leader(
        &self,
        leader_buf: &[u8],
    ) -> Result<ClientMessageLeader, BootstrapError> {
        // construct prev-hash
        let received_prev_hash = {
            if self.prev_message.is_some() {
                Some(Hash::from_bytes(
                    leader_buf[..HASH_SIZE_BYTES]
                        .try_into()
                        .expect("bad slice logic"),
                ))
            } else {
                None
            }
        };

        // construct msg-len
        let msg_len = {
            u32::from_be_bytes_min(&leader_buf[HASH_SIZE_BYTES..], MAX_BOOTSTRAP_MESSAGE_SIZE)?.0
        };
        Ok(ClientMessageLeader {
            received_prev_hash,
            msg_len,
        })
    }
}

impl io::Read for BootstrapServerBinder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.duplex.read(buf)
    }
}

impl crate::bindings::BindingReadExact for BootstrapServerBinder {
    fn set_read_timeout(&mut self, duration: Option<Duration>) -> Result<(), std::io::Error> {
        self.duplex.stream.set_read_timeout(duration)
    }
}

impl io::Write for BootstrapServerBinder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.duplex.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.duplex.flush()
    }
}

impl crate::bindings::BindingWriteExact for BootstrapServerBinder {
    fn set_write_timeout(&mut self, duration: Option<Duration>) -> Result<(), std::io::Error> {
        self.duplex.stream.set_write_timeout(duration)
    }
}
