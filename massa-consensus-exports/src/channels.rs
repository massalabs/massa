use crossbeam_channel::Sender;
use massa_execution_exports::ExecutionController;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolCommandSender;

use crate::events::ConsensusEvent;

/// Contains a reference to the pool, selector and execution controller
/// Contains a channel to send info to protocol
#[derive(Clone)]
pub struct ConsensusChannels {
    pub execution_controller: Box<dyn ExecutionController>,
    pub selector_controller: Box<dyn SelectorController>,
    pub pool_command_sender: Box<dyn PoolController>,
    pub controller_event_tx: Sender<ConsensusEvent>,
    pub protocol_command_sender: ProtocolCommandSender,
}
