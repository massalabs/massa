// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{address::Address, operation::OperationId, slot::Slot};
use serde::{Deserialize, Serialize};

/// Block status within the graph
#[derive(Eq, PartialEq, Debug, Deserialize, Serialize)]
pub enum BlockGraphStatus {
    /// received but not yet graph-processed
    Incoming,
    /// waiting for its slot
    WaitingForSlot,
    /// waiting for a missing dependency
    WaitingForDependencies,
    /// active in alternative cliques
    ActiveInAlternativeCliques,
    /// active in blockclique
    ActiveInBlockclique,
    /// forever applies
    Final,
    /// discarded for any reason
    Discarded,
    /// not found in graph
    NotFound,
}

/// filter used when retrieving SC output events
#[derive(Default, Debug, Deserialize, Clone, Serialize)]
pub struct EventFilter {
    /// optional start slot
    pub start: Option<Slot>,
    /// optional end slot
    pub end: Option<Slot>,
    /// optional emitter address
    pub emitter_address: Option<Address>,
    /// optional caller address
    pub original_caller_address: Option<Address>,
    /// optional operation id
    pub original_operation_id: Option<OperationId>,
    /// optional event status
    ///
    /// Some(true) means final
    /// Some(false) means candidate
    /// None means final _and_ candidate
    pub is_final: Option<bool>,
    /// optional execution status
    ///
    /// Some(true) means events coming from a failed sc execution
    /// Some(false) means events coming from a succeeded sc execution
    /// None means both
    pub is_error: Option<bool>,
}

/// When an address is drawn to create an endorsement it is selected for a specific index
#[derive(Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct IndexedSlot {
    /// slot
    pub slot: Slot,
    /// endorsement index in the slot
    pub index: usize,
}

impl std::fmt::Display for IndexedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Slot: {}, Index: {}", self.slot, self.index)
    }
}
