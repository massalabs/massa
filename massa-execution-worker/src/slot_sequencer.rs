//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module allows Execution to manage slot sequencing

use std::collections::{HashMap, VecDeque};

use massa_execution_exports::ExecutionConfig;
use massa_models::{
    block::BlockId,
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

/// Structure allowing execution slot sequence management
pub struct SlotSequencer {
    /// Config
    config: ExecutionConfig,

    /// Continuous sequence of slots. Should always contain at least the latest css final blocks of each thread.
    sequence: VecDeque<SlotInfo>,

    /// latest CSS-final slots (one per thread)
    latest_css_final_slots: Vec<Slot>,

    /// latest SCE-final slot
    latest_sce_final_slot: Slot,

    /// final slot execution cursor
    last_executed_final_slot: Slot,

    /// candidate slot execution cursor
    last_executed_candidate_slot: Slot,
}

impl SlotSequencer {
    /// Notify of incoming changes
    ///
    /// # Arguments
    /// * `new_css_final_blocks`: new consensus-finalized blocks
    /// * `new_blockclique`: new blockclique (if changed)
    /// * `new_blocks_storage`: storage instances for previously unseen blocks
    pub fn update(
        &mut self,
        mut new_css_final_blocks: HashMap<Slot, BlockId>,
        mut new_blockclique: Option<HashMap<Slot, BlockId>>,
        mut new_blocks_storage: PreHashMap<BlockId, Storage>,
    ) {
        // time cursor
        let shifted_now = MassaTime::now(self.config.clock_compensation)
            .expect("could not get current time")
            .saturating_sub(self.config.cursor_delay);
        let time_cursor = get_latest_block_slot_at_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            shifted_now,
        )
        .expect("could not get latest block slot at shifted execution time")
        .unwrap_or_else(|| Slot::new(0, 0));

        // compute new latest css final slots
        for (s, _) in new_css_final_blocks.iter() {
            if s.period > self.latest_css_final_slots[s.thread as usize].period {
                self.latest_css_final_slots[s.thread as usize] = *s;
            }
        }

        // get the overall max slot to iterate to
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
            std::cmp::max(
                self.sequence
                    .back()
                    .expect("slot sequence cannot be empty")
                    .slot,
                time_cursor,
            ),
        );

        // iterate to rebuild a sequence
        let mut slot = self
            .sequence
            .front()
            .expect("slot sequence should not be empty")
            .slot;
        let new_seq_len = max_slot
            .slots_since(&slot, self.config.thread_count)
            .expect("error computing new sequence length")
            .saturating_add(1);
        let mut new_sequence: VecDeque<SlotInfo> = VecDeque::with_capacity(new_seq_len as usize);
        let mut in_sce_finality = true; // we are still in the SCE finality
        while &slot < &max_slot {
            // slot is CSS-final if it is before or at a CSS-final slot in its own thread
            let mut new_css_final = &slot <= &self.latest_css_final_slots[slot.thread as usize];
            let new_css_final_block: Option<BlockId> = new_css_final_blocks.remove(&slot);

            // check if the slot is in the blockclique
            let blockclique_updated = new_blockclique.is_some();
            let new_blockclique_block = new_blockclique.and_then(|bq| bq.remove(&slot));

            // build one step of the new sequence
            let (seq_item, seq_item_overwrites_history) = SlotSequencer::sequence_build_step(
                slot,
                self.sequence.pop_front(),
                new_css_final,
                new_css_final_block,
                blockclique_updated,
                new_blockclique_block,
                &mut new_blocks_storage,
                in_sce_finality,
            );

            // this item is not SCE-final => all subsequent slots are not SCE-final
            in_sce_finality = in_sce_finality && seq_item.sce_final;

            // append the slot to the new sequence
            new_sequence.push_back(seq_item);

            // If the sequence item overwrites history before the execution cursor,
            // roll back the candidate execution cursor to the slot just before the earliest overwrite.
            if seq_item_overwrites_history && &self.last_executed_candidate_slot >= &slot {
                self.last_executed_candidate_slot = slot
                    .get_prev_slot(self.thread_count)
                    .expect("could not rollback speculative execution cursor");
            }

            // increment slot
            slot = slot
                .get_next_slot(self.thread_count)
                .expect("overflow on slot iteration");
        }
        // explicitly consume tainted containers to prevent mistakes caused by using them later
        if new_css_final_blocks.into_iter().next().is_some() {
            panic!("remaining elements in new_css_final_blocks after slot sequencing");
        }
        if let Some(bq) = new_blockclique {
            if !bq.is_empty() {
                panic!("remaining elements in new_blockclique after slot sequencing");
            }
        }
        std::mem::drop(new_blocks_storage);
    }

    /// Builds one step of the new slot sequence.
    ///
    /// # Returns
    /// A pair (SlotInfo, truncate_history: bool) where truncate_history indicates that this slot changes the content of an existing candidate slot
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
        // pop and match old sequence info on that slot
        if let Some(slot_info) = prev_item {
            // The slot was already present in the sequence.

            // The slot was already CSS-final
            if slot_info.css_final {
                // Keep it as is
                slot_info.sce_final = in_sce_finality;
                return (slot_info, false);
            }

            // The slot was not CSS-final and needs to become CSS-final
            if !slot_info.css_final && new_css_final {
                // Check if the content of the existing slot matches the incoming one.
                if slot_info.content.as_ref().map(|(b_id, _)| *b_id) == new_css_final_block {
                    // Contents match => simply mark as CSS-final and continue
                    slot_info.css_final = true;
                    slot_info.sce_final = in_sce_finality;
                    return (slot_info, false);
                }
                // Content mismatch.
                // Overwrite slot, mark history truncation and continue.
                slot_info.css_final = true;
                slot_info.sce_final = in_sce_finality;
                slot_info.content = new_css_final_block.map(|b_id| {
                    (
                        b_id,
                        new_blocks_storage
                            .remove(&b_id)
                            .expect("new css final block storage absent from new_blocks_storage"),
                    )
                });
                return (slot_info, true);
            }

            // Process blockclique
            if !blockclique_updated {
                // If there is no new blockclique, simply replicate existing active block
                return (slot_info, false);
            }
            // There is a new blockclique: check slot content matching
            if slot_info.content.as_ref().map(|(b_id, _)| *b_id) == new_blockclique_block {
                // Contents match => simply replicate existing slot
                return (slot_info, false);
            }
            // Contents mismatch.
            // Update slot info, mark history truncation
            slot_info.content = new_blockclique_block.map(|b_id| {
                (
                    b_id,
                    new_blocks_storage
                        .remove(&b_id)
                        .expect("new css blockclique block storage absent from new_blocks_storage"),
                )
            });
            return (slot_info, true);
        }

        // The slot was not in the previous sequence

        // Check if the slot is becoming CSS-final
        if new_css_final {
            let slot_info = SlotInfo {
                slot,
                css_final: true,
                sce_final: in_sce_finality,
                content: new_css_final_block.map(|b_id| {
                    (
                        b_id,
                        new_blocks_storage
                            .remove(&b_id)
                            .expect("new css final block storage absent from new_blocks_storage"),
                    )
                }),
            };
            return (slot_info, false);
        }

        // Append slot (getting values in the blockclique if necessary)
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
        (slot_info, false)
    }

    /// Get the index of a slot in the sequence
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
            .slots_since(first_slot, self.thread_count)
            .expect("could not compute slots_since first slot in sequence")
            .try_into()
            .expect("usize overflow in sequence index");
        if index >= self.sequence.len() {
            return None; // overflow
        }
        Some(index)
    }

    /// Gets an immutable reference to a SlotInfo
    fn get_slot(&self, slot: &Slot) -> Option<&SlotInfo> {
        self.get_slot_index(slot)
            .and_then(|idx| &self.sequence.get(idx))
    }

    /// Gets a mutable reference to a SlotInfo
    fn get_slot_mut(&mut self, slot: &Slot) -> Option<&mut SlotInfo> {
        self.get_slot_index(slot)
            .and_then(|idx| &self.sequence.get_mut(idx))
    }

    /// Returns true if a task is available
    pub fn is_task_available(&self) -> bool {
        // check if the next SCE-final slot is available for execution
        {
            let next_sce_final_slot = self
                .last_executed_final_slot
                .get_next_slot(self.thread_count)
                .expect("overflow in slot iteration");
            let finalization_task_available = self
                .get_slot(&next_sce_final_slot)
                .map_or(false, |s_info| s_info.sce_final);
            if finalization_task_available {
                return true;
            }
        }

        // check if the next speculative slot is available for execution
        {
            let next_candidate_slot = self
                .last_executed_candidate_slot
                .get_next_slot(self.thread_count)
                .expect("overflow in slot iteration");
            let candidate_task_available = self.get_slot(&next_candidate_slot).is_some();
            if candidate_task_available {
                return true;
            }
        }

        false
    }

    /// Clean the slot sequence by removing slots that are not useful anymore
    fn cleanup_sequence(&mut self) {
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
                self.last_executed_final_slot,
                self.last_executed_candidate_slot,
            ),
        );
        while let Some(slot_info) = self.sequence.front() {
            if slot_info.slot >= min_useful_slot {
                break;
            }
            self.sequence.pop_front();
        }
    }

    /// Take a task to process if there is any available, and call a callback function on it.
    ///
    pub fn run_task_with<F, T>(&mut self, callback: F) -> Option<T>
    where
        F: Fn(bool, &Slot, &Option<(BlockId, Storage)>) -> T,
    {
        // check if the next SCE-final slot is available for execution
        {
            let slot = self
                .last_executed_final_slot
                .get_next_slot(self.thread_count)
                .expect("overflow in slot iteration");
            if let Some(SlotInfo { content, .. }) = self.get_slot(&slot) {
                self.last_executed_final_slot = slot;
                self.last_executed_candidate_slot = std::cmp::max(
                    self.last_executed_candidate_slot,
                    self.last_executed_final_slot,
                );
                let res = Some(callback(true, &slot, content));
                self.cleanup_sequence();
                return res;
            }
        }

        // check if the next candidate slot is available for execution
        {
            let slot = self
                .last_executed_candidate_slot
                .get_next_slot(self.thread_count)
                .expect("overflow in slot iteration");
            if let Some(SlotInfo { content, .. }) = self.get_slot(&slot) {
                self.last_executed_candidate_slot = slot;
                return Some(callback(false, &slot, content));
            }
        }

        None
    }

    /// Gets the instant of the slot just after the latest slot in the sequence.
    /// Note that `config.cursor_delay` is taken into account.
    pub fn get_next_slot_deadline(&self) -> MassaTime {
        let next_slot = self
            .sequence
            .back()
            .expect("slot sequence should not be empty")
            .slot
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow in slto deadline computation");
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
