//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module allows launching the execution worker thread, returning objects to communicate with it.
//! The worker thread processes incoming notifications of blockclique changes,
//! orders active and final blocks in queues sorted by increasing slot number,
//! and requests the execution of active and final slots from execution.rs.

use crate::controller::{ExecutionControllerImpl, ExecutionInputData, ExecutionManagerImpl};
use crate::execution::ExecutionState;
use crate::request_queue::RequestQueue;
use crate::slot_sequencer::SlotSequencer;
use massa_execution_exports::{
    ExecutionChannels, ExecutionConfig, ExecutionController, ExecutionError, ExecutionManager,
    ReadOnlyExecutionOutput, ReadOnlyExecutionRequest,
};
use massa_final_state::FinalState;
use massa_models::block_id::BlockId;
use massa_models::slot::Slot;
use massa_pos_exports::SelectorController;
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_versioning::versioning::MipStore;
use parking_lot::{Condvar, Mutex, RwLock};
use std::sync::Arc;
use std::thread;
use tracing::debug;

/// Structure gathering all elements needed by the execution thread
pub(crate) struct ExecutionThread {
    // A copy of the input data allowing access to incoming requests
    input_data: Arc<(Condvar, Mutex<ExecutionInputData>)>,
    // Total continuous slot sequence
    slot_sequencer: SlotSequencer,
    // Execution state (see execution.rs) to which execution requests are sent
    execution_state: Arc<RwLock<ExecutionState>>,
    /// queue for read-only requests and response MPSCs to send back their outputs
    readonly_requests: RequestQueue<ReadOnlyExecutionRequest, ReadOnlyExecutionOutput>,
    /// Selector controller
    selector: Box<dyn SelectorController>,
}

impl ExecutionThread {
    /// Creates the `ExecutionThread` structure to gather all data and references
    /// needed by the execution worker thread.
    ///
    /// # Arguments
    /// * `config`: execution configuration
    /// * `input_data`: a copy of the input data interface to get incoming requests from
    /// * `execution_state`: an thread-safe shared access to the execution state, which can be bootstrapped or newly created
    pub fn new(
        config: ExecutionConfig,
        input_data: Arc<(Condvar, Mutex<ExecutionInputData>)>,
        execution_state: Arc<RwLock<ExecutionState>>,
        selector: Box<dyn SelectorController>,
    ) -> Self {
        // get the latest executed final slot, at the output of which the final ledger is attached
        // if we are restarting the network, use last genesis slot of the last start.

        let final_cursor = std::cmp::max(
            execution_state.read().final_cursor,
            Slot {
                period: config.last_start_period,
                thread: config.thread_count.saturating_sub(1),
            },
        );

        // create and return the ExecutionThread
        ExecutionThread {
            input_data,
            readonly_requests: RequestQueue::new(config.readonly_queue_length),
            execution_state,
            slot_sequencer: SlotSequencer::new(config, final_cursor),
            selector,
        }
    }

    /// Append incoming read-only requests to the relevant queue,
    /// Cancel those that are in excess if there are too many.
    fn update_readonly_requests(
        &mut self,
        new_requests: RequestQueue<ReadOnlyExecutionRequest, ReadOnlyExecutionOutput>,
    ) {
        // Append incoming readonly requests to our readonly request queue
        // Excess requests are cancelled
        self.readonly_requests.extend(new_requests);
    }

    /// Executes a read-only request from the queue, if any.
    /// The result of the execution is sent asynchronously through the response channel provided with the request.
    ///
    /// # Returns
    /// true if a request was executed, false otherwise
    fn execute_one_readonly_request(&mut self) -> bool {
        if let Some(req_resp) = self.readonly_requests.pop() {
            let (req, resp_tx) = req_resp.into_request_sender_pair();

            // Acquire write access to the execution state (for cache updates) and execute the read-only request
            let outcome = self.execution_state.write().execute_readonly_request(req);

            // Send the execution output through resp_tx.
            // Ignore errors because they just mean that the request emitter dropped the received
            // because it doesn't need the response anymore.
            let _ = resp_tx.send(outcome);

            return true;
        }
        false
    }

