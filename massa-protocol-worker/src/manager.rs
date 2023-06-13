use std::thread::JoinHandle;

use massa_channel::sender::MassaSender;
use massa_protocol_exports::ProtocolManager;
use tracing::info;

use crate::connectivity::ConnectivityCommand;

/// protocol manager used to stop the protocol
pub struct ProtocolManagerImpl {
    connectivity_thread: Option<(MassaSender<ConnectivityCommand>, JoinHandle<()>)>,
}

impl ProtocolManagerImpl {
    pub fn new(connectivity_thread: (MassaSender<ConnectivityCommand>, JoinHandle<()>)) -> Self {
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
