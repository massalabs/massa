use std::time::Duration;

use massa_channel::receiver::MassaReceiver;
use massa_models::{
    block::SecureShareBlock, block_id::BlockId, operation::SecureShareOperation,
    prehash::PreHashSet,
};
use massa_protocol_exports::{PeerId, ProtocolController};

use crate::{
    handlers::block_handler::{AskForBlockInfo, BlockInfoReply, BlockMessage},
    messages::Message,
};

use super::mock_network::MockNetworkController;

pub fn assert_hash_asked_to_node(
    node: &MassaReceiver<Message>,
    block_id: &BlockId,
) -> AskForBlockInfo {
    let msg = node
        .recv_timeout(Duration::from_millis(1500))
        .expect("Node didn't receive the ask for block message");
    match msg {
        Message::Block(message) => {
            if let BlockMessage::DataRequest {
                block_id: b_id,
                block_info,
            } = *message
            {
                assert_eq!(&b_id, block_id);
                block_info
            } else {
                panic!("Node didn't receive the ask for block message");
            }
        }
        _ => panic!("Node didn't receive the ask for block message"),
    }
}

pub fn assert_block_info_sent_to_node(node: &MassaReceiver<Message>, block_id: &BlockId) {
    let msg = node
        .recv_timeout(Duration::from_millis(1500))
        .expect("Node didn't receive the infos block message");
    match msg {
        Message::Block(message) => {
            if let BlockMessage::Header(header) = *message {
                assert_eq!(&header.id, block_id);
            } else {
                panic!("Node didn't receive the block header message")
            }
        }
        _ => panic!("Node didn't receive the block header message"),
    }

    let msg = node
        .recv_timeout(Duration::from_millis(3500))
        .expect("Node didn't receive the infos block message");
    match msg {
        Message::Block(message) => {
            if let BlockMessage::DataResponse {
                block_id: b_id,
                block_info,
            } = *message
            {
                assert_eq!(&b_id, block_id);
                match block_info {
                    BlockInfoReply::OperationIds(_) => {}
                    _ => panic!("Node didn't receive the infos block message"),
                }
            } else {
                panic!("Node didn't receive the infos block message")
            }
        }
        _ => panic!("Node didn't receive the infos block message"),
    }
}

/// send a block and assert it has been propagate (or not)
pub fn send_and_propagate_block(
    network_controller: &mut MockNetworkController,
    block: SecureShareBlock,
    node_id: &PeerId,
    protocol_controller: &Box<dyn ProtocolController>,
    operations: Vec<SecureShareOperation>,
) {
    network_controller
        .send_from_peer(
            node_id,
            Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
        )
        .unwrap();

    protocol_controller
        .send_wishlist_delta(
            vec![(block.id, Some(block.content.header.clone()))]
                .into_iter()
                .collect(),
            PreHashSet::<BlockId>::default(),
        )
        .unwrap();

    // Send block info to protocol.
    network_controller
        .send_from_peer(
            node_id,
            Message::Block(Box::new(BlockMessage::DataResponse {
                block_id: block.id,
                block_info: BlockInfoReply::OperationIds(block.content.operations.clone()),
            })),
        )
        .unwrap();

    // Send full ops.
    network_controller
        .send_from_peer(
            node_id,
            Message::Block(Box::new(BlockMessage::DataResponse {
                block_id: block.id,
                block_info: BlockInfoReply::Operations(operations),
            })),
        )
        .unwrap();
}
