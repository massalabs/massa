use massa_network_exports::{NetworkCommandSender, NetworkEventReceiver};
use tokio::sync::mpsc;

use crate::ProtocolCommand;

/// Contains channels (senders) used by the protocol worker
/// Contains (a) channel(s) to send info to api
#[derive(Clone)]
pub struct ProtocolSenders {
    /// network command sender
    pub network_command_sender: NetworkCommandSender,
}

/// Contains channels(receivers) used by the protocol worker
pub struct ProtocolReceivers {
    /// network event receiver
    pub network_event_receiver: NetworkEventReceiver,
    /// protocol command receiver
    pub protocol_command_receiver: mpsc::Receiver<ProtocolCommand>,
}
