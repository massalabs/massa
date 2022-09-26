
//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module allowse Execution to manage slot sequencing

use std::collections::VecDeque;

use massa_models::{slot::Slot, block::BlockId, timeslots::get_latest_block_slot_at_timestamp};
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
    content: Option< (BlockId, Storage) >
}

/// Structure allowing execution slot sequence management
pub struct SlotSequence {
    /// Thread count
    thread_count: u8,
    /// Continuous sequence of slots. Should always contain at least the latest css final blocks of each thread.
    sequence: VecDeque<SlotInfo>,

    /// latest CSS-final slots (one per thread)
    latest_css_final_slots: Vec<Slot>,

    /// latest SCE-final slot
    latest_sce_final_slot: Slot
    
}

impl SlotSequence {
    /// Notify of incoming changes
    /// 
    /// # Arguments
    /// * `new_css_final_blocks`: new consensus-finalized blocks
    /// * `new_blockclique`: new blockclique (if changed)
    /// * `new_blocks_storage`: storage instances for previously unseen blocks
    /// 
    /// # Returns
    /// If there was a history truncation, returns the slot from which the history differs (included)
    pub fn update(&mut self, mut new_css_final_blocks: HashMap<Slot, BlockId>, new_blockclique: Option<HashMap<Slot, BlockId>>, new_blocks_storage: PreHashMap<BlockId, Storage>) -> Option<Slot> {
        // time cursor
        let shifted_now = MassaTime::now(self.compensation_millis).saturating_sub(self.execution_cursor_delay).expect("could not get current time");
        let time_cursor = get_latest_block_slot_at_timestamp(self.thread_count, self.t0, self.genesis_timestamp, self.timestamp).expect("could not get latest block slot at shifted execution time").unwrap_or_else(|| Slot::new(0, 0));

        // compute new latest css final slots
        let mut new_latest_css_final_slots = self.latest_css_final_slots.clone();
        for (s, _) in new_css_final_blocks.iter() {
            if s.period > new_latest_css_final_slots[s.thread as usize].period {
                new_latest_css_final_slots[s.thread as usize] = *s;
            }
        }

        // get the overall max slot to iterate to
        let max_slot = std::max(
            *new_latest_css_final_slots.iter().max().expect("new_latest_css_final_slots is empty"),
            new_blockclique.as_ref().and_then(|bq| bq.keys().max().copied()).unwrap_or_else(|| Slot::new(0, 0)),
            self.sequence.back().expect("sequence annot be empty").slot,
            time_cursor
        );

        // init the history truncation cursor
        let mut history_truncation = None;

        // iterate to rebuild a sequence
        let mut slot = self.sequence.front().expect("slot sequence should not be empty").slot;
        let new_seq_len = max_slot.slots_since(&slot, self.thread_count).expect("error computing new sequence length").saturating_add(1);
        let mut new_sequence: VecDeque<SlotInfo> = VecDeque::with_capacity(new_seq_len);
        while &slot < &max_slot {
            // slot is CSS-final if it is before or at a CSS-final slot in its own thread
            let mut new_css_final = &slot <= &new_latest_css_final_slots[slot.thread as usize];
            let new_css_final_block: Option<BlockId> = new_css_final_blocks.remove(slot);

            // check if the slot is in the blockclique
            let blockclique_updated = new_blockclique.is_some();
            let new_blockclique_block = new_blockclique.and_then(|bq| bq.remove(&slot));

            // build one step of the new sequence
            new_sequence.push_back(
                SlotSequence::sequence_build_step(slot, self.sequence.pop_front(), new_css_final, new_css_final_block, blockclique_updated, new_blockclique_block, &mut new_blocks_storage, &mut history_truncation)
            );

            // increment slot
            slot = slot.get_next_slot(self.thread_count).expect("overflow on slot iteration");
        }
        std::mem::drop(new_css_final_blocks);  // new_css_final_blocks should be empty: drop it to avoid mistakes
        self.latest_css_final_slots = new_latest_css_final_slots;
        
        // Process SCE finality
        {
            let mut index = self.get_slot_index(&self.latest_sce_final_slot).exepct("latest SCE final slot missing from sequence") + 1;
            while let Some(slot_info) = self.sequence.get_mut(index) {
                if !slot_info.css_final {
                    break;  // found a non-css-final slot => stop iterating
                }
                slot_info.sce_final = true;
                self.latest_sce_final_slot = slot_info.slot;
                index += 1;
            }
        }

        history_truncation
    }

