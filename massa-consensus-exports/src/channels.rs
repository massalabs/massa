use massa_execution_exports::ExecutionController;
use massa_models::{
    block::{FilledBlock, SecureShareBlock},
    block_header::BlockHeader,
    block_id::BlockId,
    secure_share::SecureShare,
};
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolCommandSender;

use crate::events::ConsensusEvent;

/// Contains a reference to the pool, selector and execution controller
/// Contains a channel to send info to protocol
/// Contains channels to send info to api
#[derive(Clone)]
pub struct ConsensusChannels {
    pub execution_controller: Box<dyn ExecutionController>,
    pub selector_controller: Box<dyn SelectorController>,
    pub pool_command_sender: Box<dyn PoolController>,
    pub controller_event_tx: crossbeam_channel::Sender<ConsensusEvent>,
    pub protocol_command_sender: ProtocolCommandSender,
    pub block_sender: tokio::sync::broadcast::Sender<SecureShareBlock>,
    pub block_header_sender: tokio::sync::broadcast::Sender<SecureShare<BlockHeader, BlockId>>,
    pub filled_block_sender: tokio::sync::broadcast::Sender<FilledBlock>,
}
