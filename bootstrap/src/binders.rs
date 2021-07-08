//! Flexbuffer layer between raw data and our objects.
use super::messages::BootstrapMessage;
use crate::error::BootstrapError;
use crate::establisher::{ReadHalf, WriteHalf};
use models::{DeserializeCompact, SerializationContext, SerializeCompact};
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
        }
    }

    /// Awaits the next incomming message and deserializes it.
    pub async fn next(&mut self) -> Result<Option<(u64, BootstrapMessage)>, BootstrapError> {
        // read message length
        let mut msg_len_buf = [0u8; 4];
        if let Err(err) = self.read_half.read_exact(&mut msg_len_buf).await {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(None);
            } else {
                return Err(err.into());
            }
        }
        let msg_len = u32::from_be_bytes(msg_len_buf);

        // read message
        let mut msg_buffer = vec![0u8; msg_len as usize];
        self.read_half.read_exact(&mut msg_buffer).await?;
        let (res_msg, _res_msg_len) =
            BootstrapMessage::from_bytes_compact(&msg_buffer, &self.serialization_context)?;
        // note: it is possible that _res_msg_len < msg_len if some extras have not been read

        let res_index = self.message_index;
        self.message_index += 1;
        Ok(Some((res_index, res_msg)))
    }
}
