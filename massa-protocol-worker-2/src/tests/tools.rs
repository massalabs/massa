use std::time::Duration;

use crossbeam::channel::Receiver;
use massa_models::block_id::BlockId;

use crate::{
    handlers::block_handler::{AskForBlocksInfo, BlockInfoReply, BlockMessage},
    messages::Message,
};

pub fn assert_hash_asked_to_node(node: &Receiver<Message>, block_id: &BlockId) {
    let msg = node
        .recv_timeout(Duration::from_millis(1500))
        .expect("Node didn't receive the ask for block message");
    match msg {
        Message::Block(message) => {
            if let BlockMessage::AskForBlocks(asked) = *message {
                assert_eq!(asked.len(), 1);
                assert_eq!(&asked[0].0, block_id);
                assert_eq!(asked[0].1, AskForBlocksInfo::Info);
            } else {
                panic!("Node didn't receive the ask for block message");
            }
        }
        _ => panic!("Node didn't receive the ask for block message"),
    }
}

pub fn assert_block_info_sent_to_node(node: &Receiver<Message>, block_id: &BlockId) {
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
            if let BlockMessage::ReplyForBlocks(asked) = *message {
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
