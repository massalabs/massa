use crate::error::BootstrapError;
use async_trait::async_trait;
use massa_models::Version;

#[async_trait]
pub trait Binder<MessageType> {
    async fn handshake(&mut self, version: Version) -> Result<(), BootstrapError>;
    async fn next(&mut self) -> Result<MessageType, BootstrapError>;
    async fn send(&mut self, msg: MessageType) -> Result<(), BootstrapError>;
}
