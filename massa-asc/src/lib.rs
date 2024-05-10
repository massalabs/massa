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
pub enum AsyncRegistryCallChange {
    Set(AsyncCall),
    Delete,
}

impl AsyncRegistryCallChange {
    pub fn merge(&mut self, other: AsyncRegistryCallChange) {
        *self = other;
    }

    pub fn delete_call(&mut self) {
        *self = AsyncRegistryCallChange::Delete;
    }

    pub fn set_call(&mut self, call: AsyncCall) {
        *self = AsyncRegistryCallChange::Set(call);
    }

    pub fn get_call(&self) -> Option<&AsyncCall> {
        match self {
            AsyncRegistryCallChange::Set(v) => Some(v),
            AsyncRegistryCallChange::Delete => None,
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct AsyncRegistrySlotChanges(BTreeMap<AsyncCallId, AsyncRegistryCallChange>);

impl AsyncRegistrySlotChanges {
    pub fn merge(&mut self, other: AsyncRegistrySlotChanges) {
        for (id, change) in other.0 {
            match self.0.entry(id) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(change);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(change);
                }
            }
        }
    }

    pub fn delete_call(&mut self, id: &AsyncCallId) {
        match self.0.entry(id.clone()) {
            std::collections::btree_map::Entry::Occupied(mut v) => v.get_mut().delete_call(),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(AsyncRegistryCallChange::Delete);
            }
        }
    }

    pub fn set_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.0.insert(id, AsyncRegistryCallChange::Set(call));
    }

    pub fn get_call(&self, id: &AsyncCallId) -> Option<&AsyncCall> {
        self.0.get(id).and_then(|change| change.get_call())
    }
}

#[derive(Default, Debug, Clone)]
pub struct AsyncRegistryChanges(pub BTreeMap<Slot, AsyncRegistrySlotChanges>);

impl AsyncRegistryChanges {
    pub fn merge(&mut self, other: AsyncRegistryChanges) {
        for (slot, changes) in other.0 {
            match self.0.entry(slot) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(changes);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(changes);
                }
            }
        }
    }

    pub fn delete_call(&mut self, target_slot: Slot, id: &AsyncCallId) {
        self.0.entry(target_slot).or_default().delete_call(id)
    }

    pub fn set_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.0
            .entry(call.target_slot.clone())
            .or_default()
            .set_call(id, call);
    }

    pub fn get_call(&self, target_slot: &Slot, id: &AsyncCallId) -> Option<&AsyncCall> {
        self.0
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_call(id))
    }
}

/// A structure that lists slot calls for a given slot
#[derive(Debug, Clone)]
pub struct AsyncSlotCallsMap {
    pub slot: Slot,
    pub calls: BTreeMap<AsyncCallId, AsyncCall>,
}

impl AsyncSlotCallsMap {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            calls: BTreeMap::new(),
        }
    }

    pub fn apply_changes(&mut self, changes: &AsyncRegistryChanges) {
        let Some(slot_changes) = changes.0.get(&self.slot) else {
            return;
        };
        for (id, change) in &slot_changes.0 {
            match change {
                AsyncRegistryCallChange::Set(call) => {
                    self.calls.insert(id.clone(), call.clone());
                }
                AsyncRegistryCallChange::Delete => {
                    self.calls.remove(id);
                }
            }
        }
    }
}
