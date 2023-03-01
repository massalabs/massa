// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
use crate::messages::{
    BootstrapClientMessage, BootstrapClientMessageSerializer, BootstrapServerMessage,
    BootstrapServerMessageDeserializer,
};
use crate::settings::BootstrapClientConfig;
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_models::serialization::{DeserializeMinBEInt, SerializeMinBEInt};
use massa_models::version::{Version, VersionSerializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::{PublicKey, Signature, SIGNATURE_SIZE_BYTES};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Bootstrap client binder
pub struct BootstrapClientBinder {
    // max_bootstrap_message_size: u32,
    size_field_len: usize,
    remote_pubkey: PublicKey,
    // TODO: limit me
    duplex: Duplex,
    prev_message: Option<Hash>,
    version_serializer: VersionSerializer,
    cfg: BootstrapClientConfig,
}

impl BootstrapClientBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * duplex: duplex stream.
    /// * limit: limit max bytes per second (up and down)
    #[allow(clippy::too_many_arguments)]
    pub fn new(duplex: Duplex, remote_pubkey: PublicKey, cfg: BootstrapClientConfig) -> Self {
        let size_field_len = u32::be_bytes_min_length(cfg.max_bootstrap_message_size);
        BootstrapClientBinder {
            size_field_len,
            remote_pubkey,
            duplex,
            prev_message: None,
            version_serializer: VersionSerializer::new(),
            cfg,
        }
    }
}

impl BootstrapClientBinder {
    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    pub fn blocking_handshake(
        &mut self,
        version: Version,
        timeout: Option<Duration>,
    ) -> Result<Duration, BootstrapError> {
        // send version and randomn bytes
        let start = Instant::now();
        let msg_hash = {
            let mut version_ser = Vec::new();
            self.version_serializer
                .serialize(&version, &mut version_ser)?;
            let mut version_random_bytes =
                vec![0u8; version_ser.len() + self.cfg.randomness_size_bytes];
            version_random_bytes[..version_ser.len()].clone_from_slice(&version_ser);
            StdRng::from_entropy().fill_bytes(&mut version_random_bytes[version_ser.len()..]);
            self.write_all(&version_random_bytes, timeout)
                .map_err(|(_, e)| BootstrapError::from(e))?;
            Hash::compute_from(&version_random_bytes)
        };

        self.prev_message = Some(msg_hash);

        Ok(Instant::now().duration_since(start))
    }

