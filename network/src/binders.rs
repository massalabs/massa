// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! Flexbuffer layer between raw data and our objects.
use super::messages::Message;
use crate::error::NetworkError;
use crate::establisher::{ReadHalf, WriteHalf};
use models::{
    with_serialization_context, DeserializeCompact, DeserializeMinBEInt, SerializeCompact,
    SerializeMinBEInt,
};
use std::convert::TryInto;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Used to serialize and send data.
pub struct WriteBinder {
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
    pub fn new(write_half: WriteHalf) -> Self {
        WriteBinder {
            write_half,
            message_index: 0,
        }
    }

    /// Serializes and sends message.
    ///
    /// # Argument
    /// * msg: date to transmit.
    pub async fn send(&mut self, msg: &Message) -> Result<u64, NetworkError> {
        //        massa_trace!("binder.send", { "msg": msg });
        // serialize
        let bytes_vec = msg.to_bytes_compact()?;
        let msg_size: u32 = bytes_vec
            .len()
            .try_into()
            .map_err(|_| NetworkError::GeneralProtocolError("message too long".into()))?;

        // send length
        let max_message_size = with_serialization_context(|context| context.max_message_size);
        self.write_half
            .write_all(&msg_size.to_be_bytes_min(max_message_size)?[..])
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
    pub fn new(read_half: ReadHalf) -> Self {
        ReadBinder {
            read_half,
            message_index: 0,
            buf: Vec::new(),
            cursor: 0,
            msg_size: None,
        }
    }

    /// Awaits the next incoming message and deserializes it. Async cancel-safe.
    pub async fn next(&mut self) -> Result<Option<(u64, Message)>, NetworkError> {
        let max_message_size = with_serialization_context(|context| context.max_message_size);

        // read message size
        if self.msg_size.is_none() {
            let size_field_len = u32::be_bytes_min_length(max_message_size);
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
            let res_size = u32::from_be_bytes_min(&self.buf, max_message_size)?.0;
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
        let (res_msg, _res_msg_len) = Message::from_bytes_compact(&self.buf)?;
        self.cursor = 0;
        self.msg_size = None;
        self.buf.clear();

        let res_index = self.message_index;
        self.message_index += 1;
        Ok(Some((res_index, res_msg)))
    }
}
