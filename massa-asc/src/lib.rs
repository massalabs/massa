use massa_db_exports::ShareableMassaDBController;

/// This module implements a new version of the Autonomous Smart Contracts. (ASC)
/// This new version allow asynchronous calls to be registered for a specific slot and ensure his execution.
mod call;

pub use call::AsyncCall;
use massa_models::{asc_call_id::AsyncCallId, slot::Slot};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct AsyncCallRegistry {
    db: ShareableMassaDBController,
}

impl AsyncCallRegistry {
    pub fn new(db: ShareableMassaDBController) -> Self {
        Self { db }
    }

    pub fn get_slot_calls(slot: Slot) -> Vec<AsyncCall> {
        todo!()
    }

    pub fn get_message_by_id(id: AsyncCallId) -> Option<AsyncCall> {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub enum CallStatus {
    Emitted,
    Cancelled,
    Removed,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum AsyncRegistryCallChange {
    Emitted(AsyncCall),
    Cancelled(Option<AsyncCall>),
    Removed,
}

impl AsyncRegistryCallChange {
    pub fn merge(&mut self, other: AsyncRegistryCallChange) {
        match other {
            AsyncRegistryCallChange::Emitted(call) => {
                *self = AsyncRegistryCallChange::Emitted(call);
            }
            AsyncRegistryCallChange::Cancelled(mut opt_call) => {
                if opt_call.is_none() {
                    if let AsyncRegistryCallChange::Cancelled(v) = self {
                        // preserve the value if we have it already
                        opt_call = v.take();
                    }
                }
                *self = AsyncRegistryCallChange::Cancelled(opt_call);
            }
            AsyncRegistryCallChange::Removed => {
                *self = AsyncRegistryCallChange::Removed;
            }
        }
    }

    pub fn remove_call(&mut self) {
        *self = AsyncRegistryCallChange::Removed;
    }
}

#[derive(Default, Debug, Clone)]
pub struct AsyncRegistrySlotChanges {
    pub changes: BTreeMap<AsyncCallId, AsyncRegistryCallChange>,
}

impl AsyncRegistrySlotChanges {
    pub fn merge(&mut self, other: AsyncRegistrySlotChanges) {
        for (id, change) in other.changes {
            match self.changes.entry(id) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(change);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(change);
                }
            }
        }
    }

    /// Adds a "remove" change for the target
    pub fn remove_call(&mut self, id: &AsyncCallId) {
        match self.changes.entry(id.clone()) {
            std::collections::btree_map::Entry::Occupied(mut v) => v.get_mut().remove_call(),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(AsyncRegistryCallChange::Removed);
            }
        }
    }

    pub fn push_new_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.changes
            .insert(id, AsyncRegistryCallChange::Emitted(call));
    }

    pub fn cancel_call(&mut self, id: AsyncCallId) {
        self.changes
            .entry(id)
            .and_modify(|v| {
                let mut v_n = AsyncRegistryCallChange::Cancelled(None);
                std::mem::swap(v, &mut v_n);
                match v_n {
                    AsyncRegistryCallChange::Emitted(call) => {
                        *v = AsyncRegistryCallChange::Cancelled(Some(call));
                    }
                    AsyncRegistryCallChange::Cancelled(Some(call)) => {
                        *v = AsyncRegistryCallChange::Cancelled(Some(call));
                    }
                    AsyncRegistryCallChange::Cancelled(None) => {}
                    AsyncRegistryCallChange::Removed => {}
                }
            })
            .or_insert_with(|| AsyncRegistryCallChange::Cancelled(None));
    }
}

#[derive(Default, Debug, Clone)]
pub struct AsyncRegistryChanges {
    pub by_slot: BTreeMap<Slot, AsyncRegistrySlotChanges>,
}

impl AsyncRegistryChanges {
    pub fn merge(&mut self, other: AsyncRegistryChanges) {
        for (slot, changes) in other.by_slot {
            match self.by_slot.entry(slot) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(changes);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(changes);
                }
            }
        }
    }

    pub fn get_best_call(&self, slot: Slot) -> Option<(AsyncCallId, Option<AsyncCall>)> {
        if let Some(slot_changes) = self.by_slot.get(&slot) {
            for (id, change) in &slot_changes.changes {
                match change {
                    AsyncRegistryCallChange::Emitted(call) => {
                        return Some((id.clone(), Some(call.clone())));
                    }
                    AsyncRegistryCallChange::Cancelled(opt_call) => {
                        return Some((id.clone(), opt_call.clone()));
                    }
                    AsyncRegistryCallChange::Removed => {}
                }
            }
        }
        None
    }

    pub fn remove_call(&mut self, target_slot: Slot, id: &AsyncCallId) {
        self.by_slot.entry(target_slot).or_default().remove_call(id)
    }

    pub fn push_new_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.by_slot
            .entry(call.target_slot.clone())
            .or_default()
            .push_new_call(id, call);
    }

    pub fn call_status(&self, slot: Slot, id: &AsyncCallId) -> CallStatus {
        if let Some(slot_changes) = self.by_slot.get(&slot) {
            if let Some(change) = slot_changes.changes.get(id) {
                match change {
                    AsyncRegistryCallChange::Emitted(_) => CallStatus::Emitted,
                    AsyncRegistryCallChange::Cancelled(_) => CallStatus::Cancelled,
                    AsyncRegistryCallChange::Removed => CallStatus::Removed,
                }
            } else {
                CallStatus::Unknown
            }
        } else {
            CallStatus::Unknown
        }
    }

    pub fn cancel_call(&mut self, slot: Slot, id: AsyncCallId) {
        self.by_slot.entry(slot).or_default().cancel_call(id);
    }
}
