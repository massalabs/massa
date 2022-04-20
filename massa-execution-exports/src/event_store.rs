// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module represents an event store allowing to store, search and retrieve
//! a config-limited number of execution-generated events

use massa_models::api::EventFilter;
use massa_models::output_event::SCOutputEvent;
use std::collections::VecDeque;
use tracing::warn;

/// Store for events emitted by smart contracts
#[derive(Default, Debug, Clone)]
pub struct EventStore(VecDeque<SCOutputEvent>);

impl EventStore {
    /// Push a new smart contract event to the store
    pub fn push(&mut self, event: SCOutputEvent) {
        if self.0.iter().any(|x| x.id == event.id) {
            // push on front to avoid reversing the queue when truncating
            self.0.push_front(event);
        } else {
            // emit a warning when the event already exists
            warn!("event store already contains this event: {}", event.id);
        }
    }

    /// Take the event store
    pub fn take(&mut self) -> VecDeque<SCOutputEvent> {
        std::mem::take(&mut self.0)
    }

    /// Clear the event store
    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// Truncate the event store if its size is over the given limit
    pub fn truncate(&mut self, max_final_events: usize) {
        if self.0.len() > max_final_events {
            self.truncate(max_final_events);
        }
    }

    /// Extend the event store with another store
    pub fn extend(&mut self, other: EventStore) {
        self.0.extend(other.0.into_iter());
    }

    /// Get events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    pub fn get_filtered_sc_output_event(&self, filter: EventFilter) -> VecDeque<SCOutputEvent> {
        let mut filtered = self.0.clone();
        filtered.retain(|x| {
            let mut state = true;
            let p: u64 = x.context.slot.period;
            let t: u8 = x.context.slot.thread;
            match filter.start {
                Some(start) if p < start.period || (p == start.period && t < start.thread) => {
                    state = false;
                }
                _ => (),
            };
            match filter.end {
                Some(end) if p > end.period || (p == end.period && t > end.thread) => state = false,
                _ => (),
            };
            if filter.emitter_address.as_ref() != x.context.call_stack.front() {
                state = false;
            }
            if filter.original_caller_address.as_ref() != x.context.call_stack.back() {
                state = false;
            }
            if filter.original_operation_id != x.context.origin_operation_id {
                state = false;
            }
            state
        });
        filtered
    }
}
