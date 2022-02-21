// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements an execution controller.
//! See massa-execution-exports/controller_traits.rs for functional details.

use crate::execution::ExecutionState;
use massa_execution_exports::{
    ExecutionConfig, ExecutionController, ExecutionError, ExecutionManager, ExecutionOutput,
    ReadOnlyExecutionRequest,
};
use massa_ledger::LedgerEntry;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::Map;
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
    /// list of newly finalized blocks, index by slot
    pub finalized_blocks: HashMap<Slot, (BlockId, Block)>,
    /// blockclique, blocks indexed by slot
    pub blockclique: HashMap<Slot, (BlockId, Block)>,
    /// queue for readonly execution requests and response mpscs to send back their outputs
    pub readonly_requests: VecDeque<(
        ReadOnlyExecutionRequest,
        mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
    )>,
}

#[derive(Clone)]
/// implementation of the execution controller
pub struct ExecutionControllerImpl {
    /// execution config
    pub(crate) config: ExecutionConfig,
    /// input data to process in the VM loop
    /// with a wakeup condition variable that needs to be triggered when the data changes
    pub(crate) input_data: Arc<(Condvar, Mutex<VMInputData>)>,
    /// current execution state (see execution.rs for details)
    pub(crate) execution_state: Arc<RwLock<ExecutionState>>,
}

impl ExecutionControllerImpl {
    /// consumes and returns the input fed to the controller
    pub(crate) fn consume_input(&mut self) -> VMInputData {
        std::mem::take(&mut self.input_data.1.lock().expect("VM input data lock failed"))
    }
}

impl ExecutionController for ExecutionControllerImpl {
    /// called to signal changes on the current blockclique, also listing newly finalized blocks
    ///
    /// # arguments
    /// * finalized_blocks: list of newly finalized blocks to be appended to the input finalized blocks
    /// * blockclique: new blockclique, replaces the curren one in the input
    fn update_blockclique_status(
        &self,
        finalized_blocks: Map<BlockId, Block>,
        blockclique: Map<BlockId, Block>,
    ) {
        // index newly finalized blocks by slot
        let mapped_finalized_blocks: HashMap<_, _> = finalized_blocks
            .into_iter()
            .map(|(b_id, b)| (b.header.content.slot, (b_id, b)))
            .collect();
        // index blockclique by slot
        let mapped_blockclique = blockclique
            .into_iter()
            .map(|(b_id, b)| (b.header.content.slot, (b_id, b)))
            .collect();
        //update input data
        let mut input_data = self
            .input_data
            .1
            .lock()
            .expect("could not lock VM input data");
        input_data.blockclique = mapped_blockclique; // replace blockclique
        input_data.finalized_blocks.extend(mapped_finalized_blocks); // append finalized blocks
        input_data.blockclique_changed = true; // signal a blockclique change
        self.input_data.0.notify_one(); // wake up VM loop
    }

    /// Get the generated execution events, optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(
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
    fn get_full_ledger_entry(&self, addr: &Address) -> (Option<LedgerEntry>, Option<LedgerEntry>) {
        self.execution_state
            .read()
            .expect("could not lock execution state for reading")
            .get_full_ledger_entry(addr)
    }

    /// Executes a readonly request
    /// Read-only requests do not modify consesnsus state
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError> {
        // queue request into input, get response mpsc receiver
        let resp_rx = {
            let mut input_data = self
                .input_data
                .1
                .lock()
                .expect("could not lock VM input data");
            // limit the read-only queue length
            if input_data.readonly_requests.len() >= self.config.readonly_queue_length {
                return Err(ExecutionError::RuntimeError(
                    "too many queued readonly requests".into(),
                ));
            }
            // prepare the channel to send back the result of the read-only execution
            let (resp_tx, resp_rx) =
                std::sync::mpsc::channel::<Result<ExecutionOutput, ExecutionError>>();
            // append to the queue of input read-only requests
            input_data.readonly_requests.push_back((req, resp_tx));
            // wake up VM loop
            self.input_data.0.notify_one();
            resp_rx
        };

        // wait for the result of the execution
        match resp_rx.recv() {
            Ok(result) => return result,
            Err(err) => {
                return Err(ExecutionError::RuntimeError(format!(
                    "the VM input channel failed: {}",
                    err
                )))
            }
        }
    }
}

/// Execution manager
/// Allows creating execution controllers, and stopping the execution worker
pub struct ExecutionManagerImpl {
    /// shared reference to the execution controller
    pub(crate) controller: ExecutionControllerImpl,
    /// handle used to join the worker thread
    pub(crate) thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl ExecutionManager for ExecutionManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping VM controller...");
        // notify the worker thread to stop
        {
            let mut input_wlock = self
                .controller
                .input_data
                .1
                .lock()
                .expect("could not lock VM input data");
            input_wlock.stop = true;
            self.controller.input_data.0.notify_one();
        }
        // join the VM thread
        if let Some(join_handle) = self.thread_handle.take() {
            join_handle.join().expect("VM controller thread panicked");
        }
        info!("VM controller stopped");
    }

    /// return a new execution controller
    fn get_controller(&self) -> Box<dyn ExecutionController> {
        Box::new(self.controller.clone())
    }
}
