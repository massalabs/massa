// std
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
// third-party
use parking_lot::{Condvar, Mutex, RwLock};
// internal
use crate::event_cache::EventCache;
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;

/// structure used to communicate with controller
#[derive(Debug, Default)]
pub(crate) struct EventCacheWriterInputData {
    /// set stop to true to stop the thread
    pub stop: bool,
    pub(crate) events: VecDeque<SCOutputEvent>,
}

impl EventCacheWriterInputData {
    pub fn new() -> Self {
        Self {
            stop: Default::default(),
            events: Default::default(),
        }
    }

    /*
    /// Takes the current input data into a clone that is returned,
    /// and resets self.
    pub fn take(&mut self) -> Self {
        Self {
            stop: std::mem::take(&mut self.stop),
            events: std::mem::take(&mut self.events),
        }
    }
    */
}

/// interface that communicates with the worker thread
#[cfg_attr(feature = "test-exports", mockall_wrap::wrap, mockall::automock)]
pub trait EventCacheController: Send + Sync {
    fn save_events(&self, events: VecDeque<SCOutputEvent>);

    fn get_filtered_sc_output_events(&self, filter: &EventFilter) -> Vec<SCOutputEvent>;
}

#[derive(Clone)]
/// implementation of the event cache controller
pub struct EventCacheControllerImpl {
    /// input data to process in the VM loop
    /// with a wake-up condition variable that needs to be triggered when the data changes
    pub(crate) input_data: Arc<(Condvar, Mutex<EventCacheWriterInputData>)>,
    /// Event cache
    pub(crate) cache: Arc<RwLock<EventCache>>,
}

impl EventCacheController for EventCacheControllerImpl {
    fn save_events(&self, events: VecDeque<SCOutputEvent>) {
        // lock input data
        let mut input_data = self.input_data.1.lock();
        input_data.events.extend(events);
        // Wake up the condvar in EventCacheWriterThread waiting for events
        self.input_data.0.notify_all();
    }

    fn get_filtered_sc_output_events(&self, filter: &EventFilter) -> Vec<SCOutputEvent> {
        let mut res_0 = {
            // Read from new events first
            let lock_0 = self.input_data.1.lock();
            #[allow(clippy::unnecessary_filter_map)]
            let it = lock_0.events.iter().filter_map(|event| {
                if let Some(start) = filter.start {
                    if event.context.slot < start {
                        return None;
                    }
                }
                if let Some(end) = filter.end {
                    if event.context.slot >= end {
                        return None;
                    }
                }
                if let Some(is_final) = filter.is_final {
                    if event.context.is_final != is_final {
                        return None;
                    }
                }
                if let Some(is_error) = filter.is_error {
                    if event.context.is_error != is_error {
                        return None;
                    }
                }
                match (
                    filter.original_caller_address,
                    event.context.call_stack.front(),
                ) {
                    (Some(addr1), Some(addr2)) if addr1 != *addr2 => return None,
                    (Some(_), None) => return None,
                    _ => (),
                }
                match (filter.emitter_address, event.context.call_stack.back()) {
                    (Some(addr1), Some(addr2)) if addr1 != *addr2 => return None,
                    (Some(_), None) => return None,
                    _ => (),
                }
                match (
                    filter.original_operation_id,
                    event.context.origin_operation_id,
                ) {
                    (Some(addr1), Some(addr2)) if addr1 != addr2 => return None,
                    (Some(_), None) => return None,
                    _ => (),
                }
                Some(event)
            });

            let res_0: BTreeSet<SCOutputEvent> = it.cloned().collect();
            // Drop the lock on the queue as soon as possible to avoid deadlocks
            drop(lock_0);
            res_0
        };

        let res_1 = {
            // Read from db (on disk) events
            let lock = self.cache.read();
            let (_, res_1) = lock.get_filtered_sc_output_events(filter);
            // Drop the lock on the event cache db asap
            drop(lock);
            res_1
        };

        // Merge results
        let res_1: BTreeSet<SCOutputEvent> = BTreeSet::from_iter(res_1);
        res_0.extend(res_1);
        Vec::from_iter(res_0)
    }
}
