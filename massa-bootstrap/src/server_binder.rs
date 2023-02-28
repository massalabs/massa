// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
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
use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

/// Bootstrap server binder
pub struct BootstrapServerBinder {
    max_bootstrap_message_size: u32,
    max_consensus_block_ids: u64,
    thread_count: u8,
    max_datastore_key_length: u8,
    randomness_size_bytes: usize,
    size_field_len: usize,
    local_keypair: KeyPair,
    // TODO: limit me
    duplex: Duplex,
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
    pub fn new(duplex: Duplex, local_keypair: KeyPair, cfg: BootstrapSrvBindCfg) -> Self {
        let BootstrapSrvBindCfg {
            max_bytes_read_write: limit,
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
}

impl BootstrapServerBinder {
    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    /// MUST always be followed by a send of the `BootstrapMessage::BootstrapTime`
    pub fn blocking_handshake(
        &mut self,
        version: Version,
        timeout: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        // read version and random bytes, send signature
        let msg_hash = {
            let mut version_bytes = Vec::new();
            self.version_serializer
                .serialize(&version, &mut version_bytes)?;
            let mut msg_bytes = vec![0u8; version_bytes.len() + self.randomness_size_bytes];
            self.read_exact(&mut msg_bytes, timeout).map_err(|e| e.1)?;
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
    fn read_exact(
        &mut self,
        buf: &mut [u8],
        time_limit: Option<Duration>,
    ) -> Result<Duration, (usize, std::io::Error)> {
        let start = Instant::now();
        let time_limit = if time_limit.is_none() {
            self.duplex
                .set_read_timeout(Some(Duration::from_secs(9999999)));
            return self
                .duplex
                .read_exact(buf)
                .map_err(|er| (buf.len(), er))
                .map(|_| Instant::now().duration_since(start));
        } else {
            time_limit.unwrap()
        };

        let mut acc = 0;
        let mut remain = time_limit;
        let mut clock = Duration::from_secs(0);
        while acc < buf.len() {
            if clock > time_limit {
                return Err((acc, std::io::ErrorKind::TimedOut.into()));
            }
            self.duplex
                .set_read_timeout(Some(time_limit - clock))
                .expect("internal error");
            self.duplex.read(&mut buf[acc..]).map_err(|er| (acc, er))?;
            clock = Instant::now().duration_since(start);
        }
        Ok(Instant::now().duration_since(start))
    }

    pub fn send_msg(
        &mut self,
        msg: BootstrapServerMessage,
        timeout: Option<Duration>,
    ) -> Result<Duration, BootstrapError> {
        self.blocking_send(msg, timeout)
    }

    /// 1. Spawns a thread
    /// 2. blocks on the passed in runtime
    /// 3. uses passed in handle to send a message to the client
    /// 4. logs an error if the send times out
    /// 5. runs the passed in closure (typically a custom logging msg)
    ///
    /// consumes the binding in the process
    pub(crate) fn close_and_send_error<F>(
        mut self,
        server_outer_rt_hnd: Handle,
        msg: String,
        addr: SocketAddr,
        close_fn: F,
    ) where
        F: FnOnce() + Send + 'static,
    {
        thread::Builder::new()
            .name("bootstrap-error-send".to_string())
            .spawn(move || -> Result<(), BootstrapError> {
                let _elapsed = self.blocking_send_error(msg)?;
                close_fn();
                Ok(())
            })
            // the non-builder spawn doesn't return a Result, and documentation states that
            // it's an error at the OS level.
            .unwrap();
    }
    pub fn blocking_send_error(&mut self, error: String) -> Result<Duration, BootstrapError> {
        self.blocking_send(
            BootstrapServerMessage::BootstrapError { error },
            Some(self.write_error_timeout.into()),
        )
    }

    /// Writes the next message. NOT cancel-safe
    pub fn blocking_send(
        &mut self,
        msg: BootstrapServerMessage,
        timeout: Option<Duration>,
    ) -> Result<Duration, BootstrapError> {
        let start = Instant::now();
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

        if let Some(timeout) = timeout {}
        // send signature
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

        Ok(Instant::now().duration_since(start))
    }

    #[allow(dead_code)]
    /// Read a message sent from the client (not signed).
    pub fn blocking_next(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<(BootstrapClientMessage, Duration), BootstrapError> {
        let start = Instant::now();
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

        Ok((msg, Instant::now().duration_since(start)))
    }
}