    /// Reads the next message. NOT cancel-safe
    /// TODO: Before, this was called with a tokio timeout from fn bootstrap_from_server. It would
    ///       inteprprate it timing-out as "the server isn't trying to send us errors"
    ///       Ideally, would implement this behavior without relying on timing out
    ///       from an async context
    pub fn blocking_next(
        &mut self,
        _timeout: Option<Duration>,
    ) -> Result<(BootstrapServerMessage, Duration), BootstrapError> {
        let start = Instant::now();
        // read signature
        let sig = {
            let mut sig_bytes = [0u8; SIGNATURE_SIZE_BYTES];
            self.duplex.read_exact(&mut sig_bytes)?;
            Signature::from_bytes(&sig_bytes)?
        };

        // read message length
        let msg_len = {
            let mut msg_len_bytes = vec![0u8; self.size_field_len];
            self.duplex.read_exact(&mut msg_len_bytes[..])?;
            u32::from_be_bytes_min(&msg_len_bytes, self.cfg.max_bootstrap_message_size)?.0
        };

        // read message, check signature and check signature of the message sent just before then deserialize it
        let message_deserializer = BootstrapServerMessageDeserializer::new((&self.cfg).into());
        let message = {
            if let Some(prev_message) = self.prev_message {
                self.prev_message = Some(Hash::compute_from(&sig.to_bytes()));
                let mut sig_msg_bytes = vec![0u8; HASH_SIZE_BYTES + (msg_len as usize)];
                sig_msg_bytes[..HASH_SIZE_BYTES].copy_from_slice(prev_message.to_bytes());
                self.duplex
                    .read_exact(&mut sig_msg_bytes[HASH_SIZE_BYTES..])?;
                let msg_hash = Hash::compute_from(&sig_msg_bytes);
                self.remote_pubkey.verify_signature(&msg_hash, &sig)?;
                let (_, msg) = message_deserializer
                    .deserialize::<DeserializeError>(&sig_msg_bytes[HASH_SIZE_BYTES..])
                    .map_err(|err| BootstrapError::GeneralError(format!("{}", err)))?;
                msg
            } else {
                self.prev_message = Some(Hash::compute_from(&sig.to_bytes()));
                let mut sig_msg_bytes = vec![0u8; msg_len as usize];
                self.duplex.read_exact(&mut sig_msg_bytes[..])?;
                let msg_hash = Hash::compute_from(&sig_msg_bytes);
                self.remote_pubkey.verify_signature(&msg_hash, &sig)?;
                let (_, msg) = message_deserializer
                    .deserialize::<DeserializeError>(&sig_msg_bytes[..])
                    .map_err(|err| BootstrapError::GeneralError(format!("{}", err)))?;
                msg
            }
        };
        Ok((message, Instant::now().duration_since(start)))
    }
    /// The std-lib write_all only
    fn write_all(
        &mut self,
        data: &[u8],
        time_limit: Option<Duration>,
    ) -> Result<Duration, (usize, std::io::Error)> {
        let start = Instant::now();
        let time_limit = if time_limit.is_none() {
            self.duplex
                .set_write_timeout(Some(Duration::from_secs(9999999)))
                // Err only if Some(zero-duration) is used
                .unwrap();
            return self
                .duplex
                .write_all(data)
                .map_err(|er| (data.len(), er))
                .map(|_| Instant::now().duration_since(start));
        } else {
            time_limit.unwrap()
        };
        let mut acc = 0;
        let mut remain = time_limit;
        let mut clock = Duration::from_secs(0);
        while acc < data.len() {
            if clock > time_limit {
                return Err((acc, std::io::ErrorKind::TimedOut.into()));
            }
            self.duplex
                .set_write_timeout(Some(time_limit - clock))
                .expect("internal error");
            acc += self.duplex.write(&data[acc..]).map_err(|er| (acc, er))?;
            clock = Instant::now().duration_since(start);
        }
        Ok(Instant::now().duration_since(start))
    }

    #[allow(dead_code)]
    /// Send a message to the bootstrap server
    pub fn blocking_send(
        &mut self,
        msg: &BootstrapClientMessage,
        timeout: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        let mut msg_bytes = Vec::new();
        let message_serializer = BootstrapClientMessageSerializer::new();
        message_serializer.serialize(msg, &mut msg_bytes)?;
        let msg_len: u32 = msg_bytes.len().try_into().map_err(|e| {
            BootstrapError::GeneralError(format!("bootstrap message too large to encode: {}", e))
        })?;

        if let Some(prev_message) = self.prev_message {
            // there was a previous message
            let prev_message = prev_message.to_bytes();

            // update current previous message to be hash(prev_msg_hash + msg)
            let mut hash_data =
                Vec::with_capacity(prev_message.len().saturating_add(msg_bytes.len()));
            hash_data.extend(prev_message);
            hash_data.extend(&msg_bytes);
            self.prev_message = Some(Hash::compute_from(&hash_data));

            // send old previous message, track time taken
            let write_time = self
                .write_all(prev_message, timeout)
                .map_err(|(_, e)| BootstrapError::IoError(e))?;
            // update time
            let timeout = timeout.map(|old| old - write_time);
        } else {
            // there was no previous message

            //update current previous message
            self.prev_message = Some(Hash::compute_from(&msg_bytes));
        }

        // send message length
        let write_time = {
            let msg_len_bytes = msg_len.to_be_bytes_min(self.cfg.max_bootstrap_message_size)?;
            self.write_all(&msg_len_bytes, timeout)
                .map_err(|(_, e)| BootstrapError::IoError(e))?
        };

        let timeout = timeout.map(|old| old - write_time);
        // send message
        self.write_all(&msg_bytes, timeout)
            .map_err(|(_, e)| BootstrapError::IoError(e))?;
        Ok(())
    }
}

#[test]
fn test_write_all_timeout() {
    // create a client binder...
    // setup a connection
    // create a big message
    // set a small-enough timeout
    // assert that it fails on ClientBinding::write_all_timeout()
    // assert that it succeeds with std-libs `write_all` after setting a timeout
    // set a tiiiiiny timeout
    // assert that it fails with std-lib write_all
}
