// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::{
    mpsc::{self, Receiver},
    Arc, Mutex,
};

use massa_models::{BlockId, EndorsementId, Slot, OperationId, prehash::Set, WrappedOperation};
use massa_storage::Storage;

use crate::PoolController;

/// List of possible messages you can receive from the mock
/// Each variant corresponds to a unique method in `PoolController`,
/// Some variants wait for a response on their `response_tx` field, if present.
/// See the documentation of `PoolController` for details on parameters and return values.
pub enum MockPoolControllerMessage {
    /// Add endorsements to the pool
    AddEndorsements {
        /// Storage that contains all endorsements
        endorsements: Storage,
    },
    /// Add operations to the pool
    AddOperations {
        /// Storage that contains all operations
        operations: Storage,
    },
    /// Get block endorsements
    GetBlockEndorsements {
        /// Block id of the block endorsed
        block_id: BlockId,
        /// Slot of the endorsement
        slot: Slot,
        /// Response channel
        response_tx: mpsc::Sender<(Vec<Option<EndorsementId>>, Storage)>,
    },
    /// Get operations of a block
    GetBlockOperations {
        /// Slot of the block to search operations in
        slot: Slot,
        /// Response channel
        response_tx: mpsc::Sender<(Vec<OperationId>, Storage)>,
    },
    /// Get endorsement ids
    GetEndorsementIds {
        /// Response channel
        response_tx: mpsc::Sender<Set<EndorsementId>>,        
    },
    /// Get operations by ids
    GetOperationsByIds {
        /// Ids to fetch
        ids: Set<OperationId>,
        /// Response channel
        response_tx: mpsc::Sender<Vec<WrappedOperation>>
    },
    /// Get stats of the pool
    GetStats {
        /// Response channel
        response_tx: mpsc::Sender<(usize, usize)>
    },
    /// Notify that periods became final
    NotifyFinalCsPeriods {
        /// Periods that are final
        periods: Vec<u64>        
    }

}

/// A mocked pool controller that will intercept calls on its methods
/// and emit corresponding `MockPoolControllerMessage` messages through a MPSC in a thread-safe way.
/// For messages with a `response_tx` field, the mock will await a response through their `response_tx` channel
/// in order to simulate returning this value at the end of the call.
#[derive(Clone)]
pub struct MockPoolController(Arc<Mutex<mpsc::Sender<MockPoolControllerMessage>>>);

impl MockPoolController {
    /// Create a new pair (mock execution controller, mpsc receiver for emitted messages)
    /// Note that unbounded mpsc channels are used
    pub fn new_with_receiver() -> (Box<dyn PoolController>, Receiver<MockPoolControllerMessage>) {
        let (tx, rx) = mpsc::channel();
        (Box::new(MockPoolController(Arc::new(Mutex::new(tx)))), rx)
    }
}

/// Implements all the methods of the `PoolController` trait,
/// but simply make them emit a `MockPoolControllerMessage`.
/// If the message contains a `response_tx`,
/// a response from that channel is read and returned as return value.
/// See the documentation of `PoolController` for details on each function.
impl PoolController for MockPoolController {
    fn add_endorsements(&mut self, endorsements: Storage) {
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::AddEndorsements { endorsements })
            .unwrap();
    }

    fn add_operations(&mut self, operations: Storage) {
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::AddOperations { operations })
            .unwrap();
    }

    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetBlockEndorsements {
                block_id: *target_block,
                slot: *target_slot,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetBlockOperations { slot: *slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_endorsement_ids(&self) -> Set<EndorsementId> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetEndorsementIds { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_operations_by_ids(&self, ids: &Set<OperationId>) -> Vec<WrappedOperation> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetOperationsByIds { ids: ids.clone(), response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_stats(&self) -> (usize, usize) {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetStats { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        self.0
        .lock()
        .unwrap()
        .send(MockPoolControllerMessage::NotifyFinalCsPeriods { periods: final_cs_periods.to_vec() })
        .unwrap();
    }

    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }
}
