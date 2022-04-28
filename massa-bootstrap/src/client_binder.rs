// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::messages::BootstrapMessage;
use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
use massa_hash::Hash;
use massa_models::SerializeCompact;
use massa_models::{
    constants::BOOTSTRAP_RANDOMNESS_SIZE_BYTES, with_serialization_context, DeserializeCompact,
    DeserializeMinBEInt, SerializeMinBEInt,
};
use massa_signature::{verify_signature, PublicKey, Signature, SIGNATURE_SIZE_BYTES};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

/// Bootstrap client binder
pub struct BootstrapClientBinder {
    max_bootstrap_message_size: u32,
    size_field_len: usize,
    remote_pubkey: PublicKey,
    duplex: Duplex,
    prev_sig: Option<Signature>,
    sended_message: Option<BootstrapMessage>,
}

impl BootstrapClientBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * duplex: duplex stream.
    pub fn new(duplex: Duplex, remote_pubkey: PublicKey) -> Self {
        let max_bootstrap_message_size =
            with_serialization_context(|context| context.max_bootstrap_message_size);
        let size_field_len = u32::be_bytes_min_length(max_bootstrap_message_size);
        BootstrapClientBinder {
            max_bootstrap_message_size,
            size_field_len,
            remote_pubkey,
            duplex,
            prev_sig: None,
            sended_message: None,
        }
    }

    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    pub async fn handshake(&mut self) -> Result<(), BootstrapError> {
        // send randomness and their hash
        let rand_hash = {
            let mut random_bytes = [0u8; BOOTSTRAP_RANDOMNESS_SIZE_BYTES];
            StdRng::from_entropy().fill_bytes(&mut random_bytes);
            self.duplex.write_all(&random_bytes).await?;
            let rand_hash = Hash::compute_from(&random_bytes);
            self.duplex.write_all(&rand_hash.to_bytes()).await?;
            rand_hash
        };

        // read and check response signature
        let sig = {
            let mut sig_bytes = [0u8; SIGNATURE_SIZE_BYTES];
            self.duplex.read_exact(&mut sig_bytes).await?;
            let sig = Signature::from_bytes(&sig_bytes)?;
            verify_signature(&rand_hash, &sig, &self.remote_pubkey)?;
            sig
        };

        // save prev sig
        self.prev_sig = Some(sig);

        Ok(())
    }

    /// Reads the next message. NOT cancel-safe
    pub async fn next(&mut self) -> Result<BootstrapMessage, BootstrapError> {
        // read signature
        let sig = {
            let mut sig_bytes = [0u8; SIGNATURE_SIZE_BYTES];
            self.duplex.read_exact(&mut sig_bytes).await?;
            Signature::from_bytes(&sig_bytes)?
        };

        // read message length
        let msg_len = {
            let mut meg_len_bytes = vec![0u8; self.size_field_len];
            self.duplex.read_exact(&mut meg_len_bytes[..]).await?;
            u32::from_be_bytes_min(&meg_len_bytes, self.max_bootstrap_message_size)?.0
        };

        // read message, check signature and, optionally, signature of the message we sent just before and deserialize
        let message = {
            let mut sig_msg_bytes = vec![0u8; SIGNATURE_SIZE_BYTES + (msg_len as usize)];
            if let Some(sended_message) = &self.sended_message {
                let old_message_sig_from_server = {
                    let mut sig_bytes = [0u8; SIGNATURE_SIZE_BYTES];
                    self.duplex.read_exact(&mut sig_bytes).await?;
                    Signature::from_bytes(&sig_bytes)?
                };
                // Check if old signature matches
                {
                    let old_message = &sended_message.to_bytes_compact()?;
                    let mut old_sig_msg_bytes =
                        vec![0u8; SIGNATURE_SIZE_BYTES + (old_message.len())];
                    old_sig_msg_bytes[..SIGNATURE_SIZE_BYTES]
                        .clone_from_slice(&self.prev_sig.unwrap().to_bytes());
                    old_sig_msg_bytes[SIGNATURE_SIZE_BYTES..].clone_from_slice(old_message);
                    let old_msg_hash = Hash::compute_from(&old_sig_msg_bytes);
                    verify_signature(
                        &old_msg_hash,
                        &old_message_sig_from_server,
                        &self.remote_pubkey,
                    )?;
                };
                sig_msg_bytes[..SIGNATURE_SIZE_BYTES]
                    .clone_from_slice(&old_message_sig_from_server.to_bytes());
            } else {
                sig_msg_bytes[..SIGNATURE_SIZE_BYTES]
                    .clone_from_slice(&self.prev_sig.unwrap().to_bytes());
            }
            self.duplex
                .read_exact(&mut sig_msg_bytes[SIGNATURE_SIZE_BYTES..])
                .await?;
            let msg_hash = Hash::compute_from(&sig_msg_bytes);
            verify_signature(&msg_hash, &sig, &self.remote_pubkey)?;
            let (msg, _len) =
                BootstrapMessage::from_bytes_compact(&sig_msg_bytes[SIGNATURE_SIZE_BYTES..])?;
            msg
        };

        // save prev sig
        self.prev_sig = Some(sig);
        self.sended_message = None;

        Ok(message)
    }

    #[allow(dead_code)]
    /// Send a message to the bootstrap server
    pub async fn send(&mut self, msg: BootstrapMessage) -> Result<(), BootstrapError> {
        let msg_bytes = msg.to_bytes_compact()?;
        let msg_len: u32 = msg_bytes.len().try_into().map_err(|e| {
            BootstrapError::GeneralError(format!("bootstrap message too large to encode: {}", e))
        })?;
        if let Some(prev_sig) = self.prev_sig {
            self.duplex.write_all(&prev_sig.to_bytes()).await?;
        }
        // send message length
        {
            let msg_len_bytes = msg_len.to_be_bytes_min(self.max_bootstrap_message_size)?;
            self.duplex.write_all(&msg_len_bytes).await?;
        }

        // send message
        self.duplex.write_all(&msg_bytes).await?;
        self.sended_message = Some(msg);
        Ok(())
    }
}
