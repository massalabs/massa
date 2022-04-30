// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::messages::BootstrapMessage;
use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
use massa_hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_models::Version;
use massa_models::{
    constants::BOOTSTRAP_RANDOMNESS_SIZE_BYTES, with_serialization_context, DeserializeCompact,
    DeserializeMinBEInt, SerializeCompact, SerializeMinBEInt,
};
use massa_signature::{sign, PrivateKey};
use std::convert::TryInto;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Bootstrap server binder
pub struct BootstrapServerBinder {
    max_bootstrap_message_size: u32,
    local_privkey: PrivateKey,
    duplex: Duplex,
    prev_data: Option<Hash>,
}

impl BootstrapServerBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * duplex: duplex stream.
    pub fn new(duplex: Duplex, local_privkey: PrivateKey) -> Self {
        let max_bootstrap_message_size =
            with_serialization_context(|context| context.max_bootstrap_message_size);
        BootstrapServerBinder {
            max_bootstrap_message_size,
            local_privkey,
            duplex,
            prev_data: None,
        }
    }

    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    /// MUST always be followed by a send of the BootstrapMessage::BootstrapTime
    pub async fn handshake(&mut self, version: Version) -> Result<(), BootstrapError> {
        // read randomness, check hash
        let rand_hash = {
            let version_bytes = version.to_bytes_compact()?;
            let mut random_bytes =
                vec![0u8; (version_bytes.len() as usize) + BOOTSTRAP_RANDOMNESS_SIZE_BYTES];
            self.duplex.read_exact(&mut random_bytes).await?;
            let receive_version =
                Version::from_bytes_compact(&random_bytes[..version_bytes.len()])?;
            if !receive_version.0.is_compatible(&version) {
                return Err(BootstrapError::IncompatibleVersionError(format!("Received a bad incompatible version in handshake. (excepted: {}, received: {})", version, receive_version.0)));
            }
            let expected_hash = Hash::compute_from(&random_bytes);
            let mut hash_bytes = [0u8; HASH_SIZE_BYTES];
            self.duplex.read_exact(&mut hash_bytes).await?;
            if Hash::from_bytes(&hash_bytes)? != expected_hash {
                return Err(BootstrapError::GeneralError("wrong handshake hash".into()));
            }
            expected_hash
        };

        // save prev sig
        self.prev_data = Some(rand_hash);

        Ok(())
    }

    /// Writes the next message. NOT cancel-safe
    pub async fn send(&mut self, msg: BootstrapMessage) -> Result<(), BootstrapError> {
        // serialize message
        let msg_bytes = msg.to_bytes_compact()?;
        let msg_len: u32 = msg_bytes.len().try_into().map_err(|e| {
            BootstrapError::GeneralError(format!("bootstrap message too large to encode: {}", e))
        })?;

        // compute signature
        let sig = {
            let prev_data = self.prev_data.unwrap();
            let mut signed_data = vec![0u8; HASH_SIZE_BYTES + (msg_len as usize)];
            signed_data[..HASH_SIZE_BYTES].clone_from_slice(&prev_data.to_bytes());
            signed_data[HASH_SIZE_BYTES..].clone_from_slice(&msg_bytes);
            sign(&Hash::compute_from(&signed_data), &self.local_privkey)?
        };

        // send signature
        self.duplex.write_all(&sig.to_bytes()).await?;

        // send message length
        {
            let msg_len_bytes = msg_len.to_be_bytes_min(self.max_bootstrap_message_size)?;
            self.duplex.write_all(&msg_len_bytes).await?;
        }

        // send message
        self.duplex.write_all(&msg_bytes).await?;

        // save prev sig
        self.prev_data = Some(Hash::compute_from(&sig.to_bytes()));

        Ok(())
    }

    #[allow(dead_code)]
    /// Read a message sent from the client (not signed). NOT cancel-safe
    pub async fn next(&mut self) -> Result<BootstrapMessage, BootstrapError> {
        // read prev hash
        let hash = {
            let mut hash_bytes = [0u8; HASH_SIZE_BYTES];
            self.duplex.read_exact(&mut hash_bytes).await?;
            Hash::from_bytes(&hash_bytes)?
        };

        let size_field_len = u32::be_bytes_min_length(self.max_bootstrap_message_size);

        // read message length
        let msg_len = {
            let mut meg_len_bytes = vec![0u8; size_field_len];
            self.duplex.read_exact(&mut meg_len_bytes[..]).await?;
            u32::from_be_bytes_min(&meg_len_bytes, self.max_bootstrap_message_size)?.0
        };
        // read message and deserialize
        let mut msg_bytes = vec![0u8; msg_len as usize];
        let message = {
            self.duplex.read_exact(&mut msg_bytes).await?;
            let prev_message = self.prev_data.unwrap();
            if prev_message != hash {
                return Err(BootstrapError::GeneralError(
                    "Sequential in message has been broken".to_string(),
                ));
            }
            let (msg, _len) = BootstrapMessage::from_bytes_compact(&msg_bytes)?;
            msg
        };
        self.prev_data = Some(Hash::compute_from(&msg_bytes));
        Ok(message)
    }
}
