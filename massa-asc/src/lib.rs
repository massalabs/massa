use massa_db_exports::ShareableMassaDBController;

/// This module implements a new version of the Autonomous Smart Contracts. (ASC)
/// This new version allow asynchronous calls to be registered for a specific slot and ensure his execution.
mod call;

pub use call::AsyncCall;
use massa_models::{asc_call_id::AsyncCallId, slot::Slot};
use std::collections::BTreeMap;

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

pub struct AsyncRegistryChanges {
    pub by_slot: BTreeMap<Slot, AsyncRegistrySlotChanges>,
}

pub struct AsyncRegistrySlotChanges {
    pub changes: BTreeMap<AsyncCallId, TODO>,
}
