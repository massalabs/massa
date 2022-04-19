// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::messages::BootstrapMessage;
use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
use massa_hash::Hash;
use massa_models::{
    constants::BOOTSTRAP_RANDOMNESS_SIZE_BYTES, with_serialization_context, DeserializeCompact,
    DeserializeMinBEInt,
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

        // read message, check signature and deserialize
        let message = {
            let mut sig_msg_bytes = vec![0u8; SIGNATURE_SIZE_BYTES + (msg_len as usize)];
            sig_msg_bytes[..SIGNATURE_SIZE_BYTES]
                .clone_from_slice(&self.prev_sig.unwrap().to_bytes());
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

        Ok(message)
    }
}
