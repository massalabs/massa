use crate::error::ConsensusError;

use super::super::{
    block_database::*, config::ConsensusConfig, consensus_controller::*, random_selector::*,
    timeslots::*,
};
use communication::protocol::protocol_controller::{
    NodeId, ProtocolController, ProtocolEvent, ProtocolEventType,
};
use tokio::stream::StreamExt;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::sleep_until;

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
    selector: RandomSelector,
}

impl<ProtocolControllerT: ProtocolController + 'static> ConsensusWorker<ProtocolControllerT> {
    pub fn new(
        cfg: ConsensusConfig,
        protocol_controller: ProtocolControllerT,
        block_db: BlockDatabase,
        controller_command_rx: Receiver<ConsensusCommand>,
        controller_event_tx: Sender<ConsensusEvent>,
    ) -> Result<ConsensusWorker<ProtocolControllerT>, ConsensusError> {
        let seed = vec![0u8; 32]; // TODO temporary
        let participants_weights = vec![1u64; cfg.nodes.len()]; // TODO
        let selector = RandomSelector::new(&seed, cfg.thread_count, participants_weights)?;
        Ok(ConsensusWorker {
            cfg,
            protocol_controller,
            block_db,
            controller_command_rx,
            controller_event_tx,
            selector,
        })
    }

    pub async fn run_loop(mut self) -> Result<(), ConsensusError> {
        let (mut next_slot_thread, mut next_slot_number) = get_current_latest_block_slot(
            self.cfg.thread_count,
            self.cfg.t0_millis,
            self.cfg.genesis_timestamp_millis,
        )?
        .map_or(Ok((0u8, 0u64)), |(cur_thread, cur_slot)| {
            get_next_block_slot(self.cfg.thread_count, cur_thread, cur_slot)
        })?;
        let mut next_slot_timer = sleep_until(estimate_instant_from_timestamp(
            get_block_slot_timestamp_millis(
                self.cfg.thread_count,
                self.cfg.t0_millis,
                self.cfg.genesis_timestamp_millis,
                next_slot_thread,
                next_slot_number,
            )?,
        )?);

        loop {
            tokio::select! {
                // listen consensus commands
                res = self.controller_command_rx.next() => match res {
                    Some(cmd) => self.process_consensus_command(cmd).await,
                    None => break  // finished
                },

                // slot timer
                _ = &mut next_slot_timer => {
                    massa_trace!("slot_timer", {
                        "slot_thread": next_slot_thread,
                        "slot_number": next_slot_number
                    });

                    // check if it is our turn to create a block
                    let block_creator = self.selector.draw(next_slot_thread, next_slot_number);
                    if next_slot_number > 0 && block_creator == self.cfg.current_node_index {
                        let block = self.block_db.create_block("block".to_string(), next_slot_thread, next_slot_number)?;

                        let (is_to_propagate, hash) = self
                        .block_db
                        .acknowledge_new_block(&block, &mut self.selector)?;
                        massa_trace!("created_block", { "block": block , "hash": hash});
                        if is_to_propagate
                        {
                            self.protocol_controller
                                .propagate_block(&block, None, None)
                                .await?;
                        }
                    }

                    // reset timer for next slot
                    (next_slot_thread, next_slot_number) = get_next_block_slot(self.cfg.thread_count, next_slot_thread, next_slot_number)?;
                    next_slot_timer = sleep_until(
                        estimate_instant_from_timestamp(
                            get_block_slot_timestamp_millis(
                                self.cfg.thread_count,
                                self.cfg.t0_millis,
                                self.cfg.genesis_timestamp_millis,
                                next_slot_thread,
                                next_slot_number,
                            )?,
                        )?
                    );
                }

                // listen protocol controller events
                evt = self.protocol_controller.wait_event() => match evt {
                    Ok(ProtocolEvent(source_node_id, event)) => self.process_protocol_event(source_node_id, event).await?,
                    Err(err) => return Err(ConsensusError::CommunicationError(err)) // in a loop
                }

            }
        }

        // end loop
        self.protocol_controller.stop().await?;
        Ok(())
    }

    async fn process_consensus_command(&mut self, cmd: ConsensusCommand) {
        match cmd {
            ConsensusCommand::CreateBlock(_block) => {
                // TODO remove ?
            }
        }
    }

    async fn process_protocol_event(
        &mut self,
        source_node_id: NodeId,
        event: ProtocolEventType,
    ) -> Result<(), ConsensusError> {
        match event {
            ProtocolEventType::ReceivedBlock(block) => {
                let (is_to_propagate, hash) = self
                    .block_db
                    .acknowledge_new_block(&block, &mut self.selector)?;
                if is_to_propagate {
                    massa_trace!("received_block_ok", {"source_node_id": source_node_id, "block": block, "hash": hash});
                    self.protocol_controller
                        .propagate_block(&block, Some(source_node_id), None)
                        .await?;
                } else {
                    massa_trace!("received_block_ignore", {"source_node_id": source_node_id, "block": block, "hash": hash});
                }
            }
            ProtocolEventType::ReceivedTransaction(_transaction) => {
                // todo
            }
            ProtocolEventType::AskedBlock(block_hash) => {
                for db in &self.block_db.active_blocks {
                    if let Some(block) = db.get(&block_hash) {
                        massa_trace!("sending_block", {"dest_node_id": source_node_id, "block": block});
                        self.protocol_controller
                            .propagate_block(block, None, Some(source_node_id))
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }
}
