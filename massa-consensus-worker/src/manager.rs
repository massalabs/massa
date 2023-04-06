use massa_consensus_exports::ConsensusManager;
use std::{sync::mpsc::SyncSender, thread::JoinHandle};
use tracing::log::info;

use crate::commands::ConsensusCommand;

pub struct ConsensusManagerImpl {
    pub consensus_thread: Option<(SyncSender<ConsensusCommand>, JoinHandle<()>)>,
}

impl ConsensusManager for ConsensusManagerImpl {
    fn stop(&mut self) {
        info!("stopping consensus worker...");
        // join the consensus thread
        if let Some((tx, join_handle)) = self.consensus_thread.take() {
            drop(tx);
            join_handle
                .join()
                .expect("consensus thread panicked on try to join");
        }
        info!("consensus worker stopped");
    }
}
