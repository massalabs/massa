//! Flexbuffer layer between raw data and our objects.
use super::messages::BootstrapMessage;
use crate::error::BootstrapError;
use crate::establisher::{ReadHalf, WriteHalf};
use models::{DeserializeCompact, DeserializeMinBEInt, SerializationContext, SerializeCompact};
use std::convert::TryInto;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Used to serialize and send data.
pub struct WriteBinder {
    serialization_context: SerializationContext,
    write_half: WriteHalf,
    message_index: u64,
}

impl WriteBinder {
    /// Creates a new WriteBinder.
    ///
    /// # Argument
    /// * write_half: writer half.
    /// * serialization_context: SerializationContext instance
    /// * max_message_size: max message size in bytes
    pub fn new(write_half: WriteHalf, serialization_context: SerializationContext) -> Self {
        WriteBinder {
            serialization_context,
            write_half,
            message_index: 0,
        }
    }

    /// Serializes and sends message.
    ///
    /// # Argument
    /// * msg: date to transmit.
    pub async fn send(&mut self, msg: &BootstrapMessage) -> Result<u64, BootstrapError> {
        // serialize
        let bytes_vec = msg.to_bytes_compact(&self.serialization_context)?;
        let msg_size: u32 = bytes_vec
            .len()
            .try_into()
            .map_err(|_| BootstrapError::GeneralBootstrapError("messsage too long".into()))?;

        // send length
        self.write_half
            .write_all(&msg_size.to_be_bytes()[..])
            .await?;

        // send message
        self.write_half.write_all(&bytes_vec[..]).await?;

        let res_index = self.message_index;
        self.message_index += 1;
        Ok(res_index)
    }
}

/// Used to receive and deserialize data.
pub struct ReadBinder {
    serialization_context: SerializationContext,
    read_half: ReadHalf,
    message_index: u64,
    buf: Vec<u8>,
    cursor: usize,
    msg_size: Option<u32>,
}

impl ReadBinder {
    /// Creates a new ReadBinder.
    ///
    /// # Argument
    /// * read_half: reader half.
    /// * serialization_context: SerializationContext instance
    /// * max_message_size: max message size in bytes
    pub fn new(read_half: ReadHalf, serialization_context: SerializationContext) -> Self {
        ReadBinder {
            serialization_context,
            read_half,
            message_index: 0,
            buf: Vec::new(),
            cursor: 0,
            msg_size: None,
        }
    }

    /// Awaits the next incomming message and deserializes it. Async cancel-safe.
    pub async fn next(&mut self) -> Result<Option<(u64, BootstrapMessage)>, BootstrapError> {
        // read message size
        if self.msg_size.is_none() {
            let size_field_len =
                u32::be_bytes_min_length(self.serialization_context.max_bootstrap_message_size);
            if self.buf.len() != size_field_len {
                self.buf = vec![0u8; size_field_len];
            }
            // read size
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
            let res_size = u32::from_be_bytes_min(
                &self.buf,
                self.serialization_context.max_bootstrap_message_size,
            )?
            .0;
            self.msg_size = Some(res_size);
            if self.buf.len() != (res_size as usize) {
                self.buf = vec![0u8; res_size as usize];
            }
            self.cursor = 0;
        }

        // read message
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
        let (res_msg, _res_msg_len) =
            BootstrapMessage::from_bytes_compact(&self.buf, &self.serialization_context)?;
        self.cursor = 0;
        self.msg_size = None;
        self.buf.clear();

        let res_index = self.message_index;
        self.message_index += 1;
        Ok(Some((res_index, res_msg)))
    }
}
