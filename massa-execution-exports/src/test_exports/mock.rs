// Copyright (c) 2022 MASSA LABS <info@massa.net>
// This file defines utilities to mock the crate for testing purposes

use crate::{ExecutionController, ExecutionError, ExecutionOutput, ReadOnlyExecutionRequest};
use massa_ledger::LedgerEntry;
use massa_models::{
    output_event::SCOutputEvent, prehash::Map, Address, Block, BlockId, OperationId, Slot,
};
use std::sync::{
    mpsc::{self, Receiver},
    Arc, Mutex,
};

#[derive(Clone)]
pub enum MockExecutionControllerMessage {
    UpdateBlockcliqueStatus {
        finalized_blocks: Map<BlockId, Block>,
        blockclique: Map<BlockId, Block>,
    },
    GetFilteredScOutputEvent {
        start: Option<Slot>,
        end: Option<Slot>,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
        response_tx: mpsc::Sender<Vec<SCOutputEvent>>,
    },
    GetFullLedgerEntry {
        addr: Address,
        response_tx: mpsc::Sender<(Option<LedgerEntry>, Option<LedgerEntry>)>,
    },
    ExecuteReadonlyRequest {
        req: ReadOnlyExecutionRequest,
        response_tx: mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
    },
}

#[derive(Clone)]
pub struct MockExecutionController(Arc<Mutex<mpsc::Sender<MockExecutionControllerMessage>>>);

impl MockExecutionController {
    pub fn new() -> (
        Box<dyn ExecutionController>,
        Receiver<MockExecutionControllerMessage>,
    ) {
        let (tx, rx) = mpsc::channel();
        (
            Box::new(MockExecutionController(Arc::new(Mutex::new(tx)))),
            rx,
        )
    }
}

impl ExecutionController for MockExecutionController {
    /// Update blockclique status
    ///
    /// # arguments
    /// * finalized_blocks: newly finalized blocks
    /// * blockclique: new blockclique
    fn update_blockclique_status(
        &self,
        finalized_blocks: Map<BlockId, Block>,
        blockclique: Map<BlockId, Block>,
    ) {
        self.0
            .lock()
            .unwrap()
            .send(MockExecutionControllerMessage::UpdateBlockcliqueStatus {
                finalized_blocks,
                blockclique,
            })
            .unwrap();
    }

    /// Get events optionnally filtered by:
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
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockExecutionControllerMessage::GetFilteredScOutputEvent {
                start,
                end,
                emitter_address,
                original_caller_address,
                original_operation_id,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    /// gets a copy of a full ledger entry
    ///
    /// # return value
    /// * (final_entry, active_entry)
    fn get_full_ledger_entry(&self, addr: &Address) -> (Option<LedgerEntry>, Option<LedgerEntry>) {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockExecutionControllerMessage::GetFullLedgerEntry {
                addr: *addr,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    /// Executes a readonly request
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockExecutionControllerMessage::ExecuteReadonlyRequest { req, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }
}
