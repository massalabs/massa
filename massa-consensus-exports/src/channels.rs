use massa_execution_exports::ExecutionController;
use massa_models::block::{Block, FilledBlock};
use massa_models::block_header::BlockHeader;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolCommandSender;

use crate::events::ConsensusEvent;

/// Contains links to other modules of the node to be able to interact with them.
#[derive(Clone)]
pub struct ConsensusChannels {
    /// Interface to interact with Execution module
    pub execution_controller: Box<dyn ExecutionController>,
    /// Interface to interact with PoS module
    pub selector_controller: Box<dyn SelectorController>,
    /// Interface to interact with Pool module
    pub pool_command_sender: Box<dyn PoolController>,
    /// Channel use by the consensus to send instruction to the node globally
    pub controller_event_tx: crossbeam_channel::Sender<ConsensusEvent>,
    /// Channel to send command to the protocol
    pub protocol_command_sender: ProtocolCommandSender,
    /// Channel use by Websocket (if they are enable) to broadcast a new block integrated
    pub block_sender: tokio::sync::broadcast::Sender<Block>,
    /// Channel use by Websocket (if they are enable) to broadcast a new block header integrated
    pub block_header_sender: tokio::sync::broadcast::Sender<BlockHeader>,
    /// Channel use by Websocket (if they are enable) to broadcast a new block integrated
    pub filled_block_sender: tokio::sync::broadcast::Sender<FilledBlock>,
}
