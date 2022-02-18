use crate::config::VMConfig;
use crate::execution::ExecutionState;
use crate::types::ExecutionOutput;
use crate::types::ReadOnlyExecutionRequest;
use crate::ExecutionError;
use massa_ledger::LedgerEntry;
use massa_models::output_event::SCOutputEvent;
use massa_models::Address;
use massa_models::OperationId;
use massa_models::{Block, BlockId, Slot};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, Condvar, Mutex, RwLock};
use tracing::info;

/// structure used to communicate with the VM thread
#[derive(Default)]
pub(crate) struct VMInputData {
    /// set stop to true to stop the thread
    pub stop: bool,
    /// signal whether the blockclique changed
    pub blockclique_changed: bool,
    /// list of newly finalized blocks
    pub finalized_blocks: HashMap<Slot, (BlockId, Block)>,
    /// blockclique
    pub blockclique: HashMap<Slot, (BlockId, Block)>,
    /// readonly execution requests and response mpscs
    pub readonly_requests: VecDeque<(
        ReadOnlyExecutionRequest,
        mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
    )>,
}

/// VM controller
pub struct ExecutionController {
    /// VM config
    pub(crate) config: VMConfig,
    /// condition variable to wake up the VM loop
    pub(crate) loop_cv: Condvar,
    /// input data to process in the VM loop
    pub(crate) input_data: Mutex<VMInputData>,
    /// execution state
    pub(crate) execution_state: Arc<RwLock<ExecutionState>>,
}

impl ExecutionController {
    /// reads the list of newly finalized blocks and the new blockclique, if there was a change
    /// if found, remove from input queue
    pub(crate) fn consume_input(&mut self) -> VMInputData {
        std::mem::take(&mut self.input_data.lock().expect("VM input data lock failed"))
    }

    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    pub fn get_filtered_sc_output_event(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
    ) -> Vec<SCOutputEvent> {
        self.execution_state
            .read()
            .expect("could not lock execution state for reading")
            .get_filtered_sc_output_event(
                start,
                end,
                emitter_address,
                original_caller_address,
                original_operation_id,
            )
    }

    /// gets a copy of a full ledger entry
    ///
    /// # return value
    /// * (final_entry, active_entry)
    pub fn get_full_ledger_entry(
        &self,
        addr: &Address,
    ) -> (Option<LedgerEntry>, Option<LedgerEntry>) {
        self.execution_state
            .read()
            .expect("could not lock execution state for reading")
            .get_full_ledger_entry(addr)
    }

    /// Executes a readonly request
    pub fn execute_readonly_request(
        &mut self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError> {
        // queue request
        let resp_rx = {
            let input_data = self
                .input_data
                .lock()
                .expect("could not lock VM input data");
            if input_data.readonly_requests.len() >= self.config.readonly_queue_length {
                return Err(ExecutionError::RuntimeError(
                    "too many queued readonly requests".into(),
                ));
            }
            let (resp_tx, resp_rx) =
                std::sync::mpsc::channel::<Result<ExecutionOutput, ExecutionError>>();
            input_data.readonly_requests.push_back((req, resp_tx));
            self.loop_cv.notify_one();
            resp_rx
        };

        // wait for response
        match resp_rx.recv() {
            Ok(result) => return result,
            Err(err) => {
                return Err(ExecutionError::RuntimeError(
                    "the VM input channel is closed".into(),
                ))
            }
        }
    }
}

/// VM manager
pub struct VMManager {
    /// shared reference to the VM controller
    controller: Arc<VMController>,
    /// handle used to join the VM thread
    thread_handle: std::thread::JoinHandle<()>,
}

impl VMManager {
    /// stops the VM
    pub fn stop(self) {
        info!("stopping VM controller...");
        // notify the VM thread to stop
        {
            let mut input_wlock = self
                .controller
                .input_data
                .lock()
                .expect("could not lock VM input data");
            input_wlock.stop = true;
            self.controller.loop_cv.notify_one();
        }
        // join the VM thread
        self.thread_handle
            .join()
            .expect("VM controller thread panicked");
        info!("VM controller stopped");
    }

    /// get a shared reference to the VM controller
    pub fn get_controller(&self) -> Arc<VMController> {
        self.controller.clone()
    }
}
