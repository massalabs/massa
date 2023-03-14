use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{
    ProtocolConfig, ProtocolController, ProtocolError, ProtocolManager,
};
use massa_storage::Storage;
use tracing::debug;

use crate::{controller::ProtocolControllerImpl, manager::ProtocolManagerImpl, connectivity::start_connectivity_thread};

/// start a new `ProtocolController` from a `ProtocolConfig`
///
/// # Arguments
/// * `config`: protocol settings
/// * `consensus_controller`: interact with consensus module
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub async fn start_protocol_controller(
    config: ProtocolConfig,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<(Box<dyn ProtocolController>, Box<dyn ProtocolManager>), ProtocolError> {
    debug!("starting protocol controller");

    let connectivity_thread_handle = start_connectivity_thread(config.clone())?;

    let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

    let controller = ProtocolControllerImpl::new();

    Ok((Box::new(controller), Box::new(manager)))
}