    /// Builds one step of the new slot sequence.
    /// Does not process SCE finality.
    fn sequence_build_step(slot: Slot, prev_item: Option<SlotInfo>, new_css_final: bool, new_css_final_block: Option<BlockId>, blockclique_updated: bool, new_blockclique_block: Option<BlockId>, new_blocks_storage: &mut HashMap<BlockId, Storage>, history_truncation: &mut Option<Slot>) -> SlotInfo {
        // pop and match old sequence info on that slot
        if let Some(slot_info) = prev_item {
            // The slot was already present in the sequence.

            // The slot was already CSS-final
            if slot_info.css_final {
                // Keep it as is
                return slot_info;
            }

            // The slot was not CSS-final and needs to become CSS-final
            if !slot_info.css_final && new_css_final {
                // Ensure that there is a new blockclique from which this slot is absent
                if new_blockclique.as_ref().expect("a non-CSS-final block is being overwritten for CSS finalization and no new blockclique was provided").contains_key(&slot) {
                    panic!("a new blockclique was provided that contains a CSS-final block");
                }
                // Check if the content of the existing slot matches the incoming one.
                if slot_info.content.as_ref().map(|(b_id, _)| *b_id) == new_css_final_block {
                    // Contents match => simply mark as CSS-final and continue
                    slot_info.css_final = true;
                    return slot_info;
                }
                // Content mismatch.
                // Overwrite slot, mark history truncation and continue.
                slot_info.css_final = true;
                slot_info.content = new_css_final_block.map(|b_id| (b_id,  new_blocks_storage.remove(&b_id).expect("new css final block storage absent from new_blocks_storage")));
                *history_truncation = Some(slot);
                return slot_info;
            }
            

            // Process blockclique
            if !blockclique_updated {
                // If there is no new blockclique, simply replicate existing active block
                return slot_info;
            }
            // There is a new blockclique: check slot content matching
            if slot_info.content.as_ref().map(|(b_id, _)| *b_id) == new_blockclique_block {
                // Contents match => simply replicate existing slot
                return slot_info;
            }
            // Contents mismatch.
            // Update slot info, mark history truncation
            slot_info.content = new_bq_b_id.map(|b_id| (b_id,  new_blocks_storage.remove(&b_id).expect("new css blockclique block storage absent from new_blocks_storage")));
            *history_truncation = Some(slot);
            return slot_info;
        }
        
        // The slot was not in the previous sequence

        // Check if the slot is becoming CSS-final
        if new_css_final {
            return SlotInfo {
                slot,
                css_final: true,
                sce_final: false,
                content: new_css_final_block.map(|b_id| (
                    b_id,
                    new_blocks_storage.remove(&b_id).expect("new css final block storage absent from new_blocks_storage")
                ))
            };
        }

        // Append slot (getting values in the blockclique if necessary)
        let new_blockclique_block: Option<BlockId> = new_blockclique.and_then(|bq| bq.remove(&slot));
        return SlotInfo {
            slot,
            css_final: false,
            sce_final: false,
            content: new_blockclique_block.map(|b_id| (
                b_id,
                new_blocks_storage.remove(&b_id).expect("new css blockclique block storage absent from new_blocks_storage")
            ))
        };
    }

    /// Fill the current sequence with the result of a generator until a given slot (included).
    fn fill_until_with<F>(&mut self, until_slot: &Slot, generator: F)
    where
        F: Fn(&Slot) -> SlotInfo, 
    {
        let mut slot = self.sequence.back().expect("slot sequence should never be empty").slot;
        while &slot < until_slot {
            slot = template.slot.get_next_slot(self.thread_count).expect("overflow in slot iteration");
            self.sequence.push_back(generator(&slot));
        }
    }

    /// Get the index of a slot in the sequence
    fn get_slot_index(&self, slot: &Slot) -> Option<usize> {
        let first_slot = &self.sequence.front().expect("slot sequence should never be empty").slot;
        if slot < first_slot {
            return None;  // underflow
        }
        let index: usize = slot.slots_since(s, self.thread_count).expect("could not compute slots_since first slot in sequence").try_into().expect("usize overflow in sequence index");
        if index >= self.sequence.len() {
            return None;  // overflow
        }
        Some(index)
    }

    /// Gets an immutable reference to a SlotInfo
    pub fn get_slot(&self, slot: &Slot) -> Option<&SlotInfo> {
        self.get_slot_index(slot).and_then(|idx| &self.sequence.get(idx))
    }

    /// Gets a mutable reference to a SlotInfo
    pub fn get_slot_mut(&mut self, slot: &Slot) -> Option<&mut SlotInfo> {
        self.get_slot_index(slot).and_then(|idx| &self.sequence.get_mut(idx))
    }
}

