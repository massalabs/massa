// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::BootstrapError;
use crate::establisher::types::Duplex;
use crate::messages::{
    BootstrapClientMessage, BootstrapClientMessageSerializer, BootstrapServerMessage,
    BootstrapServerMessageDeserializer,
};
use async_speed_limit::clock::StandardClock;
use async_speed_limit::{Limiter, Resource};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_models::serialization::{DeserializeMinBEInt, SerializeMinBEInt};
use massa_models::version::{Version, VersionSerializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::{PublicKey, Signature, SIGNATURE_SIZE_BYTES};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

/// Bootstrap client binder
pub struct BootstrapClientBinder {
    max_bootstrap_message_size: u32,
    size_field_len: usize,
    remote_pubkey: PublicKey,
    duplex: Resource<Duplex, StandardClock>,
    prev_message: Option<Hash>,
    version_serializer: VersionSerializer,
    endorsement_count: u32,
    max_advertise_length: u32,
    max_bootstrap_blocks: u32,
    max_operations_per_block: u32,
    thread_count: u8,
    randomness_size_bytes: usize,
    max_bootstrap_error_length: u64,
    max_bootstrap_final_state_parts_size: u64,
    max_datastore_entry_count: u64,
    max_datastore_key_length: u8,
    max_datastore_value_length: u64,
    max_async_pool_changes: u64,
    max_async_pool_length: u64,
    max_async_message_data: u64,
    max_function_name_length: u16,
    max_parameters_size: u32,
    max_ledger_changes_count: u64,
    max_op_datastore_entry_count: u64,
    max_op_datastore_key_length: u8,
    max_op_datastore_value_length: u64,
    max_changes_slot_count: u64,
    max_rolls_length: u64,
    max_production_stats_length: u64,
    max_credits_length: u64,
}

impl BootstrapClientBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * duplex: duplex stream.
    /// * limit: limit max bytes per second (up and down)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        duplex: Duplex,
        remote_pubkey: PublicKey,
        limit: f64,
        max_bootstrap_message_size: u32,
        endorsement_count: u32,
        max_advertise_length: u32,
        max_bootstrap_blocks: u32,
        max_operations_per_block: u32,
        thread_count: u8,
        randomness_size_bytes: usize,
        max_bootstrap_error_length: u64,
        max_bootstrap_final_state_parts_size: u64,
        max_datastore_entry_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_async_pool_changes: u64,
        max_async_pool_length: u64,
        max_async_message_data: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_ledger_changes_count: u64,
        max_op_datastore_entry_count: u64,
        max_op_datastore_key_length: u8,
        max_op_datastore_value_length: u64,
        max_changes_slot_count: u64,
        max_rolls_length: u64,
        max_production_stats_length: u64,
        max_credits_length: u64,
    ) -> Self {
        let size_field_len = u32::be_bytes_min_length(max_bootstrap_message_size);
        BootstrapClientBinder {
            max_bootstrap_message_size,
            size_field_len,
            remote_pubkey,
            duplex: <Limiter>::new(limit).limit(duplex),
            prev_message: None,
            version_serializer: VersionSerializer::new(),
            endorsement_count,
            max_advertise_length,
            max_bootstrap_blocks,
            max_operations_per_block,
            thread_count,
            randomness_size_bytes,
            max_bootstrap_error_length,
            max_bootstrap_final_state_parts_size,
            max_datastore_entry_count,
            max_datastore_key_length,
            max_datastore_value_length,
            max_async_pool_changes,
            max_async_pool_length,
            max_async_message_data,
            max_function_name_length,
            max_parameters_size,
            max_ledger_changes_count,
            max_op_datastore_entry_count,
            max_op_datastore_key_length,
            max_op_datastore_value_length,
            max_changes_slot_count,
            max_rolls_length,
            max_production_stats_length,
            max_credits_length,
        }
    }
}

impl BootstrapClientBinder {
    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    pub async fn handshake(&mut self, version: Version) -> Result<(), BootstrapError> {
        // send version and randomn bytes
        let msg_hash = {
            let mut version_ser = Vec::new();
            self.version_serializer
                .serialize(&version, &mut version_ser)?;
            let mut version_random_bytes =
                vec![0u8; version_ser.len() + self.randomness_size_bytes];
            version_random_bytes[..version_ser.len()].clone_from_slice(&version_ser);
            StdRng::from_entropy().fill_bytes(&mut version_random_bytes[version_ser.len()..]);
            self.duplex.write_all(&version_random_bytes).await?;
            Hash::compute_from(&version_random_bytes)
        };

        self.prev_message = Some(msg_hash);

        Ok(())
    }

