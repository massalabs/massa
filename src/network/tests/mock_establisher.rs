use super::super::establisher::*;
use async_trait::async_trait;
use std::net::{IpAddr, SocketAddr};
use tokio::io::{duplex, DuplexStream};
use tokio::time::Duration;

type ReadHalf = tokio::io::ReadHalf<DuplexStream>;
type WriteHalf = tokio::io::WriteHalf<DuplexStream>;

#[derive(Debug)]
pub struct MockListener();

#[async_trait]
impl Listener<ReadHalf, WriteHalf> for MockListener {
    async fn accept(&mut self) -> std::io::Result<(ReadHalf, WriteHalf, SocketAddr)> {
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "blabla")) //TODO
    }
}

#[derive(Debug)]
pub struct MockConnector();

#[async_trait]
impl Connector<ReadHalf, WriteHalf> for MockConnector {
    async fn connect(&mut self, addr: SocketAddr) -> std::io::Result<(ReadHalf, WriteHalf)> {
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "blabla")) //TODO
    }
}

#[derive(Debug)]
pub struct MockEstablisher;

#[async_trait]
impl Establisher for MockEstablisher {
    type ReaderT = ReadHalf;
    type WriterT = WriteHalf;
    type ListenerT = MockListener;
    type ConnectorT = MockConnector;

    async fn get_listener(&mut self, addr: SocketAddr) -> std::io::Result<Self::ListenerT> {
        Ok(MockListener())
    }

    async fn get_connector(
        &mut self,
        timeout_duration: Duration,
    ) -> std::io::Result<Self::ConnectorT> {
        Ok(MockConnector())
    }
}

impl MockEstablisher {
    pub fn new() -> Self {
        MockEstablisher {}
    }

    // establish connection from X to target
    /*
        - send message to mock listener with (SocketAddr)
        - listener establishes on accept and returns us a DuplexStream
    */

    // on target tried to establish connection to addr
    /*
        - read messages sent from connector (only contains SocketAddr)
        - reply with a Some(DuplexStream) if we want to establish, or None otherwise
    */
}