    /// Waits for an event to trigger a new iteration in the execution main loop.
    ///
    /// # Returns
    /// `ExecutionInputData` representing the input requests,
    /// and a boolean saying whether we should stop the loop.
    fn wait_loop_event(&mut self) -> (ExecutionInputData, bool) {
        loop {
            // lock input data
            let mut input_data_lock = self.input_data.1.lock();

            // take current input data, resetting it
            let input_data: ExecutionInputData = input_data_lock.take();

            // if we need to stop, return None
            if input_data.stop {
                return (input_data, true);
            }

            // check if there is some input data
            if input_data.new_blockclique.is_some()
                || !input_data.finalized_blocks.is_empty()
                || !input_data.block_storage.is_empty()
                || !input_data.readonly_requests.is_empty()
            {
                return (input_data, false);
            }

            // the slot sequencer has a task available for execution
            if self.slot_sequencer.is_task_available() {
                return (input_data, false);
            }

            // there are read-only requests ready
            if !self.readonly_requests.is_empty() {
                return (input_data, false);
            }

            // Compute when the next slot will be
            // This is useful to wait for the next speculative miss to append to active slots.
            let wakeup_deadline = self.slot_sequencer.get_next_slot_deadline();
            let now = MassaTime::now().expect("could not get current time");
            if wakeup_deadline <= now {
                // next slot is right now: the loop needs to iterate
                return (input_data, false);
            }

            // Wait to be notified of new input, for at most time_until_next_slot
            // The return value is ignored because we don't care what woke up the condition variable.
            let _ = self.input_data.0.wait_until(
                &mut input_data_lock,
                wakeup_deadline
                    .estimate_instant()
                    .expect("could not estimate instant"),
            );
        }
    }

    /// Main loop of the execution worker
    pub fn main_loop(&mut self) {
        // This loop restarts every time an execution happens for easier tracking.
        // It also prioritizes executions in the following order:
        // 1 - final executions
        // 2 - speculative executions
        // 3 - read-only executions
        loop {
            let (input_data, stop) = self.wait_loop_event();
            debug!("Execution loop triggered, input_data = {}", input_data);

            // update the sequence of read-only requests
            self.update_readonly_requests(input_data.readonly_requests);

            if stop {
                // we need to stop
                break;
            }

            // update slot sequencer
            self.slot_sequencer.update(
                input_data.finalized_blocks,
                input_data.new_blockclique,
                input_data.block_storage,
            );

            // ask the slot sequencer for a task to be executed in priority (final is higher priority than candidate)
            let run_result = self.slot_sequencer.run_task_with(
                |is_final: bool, slot: &Slot, content: Option<&(BlockId, Storage)>| {
                    if is_final {
                        self.execution_state.write().execute_final_slot(
                            slot,
                            content,
                            self.selector.clone(),
                        )
                    } else {
                        self.execution_state.write().execute_candidate_slot(
                            slot,
                            content,
                            self.selector.clone(),
                        )
                    }
                },
            );
            if let Some(_res) = run_result {
                // A slot was executed: continue.
                continue;
            }

            // low priority: execute a read-only request (note that the queue is of finite length), if there is one ready.
            self.execute_one_readonly_request();
        }

        // We are quitting the loop.

        // Cancel pending readonly requests
        let cancel_err = ExecutionError::ChannelError(
            "readonly execution cancelled because the execution worker is closing".into(),
        );
        self.input_data
            .1
            .lock()
            .take()
            .readonly_requests
            .cancel(cancel_err);
    }
}

/// Launches an execution worker thread and returns an `ExecutionManager` to interact with it
///
/// # parameters
/// * `config`: execution configuration
/// * `final_state`: a thread-safe shared access to the final state for reading and writing
///
/// # Returns
/// A pair `(execution_manager, execution_controller)` where:
/// * `execution_manager`: allows to stop the worker
/// * `execution_controller`: allows sending requests and notifications to the worker
pub fn start_execution_worker(
    config: ExecutionConfig,
    final_state: Arc<RwLock<FinalState>>,
    selector: Box<dyn SelectorController>,
    mip_store: MipStore,
    channels: ExecutionChannels,
) -> (Box<dyn ExecutionManager>, Box<dyn ExecutionController>) {
    // create an execution state
    let execution_state = Arc::new(RwLock::new(ExecutionState::new(
        config.clone(),
        final_state,
        mip_store,
        selector.clone(),
        channels,
    )));

    // define the input data interface
    let input_data = Arc::new((
        Condvar::new(),
        Mutex::new(ExecutionInputData::new(config.clone())),
    ));

    // create a controller
    let controller = ExecutionControllerImpl {
        input_data: input_data.clone(),
        execution_state: execution_state.clone(),
    };

    // launch the execution thread
    let input_data_clone = input_data.clone();
    let thread_builder = thread::Builder::new().name("execution".into());
    let thread_handle = thread_builder
        .spawn(move || {
            ExecutionThread::new(config, input_data_clone, execution_state, selector).main_loop();
        })
        .expect("failed to spawn thread : execution");
    // create a manager
    let manager = ExecutionManagerImpl {
        input_data,
        thread_handle: Some(thread_handle),
    };

    // return the execution manager and controller pair
    (Box::new(manager), Box::new(controller))
}
