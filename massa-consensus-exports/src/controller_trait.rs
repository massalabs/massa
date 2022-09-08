use massa_graph::BootstrapableGraph;

use crate::error::ConsensusResult;

/// TODO: Doc
pub trait ConsensusController: Send + Sync {
    /// TODO: Doc
    fn export_bootstrap_state(&self) -> ConsensusResult<Vec<u64>>;
}

pub trait ConsensusManager {
    fn stop(&mut self);
}
