use massa_models::{Address, Block};

use crate::RollUpdates;

#[derive(Clone)]
pub struct POSBlock {
    /// The block itself, as it was created
    pub block: Block,
    /// list of (period, address, did_create) for all block/endorsement creation events
    pub production_events: Vec<(u64, Address, bool)>,
    /// Address -> RollUpdate
    pub roll_updates: RollUpdates,
}
