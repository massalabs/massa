use async_trait::async_trait;
use std::io;
use std::net::SocketAddr;
use time::UTime;
use tokio::io::{AsyncRead, AsyncWrite};

/// The establisher establishes connections.
#[async_trait]
pub trait Establisher
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type ReaderT: AsyncRead + Send + Sync + Unpin + std::fmt::Debug;
    type WriterT: AsyncWrite + Send + Sync + Unpin + std::fmt::Debug;
    type ListenerT: Listener<Self::ReaderT, Self::WriterT>;
    type ConnectorT: Connector<Self::ReaderT, Self::WriterT>;

    async fn get_listener(&mut self, addr: SocketAddr) -> io::Result<Self::ListenerT>;
    async fn get_connector(&mut self, timeout_duration: UTime) -> io::Result<Self::ConnectorT>;
}

/// Listens for connections.
#[async_trait]
pub trait Listener<ReaderT, WriterT>
where
    ReaderT: AsyncRead + Send + Sync + Unpin + std::fmt::Debug,
    WriterT: AsyncWrite + Send + Sync + Unpin + std::fmt::Debug,
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    async fn accept(&mut self) -> io::Result<(ReaderT, WriterT, SocketAddr)>;
}

/// Manages connection timeouts.
#[async_trait]
pub trait Connector<ReaderT, WriterT>
where
    ReaderT: AsyncRead + Send + Sync + Unpin + std::fmt::Debug,
    WriterT: AsyncWrite + Send + Sync + Unpin + std::fmt::Debug,
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    async fn connect(&mut self, addr: SocketAddr) -> io::Result<(ReaderT, WriterT)>;
}
