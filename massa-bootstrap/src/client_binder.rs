// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::BootstrapError;
use crate::messages::{
    BootstrapClientMessage, BootstrapClientMessageSerializer, BootstrapServerMessage,
    BootstrapServerMessageDeserializer,
};
use crate::settings::BootstrapClientConfig;
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_logging::massa_trace;
use massa_models::serialization::{DeserializeMinBEInt, SerializeMinBEInt};
use massa_models::version::{Version, VersionSerializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::{PublicKey, Signature, SIGNATURE_SIZE_BYTES};
use massa_time::MassaTime;
use rand::{rngs::StdRng, RngCore, SeedableRng};

/// Bootstrap client binder
pub struct BootstrapClientBinder {
    // max_bootstrap_message_size: u32,
    size_field_len: usize,
    remote_pubkey: PublicKey,
    duplex: TcpStream,
    prev_message: Hash,
    cfg: BootstrapClientConfig,
}
pub struct NonHSClientBinder {
    // max_bootstrap_message_size: u32,
    size_field_len: usize,
    remote_pubkey: PublicKey,
    duplex: TcpStream,
    version_serializer: VersionSerializer,
    cfg: BootstrapClientConfig,
}

impl NonHSClientBinder {
    /// Encapsulates a new connection to a client, ready to perform a handshake.
    pub fn new(duplex: TcpStream, remote_pubkey: PublicKey, cfg: BootstrapClientConfig) -> Self {
        let size_field_len = u32::be_bytes_min_length(cfg.max_bootstrap_message_size);
        Self {
            size_field_len,
            remote_pubkey,
            duplex,
            version_serializer: VersionSerializer::new(),
            cfg,
        }
    }
    /// Attempts to upgrade the connection to a client by performing a handshake, providing a ping estimation in the process
    /// NOT cancel-safe, so does not return the binder if cancelled
    pub fn handshake(
        mut self,
        version: Version,
        duration: Duration,
    ) -> Result<(BootstrapClientBinder, MassaTime), BootstrapError> {
        // read error (if sent by the server)
        self = self.check_no_error(duration)?;

        // handshake
        // send version and randomn bytes
        let mut version_ser = Vec::new();
        self.version_serializer
            .serialize(&version, &mut version_ser)?;
        let mut version_random_bytes =
            vec![0u8; version_ser.len() + self.cfg.randomness_size_bytes];
        version_random_bytes[..version_ser.len()].clone_from_slice(&version_ser);
        StdRng::from_entropy().fill_bytes(&mut version_random_bytes[version_ser.len()..]);

        // do the send
        let send_time_uncompensated = MassaTime::now()?;
        self.duplex.write_all(&version_random_bytes)?;

        // compute ping
        let ping = MassaTime::now()?.saturating_sub(send_time_uncompensated);

        let msg_hash = Hash::compute_from(&version_random_bytes);

        Ok((
            BootstrapClientBinder {
                size_field_len: self.size_field_len,
                remote_pubkey: self.remote_pubkey,
                duplex: self.duplex,
                prev_message: msg_hash,
                cfg: self.cfg,
            },
            ping,
        ))
    }

    /// Checks that no errors were sent by the server
    pub fn check_no_error(self, duration: Duration) -> Result<Self, BootstrapError> {
        // read error (if sent by the server)
        // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
        self.duplex.set_read_timeout(Some(duration))?;
        match self
            .duplex
            .peek(&mut [0u8; SIGNATURE_SIZE_BYTES])
            .map_err(Into::into)
        {
            Err(BootstrapError::TimedOut(_)) | Ok(0) => {
                massa_trace!(
                    "bootstrap.lib.bootstrap_from_server: No error sent at connection",
                    {}
                );
            }
            Err(e) => return Err(e),
            Ok(_n) => {
                return Err(BootstrapError::GeneralError(
                    "bootstrap server handshake unexpected bytes available".to_string(),
                ));
            }
        };
        Ok(self)
    }
}

impl BootstrapClientBinder {
    /// Reads the next message. NOT cancel-safe
    pub fn next_timeout(
        &mut self,
        duration: Option<Duration>,
    ) -> Result<BootstrapServerMessage, BootstrapError> {
        self.duplex.set_read_timeout(duration)?;
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
        let prev_message = self.prev_message;
        let message = {
            self.prev_message = Hash::compute_from(&sig.to_bytes());
            let mut sig_msg_bytes = vec![0u8; HASH_SIZE_BYTES + (msg_len as usize)];
            sig_msg_bytes[..HASH_SIZE_BYTES].copy_from_slice(prev_message.to_bytes());
            self.duplex
                .read_exact(&mut sig_msg_bytes[HASH_SIZE_BYTES..])?;
            let msg_hash = Hash::compute_from(&sig_msg_bytes);
            self.remote_pubkey.verify_signature(&msg_hash, &sig)?;
            let (_, msg) = message_deserializer
                .deserialize::<DeserializeError>(&sig_msg_bytes[HASH_SIZE_BYTES..])
                .map_err(|err| BootstrapError::DeserializeError(format!("{}", err)))?;
            msg
        };
        Ok(message)
    }

    #[allow(dead_code)]
    /// Send a message to the bootstrap server
    pub fn send_timeout(
        &mut self,
        msg: &BootstrapClientMessage,
        duration: Option<Duration>,
    ) -> Result<(), BootstrapError> {
        let mut msg_bytes = Vec::new();
        let message_serializer = BootstrapClientMessageSerializer::new();
        message_serializer.serialize(msg, &mut msg_bytes)?;
        let msg_len: u32 = msg_bytes.len().try_into().map_err(|e| {
            BootstrapError::GeneralError(format!("bootstrap message too large to encode: {}", e))
        })?;

        let prev_message = self.prev_message;
        // there was a previous message
        let prev_message = prev_message.to_bytes();

        // update current previous message to be hash(prev_msg_hash + msg)
        let mut hash_data = Vec::with_capacity(prev_message.len().saturating_add(msg_bytes.len()));
        hash_data.extend(prev_message);
        hash_data.extend(&msg_bytes);
        self.prev_message = Hash::compute_from(&hash_data);

        // send old previous message
        self.duplex.write_all(prev_message)?;

        // send message length
        {
            self.duplex.set_write_timeout(duration)?;
            let msg_len_bytes = msg_len.to_be_bytes_min(self.cfg.max_bootstrap_message_size)?;
            self.duplex.write_all(&msg_len_bytes)?;
        }

        // send message
        self.duplex.write_all(&msg_bytes)?;
        Ok(())
    }
}
