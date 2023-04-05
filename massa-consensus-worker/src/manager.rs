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
            //TODO: Remove when we have a good fix for https://github.com/massalabs/massa/issues/3766
            tx.send(ConsensusCommand::Stop)
                .expect("consensus thread panicked on try to send stop message to worker");
            drop(tx);
            join_handle
                .join()
                .expect("consensus thread panicked on try to join");
        }
        info!("consensus worker stopped");
    }
}
