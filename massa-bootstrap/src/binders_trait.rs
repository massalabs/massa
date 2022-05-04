use crate::error::BootstrapError;
use massa_models::Version;
use async_trait::async_trait;

#[async_trait]
pub trait Binder<MessageType> {
    async fn handshake(&mut self, version: Version) -> Result<(), BootstrapError>;
    async fn next(&mut self) -> Result<MessageType, BootstrapError>;
    async fn send(&mut self, msg: MessageType) -> Result<(), BootstrapError>;
}