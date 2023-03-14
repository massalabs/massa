use massa_protocol_exports_2::ProtocolManager;

/// protocol manager used to stop the protocol
pub struct ProtocolManagerImpl {}

impl ProtocolManager for ProtocolManagerImpl {
    /// Stop the protocol controller
    fn stop(&mut self) {
        // info!("stopping protocol controller...");
        // drop(self.manager_tx);
        // let network_event_receiver = self.join_handle.await??;
        // info!("protocol controller stopped");
        // Ok(network_event_receiver)
    }
}
