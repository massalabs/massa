use massa_consensus_exports::ConsensusController;
use massa_models::block::Block;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolController;
use massa_storage::Storage;

/// History of block production from latest to oldest
/// todo: redesign type (maybe add slots, draws...)
pub type ProductionHistory = Vec<Block>;

/// List of channels the factory will send commands to
pub struct FactoryChannels {
    /// selector controller to get draws
    pub selector: Box<dyn SelectorController>,
    /// consensus controller
    pub consensus: Box<dyn ConsensusController>,
    /// pool controller
    pub pool: Box<dyn PoolController>,
    /// protocol controller
    pub protocol: Box<dyn ProtocolController>,
    /// storage instance
    pub storage: Storage,
}

impl Clone for FactoryChannels {
    ///
    fn clone(&self) -> Self {
        Self {
            selector: self.selector.clone(),
            consensus: self.consensus.clone(),
            pool: self.pool.clone(),
            protocol: self.protocol.clone(),
            storage: self.storage.clone("factory".into()),
        }
    }
}
