use massa_execution_exports::ExecutionController;
use massa_models::{endorsement::SecureShareEndorsement, operation::SecureShareOperation};
use massa_pos_exports::SelectorController;
use massa_versioning::versioning::MipStore;

/// channels used by the pool worker
#[derive(Clone)]
pub struct PoolChannels {
    /// Communication with the execution module
    pub execution_controller: Box<dyn ExecutionController>,
    /// Selector to get draws
    pub selector: Box<dyn SelectorController>,
    /// Broadcasts used by the pool worker to send new operations and endorsements
    pub broadcasts: PoolBroadcasts,
    /// MIP store, used to decide the consensus signature layout (chain-scoped
    /// vs legacy) when creating denunciations (F90 / PDF #11).
    pub mip_store: MipStore,
    /// Local chain id, folded into the signed hash of endorsements / block headers
    /// once MIP-0002 `Execution` v2 is active.
    pub chain_id: u64,
}

/// Broadcasts used by the pool worker to send new operations and endorsements
#[derive(Clone)]
pub struct PoolBroadcasts {
    /// Broadcast channel for new endorsements
    pub endorsement_sender: tokio::sync::broadcast::Sender<SecureShareEndorsement>,
    /// Broadcast channel for new operations
    pub operation_sender: tokio::sync::broadcast::Sender<SecureShareOperation>,
}
