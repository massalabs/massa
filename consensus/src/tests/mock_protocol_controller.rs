use communication::protocol::{
    NodeId, ProtocolCommand, ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver,
};
use crypto::hash::Hash;
use crypto::signature::Signature;
use models::block::{Block, BlockHeader};
use tokio::sync::mpsc;

const CHANNEL_SIZE: usize = 16;

pub struct MockProtocolController {
    protocol_command_rx: mpsc::Receiver<ProtocolCommand>,
    protocol_event_tx: mpsc::Sender<ProtocolEvent>,
}

impl MockProtocolController {
    pub fn new() -> (Self, ProtocolCommandSender, ProtocolEventReceiver) {
        let (protocol_command_tx, protocol_command_rx) =
            mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);
        let (protocol_event_tx, protocol_event_rx) = mpsc::channel::<ProtocolEvent>(CHANNEL_SIZE);
        (
            MockProtocolController {
                protocol_event_tx,
                protocol_command_rx,
            },
            ProtocolCommandSender(protocol_command_tx),
            ProtocolEventReceiver(protocol_event_rx),
        )
    }

    pub async fn wait_command(&mut self) -> Option<ProtocolCommand> {
        Some(self.protocol_command_rx.recv().await?)
    }

    pub async fn receive_block(&mut self, block: Block) {
        let hash = block.header.compute_hash().expect("Failed to compute hash");
        self.protocol_event_tx
            .send(ProtocolEvent::ReceivedBlock { hash, block })
            .await
            .expect("could not send protocol event");
    }

    pub async fn receive_header(&mut self, header: BlockHeader) {
        let hash = header.compute_hash().expect("Failed to compute hash");
        self.protocol_event_tx
            .send(ProtocolEvent::ReceivedBlockHeader { hash, header })
            .await
            .expect("could not send protocol event");
    }

    pub async fn receive_transaction(&mut self, transaction: String) {
        self.protocol_event_tx
            .send(ProtocolEvent::ReceivedTransaction(transaction))
            .await
            .expect("could not send protocol event");
    }

    pub async fn receive_get_active_block(&mut self, _source_node_id: NodeId, hash: Hash) {
        self.protocol_event_tx
            .send(ProtocolEvent::GetBlock(hash))
            .await
            .expect("could not send protocol event");
    }

    // ignore all commands while waiting for a futrue
    pub async fn ignore_commands_while<FutureT: futures::Future + Unpin>(
        &mut self,
        mut future: FutureT,
    ) -> FutureT::Output {
        loop {
            tokio::select!(
                res = &mut future => return res,
                cmd = self.wait_command() => match cmd {
                    Some(_) => {},
                    None => return future.await,  // if the network controlled dies, wait for the future to finish
                }
            );
        }
    }
}