    /// Reads the next message. NOT cancel-safe
    pub async fn next(&mut self) -> Result<BootstrapServerMessage, BootstrapError> {
        // read signature
        let sig = {
            let mut sig_bytes = [0u8; SIGNATURE_SIZE_BYTES];
            self.duplex.read_exact(&mut sig_bytes).await?;
            Signature::from_bytes(&sig_bytes)?
        };

        // read message length
        let msg_len = {
            let mut msg_len_bytes = vec![0u8; self.size_field_len];
            self.duplex.read_exact(&mut msg_len_bytes[..]).await?;
            u32::from_be_bytes_min(&msg_len_bytes, self.max_bootstrap_message_size)?.0
        };

        // read message, check signature and check signature of the message sent just before then deserialize it
        let message_deserializer = BootstrapServerMessageDeserializer::new(
            self.thread_count,
            self.endorsement_count,
            self.max_advertise_length,
            self.max_bootstrap_blocks,
            self.max_operations_per_block,
            self.max_bootstrap_final_state_parts_size,
            self.max_async_pool_changes,
            self.max_async_pool_length,
            self.max_async_message_data,
            self.max_ledger_changes_count,
            self.max_datastore_key_length,
            self.max_datastore_value_length,
            self.max_datastore_entry_count,
            self.max_function_name_length,
            self.max_parameters_size,
            self.max_bootstrap_error_length,
            self.max_op_datastore_entry_count,
            self.max_op_datastore_key_length,
            self.max_op_datastore_value_length,
            self.max_changes_slot_count,
            self.max_rolls_length,
            self.max_production_stats_length,
            self.max_credits_length,
        );
        let message = {
            if let Some(prev_message) = self.prev_message {
                self.prev_message = Some(Hash::compute_from(&sig.to_bytes()));
                let mut sig_msg_bytes = vec![0u8; HASH_SIZE_BYTES + (msg_len as usize)];
                sig_msg_bytes[..HASH_SIZE_BYTES].copy_from_slice(prev_message.to_bytes());
                self.duplex
                    .read_exact(&mut sig_msg_bytes[HASH_SIZE_BYTES..])
                    .await?;
                let msg_hash = Hash::compute_from(&sig_msg_bytes);
                self.remote_pubkey.verify_signature(&msg_hash, &sig)?;
                let (_, msg) = message_deserializer
                    .deserialize::<DeserializeError>(&sig_msg_bytes[HASH_SIZE_BYTES..])
                    .map_err(|err| BootstrapError::GeneralError(format!("{}", err)))?;
                msg
            } else {
                self.prev_message = Some(Hash::compute_from(&sig.to_bytes()));
                let mut sig_msg_bytes = vec![0u8; msg_len as usize];
                self.duplex.read_exact(&mut sig_msg_bytes[..]).await?;
                let msg_hash = Hash::compute_from(&sig_msg_bytes);
                self.remote_pubkey.verify_signature(&msg_hash, &sig)?;
                let (_, msg) = message_deserializer
                    .deserialize::<DeserializeError>(&sig_msg_bytes[..])
                    .map_err(|err| BootstrapError::GeneralError(format!("{}", err)))?;
                msg
            }
        };
        Ok(message)
    }

    #[allow(dead_code)]
    /// Send a message to the bootstrap server
    pub async fn send(&mut self, msg: &BootstrapClientMessage) -> Result<(), BootstrapError> {
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

            // send old previous message
            self.duplex.write_all(prev_message).await?;
        } else {
            // there was no previous message

            //update current previous message
            self.prev_message = Some(Hash::compute_from(&msg_bytes));
        }

        // send message length
        {
            let msg_len_bytes = msg_len.to_be_bytes_min(self.max_bootstrap_message_size)?;
            self.duplex.write_all(&msg_len_bytes).await?;
        }

        // send message
        self.duplex.write_all(&msg_bytes).await?;
        Ok(())
    }
}
