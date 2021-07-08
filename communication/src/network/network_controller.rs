use crate::CommunicationError;

use super::establisher::Establisher;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Default, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConnectionId(pub u64);

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Debug for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ConnectionClosureReason {
    Normal,
    Failed,
    Banned,
}

#[derive(Debug)]
pub enum NetworkEvent<ReaderT, WriterT>
where
    ReaderT: AsyncRead + Send + Sync + Unpin + std::fmt::Debug,
    WriterT: AsyncWrite + Send + Sync + Unpin + std::fmt::Debug,
{
    NewConnection((ConnectionId, ReaderT, WriterT)),
    ConnectionBanned(ConnectionId),
}

#[async_trait]
pub trait NetworkController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type EstablisherT: Establisher + Send + Sync + Unpin + std::fmt::Debug;
    type ReaderT: AsyncRead + Send + Sync + Unpin + std::fmt::Debug;
    type WriterT: AsyncWrite + Send + Sync + Unpin + std::fmt::Debug;
    async fn stop(mut self) -> Result<(), CommunicationError>;
    async fn wait_event(
        &mut self,
    ) -> Result<NetworkEvent<Self::ReaderT, Self::WriterT>, CommunicationError>;
    async fn merge_advertised_peer_list(
        &mut self,
        ips: Vec<IpAddr>,
    ) -> Result<(), CommunicationError>;
    async fn get_advertisable_peer_list(&mut self) -> Result<Vec<IpAddr>, CommunicationError>;
    async fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), CommunicationError>;
    async fn connection_alive(&mut self, id: ConnectionId) -> Result<(), CommunicationError>;
}
