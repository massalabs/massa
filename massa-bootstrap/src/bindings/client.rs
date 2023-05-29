// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::bindings::BindingReadExact;
use crate::error::BootstrapError;
use crate::messages::{
    BootstrapClientMessage, BootstrapClientMessageSerializer, BootstrapServerMessage,
    BootstrapServerMessageDeserializer,
};
use crate::settings::BootstrapClientConfig;
use massa_hash::Hash;
use massa_models::config::{
    MAX_BOOTSTRAP_MESSAGE_SIZE, MAX_BOOTSTRAP_MESSAGE_SIZE_BYTES, SIGNATURE_DESER_SIZE,
};
use massa_models::serialization::{DeserializeMinBEInt, SerializeMinBEInt};
use massa_models::version::{Version, VersionSerializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::{PublicKey, Signature};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use std::time::Instant;
use std::{io::Write, net::TcpStream, time::Duration};

/// Bootstrap client binder
pub struct BootstrapClientBinder {
    remote_pubkey: PublicKey,
    duplex: TcpStream,
    prev_message: Option<Hash>,
    version_serializer: VersionSerializer,
    cfg: BootstrapClientConfig,
}

const KNOWN_PREFIX_LEN: usize = SIGNATURE_DESER_SIZE + MAX_BOOTSTRAP_MESSAGE_SIZE_BYTES;
/// The known-length component of a message to be received.
struct ServerMessageLeader {
    sig: Signature,
    msg_len: u32,
}

impl BootstrapClientBinder {
    /// Creates a new `WriteBinder`.
    ///
    /// # Argument
    /// * duplex: duplex stream.
    /// * limit: limit max bytes per second (up and down)
    #[allow(clippy::too_many_arguments)]
    pub fn new(duplex: TcpStream, remote_pubkey: PublicKey, cfg: BootstrapClientConfig) -> Self {
        BootstrapClientBinder {
            remote_pubkey,
            duplex,
            prev_message: None,
            version_serializer: VersionSerializer::new(),
            cfg,
        }
    }

    /// Performs a handshake. Should be called after connection
    /// NOT cancel-safe
    pub fn handshake(&mut self, version: Version) -> Result<(), BootstrapError> {
        // send version and randomn bytes
        let msg_hash = {
            let mut version_ser = Vec::new();
            self.version_serializer
                .serialize(&version, &mut version_ser)?;
            let mut version_random_bytes =
                vec![0u8; version_ser.len() + self.cfg.randomness_size_bytes];
            version_random_bytes[..version_ser.len()].clone_from_slice(&version_ser);
            StdRng::from_entropy().fill_bytes(&mut version_random_bytes[version_ser.len()..]);
            self.duplex.write_all(&version_random_bytes)?;
            Hash::compute_from(&version_random_bytes)
        };

        self.prev_message = Some(msg_hash);

        Ok(())
    }

    /// Reads the next message.
    pub fn next_timeout(
        &mut self,
        duration: Option<Duration>,
    ) -> Result<BootstrapServerMessage, BootstrapError> {
        let deadline = duration.map(|d| Instant::now() + d);

        // read the known-len component of the message
        let mut known_len_buff = [0u8; KNOWN_PREFIX_LEN];
        // TODO: handle a partial read
        self.read_exact_timeout(&mut known_len_buff, deadline)
            .map_err(|(err, _consumed)| err)?;

        let ServerMessageLeader { sig, msg_len } = self.decode_msg_leader(&known_len_buff)?;

        // Update this bindings "most recently received" message hash, retaining the replaced value
        let message_deserializer = BootstrapServerMessageDeserializer::new((&self.cfg).into());
        let prev_msg = self
            .prev_message
            .replace(Hash::compute_from(&sig.to_bytes()));

        let message = {
            if let Some(prev_msg) = prev_msg {
                // Consume the rest of the message from the stream
                let mut stream_bytes = vec![0u8; msg_len as usize];

                // TODO: handle a partial read
                self.read_exact_timeout(&mut stream_bytes[..], deadline)
                    .map_err(|(e, _consumed)| e)?;
                let msg_bytes = &mut stream_bytes[..];

                // prepend the received message with the previous messages hash, and derive the new hash.
                // TODO: some sort of recovery if this fails?
                let rehash_seed = &[prev_msg.to_bytes().as_slice(), msg_bytes].concat();
                let msg_hash = Hash::compute_from(rehash_seed);
                self.remote_pubkey.verify_signature(&msg_hash, &sig)?;

                // ...And deserialize
                let (_, msg) = message_deserializer
                    .deserialize::<DeserializeError>(msg_bytes)
                    .map_err(|err| BootstrapError::DeserializeError(format!("{}", err)))?;
                msg
            } else {
                // Consume the rest of the message from the stream
                let mut stream_bytes = vec![0u8; msg_len as usize];

                // TODO: handle a partial read
                self.read_exact_timeout(&mut stream_bytes[..], deadline)
                    .map_err(|(e, _)| e)?;
                let sig_msg_bytes = &mut stream_bytes[..];

                // Compute the hash and verify
                let msg_hash = Hash::compute_from(sig_msg_bytes);
                self.remote_pubkey.verify_signature(&msg_hash, &sig)?;

                // ...And deserialize
                let (_, msg) = message_deserializer
                    .deserialize::<DeserializeError>(sig_msg_bytes)
                    .map_err(|err| BootstrapError::DeserializeError(format!("{}", err)))?;
                msg
            }
        };
        Ok(message)
    }

    // TODO: use a proper (de)serializer: https://github.com/massalabs/massa/pull/3745#discussion_r1169733161
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

        let mut write_buf = Vec::new();
        if let Some(prev_message) = self.prev_message {
            // there was a previous message
            let prev_message = prev_message.to_bytes();

            // update current previous message to be hash(prev_msg_hash + msg)
            let mut hash_data =
                Vec::with_capacity(prev_message.len().saturating_add(msg_bytes.len()));
            hash_data.extend(prev_message);
            hash_data.extend(&msg_bytes);
            self.prev_message = Some(Hash::compute_from(&hash_data));

            // Provide the signature saved as the previous message
            write_buf.extend(prev_message);
        } else {
            // No previous message, so we set the hash-chain genesis to the hash of the first msg
            self.prev_message = Some(Hash::compute_from(&msg_bytes));
        }

        // Provide the message length
        let msg_len_bytes = msg_len.to_be_bytes_min(MAX_BOOTSTRAP_MESSAGE_SIZE)?;
        write_buf.extend(&msg_len_bytes);

        // Provide the message
        write_buf.extend(&msg_bytes);

        let mut send_timeout = |timeout: Duration| -> Result<(), BootstrapError> {
            let start_time = std::time::Instant::now();
            self.duplex.set_write_timeout(Some(timeout))?;
            let mut total_bytes_written = 0;
            let chunk_size = 1024;

            while total_bytes_written < write_buf.len() {
                if start_time.elapsed() >= timeout {
                    return Err(BootstrapError::TimedOut(
                        std::io::ErrorKind::TimedOut.into(),
                    ));
                }

                let end = (total_bytes_written + chunk_size).min(write_buf.len());
                match self.duplex.write(&write_buf[total_bytes_written..end]) {
                    Ok(bytes_written) => {
                        total_bytes_written += bytes_written;
                    }
                    Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        // Timeout exceeded
                        if start_time.elapsed() >= timeout {
                            return Err(BootstrapError::TimedOut(
                                std::io::ErrorKind::TimedOut.into(),
                            ));
                        }
                    }
                    Err(err) => {
                        return Err(BootstrapError::IoError(err));
                    }
                }
            }

            Ok(())
        };

        if let Some(timeout) = duration {
            let to_return = send_timeout(timeout);
            // Reset the timeout to None
            self.duplex.set_write_timeout(None)?;
            to_return
        } else {
            self.duplex
                .write_all(&write_buf)
                .map_err(BootstrapError::IoError)
        }
    }

    /// We are using this instead of of our library deserializer as the process is relatively straight forward
    /// and makes error-type management cleaner
    fn decode_msg_leader(
        &self,
        leader_buff: &[u8; SIGNATURE_DESER_SIZE + MAX_BOOTSTRAP_MESSAGE_SIZE_BYTES],
    ) -> Result<ServerMessageLeader, BootstrapError> {
        let sig = Signature::from_bytes(leader_buff)?;

        // construct the message len from the leader-bufff
        let msg_len = u32::from_be_bytes_min(
            &leader_buff[SIGNATURE_DESER_SIZE..],
            MAX_BOOTSTRAP_MESSAGE_SIZE,
        )?
        .0;
        Ok(ServerMessageLeader { sig, msg_len })
    }
}

impl crate::bindings::BindingReadExact for BootstrapClientBinder {
    fn set_read_timeout(&mut self, duration: Option<Duration>) -> Result<(), std::io::Error> {
        self.duplex.set_read_timeout(duration)
    }
}

impl std::io::Read for BootstrapClientBinder {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.duplex.read(buf)
    }
}
