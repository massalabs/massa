//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module allows Execution to manage slot sequencing.

use std::collections::{HashMap, VecDeque};

use massa_execution_exports::ExecutionConfig;
use massa_models::{
    block_id::BlockId,
    prehash::PreHashMap,
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
};
use massa_storage::Storage;
use massa_time::MassaTime;

/// Information about a slot in the execution sequence
#[derive(Debug, Clone)]
pub struct SlotInfo {
    /// Slot
    slot: Slot,
    /// Whether the slot is CSS-final
    css_final: bool,
    /// Whether the slot is SCE-final
    sce_final: bool,
    /// Content of the slot (None if miss, otherwise block ID and associated Storage instance)
    content: Option<(BlockId, Storage)>,
}

impl SlotInfo {
    /// Get the block ID (if any) at that slot
    pub fn get_block_id(&self) -> Option<&BlockId> {
        self.content.as_ref().map(|(b_id, _)| b_id)
    }
}

/// Structure allowing execution slot sequence management.
///
/// The `SlotSequencer::update` method is called to notify the sequencer about blocks becoming CSS-final, about changes in the blockclique, or simply about slot ticks.
/// The sequencer then internally sequences the slots, and prepares a queue of slots that are ready to be executed by `Execution`.
/// `SlotSequencer::is_task_available` allows checking if a slot is ready to be executed.
/// `SlotSequencer::run_task_with` allows running the next slot in the queue, if any.
/// Note that SCE-final slots are executed in priority over candidate slots.
/// `SlotSequencer::get_next_slot_deadline` allows getting the time at which the next slot will happen (this is useful to sequence slots as they happen even if there is no block there).
pub struct SlotSequencer {
    /// Config
    config: ExecutionConfig,

    /// Continuous sequence of slots containing all the slots relevant for Execution and their current states (see `SlotInfo`). Oldest slot is at front.
    sequence: VecDeque<SlotInfo>,

    /// latest CSS-final slots (one per thread)
    latest_css_final_slots: Vec<Slot>,

    /// latest SCE-final slot
    latest_sce_final_slot: Slot,

    /// final slot execution cursor
    latest_executed_final_slot: Slot,

    /// candidate slot execution cursor
    latest_executed_candidate_slot: Slot,
}

impl SlotSequencer {
    /// Create a new slot sequencer.
    /// Note that this will create a SlotSequencer with an empty internal sequence
    /// which makes it unusable until `SlotSequencer::update` is called a first time to feed the CSS-final blocks.
    ///
    /// # Arguments
    /// * `final_cursor`: latest executed SCE-final slot. This is useful on bootstrap in particular in order to avoid re-executing previously executed slots.
    pub fn new(config: ExecutionConfig, final_cursor: Slot) -> Self {
        SlotSequencer {
            sequence: Default::default(),
            latest_css_final_slots: (0..config.thread_count).map(|t| Slot::new(0, t)).collect(),
            latest_sce_final_slot: final_cursor,
            latest_executed_final_slot: final_cursor,
            latest_executed_candidate_slot: final_cursor,
            config,
        }
    }

