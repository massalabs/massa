use tokio::sync::mpsc;

use crate::VersioningCommand;

/// Contains channels (senders) used by the protocol worker
/// Contains (a) channel(s) to send info to api
#[derive(Clone)]
pub struct VersioningSenders {
    //pub sender: mpsc::Sender<>,
}

/// Contains channels(receivers) used by the protocol worker
pub struct VersioningReceivers {
    pub versioning_command_receiver: mpsc::Receiver<VersioningCommand>,
}
