//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use parking_lot::RwLock;
use std::sync::{mpsc, Arc};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::denunciation_factory::DenunciationFactoryWorker;
use crate::types::DenunciationsRequest;
use crate::{
    block_factory::BlockFactoryWorker, endorsement_factory::EndorsementFactoryWorker,
    manager::FactoryManagerImpl,
};
use massa_factory_exports::{FactoryChannels, FactoryConfig, FactoryManager};
use massa_models::denunciation::DenunciationPrecursor;
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
pub fn start_factory(
    cfg: FactoryConfig,
    wallet: Arc<RwLock<Wallet>>,
    channels: FactoryChannels,
    denunciation_factory_consensus_receiver: Receiver<DenunciationPrecursor>,
    denunciation_factory_endorsement_pool_receiver: Receiver<DenunciationPrecursor>,
    block_factory_request_sender: Sender<DenunciationsRequest>,
    block_factory_request_receiver: Receiver<DenunciationsRequest>,
) -> Box<dyn FactoryManager> {
    // create block factory channel
    let (block_worker_tx, block_worker_rx) = mpsc::channel::<()>();

    // create endorsement factory channel
    let (endorsement_worker_tx, endorsement_worker_rx) = mpsc::channel::<()>();

    // create denunciation factory channel
    let (denunciation_worker_tx, denunciation_worker_rx) = unbounded::<()>();

    // start block factory worker
    let block_worker_handle = BlockFactoryWorker::spawn(
        cfg.clone(),
        wallet.clone(),
        channels.clone(),
        block_worker_rx,
        block_factory_request_sender,
    );

    // start endorsement factory worker
    let endorsement_worker_handle = EndorsementFactoryWorker::spawn(
        cfg.clone(),
        wallet,
        channels.clone(),
        endorsement_worker_rx,
    );

    // start denunciation factory worker
    let denunciation_worker_handle = DenunciationFactoryWorker::spawn(
        cfg,
        channels,
        denunciation_worker_rx,
        denunciation_factory_consensus_receiver,
        denunciation_factory_endorsement_pool_receiver,
        block_factory_request_receiver,
    );

    // create factory manager
    let manager = FactoryManagerImpl {
        block_worker: Some((block_worker_tx, block_worker_handle)),
        endorsement_worker: Some((endorsement_worker_tx, endorsement_worker_handle)),
        denunciation_worker: Some((denunciation_worker_tx, denunciation_worker_handle)),
    };

    Box::new(manager)
}
