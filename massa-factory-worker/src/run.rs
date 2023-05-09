//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use parking_lot::RwLock;
use std::sync::{mpsc, Arc};

use crate::{
    block_factory::BlockFactoryWorker, endorsement_factory::EndorsementFactoryWorker,
    manager::FactoryManagerImpl,
};
use massa_factory_exports::{FactoryChannels, FactoryConfig, FactoryManager};
use massa_wallet::Wallet;

/// Start factory
///
/// # Arguments
/// * `cfg`: factory configuration
/// * `wallet`: atomic reference to the node wallet
/// * `channels`: channels to communicate with other modules
///
/// # Return value
/// Returns a factory manager allowing to stop the workers cleanly.
pub(crate)  fn start_factory(
    cfg: FactoryConfig,
    wallet: Arc<RwLock<Wallet>>,
    channels: FactoryChannels,
) -> Box<dyn FactoryManager> {
    // create block factory channel
    let (block_worker_tx, block_worker_rx) = mpsc::channel::<()>();

    // create endorsement factory channel
    let (endorsement_worker_tx, endorsement_worker_rx) = mpsc::channel::<()>();

    // start block factory worker
    let block_worker_handle = BlockFactoryWorker::spawn(
        cfg.clone(),
        wallet.clone(),
        channels.clone(),
        block_worker_rx,
    );

    // start endorsement factory worker
    let endorsement_worker_handle =
        EndorsementFactoryWorker::spawn(cfg, wallet, channels, endorsement_worker_rx);

    // create factory manager
    let manager = FactoryManagerImpl {
        block_worker: Some((block_worker_tx, block_worker_handle)),
        endorsement_worker: Some((endorsement_worker_tx, endorsement_worker_handle)),
    };

    Box::new(manager)
}
