use async_trait::async_trait;
use communication::protocol::protocol_controller::ProtocolController;

#[derive(Clone, Debug)]
pub enum ConsensusEvent {}

#[async_trait]
pub trait ConsensusController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type ProtocolControllerT: ProtocolController;
    async fn wait_event(&mut self) -> ConsensusEvent;
    async fn generate_random_block(&self);
}
