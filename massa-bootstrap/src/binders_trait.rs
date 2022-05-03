use crate::{error::BootstrapError, messages::BootstrapMessage};
use massa_models::Version;
use async_trait::async_trait;

#[async_trait]
pub trait Binder {
    async fn handshake(&mut self, version: Version) -> Result<(), BootstrapError>;
    async fn next(&mut self) -> Result<BootstrapMessage, BootstrapError>;
    async fn send(&mut self, msg: BootstrapMessage) -> Result<(), BootstrapError>;
}