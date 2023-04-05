// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::{
    mpsc::{self,
           // Receiver
           },
    Arc, Mutex,
};
use crossbeam_channel::{Sender, Receiver};

use massa_models::config::THREAD_COUNT;
use massa_models::denunciation::Denunciation;
use massa_models::{
    block_id::BlockId, endorsement::EndorsementId, operation::OperationId, slot::Slot,
};
use massa_storage::Storage;
use massa_time::MassaTime;

use crate::PoolController;

/// Test tool to mock pool controller responses
pub struct PoolEventReceiver(pub Receiver<MockPoolControllerMessage>);

/// List of possible messages you can receive from the mock
/// Each variant corresponds to a unique method in `PoolController`,
/// Some variants wait for a response on their `response_tx` field, if present.
/// See the documentation of `PoolController` for details on parameters and return values.
#[derive(Debug)]
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
    /// Add denunciation to the pool
    AddDenunciation {
        /// The denunciation to add
        denunciation: Denunciation,
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
    GetEndorsementCount {
        /// Response channel
        response_tx: mpsc::Sender<usize>,
    },
    /// Get operations by ids
    GetOperationCount {
        /// Response channel
        response_tx: mpsc::Sender<usize>,
    },
    /// Get denunciation count
    GetDenunciationCount {
        /// Response channel
        response_tx: mpsc::Sender<usize>,
    },
    /// Contains endorsements
    ContainsEndorsements {
        /// ids to search
        ids: Vec<EndorsementId>,
        /// Response channel
        response_tx: mpsc::Sender<Vec<bool>>,
    },
    /// Contains endorsements
    ContainsOperations {
        /// ids to search
        ids: Vec<OperationId>,
        /// Response channel
        response_tx: mpsc::Sender<Vec<bool>>,
    },
    /// Get stats of the pool
    GetStats {
        /// Response channel
        response_tx: mpsc::Sender<(usize, usize)>,
    },
    /// Notify that periods became final
    NotifyFinalCsPeriods {
        /// Periods that are final
        periods: Vec<u64>,
    },
    /// No need to specify the response
    Any,
}

/// A mocked pool controller that will intercept calls on its methods
/// and emit corresponding `MockPoolControllerMessage` messages through a MPSC in a thread-safe way.
/// For messages with a `response_tx` field, the mock will await a response through their `response_tx` channel
/// in order to simulate returning this value at the end of the call.
#[derive(Clone)]
// pub struct MockPoolController(Arc<Mutex<mpsc::Sender<MockPoolControllerMessage>>>);
pub struct MockPoolController {
    q: Arc<Mutex<Sender<MockPoolControllerMessage>>>,
    last_final_cs_periods: Vec<u64>,
}

impl MockPoolController {
    /// Create a new pair (mock execution controller, mpsc receiver for emitted messages)
    /// Note that unbounded mpsc channels are used
    pub fn new_with_receiver() -> (Box<dyn PoolController>, PoolEventReceiver) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            Box::new(MockPoolController {
                q: Arc::new(Mutex::new(tx)),
                last_final_cs_periods: vec![0u64; THREAD_COUNT as usize],
            }),
            PoolEventReceiver(rx),
        )
    }
}

impl PoolEventReceiver {
    /// wait command
    pub fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(MockPoolControllerMessage) -> Option<T>,
    {
        match self.0.recv_timeout(timeout.into()) {
            Ok(msg) => filter_map(msg),
            Err(_) => None,
        }
    }
}

/// Implements all the methods of the `PoolController` trait,
/// but simply make them emit a `MockPoolControllerMessage`.
/// If the message contains a `response_tx`,
/// a response from that channel is read and returned as return value.
/// See the documentation of `PoolController` for details on each function.
impl PoolController for MockPoolController {
    fn add_endorsements(&mut self, endorsements: Storage) {
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::AddEndorsements { endorsements })
            .unwrap();
    }

    fn add_operations(&mut self, operations: Storage) {
        self.q
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
        self.q
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
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetBlockOperations {
                slot: *slot,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_endorsement_count(&self) -> usize {
        let (response_tx, response_rx) = mpsc::channel();
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetEndorsementCount { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_operation_count(&self) -> usize {
        let (response_tx, response_rx) = mpsc::channel();
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetOperationCount { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool> {
        let (response_tx, response_rx) = mpsc::channel();
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::ContainsEndorsements {
                ids: endorsements.to_vec(),
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool> {
        let (response_tx, response_rx) = mpsc::channel();
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::ContainsOperations {
                ids: operations.to_vec(),
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        self.last_final_cs_periods = final_cs_periods.to_vec();
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::NotifyFinalCsPeriods {
                periods: final_cs_periods.to_vec(),
            })
            .unwrap();
    }

    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }

    fn add_denunciation(&mut self, denunciation: Denunciation) {
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::AddDenunciation { denunciation })
            .unwrap();
    }

    fn get_final_cs_periods(&self) -> &Vec<u64> {
        &self.last_final_cs_periods
    }

    fn get_denunciation_count(&self) -> usize {
        let (response_tx, response_rx) = mpsc::channel();
        self.q
            .lock()
            .unwrap()
            .send(MockPoolControllerMessage::GetDenunciationCount { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }
}
