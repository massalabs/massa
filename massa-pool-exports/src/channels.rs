use massa_models::operation::Operation;

/// channels used by the pool worker
#[derive(Clone)]
pub struct PoolChannels {
    /// Broadcast sender(channel) for new operations
    pub operation_sender: tokio::sync::broadcast::Sender<Operation>,
}
