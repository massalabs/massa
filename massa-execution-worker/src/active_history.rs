use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncMessageUpdate};
use massa_execution_exports::ExecutionOutput;
use massa_ledger_exports::{LedgerEntry, LedgerEntryUpdate};
use massa_models::denunciation::DenunciationIndex;
use massa_models::prehash::{CapacityAllocator, PreHashMap, PreHashSet};
use massa_models::types::{Applicable, SetOrDelete, SetOrKeep, SetUpdateOrDelete};
use massa_models::{
    address::Address, amount::Amount, bytecode::Bytecode, operation::OperationId, slot::Slot,
};
use massa_pos_exports::DeferredCredits;
use std::collections::VecDeque;

#[derive(Default)]
/// History of the outputs of recently executed slots.
/// Slots should be consecutive, oldest at the beginning and latest at the back.
pub(crate) struct ActiveHistory(pub VecDeque<ExecutionOutput>);

/// Result of a lazy, active history search
pub enum HistorySearchResult<T> {
    Present(T),
    Absent,
    NoInfo,
}

/// Result of the search for a slot index in history
#[derive(Debug)]
pub enum SlotIndexPosition {
    /// out of bounds in the past
    Past,
    /// out of bounds in the future
    Future,
    /// found in history at a given index
    Found(usize),
    /// history is empty
    NoHistory,
}

impl ActiveHistory {
    /// Remove `slot` and the slots after it from history
    pub fn truncate_from(&mut self, slot: &Slot, thread_count: u8) {
        match self.get_slot_index(slot, thread_count) {
            SlotIndexPosition::Past => self.0.clear(),
            SlotIndexPosition::Found(index) => self.0.truncate(index),
            _ => {}
        }
    }

