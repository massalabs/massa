use crate::execution::ExecutionState;
use crate::speculative_ledger::SpeculativeLedger;
use crate::types::ExecutionOutput;
use crate::ExecutionError;
use crate::{config::VMConfig, types::ReadOnlyExecutionRequest, vm_thread::VMThread};
use massa_ledger::FinalLedger;
use massa_models::{Block, BlockId, Slot};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, Condvar, Mutex, RwLock};
use tracing::info;

/// structure used to communicate with the VM thread
#[derive(Default)]
pub struct VMInputData {
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
pub struct VMController {
    /// condition variable to wake up the VM loop
    pub loop_cv: Condvar,
    /// input data to process in the VM loop
    pub input_data: Mutex<VMInputData>,
    /// execution state
    pub execution_state: Arc<RwLock<ExecutionState>>,
}

impl VMController {
    /// reads the list of newly finalized blocks and the new blockclique, if there was a change
    /// if found, remove from input queue
    pub(crate) fn consume_input(&mut self) -> VMInputData {
        std::mem::take(&mut self.input_data.lock().expect("VM input data lock failed"))
    }

    /// Executes a readonly request
    pub fn execute_readonly_request(
        &mut self,
        req: ReadOnlyExecutionRequest,
        max_queue_length: usize,
    ) -> Result<ExecutionOutput, ExecutionError> {
        // queue request
        let resp_rx = {
            let input_data = self
                .input_data
                .lock()
                .expect("could not lock VM input data");
            if input_data.readonly_requests.len() > max_queue_length {
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
                .expect("could not w-lock VM input data");
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
