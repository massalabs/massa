use crate::network::PeerInfo;
use crate::CommunicationError;

use super::establisher::Establisher;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::IpAddr};
use tokio::io::{AsyncRead, AsyncWrite};

/// A unique connecion id for a node
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
    /// Connection was closed properly
    Normal,
    /// Connection failed for some reason
    Failed,
    /// Connection closed after node ban
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

/// Manages connection with one node.
#[async_trait]
pub trait NetworkController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type EstablisherT: Establisher + Send + Sync + Unpin + std::fmt::Debug;
    type ReaderT: AsyncRead + Send + Sync + Unpin + std::fmt::Debug;
    type WriterT: AsyncWrite + Send + Sync + Unpin + std::fmt::Debug;

    /// Used to close properly everything down.
    async fn stop(mut self) -> Result<(), CommunicationError>;

    /// Used to listen network events.
    async fn wait_event(
        &mut self,
    ) -> Result<NetworkEvent<Self::ReaderT, Self::WriterT>, CommunicationError>;

    /// Transmit the order to merge the peer list.
    ///
    /// # Argument
    /// ips : vec of advertized ips
    async fn merge_advertised_peer_list(
        &mut self,
        ips: Vec<IpAddr>,
    ) -> Result<(), CommunicationError>;

    /// Send the order to get advertisable peer list.
    async fn get_advertisable_peer_list(&mut self) -> Result<Vec<IpAddr>, CommunicationError>;

    /// Send the order to get peers.
    async fn get_peers(&mut self) -> Result<HashMap<IpAddr, PeerInfo>, CommunicationError>;

    /// Send the information that the connection has been closed for given reason.
    ///
    /// # Arguments
    /// * id : connenction id of the closed connection
    /// * reason : connection closure reason
    async fn connection_closed(
        &mut self,
        id: ConnectionId,
        reason: ConnectionClosureReason,
    ) -> Result<(), CommunicationError>;

    /// Send the information that the connection is alive.
    ///
    /// # Arguments
    /// * id : connenction id
    async fn connection_alive(&mut self, id: ConnectionId) -> Result<(), CommunicationError>;
}
