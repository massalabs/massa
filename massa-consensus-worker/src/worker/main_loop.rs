use std::{sync::mpsc, time::Instant};

use massa_consensus_exports::{error::ConsensusError, events::ConsensusEvent};
use massa_models::{
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp},
};
use massa_time::MassaTime;
use tracing::log::{info, warn};

use crate::commands::ConsensusCommand;

use super::ConsensusWorker;

enum WaitingStatus {
    Ended,
    Interrupted,
    Disconnected,
}

impl ConsensusWorker {
    /// Execute a command received from the controller also run an update of the graph after processing the command.
    ///
    /// # Arguments:
    /// * `command`: the command to execute
    ///
    /// # Returns:
    /// If successful, returns true if the loop should continue, false if it should stop.
    /// An error if the command failed
    fn manage_command(&mut self, command: ConsensusCommand) -> Result<bool, ConsensusError> {
        let mut write_shared_state = self.shared_state.write();
        match command {
            ConsensusCommand::RegisterBlockHeader(block_id, header) => {
                write_shared_state.register_block_header(block_id, header, self.previous_slot)?;
                write_shared_state.block_db_changed()?;
                Ok(true)
            }
            ConsensusCommand::RegisterBlock(block_id, slot, block_storage, created) => {
                write_shared_state.register_block(
                    block_id,
                    slot,
                    self.previous_slot,
                    block_storage,
                    created,
                )?;
                write_shared_state.block_db_changed()?;
                Ok(true)
            }
            ConsensusCommand::Stop => Ok(false),
            ConsensusCommand::MarkInvalidBlock(block_id, header) => {
                write_shared_state.mark_invalid_block(&block_id, header);
                Ok(true)
            }
        }
    }

    /// Wait and interrupt if we receive a command, a stop signal or we reach the `instant`
    ///
    /// # Return:
    /// WaitingStatus::Interrupted => if a command has been executed
    /// WaitingStatus::Ended => if we reached the `instant`
    /// WaitingStatus::Disconnected => if we received a stop signal
    fn wait_slot_or_command(&mut self, deadline: Instant) -> WaitingStatus {
        match self.command_receiver.recv_deadline(deadline) {
            // message received => manage it
            Ok(command) => match self.manage_command(command) {
                Ok(true) => WaitingStatus::Interrupted,
                Ok(false) => WaitingStatus::Disconnected,
                Err(err) => {
                    warn!("Error in consensus: {}", err);
                    WaitingStatus::Interrupted
                }
            },
            // timeout => continue main loop
            Err(mpsc::RecvTimeoutError::Timeout) => WaitingStatus::Ended,
            // channel disconnected (sender dropped) => quit main loop
            Err(mpsc::RecvTimeoutError::Disconnected) => WaitingStatus::Disconnected,
        }
    }

    /// Gets the next slot and the instant when it will happen.
    /// Slots can be skipped if we waited too much in-between.
    /// Extra safety against double-production caused by clock adjustments (this is the role of the `previous_slot` parameter).
    fn get_next_slot(&self, previous_slot: Option<Slot>) -> (Slot, Instant) {
        // get current absolute time
        let now = MassaTime::now().expect("could not get current time");

        // get closest slot according to the current absolute time
        let mut next_slot = get_closest_slot_to_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now,
        );

        // protection against double-production on unexpected system clock adjustment
        if let Some(prev_slot) = previous_slot {
            if next_slot <= prev_slot {
                next_slot = prev_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("could not compute next slot");
            }
        }

        // get the timestamp of the target slot
        let next_instant = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            next_slot,
        )
        .expect("could not get block slot timestamp")
        .estimate_instant()
        .expect("could not estimate block slot instant");

        (next_slot, next_instant)
    }

    /// Runs in loop forever. This loop must stop every slot to perform operations on stats and graph
    /// but can be stopped anytime by a command received.
    pub fn run(&mut self) {
        let mut last_prune = Instant::now();
        loop {
            match self.wait_slot_or_command(self.next_instant) {
                // When we reached the instant of the next slot
                WaitingStatus::Ended => {
                    if let Some(end) = self.config.end_timestamp {
                        // The testnet has ended. Will be removed for mainnet.
                        if self.next_instant > end.estimate_instant().unwrap() {
                            info!("This episode has come to an end, please get the latest testnet node version to continue");
                            let _ = self
                                .shared_state
                                .read()
                                .channels
                                .controller_event_tx
                                .send(ConsensusEvent::Stop);
                            break;
                        }
                    }
                    let previous_cycle = self
                        .previous_slot
                        .map(|s| s.get_cycle(self.config.periods_per_cycle));
                    let observed_cycle = self.next_slot.get_cycle(self.config.periods_per_cycle);
                    if previous_cycle.is_none() {
                        // first cycle observed
                        info!("Massa network has started ! 🎉")
                    }
                    if previous_cycle < Some(observed_cycle) {
                        info!("Started cycle {}", observed_cycle);
                    }
                    // Execute all operations and checks that should be performed at each slot
                    {
                        let mut write_shared_state = self.shared_state.write();
                        if let Err(err) = write_shared_state.slot_tick(self.next_slot) {
                            warn!("Error while processing block tick: {}", err);
                        }
                    };
                    if last_prune.elapsed().as_millis()
                        > self.config.block_db_prune_interval.to_millis() as u128
                    {
                        self.shared_state
                            .write()
                            .prune()
                            .expect("Error while pruning");
                        last_prune = Instant::now();
                    }
                    self.previous_slot = Some(self.next_slot);
                    (self.next_slot, self.next_instant) = self.get_next_slot(Some(self.next_slot));
                }
                WaitingStatus::Disconnected => {
                    break;
                }
                WaitingStatus::Interrupted => {
                    continue;
                }
            };
        }
    }
}
