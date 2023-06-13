use massa_execution_exports::ExecutionController;
use massa_models::{endorsement::SecureShareEndorsement, operation::SecureShareOperation};
use massa_pos_exports::SelectorController;

/// channels used by the pool worker
#[derive(Clone)]
pub struct PoolChannels {
    /// Communication with the execution module
    pub execution_controller: Box<dyn ExecutionController>,
    /// Broadcast channel for new endorsements
    pub endorsement_sender: tokio::sync::broadcast::Sender<SecureShareEndorsement>,
    /// Broadcast channel for new operations
    pub operation_sender: tokio::sync::broadcast::Sender<SecureShareOperation>,
    /// Selector to get draws
    pub selector: Box<dyn SelectorController>,
}
