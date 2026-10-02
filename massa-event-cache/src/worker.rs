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

    /// Waits until there is something to do: events to flush and/or a stop request.
    ///
    /// The input is not consumed here: see [`Self::main_loop`] for why the batch is only taken once
    /// the cache is locked.
    fn wait_for_input(&self) {
        let mut input_data_lock = self.input_data.1.lock();
        while input_data_lock.events.is_empty() && !input_data_lock.stop {
            self.input_data.0.wait(&mut input_data_lock);
        }
    }

    /// Main loop of the worker
    pub fn main_loop(&mut self) {
        loop {
            self.wait_for_input();

            // Lock the cache before taking the batch out of the queue. Readers look for events in
            // the queue, then in the cache. Taking the batch first left a window where the events
            // were in neither, so a reader could miss events that execution had already marked as
            // final. With the cache locked first, a reader that no longer finds them in the queue
            // waits on the cache lock until they are inserted. Readers never hold both locks, so
            // this order cannot deadlock.
            let mut cache = self.cache.write();

            // Take the whole input, resetting it. The stop flag comes with the events so that a
            // final queued batch is still flushed before the loop terminates.
            let input_data: EventCacheWriterInputData =
                std::mem::take(&mut *self.input_data.1.lock());
            debug!(
                "Event cache writer loop triggered, {} events, stop = {}",
                input_data.events.len(),
                input_data.stop
            );

            if !input_data.events.is_empty() {
                cache.insert_multi_it(input_data.events.into_iter());
            }
            // drop the lock as early as possible
            drop(cache);

            if input_data.stop {
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

    /// Reproduces the handoff race between the writer and readers: once the writer has taken a batch
    /// out of the queue, the events must stay visible to readers until they are in the cache.
    ///
    /// The test holds a read lock on the cache, so the writer cannot insert. A reader at that point
    /// must still find the saved event, either in the queue or in the cache. The writer used to empty
    /// the queue first and lock the cache afterwards: the event was in neither.
    #[test]
    fn saved_events_stay_visible_while_the_writer_waits_for_the_cache() {
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
        let input_data = Arc::new((Condvar::new(), Mutex::new(EventCacheWriterInputData::new())));
        let controller = EventCacheControllerImpl {
            input_data: input_data.clone(),
            cache: cache.clone(),
        };

        let mut worker = EventCacheWriterThread::new(input_data.clone(), cache.clone());
        let handle = thread::spawn(move || worker.main_loop());

        let filter = massa_models::execution::EventFilter::default();
        {
            // keep the writer from inserting
            let cache_read = cache.read();

            controller.save_events([sample_event()].into());
            // let the writer wake up and process the batch as far as it can
            thread::sleep(Duration::from_millis(200));

            let in_queue = !input_data.1.lock().events.is_empty();
            let in_cache = !cache_read
                .get_filtered_sc_output_events(&filter)
                .1
                .is_empty();
            assert!(
                in_queue || in_cache,
                "a saved event is neither in the queue nor in the cache"
            );
        }

        // once the cache is free, the writer inserts the event
        let deadline = Instant::now() + Duration::from_secs(10);
        while cache
            .read()
            .get_filtered_sc_output_events(&filter)
            .1
            .is_empty()
        {
            assert!(Instant::now() < deadline, "the event was never inserted");
            thread::sleep(Duration::from_millis(10));
        }

        {
            let mut input = input_data.1.lock();
            input.stop = true;
            input_data.0.notify_one();
        }
        handle.join().expect("event cache writer thread panicked");
    }
}
