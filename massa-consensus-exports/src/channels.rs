use massa_execution_exports::ExecutionController;
use massa_models::block::{FilledBlock, SecureShareBlock};
use massa_models::block_header::BlockHeader;
use massa_models::block_id::BlockId;
use massa_models::secure_share::SecureShare;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolController;

use crate::events::ConsensusEvent;

/// Contains links to other modules of the node to be able to interact with them.
#[derive(Clone)]
pub struct ConsensusChannels {
    /// Interface to interact with Execution module
    pub execution_controller: Box<dyn ExecutionController>,
    /// Interface to interact with PoS module
    pub selector_controller: Box<dyn SelectorController>,
    /// Interface to interact with Pool module
    pub pool_controller: Box<dyn PoolController>,
    /// Interface to interact with Protocol module
    pub protocol_controller: Box<dyn ProtocolController>,
    /// Channel used by the consensus to send events to the node globally
    pub controller_event_tx: crossbeam_channel::Sender<ConsensusEvent>,
    /// Channel used for Websocket broadcast (if enabled) of new blocks being integrated in the graph
    pub block_sender: tokio::sync::broadcast::Sender<SecureShareBlock>,
    /// Channel used for Websocket broadcast (if enabled) of new block headers being integrated in the graph
    pub block_header_sender: tokio::sync::broadcast::Sender<SecureShare<BlockHeader, BlockId>>,
    /// Channel use by Websocket (if they are enable) to broadcast a new block integrated
    pub filled_block_sender: tokio::sync::broadcast::Sender<FilledBlock>,
}
