//! Flexbuffer layer between raw data and our objects.
use super::messages::Message;
use crate::error::CommunicationError;
use crate::network::{ReadHalf, WriteHalf};
use models::{
    DeserializeCompact, DeserializeMinBEInt, SerializationContext, SerializeCompact,
    SerializeMinBEInt,
};
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
    pub async fn send(&mut self, msg: &Message) -> Result<u64, CommunicationError> {
        //        massa_trace!("binder.send", { "msg": msg });
        // serialize
        let bytes_vec = msg.to_bytes_compact(&self.serialization_context)?;
        let msg_size: u32 = bytes_vec
            .len()
            .try_into()
            .map_err(|_| CommunicationError::GeneralProtocolError("messsage too long".into()))?;

        // send length
        self.write_half
            .write_all(&msg_size.to_be_bytes_min(self.serialization_context.max_message_size)?[..])
            .await?;

        // send message
        self.write_half.write_all(&bytes_vec[..]).await?;

        let res_index = self.message_index;
        self.message_index += 1;
        //        massa_trace!("binder.send end", { "index": res_index });
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
    pub async fn next(&mut self) -> Result<Option<(u64, Message)>, CommunicationError> {
        //        massa_trace!("binder.next start", {});
        // read message length
        let msg_len_len = u32::be_bytes_min_length(self.serialization_context.max_message_size);
        let mut msg_len_buf = vec![0u8; msg_len_len];
        if let Err(err) = self.read_half.read_exact(&mut msg_len_buf).await {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(None);
            } else {
                return Err(err.into());
            }
        }
        let (msg_len, _) =
            u32::from_be_bytes_min(&msg_len_buf, self.serialization_context.max_message_size)?;

        // read message
        let mut msg_buffer = vec![0u8; msg_len as usize];
        self.read_half.read_exact(&mut msg_buffer).await?;
        let (res_msg, _res_msg_len) =
            Message::from_bytes_compact(&msg_buffer, &self.serialization_context)?;
        // note: it is possible that _res_msg_len < msg_len if some extras have not been read

        let res_index = self.message_index;
        self.message_index += 1;
        //        massa_trace!("binder.next", { "msg": res_msg });
        Ok(Some((res_index, res_msg)))
    }
}
