// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module represents an event store allowing to store, search and retrieve
//! a config-limited number of execution-generated events

use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;
use std::collections::VecDeque;

/// Store for events emitted by smart contracts
#[derive(Default, Debug, Clone)]
pub struct EventStore(pub VecDeque<SCOutputEvent>);

impl EventStore {
    /// Push a new smart contract event to the store
    pub fn push(&mut self, event: SCOutputEvent) {
        self.0.push_back(event);
    }

    /// Take the event store
    pub(crate) fn take(&mut self) -> VecDeque<SCOutputEvent> {
        std::mem::take(&mut self.0)
    }

    /// Clear the event store
    pub(crate) fn clear(&mut self) {
        self.0.clear()
    }

    /// Prune the event store if its size is over the given limit
    pub fn prune(&mut self, max_events: usize) {
        while self.0.len() > max_events {
            self.0.pop_front();
        }
    }

    /// Extend the event store with another store
    pub fn extend(&mut self, other: EventStore) {
        self.0.extend(other.0.into_iter());
    }

    /// Set the events of this store as final
    pub fn finalize(&mut self) {
        for output in self.0.iter_mut() {
            output.context.is_final = true;
        }
    }

    /// Get events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    /// * is final
    pub fn get_filtered_sc_output_events(&self, filter: &EventFilter) -> VecDeque<SCOutputEvent> {
        self.0
            .iter()
            .filter(|x| {
                if let Some(start) = filter.start {
                    if x.context.slot < start {
                        return false;
                    }
                }
                if let Some(end) = filter.end {
                    if x.context.slot >= end {
                        return false;
                    }
                }
                if let Some(is_final) = filter.is_final {
                    if x.context.is_final != is_final {
                        return false;
                    }
                }
                if let Some(is_error) = filter.is_error {
                    if x.context.is_error != is_error {
                        return false;
                    }
                }
                match (filter.emitter_address, x.context.call_stack.front()) {
                    (Some(addr1), Some(addr2)) if addr1 != *addr2 => return false,
                    (Some(_), None) => return false,
                    _ => (),
                }
                match (filter.original_caller_address, x.context.call_stack.back()) {
                    (Some(addr1), Some(addr2)) if addr1 != *addr2 => return false,
                    (Some(_), None) => return false,
                    _ => (),
                }
                match (filter.original_operation_id, x.context.origin_operation_id) {
                    (Some(addr1), Some(addr2)) if addr1 != addr2 => return false,
                    (Some(_), None) => return false,
                    _ => (),
                }
                true
            })
            .cloned()
            .collect()
    }
}

#[test]
fn test_prune() {
    use massa_models::output_event::{EventExecutionContext, SCOutputEvent};
    use massa_models::slot::Slot;

    let mut store = EventStore(VecDeque::new());
    for i in 0..10 {
        store.push(SCOutputEvent {
            context: EventExecutionContext {
                slot: Slot::new(i, 0),
                block: None,
                read_only: false,
                index_in_slot: 1,
                call_stack: VecDeque::new(),
                origin_operation_id: None,
                is_final: false,
                is_error: false,
            },
            data: i.to_string(),
        });
    }
    assert_eq!(store.0.len(), 10);
    store.prune(3);
    assert_eq!(store.0.len(), 3);
    assert_eq!(store.0[2].data, "9");
    assert_eq!(store.0[1].data, "8");
    assert_eq!(store.0[0].data, "7");
}
