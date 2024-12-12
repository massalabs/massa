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

            // take current input data, resetting it
            let input_data: EventCacheWriterInputData = std::mem::take(&mut input_data_lock);

            // Check if there is some input data
            if !input_data.events.is_empty() {
                return (input_data, false);
            }

            // if we need to stop, return None
            if input_data.stop {
                return (input_data, true);
            }

            self.input_data.0.wait(&mut input_data_lock);
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

            if stop {
                // we need to stop
                break;
            }

            {
                let mut lock = self.cache.write();
                lock.insert_multi_it(input_data.events.into_iter());
                // drop the lock as early as possible
                drop(lock);
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
