use crate::CommunicationError;

use super::messages::Message;
use futures::SinkExt;
use serde::{Deserialize, Serialize};

use std::marker::Unpin;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

pub struct WriteBinder<T: AsyncWrite + Unpin> {
    framed_writer: FramedWrite<T, LengthDelimitedCodec>,
    message_index: u64,
}

impl<T: AsyncWrite + Unpin> WriteBinder<T> {
    pub fn new(writer: T) -> Self {
        WriteBinder {
            framed_writer: FramedWrite::new(writer, LengthDelimitedCodec::new()),
            message_index: 0,
        }
    }

    pub async fn send(&mut self, msg: &Message) -> Result<u64, CommunicationError> {
        let mut serializer = flexbuffers::FlexbufferSerializer::new();
        msg.serialize(&mut serializer)?;
        self.framed_writer
            .send(serializer.take_buffer().into())
            .await?;
        let res_index = self.message_index;
        self.message_index += 1;
        Ok(res_index)
    }
}

pub struct ReadBinder<T: AsyncRead + Unpin> {
    framed_reader: FramedRead<T, LengthDelimitedCodec>,
    message_index: u64,
}

impl<T: AsyncRead + Unpin> ReadBinder<T> {
    pub fn new(reader: T) -> Self {
        ReadBinder {
            framed_reader: FramedRead::new(reader, LengthDelimitedCodec::new()),
            message_index: 0,
        }
    }

    pub async fn next(&mut self) -> Result<Option<(u64, Message)>, CommunicationError> {
        let buf: Vec<u8> = match self.framed_reader.next().await {
            Some(b) => b?.into_iter().collect(),
            None => return Ok(None),
        };
        let res_msg = Message::deserialize(flexbuffers::Reader::get_root(&buf)?)?;
        let res_index = self.message_index;
        self.message_index += 1;
        Ok(Some((res_index, res_msg)))
    }
}
