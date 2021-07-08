use super::super::{block_database::*, config::ConsensusConfig, consensus_controller::*};
use crate::protocol::protocol_controller::{
    NodeId, ProtocolController, ProtocolEvent, ProtocolEventType,
};
use tokio::stream::StreamExt;
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Clone, Debug)]
pub enum ConsensusCommand {
    CreateBlock(String),
}

pub struct ConsensusWorker<ProtocolControllerT: ProtocolController + 'static> {
    cfg: ConsensusConfig,
    protocol_controller: ProtocolControllerT,
    block_db: BlockDatabase,
    controller_command_rx: Receiver<ConsensusCommand>,
    controller_event_tx: Sender<ConsensusEvent>,
}

impl<ProtocolControllerT: ProtocolController + 'static> ConsensusWorker<ProtocolControllerT> {
    pub fn new(
        cfg: ConsensusConfig,
        protocol_controller: ProtocolControllerT,
        block_db: BlockDatabase,
        controller_command_rx: Receiver<ConsensusCommand>,
        controller_event_tx: Sender<ConsensusEvent>,
    ) -> ConsensusWorker<ProtocolControllerT> {
        ConsensusWorker {
            cfg,
            protocol_controller,
            block_db,
            controller_command_rx,
            controller_event_tx,
        }
    }

    pub async fn run_loop(mut self) {
        loop {
            tokio::select! {
                // listen consensus commands
                res = self.controller_command_rx.next() => match res {
                    Some(cmd) => self.process_consensus_command(cmd).await,
                    None => break  // finished
                },

                // listen protocol controller events
                ProtocolEvent(source_node_id, event) = self.protocol_controller.wait_event() =>
                    self.process_protocol_event(source_node_id, event).await,
            }
        }

        // end loop
        self.protocol_controller.stop().await;
    }

    async fn process_consensus_command(&mut self, cmd: ConsensusCommand) {
        match cmd {
            ConsensusCommand::CreateBlock(block) => {
                massa_trace!("created_block", { "block": block });
                let block = self.block_db.create_block(block);
                if self.block_db.acknowledge_new_block(&block) {
                    self.protocol_controller
                        .propagate_block(&block, None, None)
                        .await;
                }
            }
        }
    }

    async fn process_protocol_event(&mut self, source_node_id: NodeId, event: ProtocolEventType) {
        match event {
            ProtocolEventType::ReceivedBlock(block) => {
                if self.block_db.acknowledge_new_block(&block) {
                    massa_trace!("received_block_ok", {"source_node_id": source_node_id, "block": block});
                    self.protocol_controller
                        .propagate_block(&block, Some(source_node_id), None)
                        .await;
                } else {
                    massa_trace!("received_block_ignore", {"source_node_id": source_node_id, "block": block});
                }
            }
            ProtocolEventType::ReceivedTransaction(transaction) => {
                // todo
            }
            ProtocolEventType::AskedBlock(block_hash) => {
                for db in &self.block_db.0 {
                    if let Some(block) = db.get(&block_hash) {
                        massa_trace!("sending_block", {"dest_node_id": source_node_id, "block": block});
                        self.protocol_controller
                            .propagate_block(block, None, Some(source_node_id))
                            .await;
                    }
                }
            }
        }
    }
}
