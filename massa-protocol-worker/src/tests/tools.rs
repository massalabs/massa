use std::time::Duration;

use massa_channel::receiver::MassaReceiver;
use massa_models::{
    block::SecureShareBlock, block_id::BlockId, operation::SecureShareOperation,
    prehash::PreHashSet,
};
use massa_protocol_exports::{PeerId, ProtocolController};

use crate::{
    handlers::block_handler::{BlockInfoReply, BlockMessage},
    messages::Message,
};

use super::mock_network::MockNetworkController;

pub fn assert_hash_asked_to_node(node: &MassaReceiver<Message>, block_id: &BlockId) {
    let msg = node
        .recv_timeout(Duration::from_millis(1500))
        .expect("Node didn't receive the ask for block message");
    match msg {
        Message::Block(message) => {
            if let BlockMessage::BlockDataRequest(asked) = *message {
                assert_eq!(asked.len(), 1);
                assert_eq!(&asked[0].0, block_id);
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
            if let BlockMessage::BlockHeader(header) = *message {
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
            if let BlockMessage::BlockDataResponse(asked) = *message {
                assert_eq!(asked.len(), 1);
                assert_eq!(&asked[0].0, block_id);
                match asked[0].1 {
                    BlockInfoReply::Info(_) => {}
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
            Message::Block(Box::new(BlockMessage::BlockHeader(
                block.content.header.clone(),
            ))),
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
    let info = vec![(
        block.id,
        BlockInfoReply::Info(block.content.operations.clone()),
    )];
    network_controller
        .send_from_peer(
            node_id,
            Message::Block(Box::new(BlockMessage::BlockDataResponse(info))),
        )
        .unwrap();

    // Send full ops.
    let info = vec![(block.id, BlockInfoReply::Operations(operations))];
    network_controller
        .send_from_peer(
            node_id,
            Message::Block(Box::new(BlockMessage::BlockDataResponse(info))),
        )
        .unwrap();
}