    /// Lazily query (from end to beginning) the active list of executed ops to check if an op was executed.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_executed_op(&self, op_id: &OperationId) -> HistorySearchResult<()> {
        for history_element in self.0.iter().rev() {
            if history_element
                .state_changes
                .executed_ops_changes
                .contains_key(op_id)
            {
                return HistorySearchResult::Present(());
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Lazily query (from end to beginning) a message based on its id
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_message(
        &self,
        message_id: &AsyncMessageId,
        mut current_updates: AsyncMessageUpdate,
        execution_compound_version: u32,
    ) -> HistorySearchResult<SetUpdateOrDelete<AsyncMessage, AsyncMessageUpdate>> {
        for history_element in self.0.iter().rev() {
            match history_element
                .state_changes
                .async_pool_changes
                .0
                .get(message_id)
            {
                Some(SetUpdateOrDelete::Set(msg)) => {
                    let mut msg = msg.clone();
                    msg.apply(current_updates);
                    return HistorySearchResult::Present(SetUpdateOrDelete::Set(msg));
                }
                Some(SetUpdateOrDelete::Update(msg_update)) => match execution_compound_version {
                    0 => {
                        current_updates.apply(msg_update.clone());
                    }
                    _ => {
                        let mut combined_message_update = msg_update.clone();
                        combined_message_update.apply(current_updates);
                        current_updates = combined_message_update;
                    }
                },
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                _ => (),
            }
        }

        // Note:
        // Return Present here as we can have a message in the final state and only an update
        // in the active history. So in this case, we return the current updates
        HistorySearchResult::Present(SetUpdateOrDelete::Update(current_updates))
    }

    /// Lazily query (from end to beginning) the active list of executed denunciations.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_executed_denunciation(
        &self,
        de_idx: &DenunciationIndex,
    ) -> HistorySearchResult<()> {
        for history_element in self.0.iter().rev() {
            if history_element
                .state_changes
                .executed_denunciations_changes
                .contains(de_idx)
            {
                return HistorySearchResult::Present(());
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Lazily query (from end to beginning) the active balance of an address after a given index.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_balance(&self, addr: &Address) -> HistorySearchResult<Amount> {
        for output in self.0.iter().rev() {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => return HistorySearchResult::Present(v.balance),
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    balance: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Present(*v),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                _ => (),
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Lazily query (from end to beginning) the active bytecode of an address after a given index.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_bytecode(&self, addr: &Address) -> HistorySearchResult<Bytecode> {
        for output in self.0.iter().rev() {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => {
                    return HistorySearchResult::Present(v.bytecode.clone())
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Present(v.clone()),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                _ => (),
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Lazily query (from end to beginning) the active datastore entry of an address after a given index.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_active_history_data_entry(
        &self,
        addr: &Address,
        key: &[u8],
    ) -> HistorySearchResult<Vec<u8>> {
        for output in self.0.iter().rev() {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(LedgerEntry { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(value) => return HistorySearchResult::Present(value.to_vec()),
                        None => return HistorySearchResult::Absent,
                    }
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(SetOrDelete::Set(value)) => {
                            return HistorySearchResult::Present(value.to_vec())
                        }
                        Some(SetOrDelete::Delete) => return HistorySearchResult::Absent,
                        None => (),
                    }
                }
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                None => (),
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Starting from the newest element in history, return the first existing roll change of `addr`.
    ///
    /// # Arguments
    /// * `addr`: address to fetch the rolls from
    pub fn fetch_roll_count(&self, addr: &Address) -> Option<u64> {
        self.0.iter().rev().find_map(|output| {
            output
                .state_changes
                .pos_changes
                .roll_changes
                .get(addr)
                .cloned()
        })
    }

    /// Gets all the deferred credits that will be credited until a given slot (included)
    pub fn get_all_deferred_credits_until(&self, slot: &Slot) -> DeferredCredits {
        self.0.iter().fold(DeferredCredits::new(), |mut acc, e| {
            acc.extend(
                e.state_changes
                    .pos_changes
                    .deferred_credits
                    .get_slot_range(..=slot),
            );
            acc
        })
    }

    /// Gets the deferred credits for a given address that will be credited at a given slot
    pub(crate) fn get_address_deferred_credit_for(
        &self,
        addr: &Address,
        slot: &Slot,
    ) -> Option<Amount> {
        for hist_item in self.0.iter().rev() {
            if let Some(v) = hist_item
                .state_changes
                .pos_changes
                .deferred_credits
                .get_address_credits_for_slot(addr, slot)
            {
                return Some(v);
            }
        }
        None
    }

    /// Gets the execution trail hash
    pub fn get_execution_trail_hash(&self) -> HistorySearchResult<massa_hash::Hash> {
        for history_element in self.0.iter().rev() {
            if let SetOrKeep::Set(hash) = history_element.state_changes.execution_trail_hash_change
            {
                return HistorySearchResult::Present(hash);
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Gets the index of a slot in history
    pub fn get_slot_index(&self, slot: &Slot, thread_count: u8) -> SlotIndexPosition {
        let first_slot = match self.0.front() {
            Some(itm) => &itm.slot,
            None => return SlotIndexPosition::NoHistory,
        };
        if slot < first_slot {
            return SlotIndexPosition::Past; // too old
        }
        let index: usize = match slot.slots_since(first_slot, thread_count) {
            Err(_) => return SlotIndexPosition::Past, // overflow
            Ok(d) => {
                match d.try_into() {
                    Ok(d) => d,
                    Err(_) => return SlotIndexPosition::Future, // usize overflow
                }
            }
        };
        if index >= self.0.len() {
            // in the future
            return SlotIndexPosition::Future;
        }
        SlotIndexPosition::Found(index)
    }

    /// Find the history range of a cycle
    ///
    /// # Return value
    ///
    /// Tuple with the following elements:
    /// * a range of indices
    /// * a boolean indicating that the cycle overflows before the beginning of history
    /// * a boolean indicating that the cycle overflows after the end of history
    pub fn find_cycle_indices(
        &self,
        cycle: u64,
        periods_per_cycle: u64,
        thread_count: u8,
    ) -> (std::ops::Range<usize>, bool, bool) {
        // Get first and last slots indices of cycle in history. We consider overflows as "Future"
        let first_index = Slot::new_first_of_cycle(cycle, periods_per_cycle).map_or_else(
            |_e| SlotIndexPosition::Future,
            |s| self.get_slot_index(&s, thread_count),
        );
        let last_index = Slot::new_last_of_cycle(cycle, periods_per_cycle, thread_count)
            .map_or_else(
                |_e| SlotIndexPosition::Future,
                |s| self.get_slot_index(&s, thread_count),
            );

        match (first_index, last_index) {
            // history is empty
            (SlotIndexPosition::NoHistory, _) => (0..0, true, true),
            (_, SlotIndexPosition::NoHistory) => (0..0, true, true),

            // cycle starts in the future
            (SlotIndexPosition::Future, _) => (0..0, false, true),

            // cycle ends before history starts
            (_, SlotIndexPosition::Past) => (0..0, true, false),

            // the history is strictly included within the cycle
            (SlotIndexPosition::Past, SlotIndexPosition::Future) => (0..self.0.len(), true, true),

            // cycle begins before and ends during history
            (SlotIndexPosition::Past, SlotIndexPosition::Found(idx)) => {
                (0..idx.saturating_add(1), true, false)
            }

            // cycle starts during the history and ends after the end of history
            (SlotIndexPosition::Found(idx), SlotIndexPosition::Future) => {
                (idx..self.0.len(), false, true)
            }

            // cycle starts and ends during active history
            (SlotIndexPosition::Found(idx1), SlotIndexPosition::Found(idx2)) => {
                (idx1..idx2.saturating_add(1), false, false)
            }
        }
    }

    /// Get the execution statuses of a set of operations.
    /// Returns a list where each element is None if no execution was found for that op,
    /// or a boolean indicating whether the execution was successful (true) or had an error (false).
    pub fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<Option<bool>> {
        let mut to_find: PreHashSet<OperationId> = batch.iter().copied().collect();
        let mut found = PreHashMap::with_capacity(to_find.len());
        for hist_item in self.0.iter().rev() {
            to_find.retain(|op_id| {
                if let Some((success, _expiry_slot)) =
                    hist_item.state_changes.executed_ops_changes.get(op_id)
                {
                    found.insert(*op_id, *success);
                    false
                } else {
                    true
                }
            });
            if to_find.is_empty() {
                break;
            }
        }
        batch
            .iter()
            .map(|op_id| found.get(op_id).copied())
            .collect()
    }
}

#[cfg(test)]
mod test {
    // std
    use std::cmp::Reverse;
    use std::collections::BTreeMap;
    use std::fmt::Formatter;
    use std::str::FromStr;
    // third-party
    use assert_matches::assert_matches;
    use num::rational::Ratio;
    // internal
    use super::*;
    use massa_async_pool::AsyncPoolChanges;
    use massa_final_state::StateChanges;

    impl<T: std::fmt::Debug> std::fmt::Debug for HistorySearchResult<T> {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                HistorySearchResult::Present(obj) => write!(f, "{:?}", obj),
                HistorySearchResult::Absent => write!(f, "Absent"),
                HistorySearchResult::NoInfo => write!(f, "NoInfo"),
            }
        }
    }

    #[test]
    fn test_fetch_message() {
        // Unit testing ActiveHistory.fetch_msg

        // Setup some addresses

        let addr_sender =
            Address::from_str("AU12fZLkHnLED3okr8Lduyty7dz9ZKkd24xMCc2JJWPcdmfn2eUEx").unwrap();
        let addr_dest =
            Address::from_str("AS12fZLkHnLED3okr8Lduyty7dz9ZKkd24xMCc2JJWPcdmfn2eUEx").unwrap();

        // Create 3 async messages (+ 3 message id's)

        let rev: Reverse<Ratio<u64>> = Default::default();
        let message_id: AsyncMessageId = (rev, Slot::new(1, 0), 1u64);
        let emission_slot_2 = Slot::new(1, 0);
        let emission_index_2 = 2;
        let message_id_2: AsyncMessageId = (rev, emission_slot_2, emission_index_2);
        let msg_2 = AsyncMessage::new(
            emission_slot_2,
            emission_index_2,
            addr_sender,
            addr_dest,
            "send_fee_to".to_string(), // SC function name
            0,
            Default::default(),
            Default::default(),
            Slot::new(3, 0),
            Slot::new(4, 0),
            vec![],
            None,
            None,
        );

        let emission_slot_3 = Slot::new(2, 0);
        let emission_index_3 = 3;
        let message_id_3: AsyncMessageId = (rev, emission_slot_3, emission_index_3);
        let msg_3 = AsyncMessage::new(
            emission_slot_3,
            emission_index_3,
            addr_sender,
            addr_dest,
            "send_max_fee_to".to_string(), // SC function name
            0,
            Default::default(),
            Default::default(),
            Slot::new(4, 0),
            Slot::new(5, 0),
            vec![],
            None,
            None,
        );

        let msg_3_function_new_2 = "send_max_fee_to_v2".to_string();
        let msg_3_coins_new = Amount::from_raw(1000);
        let msg_update_3_2 = AsyncMessageUpdate {
            coins: SetOrKeep::Set(msg_3_coins_new),
            function: SetOrKeep::Set(msg_3_function_new_2.clone()),
            ..Default::default()
        };

        let emission_slot_3_2 = Slot::new(2, 2);
        let emission_index_3_2 = 4;
        let message_id_3_2: AsyncMessageId = (rev, emission_slot_3_2, emission_index_3_2);

        // Put them in 2 async pool changes

        let async_pool_changes_1 = AsyncPoolChanges(BTreeMap::from([
            (message_id, SetUpdateOrDelete::Delete),
            (message_id_2, SetUpdateOrDelete::Set(msg_2.clone())),
            (message_id_3, SetUpdateOrDelete::Set(msg_3.clone())),
        ]));
        let async_pool_changes_2 = AsyncPoolChanges(BTreeMap::from([(
            message_id_3,
            SetUpdateOrDelete::Update(msg_update_3_2.clone()),
        )]));

        // Then put into 2 state changes

        let state_changes_1 = StateChanges {
            async_pool_changes: async_pool_changes_1,
            ..Default::default()
        };
        let state_changes_2 = StateChanges {
            async_pool_changes: async_pool_changes_2,
            ..Default::default()
        };

        let exec_output_1 = ExecutionOutput {
            slot: Slot::new(1, 0),
            block_info: None,
            state_changes: state_changes_1.clone(),
            events: Default::default(),
            #[cfg(feature = "execution-trace")]
            slot_trace: None,
            #[cfg(feature = "dump-block")]
            storage: None,
            deferred_credits_execution: vec![],
            cancel_async_message_execution: vec![],
            auto_sell_execution: vec![],
        };
        let exec_output_2 = ExecutionOutput {
            slot: Slot::new(1, 1),
            block_info: None,
            state_changes: state_changes_2.clone(),
            events: Default::default(),
            #[cfg(feature = "execution-trace")]
            slot_trace: None,
            #[cfg(feature = "dump-block")]
            storage: None,
            deferred_credits_execution: vec![],
            cancel_async_message_execution: vec![],
            auto_sell_execution: vec![],
        };

        let active_history = ActiveHistory(VecDeque::from([exec_output_1, exec_output_2]));

        // Test fetch_message with message_id (expect HistorySearchResult::Absent)
        {
            let current_updates = AsyncMessageUpdate::default();
            let fetched = active_history.fetch_message(&message_id, current_updates, 1);
            assert_matches!(fetched, HistorySearchResult::Absent);
        }

        // Test fetch_message with message_id_2 (expect HistorySearchResult::Set)
        {
            let current_updates = AsyncMessageUpdate::default();
            let fetched = active_history.fetch_message(&message_id_2, current_updates, 1);

            if let HistorySearchResult::Present(SetUpdateOrDelete::Set(msg)) = fetched {
                assert_eq!(msg, msg_2);
            } else {
                panic!(
                    "Expected a HistorySearchRestul::Set(...) and not: {:?}",
                    fetched
                )
            }
        }

        {
            // Test fetch_message with message_id_2 (expect HistorySearchResult::Set) + current_updates
            // (which modifies validity_end)

            let validity_end_new = Slot::new(5, 0);
            let current_updates = AsyncMessageUpdate {
                validity_end: SetOrKeep::Set(validity_end_new),
                ..Default::default()
            };
            let fetched = active_history.fetch_message(&message_id_2, current_updates, 1);

            if let HistorySearchResult::Present(SetUpdateOrDelete::Set(msg)) = fetched {
                assert_ne!(msg, msg_2);
                assert_eq!(msg.validity_end, Slot::new(5, 0));
            } else {
                panic!(
                    "Expected a HistorySearchRestul::Set(...) and not: {:?}",
                    fetched
                )
            }
        }

        // Test fetch_message with message_id_3 (expect HistorySearchResult::Present)
        {
            let current_updates = AsyncMessageUpdate::default();
            let fetched = active_history.fetch_message(&message_id_3, current_updates, 1);

            if let HistorySearchResult::Present(SetUpdateOrDelete::Set(msg)) = fetched {
                // Check the updates were applied correctly
                assert_eq!(msg.coins, msg_3_coins_new);
                // function should == "send_max_fee_to_v2" (latest value) and not "send_max_fee_to"
                assert_eq!(msg.function, msg_3_function_new_2);
            } else {
                panic!(
                    "Expected a HistorySearchResult::Set(...) and not: {:?}",
                    fetched
                );
            }
        }

        // Test fetch_message with message_id_3_2 (expect HistorySearchResult::Update)
        // Expect updates to be empty (or default) here
        {
            let current_updates = AsyncMessageUpdate::default();
            let fetched = active_history.fetch_message(&message_id_3_2, current_updates, 1);
            if let HistorySearchResult::Present(SetUpdateOrDelete::Update(updates)) = fetched {
                assert_eq!(updates, AsyncMessageUpdate::default());
            } else {
                panic!(
                    "Expected a HistorySearchResult::Present(...) and not: {:?}",
                    fetched
                );
            }
        }
    }
}
