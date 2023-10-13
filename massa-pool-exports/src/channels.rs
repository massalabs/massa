use massa_execution_exports::ExecutionController;
use massa_models::{endorsement::SecureShareEndorsement, operation::SecureShareOperation};
use massa_pos_exports::SelectorController;

/// channels used by the pool worker
#[derive(Clone)]
pub struct PoolChannels {
    /// Communication with the execution module
    pub execution_controller: Box<dyn ExecutionController>,
    /// Selector to get draws
    pub selector: Box<dyn SelectorController>,
    /// Broadcasts used by the pool worker to send new operations and endorsements
    pub broadcasts: PoolBroadcasts,
}

/// Broadcasts used by the pool worker to send new operations and endorsements
#[derive(Clone)]
pub struct PoolBroadcasts {
    /// Broadcast channel for new endorsements
    pub endorsement_sender: tokio::sync::broadcast::Sender<SecureShareEndorsement>,
    /// Broadcast channel for new operations
    pub operation_sender: tokio::sync::broadcast::Sender<SecureShareOperation>,
}
