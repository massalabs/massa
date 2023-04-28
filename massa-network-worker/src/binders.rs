// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! `Flexbuffer` layer between raw data and our objects.
use crate::messages::{MessageDeserializer, MessageSerializer};

use super::messages::Message;
use massa_models::{
    error::ModelsError,
    serialization::{u32_be_bytes_min_length, DeserializeMinBEInt, SerializeMinBEInt},
};
use massa_network_exports::{NetworkError, ReadHalf, WriteHalf};
use massa_serialization::Serializer;
use massa_serialization::{DeserializeError, Deserializer};
use std::convert::TryInto;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

/// Used to serialize and send data.
pub struct WriteBinder {
    pub(crate) write_half: WriteHalf,
    message_index: u64,
    max_message_size: u32,
}

impl WriteBinder {
    /// Creates a new `WriteBinder` with a bandwidth `limit` in bytes per second.
    ///
    /// # Argument
    /// * `write_half`: writer half.
    /// * `limit`: limit max bytes per second write
    pub fn new(write_half: WriteHalf, _limit: f64, max_message_size: u32) -> Self {
        WriteBinder {
            write_half,
            message_index: 0,
            max_message_size,
        }
    }

    /// Sends a serialized message.
    ///
    /// # Argument
    /// * `buf`: data to transmit.
    pub async fn send(&mut self, msg: &Message) -> Result<u64, NetworkError> {
        //        massa_trace!("binder.send", { "msg": msg });
        let mut buf = Vec::new();
        MessageSerializer::new().serialize(msg, &mut buf)?;
        let msg_size: u32 = buf
            .len()
            .try_into()
            .map_err(|_| NetworkError::GeneralProtocolError("message too long".into()))?;
        self.write_half
            .write_all(&msg_size.to_be_bytes_min(self.max_message_size)?[..])
            .await?;

        // send message
        self.write_half.write_all(&buf).await?;

        let res_index = self.message_index;
        self.message_index += 1;
        //        massa_trace!("binder.send END", { "index": res_index });
        Ok(res_index)
    }
}

/// Used to receive and deserialize data.
pub struct ReadBinder {
    read_half: ReadHalf,
    message_index: u64,
    buf: Vec<u8>,
    cursor: usize,
    msg_size: Option<u32>,
    max_message_size: u32,
    message_deserializer: MessageDeserializer,
}

impl ReadBinder {
    /// Creates a new `ReadBinder` with a bandwidth `limit` in bytes per second.
    ///
    /// # Argument
    /// * `read_half`: reader half.
    /// * `limit`: limit max bytes per second read.
    pub fn new(
        read_half: ReadHalf,
        _limit: f64,
        max_message_size: u32,
        message_deserializer: MessageDeserializer,
    ) -> Self {
        ReadBinder {
            read_half,
            message_index: 0,
            buf: Vec::new(),
            cursor: 0,
            msg_size: None,
            max_message_size,
            message_deserializer,
        }
    }

    /// Awaits the next incoming message and deserializes it. Asynchronous cancel-safe.
    /// Returns the message, as well as the serialized object in the case of a block.
    ///
    /// This function must be asynchronous cancel-safe.
    /// This means that the function can restart from the beginning at any "await" point and we need to avoid losing any data,
    /// and we can only use the cancel-safe read() function
    /// `https://doc.rust-lang.org/std/io/trait.Read.html#tymethod.read`
    /// that returns an arbitrary number of bytes that were received that is `> 0 and <= buffer.len()`,
    /// or = 0 if there is no more data.
    /// We can't use `read_exact` and similar because they are not cancel-safe:
    /// `https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html#cancel-safety-2`
    pub async fn next(&mut self) -> Result<Option<(u64, Message)>, NetworkError> {
        // check if we are in the process of reading the message length
        if self.msg_size.is_none() {
            // pre-allocate the buffer to fit the encoded message size if the buffer is not already allocated
            let size_field_len = u32_be_bytes_min_length(self.max_message_size);
            if self.buf.len() != size_field_len {
                self.buf = vec![0u8; size_field_len];
            }

            // Try to read the full message size field
            // The self.cursor variable indicates how many bytes of the "message size" field we have received so far.
            // We need to keep all states (buffer and cursor) to ensure that if the function restarts at the read's await,
            // the state will remain consistent and resume the readout smoothly.
            while self.cursor < size_field_len {
                match self.read_half.read(&mut self.buf[self.cursor..]).await {
                    Ok(nr) => {
                        if nr == 0 {
                            return Ok(None);
                        }
                        self.cursor += nr;
                    }
                    Err(err) => {
                        if err.kind() == std::io::ErrorKind::UnexpectedEof {
                            return Ok(None);
                        } else {
                            return Err(err.into());
                        }
                    }
                }
            }

            // once we have all the message size bytes, deserialize it
            let res_size = u32::from_be_bytes_min(&self.buf, self.max_message_size)?.0;
            // set self.msg_size to indicate that we are now in the process of reading the message contents (and not the size anymore).
            self.msg_size = Some(res_size);
            // allocate the buffer to match the message length
            if self.buf.len() != (res_size as usize) {
                self.buf = vec![0u8; res_size as usize];
            }
            // reset the cursor so that it now represents how many content bytes have been read so far
            self.cursor = 0;
        }

        // read message in the same cancel-safe way as msg_size above
        while self.cursor < self.msg_size.unwrap() as usize {
            // does not panic
            match self.read_half.read(&mut self.buf[self.cursor..]).await {
                Ok(nr) => {
                    if nr == 0 {
                        return Ok(None);
                    }
                    self.cursor += nr;
                }
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::UnexpectedEof {
                        return Ok(None);
                    } else {
                        return Err(err.into());
                    }
                }
            }
        }
        let (_, res_msg) = self
            .message_deserializer
            .deserialize::<DeserializeError>(&self.buf)
            .map_err(|err| {
                warn!("error deserializing message: {:?}", err);
                NetworkError::ModelsError(ModelsError::DeserializeError(err.to_string()))
            })?;

        // now the message readout is over, we reset the state to start reading the next message's size field again at the next run
        self.cursor = 0;
        self.msg_size = None;

        // clear the buffer to not leave dangling data around (note that clear() doesn't deallocate)
        self.buf.clear();

        // update sequence numbers and return the deserialized message
        let res_index = self.message_index;
        self.message_index += 1;
        Ok(Some((res_index, res_msg)))
    }
}