    /// Internal method that inits the sequencer.
    /// This method is called on the first call to `SlotSequencer::update`.
    /// It allows feeding the initial sequence of CSS-final blocks to the sequencer.
    ///
    /// # Arguments
    ///
    /// `initial_css_final_blocks`: the list of CSS-final blocks (must not be empty)
    /// `initial_blockclique`: initial blockclique, usually empty except on major bootstrap latency
    /// `blocks_storage`: Storage instances for all the blocks referenced in `initial_css_final_blocks` and `blocks_storage`
    fn init(
        &mut self,
        mut initial_css_final_blocks: HashMap<Slot, BlockId>,
        mut initial_blockclique: HashMap<Slot, BlockId>,
        mut blocks_storage: PreHashMap<BlockId, Storage>,
    ) {
        // Compute the latest CSS-final slots
        for (s, _) in initial_css_final_blocks.iter() {
            if s.period > self.latest_css_final_slots[s.thread as usize].period {
                self.latest_css_final_slots[s.thread as usize] = *s;
            }
        }

        // Build the slot sequence

        // Get the starting slot of the sequence: the earliest CSS-final slot
        let mut slot = *initial_css_final_blocks
            .keys()
            .min()
            .expect("init call should be done with non-empty new_css_final_blocks");

        // Compute the maximal slot until which the slot sequence is useful.
        // This is the max between the latest CSS-final slot, the latest blockclique slot,
        // and the latest slot to be executed in time.
        let max_slot = std::cmp::max(
            std::cmp::max(
                *self
                    .latest_css_final_slots
                    .iter()
                    .max()
                    .expect("latest_css_final_slots is empty"),
                initial_blockclique
                    .keys()
                    .max()
                    .copied()
                    .unwrap_or_else(|| Slot::new(0, 0)),
            ),
            self.get_time_cursor(),
        );

        // Iterate from the starting slot to the `max_slot` to build the slot sequence.
        while slot <= max_slot {
            // If the slot is rearlier than (or equal to) the latest CSS-final slot in that thread => mark the slot as CSS-final
            let css_final = slot <= self.latest_css_final_slots[slot.thread as usize];

            // If the slot is before (or equal to) the latest SCE-final slot => mark the slot as SCE-final.
            // Note that `self.latest_sce_final_slot` was initialized in `Self::new`.
            let sce_final = slot <= self.latest_sce_final_slot;

            // Gather the content of the slot by looking for a block at that slot inside `initial_css_final_blocks` (and remove it from there if found).
            // If not found, look into `initial_blockclique` (and remove it from there if found).
            // If still not found, assume a miss (content = None).
            let content = initial_css_final_blocks
                .remove(&slot)
                .or_else(|| initial_blockclique.remove(&slot))
                .map(|b_id| {
                    // A block was found for that slot (not a miss)
                    (
                        b_id,
                        // The block's storage should be in `blocks_storage`
                        blocks_storage
                            .remove(&b_id)
                            .expect("block storage missing in execution init"),
                    )
                });

            // Build the `SlotInfo` for that slot and add it to the sequence
            self.sequence.push_back(SlotInfo {
                slot,
                css_final,
                sce_final,
                content,
            });

            // Increment slot for the next loop iteration
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("overflow in slot iteration");
        }
        // Explicitly consume tainted containers to prevent mistakes caused by using them later.
        if initial_css_final_blocks.into_iter().next().is_some() {
            panic!("remaining elements in css_final_blocks after slot sequencing");
        }
        if initial_blockclique.into_iter().next().is_some() {
            panic!("remaining elements in blockclique after slot sequencing");
        }
        std::mem::drop(blocks_storage);

        // Cleanup the constructed sequence to remove older, executed CSS-final slots
        self.cleanup_sequence();
    }

