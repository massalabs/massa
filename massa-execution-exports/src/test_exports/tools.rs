// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::EventStore;
use massa_models::output_event::{EventExecutionContext, SCOutputEvent};
use massa_models::slot::Slot;
use rand::Rng;

use std::collections::VecDeque;

/// generates a random eventstore
pub fn get_random_eventstore(limit: u64) -> EventStore {
    let mut rng = rand::thread_rng();
    let mut store = EventStore(VecDeque::new());

    for i in 0..limit {
        let is_final: bool = rng.gen();
        let is_error = !is_final;
        store.push(SCOutputEvent {
            context: EventExecutionContext {
                slot: Slot::new(i, 0),
                block: None,
                read_only: false,
                index_in_slot: 1,
                call_stack: VecDeque::new(),
                origin_operation_id: None,
                is_final,
                is_error,
            },
            data: i.to_string(),
        });
    }

    store
}
