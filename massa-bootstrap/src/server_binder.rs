// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::messages::BootstrapMessage;
use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
use massa_hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_models::{constants::BOOTSTRAP_RANDOMNESS_SIZE_BYTES, SerializeMinBEInt};
use massa_models::{with_serialization_context, SerializeCompact};
use massa_signature::{sign, PrivateKey, Signature, SIGNATURE_SIZE_BYTES};
use std::convert::TryInto;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Bootstrap server binder
pub struct BootstrapServerBinder {
    max_bootstrap_message_size: u32,
    local_privkey: PrivateKey,
    duplex: Duplex,
    prev_sig: Option<Signature>,
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
            prev_sig: None,
        }
    }

    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    pub async fn handshake(&mut self) -> Result<(), BootstrapError> {
        // read randomness, check hash
        let rand_hash = {
            let mut random_bytes = [0u8; BOOTSTRAP_RANDOMNESS_SIZE_BYTES];
            self.duplex.read_exact(&mut random_bytes).await?;
            let expected_hash = Hash::compute_from(&random_bytes);
            let mut hash_bytes = [0u8; HASH_SIZE_BYTES];
            self.duplex.read_exact(&mut hash_bytes).await?;
            if Hash::from_bytes(&hash_bytes)? != expected_hash {
                return Err(BootstrapError::GeneralError("wrong handshake hash".into()));
            }
            expected_hash
        };

        // send signature
        let sig = sign(&rand_hash, &self.local_privkey)?;
        self.duplex.write_all(&sig.to_bytes()).await?;

        // save prev sig
        self.prev_sig = Some(sig);

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
            let mut signed_data = vec![0u8; SIGNATURE_SIZE_BYTES + (msg_len as usize)];
            signed_data[..SIGNATURE_SIZE_BYTES]
                .clone_from_slice(&self.prev_sig.unwrap().to_bytes());
            signed_data[SIGNATURE_SIZE_BYTES..].clone_from_slice(&msg_bytes);
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
        self.prev_sig = Some(sig);

        Ok(())
    }
}
