use massa_consensus_exports::ConsensusController;
use massa_models::block::Block;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolController;
use massa_storage::Storage;

/// History of block production from latest to oldest
/// todo: redesign type (maybe add slots, draws...)
pub(crate)  type ProductionHistory = Vec<Block>;

/// List of channels the factory will send commands to
#[derive(Clone)]
pub(crate)  struct FactoryChannels {
    /// selector controller to get draws
    pub(crate)  selector: Box<dyn SelectorController>,
    /// consensus controller
    pub(crate)  consensus: Box<dyn ConsensusController>,
    /// pool controller
    pub(crate)  pool: Box<dyn PoolController>,
    /// protocol controller
    pub(crate)  protocol: Box<dyn ProtocolController>,
    /// storage instance
    pub(crate)  storage: Storage,
}
