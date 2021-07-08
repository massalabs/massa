use super::establisher::*;
use async_trait::async_trait;
use std::io;
use std::net::SocketAddr;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

#[derive(Debug)]
pub struct DefaultListener(TcpListener);

#[async_trait]
impl Listener<OwnedReadHalf, OwnedWriteHalf> for DefaultListener {
    async fn accept(&mut self) -> io::Result<(OwnedReadHalf, OwnedWriteHalf, SocketAddr)> {
        let (sock, remote_addr) = self.0.accept().await?;
        let (read_half, write_half) = sock.into_split();
        Ok((read_half, write_half, remote_addr))
    }
}

#[derive(Debug)]
pub struct DefaultConnector(Duration);

#[async_trait]
impl Connector<OwnedReadHalf, OwnedWriteHalf> for DefaultConnector {
    async fn connect(&mut self, addr: SocketAddr) -> io::Result<(OwnedReadHalf, OwnedWriteHalf)> {
        match timeout(self.0, TcpStream::connect(addr)).await {
            Ok(Ok(sock)) => {
                let (reader, writer) = sock.into_split();
                Ok((reader, writer))
            }
            Ok(Err(e)) => Err(e),
            Err(e) => Err(io::Error::new(io::ErrorKind::TimedOut, e)),
        }
    }
}

#[derive(Debug)]
pub struct DefaultEstablisher;

#[async_trait]
impl Establisher for DefaultEstablisher {
    type ReaderT = OwnedReadHalf;
    type WriterT = OwnedWriteHalf;
    type ListenerT = DefaultListener;
    type ConnectorT = DefaultConnector;

    async fn get_listener(&mut self, addr: SocketAddr) -> io::Result<Self::ListenerT> {
        Ok(DefaultListener(TcpListener::bind(addr).await?))
    }

    async fn get_connector(&mut self, timeout_duration: Duration) -> io::Result<Self::ConnectorT> {
        Ok(DefaultConnector(timeout_duration))
    }
}

impl DefaultEstablisher {
    pub fn new() -> Self {
        DefaultEstablisher {}
    }
}
