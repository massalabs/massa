// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements a factory manager.
//! See `massa-factory-exports/manager_traits.rs` for functional details.

use std::{sync::mpsc, thread::JoinHandle};

use massa_factory_exports::FactoryManager;
use tracing::{info, warn};

/// Implementation of the factory manager
/// Allows stopping the factory worker
pub struct FactoryManagerImpl {
    /// block worker message sender and join handle
    pub(crate) block_worker: Option<(mpsc::Sender<()>, JoinHandle<()>)>,

    /// endorsement worker message sender and join handle
    pub(crate) endorsement_worker: Option<(mpsc::Sender<()>, JoinHandle<()>)>,

    /// denunciation worker message sender and join handle
    pub(crate) denunciation_worker: Option<(crossbeam_channel::Sender<()>, JoinHandle<()>)>,
}

impl FactoryManager for FactoryManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping factory...");
        if let Some((chan_tx, join_handle)) = self.block_worker.take() {
            std::mem::drop(chan_tx);
            if let Err(err) = join_handle.join() {
                warn!("block factory worker panicked: {:?}", err);
            }
        }
        if let Some((chan_tx, join_handle)) = self.endorsement_worker.take() {
            std::mem::drop(chan_tx);
            if let Err(err) = join_handle.join() {
                warn!("endorsement factory worker panicked: {:?}", err);
            }
        }
        if let Some((chan_tx, join_handle)) = self.denunciation_worker.take() {
            std::mem::drop(chan_tx);
            if let Err(err) = join_handle.join() {
                warn!("denunciation factory worker panicked: {:?}", err);
            }
        }
        info!("factory stopped");
    }
}
