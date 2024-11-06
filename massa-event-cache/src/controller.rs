// std
use std::collections::VecDeque;
use std::sync::Arc;
// third-party
use parking_lot::{Condvar, Mutex, RwLock};
// internal
use crate::event_cache::EventCache;
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;

/// structure used to communicate with controller
#[derive(Debug)]
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

    /// Takes the current input data into a clone that is returned,
    /// and resets self.
    pub fn take(&mut self) -> Self {
        Self {
            stop: std::mem::take(&mut self.stop),
            events: std::mem::take(&mut self.events),
        }
    }
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
        input_data.events = events;
        // wake up VM loop
        self.input_data.0.notify_one();
    }

    fn get_filtered_sc_output_events(&self, filter: &EventFilter) -> Vec<SCOutputEvent> {
        let lock = self.cache.read();
        lock.get_filtered_sc_output_events(filter).collect()
    }
}
