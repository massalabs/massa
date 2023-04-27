use std::thread::JoinHandle;

use crossbeam::channel::Sender;
use massa_protocol_exports::ProtocolManager;
use tracing::info;

use crate::connectivity::ConnectivityCommand;

/// protocol manager used to stop the protocol
pub struct ProtocolManagerImpl {
    connectivity_thread: Option<(Sender<ConnectivityCommand>, JoinHandle<()>)>,
}

impl ProtocolManagerImpl {
    pub fn new(connectivity_thread: (Sender<ConnectivityCommand>, JoinHandle<()>)) -> Self {
        Self {
            connectivity_thread: Some(connectivity_thread),
        }
    }
}

impl ProtocolManager for ProtocolManagerImpl {
    /// Stop the protocol module
    fn stop(&mut self) {
        info!("stopping protocol module...");
        if let Some((tx, join_handle)) = self.connectivity_thread.take() {
            tx.send(ConnectivityCommand::Stop)
                .expect("Failed to send stop command of protocol");
            drop(tx);
            join_handle
                .join()
                .expect("connectivity thread panicked on try to join");
        }
    }
}