    /// Internal function allowing to get the latest slot we should execute at the current time.
    /// This is useful to fill the sequencer with slots as they happen, even if there are no blocks there.
    ///
    /// Note that this time cursor is shifted by `self.config.cursor_delay`
    /// to avoid computing speculative slots that are too recent, and therefore subject to frequent re-writes.
    fn get_time_cursor(&self) -> Slot {
        let shifted_now = MassaTime::now()
            .expect("could not get current time")
            .saturating_sub(self.config.cursor_delay);
        get_latest_block_slot_at_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            shifted_now,
        )
        .expect("could not get latest block slot at shifted execution time")
        .unwrap_or_else(|| Slot::new(0, 0))
    }

    /// Notify the sequencer of incoming changes: CSS-finalized blocks and changes in the blockclique.
    /// This function is also called on time slots to ensure new slots are taken into account even if they don't contain a block.
    ///
    /// # Arguments
    /// * `new_css_final_blocks`: new CSS-finalized blocks
    /// * `new_blockclique`: new blockclique (if changed since the last call to this method, otherwise None)
    /// * `new_blocks_storage`: storage instances for blocks that have not been seen previously by the sequencer
    pub fn update(
        &mut self,
        mut new_css_final_blocks: HashMap<Slot, BlockId>,
        mut new_blockclique: Option<HashMap<Slot, BlockId>>,
        mut new_blocks_storage: PreHashMap<BlockId, Storage>,
    ) {
        // If the slot sequence is empty, initialize it by calling `Self::init` and quit.
        // This happens on the first call to `Self::update` (see the doc of `Self::update`).
        if self.sequence.is_empty() {
            if !new_css_final_blocks.is_empty() {
                self.init(
                    new_css_final_blocks,
                    new_blockclique.unwrap_or_default(),
                    new_blocks_storage,
                );
            }
            return;
        }

        // Update the list of latest CSS-final slots
        for (s, _) in new_css_final_blocks.iter() {
            if s.period > self.latest_css_final_slots[s.thread as usize].period {
                self.latest_css_final_slots[s.thread as usize] = *s;
            }
        }

        // Build the slot sequence:.
        // For this, we build a new slot sequence (`new_sequence`) that replaces the old `self.slot_sequence`.
        // For performance, we build the new sequence by recycling elements from the old `self.sequence`
        // and gathering new ones from `new_css_final_blocks` and `new_blockclique`.

        // Get earliest useful slot to start the new sequence from (eg. the earliest slot of the previous sequence)
        let mut slot = self
            .sequence
            .front()
            .expect("slot sequence should not be empty")
            .slot;

        // Get the latest useful slot until which we build the new sequence.
        // It is chosen to be the max between:
        //  * the latest CSS-final slot
        //  * the latest blockclique slot
        //  * the latest slot of the previous sequence
        let max_slot = std::cmp::max(
            std::cmp::max(
                *self
                    .latest_css_final_slots
                    .iter()
                    .max()
                    .expect("latest_css_final_slots is empty"),
                new_blockclique
                    .as_ref()
                    .and_then(|bq| bq.keys().max().copied())
                    .unwrap_or_else(|| Slot::new(0, 0)),
            ),
            self.sequence
                .back()
                .expect("slot sequence cannot be empty")
                .slot,
        );

        // Preallocate the new sequence of slots
        let new_seq_len = max_slot
            .slots_since(&slot, self.config.thread_count)
            .expect("error computing new sequence length")
            .saturating_add(1);
        let mut new_sequence: VecDeque<SlotInfo> = VecDeque::with_capacity(new_seq_len as usize);

        // Mark that we are currently iterating over slots that are SCE-final.
        // The very first slot of the sequence must be SCE-final,
        // and as soon as we reach a slot that is not SCE-final,
        // this variable is set to `false` and remains like this until the end of the loop.
        let mut in_sce_finality = true;

        // Loop over the slots to build the new sequence
        while slot <= max_slot {
            // The slot is now CSS-final if it is before or at the latest CSS-final slot in its own thread
            let new_css_final = slot <= self.latest_css_final_slots[slot.thread as usize];

            // Try to get a block at the current slot by consuming it from the new CSS-final blocks.
            let new_css_final_block: Option<BlockId> = new_css_final_blocks.remove(&slot);

            // Check if we were notified of a blockclique change.
            let blockclique_updated = new_blockclique.is_some();

            // Try to get a block at the current slot by consuming it from the new blockclique (if any).
            let new_blockclique_block = new_blockclique.as_mut().and_then(|bq| bq.remove(&slot));

            // Pops out the current slot from the old sequence (if present)
            let prev_item = self.sequence.pop_front();
            if let Some(prev_info) = prev_item.as_ref() {
                if prev_info.slot != slot {
                    panic!("slot sequencing mismatch");
                }
            }

            // Generate one slot of the new sequence by calling the internal method `Self::sequence_build_step`.
            // `seq_item` is the obtained slot.
            // `seq_item_overwrites_history` is `true` only if the obtained slot overwrites (and changes) an existing speculative slot.
            let (seq_item, seq_item_overwrites_history) = SlotSequencer::sequence_build_step(
                slot,
                prev_item,
                new_css_final,
                new_css_final_block,
                blockclique_updated,
                new_blockclique_block,
                &mut new_blocks_storage,
                in_sce_finality,
            );

            // If the computed slot is not SCE-final => all subsequent slots are not SCE-final
            in_sce_finality = in_sce_finality && seq_item.sce_final;

            // Append the slot to the new sequence.
            new_sequence.push_back(seq_item);

            // If this slot is SCE-final => update the latest SCE-final slot
            if in_sce_finality {
                self.latest_sce_final_slot = slot;
            }

            // If the obtained slot overwrites history before the candidate execution cursor,
            // roll back the candidate execution cursor to the slot just before the overwrite.
            if seq_item_overwrites_history && self.latest_executed_candidate_slot >= slot {
                self.latest_executed_candidate_slot = slot
                    .get_prev_slot(self.config.thread_count)
                    .expect("could not rollback speculative execution cursor");
            }

            // Increment slot for the next loop iteration.
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("overflow on slot iteration");
        }
        // Explicitly consume tainted containers to prevent mistakes caused by using them later.
        if !self.sequence.is_empty() {
            panic!("some items of the old slot sequence have been unexpectedly not processed");
        }
        if new_css_final_blocks.into_iter().next().is_some() {
            panic!("remaining elements in new_css_final_blocks after slot sequencing");
        }
        if let Some(bq) = new_blockclique {
            if !bq.is_empty() {
                panic!("remaining elements in new_blockclique after slot sequencing");
            }
        }
        std::mem::drop(new_blocks_storage);

        // Set the slot sequence to be the new sequence.
        self.sequence = new_sequence;

        // Cleanup the sequence
        self.cleanup_sequence();
    }

    /// Internal method called by `Self::update` to construct one slot of the new slot sequence
    /// by using info about newly CSS-finalized blocks, the new blockclique (if any) and the previous state of that slot.
    ///
    /// # Arguments
    /// * `slot`: the slot being constructed
    /// * `prev_item`: the corresponding slot status from the old sequence, if any
    /// * `new_css_final`: whether this slot was CSS-finalized
    /// * `new_css_final_block`: newly CSS-finalized block at that slot, if any
    /// * `blockclique_updated`: whether a new blockclique was provided when `Self::update` was called
    /// * `new_blockclique_block`: block at that slot within the new blockclique, if any
    /// * `new_blocks_storage`: a mutable reference to the list dictionary of block storages provided to `Self::update`
    /// * `in_sce_finality`: whether the previous slot was SCE-final
    ///
    /// # Returns
    /// A pair (SlotInfo, truncate_history: bool) where truncate_history indicates that this slot changes the content of an existing candidate slot
    #[allow(clippy::too_many_arguments)]
    fn sequence_build_step(
        slot: Slot,
        prev_item: Option<SlotInfo>,
        new_css_final: bool,
        new_css_final_block: Option<BlockId>,
        blockclique_updated: bool,
        new_blockclique_block: Option<BlockId>,
        new_blocks_storage: &mut PreHashMap<BlockId, Storage>,
        in_sce_finality: bool,
    ) -> (SlotInfo, bool) {
        // Match the slot state from the old sequence.
        // Most old slot states can be partially or completely recycled for performance.
        if let Some(mut prev_slot_info) = prev_item {
            // The slot was already present in the old sequence.

            // The slot was CSS-final
            if prev_slot_info.css_final {
                // This CSS-final slot is SCE-final if the previous one was SCE-final.
                prev_slot_info.sce_final = in_sce_finality;

                // Return the obtained slot.
                return (prev_slot_info, false);
            }

            // The slot was not CSS-final and needs to become CSS-final
            if !prev_slot_info.css_final && new_css_final {
                // Check if the content of the existing slot matches the newly CSS-finalized one.
                if prev_slot_info.get_block_id() == new_css_final_block.as_ref() {
                    // Contents match => mark the slot as CSS-final
                    prev_slot_info.css_final = true;

                    // This CSS-final slot is SCE-final if the previous slot was SCE-final
                    prev_slot_info.sce_final = in_sce_finality;

                    // Return the obtained slot.
                    return (prev_slot_info, false);
                }

                // Here we know that the newly CSS-finalized slot has different contents than it used to in its previously non-CSS-final state.

                // Mark the slot as CSS-final
                prev_slot_info.css_final = true;

                // This CSS-final slot is SCE-final if the previous slot was SCE-final
                prev_slot_info.sce_final = in_sce_finality;

                // Overwrite the contents of the slot with the newly CSS-finalized block
                prev_slot_info.content = new_css_final_block.map(|b_id| {
                    (
                        b_id,
                        // Can't recycle any old Storage because of the mismatch: get it from `new_blocks_storage`.
                        new_blocks_storage
                            .remove(&b_id)
                            .expect("new css final block storage absent from new_blocks_storage"),
                    )
                });

                // Return the computed slot state and signal history truncation at this slot.
                return (prev_slot_info, true);
            }

            // Process blockclique.

            // If there is no new blockclique, simply replicate the existing speculative slot state.
            if !blockclique_updated {
                return (prev_slot_info, false);
            }

            // There is a new blockclique: check whether the new slot content matches the old one
            if new_blockclique_block.as_ref() == prev_slot_info.get_block_id() {
                // Contents match => simply return the old slot state
                return (prev_slot_info, false);
            }

            // Here we know that there is a new blockclique and that its contents mismatch the old ones at this slot.

            // Overwrite the slot state contents.
            prev_slot_info.content = new_blockclique_block.map(|b_id| {
                (
                    b_id,
                    // Can't recycle any old Storage because of the mismatch: get it from `new_blocks_storage`.
                    new_blocks_storage
                        .remove(&b_id)
                        .expect("new css blockclique block storage absent from new_blocks_storage"),
                )
            });

            // Return the computed slot state and signal history truncation a this slot.
            return (prev_slot_info, true);
        }

        // This slot was not present in the old slot sequence.

        // Check if the new slot is CSS-final.
        if new_css_final {
            // The slot was absent (or considered a miss) before.
            // So, if there is a new block there, consider that it caused a mismatch at this slot.
            let mismatch = new_css_final_block.is_some();

            // Generate the new CSS-final slot state.
            let slot_info = SlotInfo {
                slot,
                css_final: true,
                sce_final: in_sce_finality, // This CSS-final slot is SCE-final if the previous slot was SCE-final
                content: new_css_final_block.map(|b_id| {
                    // Get the newly CSS-finalized block at that slot, if any
                    (
                        b_id,
                        // Get the block Storage from `new_blocks_storage` as this slot is new to the sequencer.
                        new_blocks_storage
                            .remove(&b_id)
                            .expect("new css final block storage absent from new_blocks_storage"),
                    )
                }),
            };

            // Return the newly created CSS-final slot.
            return (slot_info, mismatch);
        }

        // Here we know that the slot was absent from the old sequence and that it is not CSS-final.

        // The slot was absent (or considered a miss) before.
        // So, if there is a new block there, consider that it caused a mismatch at this slot.
        let mismatch = new_blockclique_block.is_some();

        // Generate a new speculative slot state for that slot.
        let slot_info = SlotInfo {
            slot,
            css_final: false,
            sce_final: false,
            content: new_blockclique_block.map(|b_id| {
                (
                    b_id,
                    new_blocks_storage
                        .remove(&b_id)
                        .expect("new css blockclique block storage absent from new_blocks_storage"),
                )
            }),
        };

        // Return the newly created speculative slot state.
        (slot_info, mismatch)
    }

    /// Get the index of a slot in the sequence, if present, otherwise None
    fn get_slot_index(&self, slot: &Slot) -> Option<usize> {
        let first_slot = &self
            .sequence
            .front()
            .expect("slot sequence should never be empty")
            .slot;
        if slot < first_slot {
            return None; // underflow
        }
        let index: usize = slot
            .slots_since(first_slot, self.config.thread_count)
            .expect("could not compute slots_since first slot in sequence")
            .try_into()
            .expect("usize overflow in sequence index");
        if index >= self.sequence.len() {
            return None; // overflow
        }
        Some(index)
    }

    /// Gets an immutable reference to a SlotInfo, if any, otherwise None
    fn get_slot(&self, slot: &Slot) -> Option<&SlotInfo> {
        self.get_slot_index(slot)
            .and_then(|idx| self.sequence.get(idx))
    }

    /// Returns true if there is a queued slot that needs to be executed now.
    pub fn is_task_available(&self) -> bool {
        // The sequence is empty => nothing to do.
        if self.sequence.is_empty() {
            return false;
        }

        // Check if the next SCE-final slot is available for execution
        {
            // Get the slot just after the last executed SCE-final slot
            let next_sce_final_slot = self
                .latest_executed_final_slot
                .get_next_slot(self.config.thread_count)
                .expect("overflow in slot iteration");
            // Read whether that slot is present in the slot sequence and is marked as SCE-final.
            let finalization_task_available = self
                .get_slot(&next_sce_final_slot)
                .map_or(false, |s_info| s_info.sce_final);
            if finalization_task_available {
                // A non-executed SCE-final slot is ready for execution.
                return true;
            }
        }

        // Check if the next candidate slot is available for execution.
        {
            // Get the slot just after the last executed candidate slot.
            let next_candidate_slot = self
                .latest_executed_candidate_slot
                .get_next_slot(self.config.thread_count)
                .expect("overflow in slot iteration");
            // The candidate slot is considered ready for execution
            // if it is later (or at) the current time cursor.
            // In the case in which it is absent from the sequence,
            // it will be considered a miss by run_task_with.
            if self.get_time_cursor() >= next_candidate_slot {
                // A non-executed candidate slot is ready for execution.
                return true;
            }
        }

        // There is nothing to execute.
        false
    }

    /// Clean the slot sequence by removing slots that are not useful anymore.
    /// The removed slots the ones that are strictly before the earliest executed CSS-final slot.
    /// This function is called on `Self::init` to cleanup bootstrap artifacts,
    /// and when a task is processed with `Self::run_task_with`.
    fn cleanup_sequence(&mut self) {
        // Compute the earliest still-useful slot as the earliest between:
        // * the latest CSS-final slots
        // * the latest SCE-final slot
        // * the latest executed SCE-final slot
        // * the latest executed candidate slot
        let min_useful_slot = std::cmp::min(
            std::cmp::min(
                *self
                    .latest_css_final_slots
                    .iter()
                    .min()
                    .expect("latest_css_final_slots should not be empty"),
                self.latest_sce_final_slot,
            ),
            std::cmp::min(
                self.latest_executed_final_slot,
                self.latest_executed_candidate_slot,
            ),
        );
        // Pop slots from the front of the sequence as long as they are strictly before the earliest useful slot.
        while let Some(slot_info) = self.sequence.front() {
            if slot_info.slot >= min_useful_slot {
                break;
            }
            self.sequence.pop_front();
        }
    }

    /// If a slot is ready for execution, this method will mark it as executed and call the provided callback function on it for execution.
    /// SCE-final slots are executed in priority over candidate slots.
    ///
    /// # Arguments
    /// * `callback`: callback function that executes the slot
    ///   * Callback arguments:
    ///     * a boolean indicating whether or not the slot is SCE-final
    ///     * a reference to the slot
    ///     * a reference to the block at that slot and its storage, if any (otherwise None)
    ///   * Callback return value: an arbitrary `T`
    ///
    /// # Returns
    /// An option that is `None` if there was no task to be executed,
    /// or `Some(T)` where `T` is the value returned by the `callback` function otherwise.
    pub fn run_task_with<F, T>(&mut self, callback: F) -> Option<T>
    where
        F: Fn(bool, &Slot, Option<&(BlockId, Storage)>) -> T,
    {
        // The slot sequence is empty => nothing to do.
        if self.sequence.is_empty() {
            return None;
        }

        // High priority: execute the next SCE-final that is available for execution, if any.
        {
            // Get the slot just after the latest executed SCE-final slot.
            let slot = self
                .latest_executed_final_slot
                .get_next_slot(self.config.thread_count)
                .expect("overflow in slot iteration");
            // Check whether that slot is in the sequence and marked as SCE-final.
            if let Some(SlotInfo {
                sce_final, content, ..
            }) = self.get_slot(&slot)
            {
                if *sce_final {
                    // There is an SCE-final slot ready for execution.

                    // Call the callback function to execute the slot.
                    let res = Some(callback(true, &slot, content.as_ref()));

                    // Update the SCE-final execution cursor.
                    self.latest_executed_final_slot = slot;

                    // If the speculative execution cursor is late on the SCE-final one, make it catch up.
                    self.latest_executed_candidate_slot = std::cmp::max(
                        self.latest_executed_candidate_slot,
                        self.latest_executed_final_slot,
                    );

                    // Clean the sequence from the executed CSS-final slot if it is not useful anymore.
                    self.cleanup_sequence();

                    // Return `Some(result of the callback)`.
                    return res;
                }
            }
        }

        // Here we know that there are no SCE-final slots to execute.

        // Low priority: execute the next candidate slot that is available for execution, if any.
        {
            // Get the slot just after the latest executed speculative slot.
            let slot = self
                .latest_executed_candidate_slot
                .get_next_slot(self.config.thread_count)
                .expect("overflow in slot iteration");

            // Check if that slot is before (or equal to) the time cursor, and available in the sequence.
            if self.get_time_cursor() >= slot {
                // The slot is ready for speculative execution.

                // Consider it a miss if it is absent from the sequence.
                let content = self.get_slot(&slot).and_then(|nfo| nfo.content.as_ref());

                // Call the `callback` function to execute the slot.
                let res = Some(callback(false, &slot, content));

                // Update the latest executed candidate slot cursor.
                self.latest_executed_candidate_slot = slot;

                // Return `Some(result of the callback)`.
                return res;
            }
        }

        // No slot was available for execution.
        None
    }

    /// Gets the instant of the slot just after the latest slot in the sequence.
    /// Note that `config.cursor_delay` is taken into account.
    pub fn get_next_slot_deadline(&self) -> MassaTime {
        // The slot sequence is empty.
        // This means that we are still waiting for `Self::update` to be called for the first time.
        // To avoid CPU-intensive loops upstream, just register a wake-up after a single slot delay (t0/T).
        if self.sequence.is_empty() {
            return MassaTime::now()
                .expect("could not get current time")
                .saturating_add(
                    self.config
                        .t0
                        .checked_div_u64(self.config.thread_count as u64)
                        .unwrap(),
                );
        }

        // Compute the next slot after the current time cursor.
        let next_slot = self
            .get_time_cursor()
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow in slot deadline computation");

        // Return the timestamp of that slot, shifted by the cursor delay.
        get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            next_slot,
        )
        .expect("could not compute slot timestamp")
        .saturating_add(self.config.cursor_delay)
    }
}
