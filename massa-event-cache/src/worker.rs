// std
use std::sync::Arc;
use std::thread;
// third-party
// use massa_time::MassaTime;
use parking_lot::{Condvar, Mutex, RwLock};
use tracing::{debug, info};
// internal
use crate::config::EventCacheConfig;
use crate::controller::{
    EventCacheController, EventCacheControllerImpl, EventCacheWriterInputData,
};
use crate::event_cache::EventCache;

/// Structure gathering all elements needed by the event cache thread
pub(crate) struct EventCacheWriterThread {
    // A copy of the input data allowing access to incoming requests
    input_data: Arc<(Condvar, Mutex<EventCacheWriterInputData>)>,
    /// Event cache
    cache: Arc<RwLock<EventCache>>,
}

impl EventCacheWriterThread {
    fn new(
        input_data: Arc<(Condvar, Mutex<EventCacheWriterInputData>)>,
        event_cache: Arc<RwLock<EventCache>>,
    ) -> Self {
        Self {
            input_data,
            cache: event_cache,
        }
    }

    /// Waits for an event to trigger a new iteration in the event cache main loop.
    ///
    /// # Returns
    /// `ExecutionInputData` representing the input requests,
    /// and a boolean saying whether we should stop the loop.
    fn wait_loop_event(&mut self) -> (EventCacheWriterInputData, bool) {
        loop {
            // lock input data
            let mut input_data_lock = self.input_data.1.lock();

            // Only consume the shared input once there is something to do
            // (events to flush and/or a stop request). Taking the whole input
            // unconditionally used to consume the `stop` flag together with a
            // pending batch of events and then drop it when returning early,
            // which could make the writer wait forever after `stop()`.
            if input_data_lock.events.is_empty() && !input_data_lock.stop {
                self.input_data.0.wait(&mut input_data_lock);
                continue;
            }

            // take current input data, resetting it
            let input_data: EventCacheWriterInputData = std::mem::take(&mut *input_data_lock);
            // Propagate the (durable) stop flag alongside the taken events so a
            // final queued batch is still flushed before the loop terminates.
            let stop = input_data.stop;
            return (input_data, stop);
        }
    }

    /// Main loop of the worker
    pub fn main_loop(&mut self) {
        loop {
            let (input_data, stop) = self.wait_loop_event();
            debug!(
                "Event cache writer loop triggered, input_data = {:?}",
                input_data
            );

            // Always flush any queued events, even if this iteration also
            // observed a stop request, so no final batch is silently lost.
            if !input_data.events.is_empty() {
                let mut lock = self.cache.write();
                lock.insert_multi_it(input_data.events.into_iter());
                // drop the lock as early as possible
                drop(lock);
            }

            if stop {
                // we need to stop
                break;
            }
        }
    }
}

/// Event cache manager trait used to stop the event cache thread
pub trait EventCacheManager {
    /// Stop the event cache thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of `Box<dyn ExecutionManager>`
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}

/// ... manager
/// Allows stopping the ... worker
pub struct EventCacheWriterManagerImpl {
    /// input data to process in the VM loop
    /// with a wake-up condition variable that needs to be triggered when the data changes
    pub(crate) input_data: Arc<(Condvar, Mutex<EventCacheWriterInputData>)>,
    /// handle used to join the worker thread
    pub(crate) thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl EventCacheManager for EventCacheWriterManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("Stopping Execution controller...");
        // notify the worker thread to stop
        {
            let mut input_wlock = self.input_data.1.lock();
            input_wlock.stop = true;
            self.input_data.0.notify_one();
        }
        // join the thread
        if let Some(join_handle) = self.thread_handle.take() {
            join_handle.join().expect("VM controller thread panicked");
        }
        info!("Execution controller stopped");
    }
}

pub fn start_event_cache_writer_worker(
    cfg: EventCacheConfig,
) -> (Box<dyn EventCacheManager>, Box<dyn EventCacheController>) {
    let event_cache = Arc::new(RwLock::new(EventCache::new(
        cfg.event_cache_path.as_path(),
        cfg.max_event_cache_length,
        cfg.snip_amount,
        cfg.thread_count,
        cfg.max_call_stack_length,
        cfg.max_event_data_length,
        cfg.max_events_per_operation,
        cfg.max_operations_per_block,
        cfg.max_events_per_query,
    )));

    // define the input data interface
    let input_data = Arc::new((Condvar::new(), Mutex::new(EventCacheWriterInputData::new())));
    let input_data_clone = input_data.clone();

    // create a controller
    let controller = EventCacheControllerImpl {
        input_data: input_data.clone(),
        cache: event_cache.clone(),
    };

    let thread_builder = thread::Builder::new().name("event_cache".into());
    let thread_handle = thread_builder
        .spawn(move || {
            EventCacheWriterThread::new(input_data_clone, event_cache).main_loop();
        })
        .expect("failed to spawn thread : event_cache");

    // create a manager
    let manager = EventCacheWriterManagerImpl {
        input_data,
        thread_handle: Some(thread_handle),
    };

    // return the manager and controller pair
    (Box::new(manager), Box::new(controller))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::EventCacheWriterInputData;
    use crate::event_cache::EventCache;
    use massa_models::config::{
        MAX_EVENT_DATA_SIZE, MAX_EVENT_PER_OPERATION, MAX_OPERATIONS_PER_BLOCK,
        MAX_RECURSIVE_CALLS_DEPTH, THREAD_COUNT,
    };
    use massa_models::output_event::{EventExecutionContext, SCOutputEvent};
    use massa_models::slot::Slot;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn sample_event() -> SCOutputEvent {
        SCOutputEvent {
            context: EventExecutionContext {
                slot: Slot::new(1, 0),
                block: None,
                read_only: false,
                index_in_slot: 0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
                deferred_call_id: None,
                async_msg_id: None,
            },
            data: "shutdown-race event".to_string(),
        }
    }

    /// Reproduces the lost-stop-signal race: a final batch of events is queued
    /// *together with* a stop request. The writer must flush the batch and then
    /// terminate, instead of consuming the stop flag with the batch and waiting
    /// on the condvar forever.
    #[test]
    fn stop_is_not_lost_when_a_final_batch_is_queued() {
        let tmp = TempDir::new().unwrap();
        let cache = Arc::new(RwLock::new(EventCache::new(
            tmp.path(),
            1000,
            300,
            THREAD_COUNT,
            MAX_RECURSIVE_CALLS_DEPTH,
            MAX_EVENT_DATA_SIZE as u64,
            MAX_EVENT_PER_OPERATION as u64,
            MAX_OPERATIONS_PER_BLOCK as u64,
            5000,
        )));

        let mut input = EventCacheWriterInputData::new();
        input.events.push_back(sample_event());
        input.stop = true;
        let input_data = Arc::new((Condvar::new(), Mutex::new(input)));

        let mut worker = EventCacheWriterThread::new(input_data, cache);
        let handle = thread::spawn(move || worker.main_loop());

        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.is_finished() {
            assert!(
                Instant::now() < deadline,
                "event cache writer thread did not stop after a stop request"
            );
            thread::sleep(Duration::from_millis(10));
        }
        handle.join().expect("event cache writer thread panicked");
    }
}
