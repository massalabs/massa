//! Flexbuffer layer between raw data and our objects.
use super::messages::Message;
use crate::error::{CommunicationError, FlexbufferError};
use crate::network::{ReadHalf, WriteHalf};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

/// Used to serialize and send data.
pub struct WriteBinder {
    framed_writer: FramedWrite<WriteHalf, LengthDelimitedCodec>,
    message_index: u64,
}

impl WriteBinder {
    /// Creates a new WriteBinder.
    ///
    /// # Argument
    /// * writer: inner part of the underlying FramedWrite.
    pub fn new(writer: WriteHalf) -> Self {
        WriteBinder {
            framed_writer: FramedWrite::new(writer, LengthDelimitedCodec::new()),
            message_index: 0,
        }
    }

    /// Serializes and sends message.
    ///
    /// # Argument
    /// * msg: date to transmit.
    pub async fn send(&mut self, msg: &Message) -> Result<u64, CommunicationError> {
        let mut serializer = flexbuffers::FlexbufferSerializer::new();
        msg.serialize(&mut serializer)
            .map_err(|err| FlexbufferError::from(err))?;
        self.framed_writer
            .send(serializer.take_buffer().into())
            .await?;
        let res_index = self.message_index;
        self.message_index += 1;
        Ok(res_index)
    }
}

/// Used to receive and deserialize date.
pub struct ReadBinder {
    framed_reader: FramedRead<ReadHalf, LengthDelimitedCodec>,
    message_index: u64,
}

impl ReadBinder {
    /// Creates a new ReadBinder.
    ///
    /// # Argument
    /// * reader: inner part of the underlying FramedRead.
    pub fn new(reader: ReadHalf) -> Self {
        ReadBinder {
            framed_reader: FramedRead::new(reader, LengthDelimitedCodec::new()),
            message_index: 0,
        }
    }

    /// Awaits the next incomming message and deserialize it.
    pub async fn next(&mut self) -> Result<Option<(u64, Message)>, CommunicationError> {
        let buf: Vec<u8> = match self.framed_reader.next().await {
            Some(b) => b?.into_iter().collect(),
            None => return Ok(None),
        };
        let res_msg = Message::deserialize(
            flexbuffers::Reader::get_root(&buf).map_err(|err| FlexbufferError::from(err))?,
        )
        .map_err(|err| FlexbufferError::from(err))?;
        let res_index = self.message_index;
        self.message_index += 1;
        Ok(Some((res_index, res_msg)))
    }
}
