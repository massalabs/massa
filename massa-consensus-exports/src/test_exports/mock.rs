// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::{
    mpsc::{self, Receiver},
    Arc, Mutex,
};

use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId},
    clique::Clique,
    prehash::PreHashSet,
    slot::Slot,
    stats::ConsensusStats,
    streaming_step::StreamingStep,
    wrapped::Wrapped,
};
use massa_storage::Storage;
use massa_time::MassaTime;

use crate::{
    block_graph_export::BlockGraphExport, bootstrapable_graph::BootstrapableGraph,
    error::ConsensusError, ConsensusController,
};

/// Test tool to mock graph controller responses
pub struct ConsensusEventReceiver(pub Receiver<MockConsensusControllerMessage>);

/// List of possible messages you can receive from the mock
/// Each variant corresponds to a unique method in `ConsensusController`,
/// Some variants wait for a response on their `response_tx` field, if present.
/// See the documentation of `ConsensusController` for details on parameters and return values.
#[derive(Clone, Debug)]
pub enum MockConsensusControllerMessage {
    GetBlockStatuses {
        block_ids: Vec<BlockId>,
        response_tx: mpsc::Sender<Vec<BlockGraphStatus>>,
    },
    GetBlockGraphStatuses {
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
        response_tx: mpsc::Sender<Result<BlockGraphExport, ConsensusError>>,
    },
    GetCliques {
        response_tx: mpsc::Sender<Vec<Clique>>,
    },
    GetBootstrapableGraph {
        cursor: StreamingStep<PreHashSet<BlockId>>,
        execution_cursor: StreamingStep<Slot>,
        response_tx: mpsc::Sender<
            Result<
                (
                    BootstrapableGraph,
                    PreHashSet<BlockId>,
                    StreamingStep<PreHashSet<BlockId>>,
                ),
                ConsensusError,
            >,
        >,
    },
    GetStats {
        response_tx: mpsc::Sender<Result<ConsensusStats, ConsensusError>>,
    },
    GetBestParents {
        response_tx: mpsc::Sender<Vec<(BlockId, u64)>>,
    },
    GetBlockcliqueBlockAtSlot {
        slot: Slot,
        response_tx: mpsc::Sender<Option<BlockId>>,
    },
    GetLatestBlockcliqueBlockAtSlot {
        slot: Slot,
        response_tx: mpsc::Sender<BlockId>,
    },
    MarkInvalidBlock {
        block_id: BlockId,
        header: Wrapped<BlockHeader, BlockId>,
    },
    RegisterBlock {
        block_id: BlockId,
        slot: Slot,
        block_storage: Storage,
        created: bool,
    },
    RegisterBlockHeader {
        block_id: BlockId,
        header: Wrapped<BlockHeader, BlockId>,
    },
}

/// A mocked graph controller that will intercept calls on its methods
/// and emit corresponding `MockConsensusControllerMessage` messages through a MPSC in a thread-safe way.
/// For messages with a `response_tx` field, the mock will await a response through their `response_tx` channel
/// in order to simulate returning this value at the end of the call.
#[derive(Clone)]
pub struct MockConsensusController(Arc<Mutex<mpsc::Sender<MockConsensusControllerMessage>>>);

impl MockConsensusController {
    /// Create a new pair (mock graph controller, mpsc receiver for emitted messages)
    /// Note that unbounded mpsc channels are used
    pub fn new_with_receiver() -> (Box<dyn ConsensusController>, ConsensusEventReceiver) {
        let (tx, rx) = mpsc::channel();
        (
            Box::new(MockConsensusController(Arc::new(Mutex::new(tx)))),
            ConsensusEventReceiver(rx),
        )
    }
}

impl ConsensusEventReceiver {
    /// wait command
    pub fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(MockConsensusControllerMessage) -> Option<T>,
    {
        match self.0.recv_timeout(timeout.into()) {
            Ok(msg) => filter_map(msg),
            Err(_) => None,
        }
    }
}

/// Implements all the methods of the `ConsensusController` trait,
/// but simply make them emit a `MockConsensusControllerMessage`.
/// If the message contains a `response_tx`,
/// a response from that channel is read and returned as return value.
/// See the documentation of `ConsensusController` for details on each function.
impl ConsensusController for MockConsensusController {
    fn get_block_graph_status(
        &self,
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
    ) -> Result<BlockGraphExport, ConsensusError> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetBlockGraphStatuses {
                start_slot,
                end_slot,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_block_statuses(&self, ids: &[BlockId]) -> Vec<BlockGraphStatus> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetBlockStatuses {
                block_ids: ids.to_vec(),
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_cliques(&self) -> Vec<Clique> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetCliques { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_bootstrap_part(
        &self,
        cursor: StreamingStep<PreHashSet<BlockId>>,
        execution_cursor: StreamingStep<Slot>,
    ) -> Result<
        (
            BootstrapableGraph,
            PreHashSet<BlockId>,
            StreamingStep<PreHashSet<BlockId>>,
        ),
        ConsensusError,
    > {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetBootstrapableGraph {
                cursor,
                execution_cursor,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_stats(&self) -> Result<ConsensusStats, ConsensusError> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetStats { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_best_parents(&self) -> Vec<(BlockId, u64)> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetBestParents { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::GetBlockcliqueBlockAtSlot { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(
                MockConsensusControllerMessage::GetLatestBlockcliqueBlockAtSlot {
                    slot,
                    response_tx,
                },
            )
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn mark_invalid_block(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::MarkInvalidBlock { block_id, header })
            .unwrap();
    }

    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage, created: bool) {
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::RegisterBlock {
                block_id,
                slot,
                block_storage,
                created,
            })
            .unwrap();
    }

    fn register_block_header(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        self.0
            .lock()
            .unwrap()
            .send(MockConsensusControllerMessage::RegisterBlockHeader { block_id, header })
            .unwrap();
    }

    fn subscribe_new_blocks_headers(&self, _sink: jsonrpsee::SubscriptionSink) {
        panic!("Not implemented")
    }

    fn subscribe_new_blocks(&self, _sink: jsonrpsee::SubscriptionSink) {
        panic!("Not implemented")
    }

    fn subscribe_new_filled_blocks(&self, _sink: jsonrpsee::SubscriptionSink) {
        panic!("Not implemented")
    }

    fn clone_box(&self) -> Box<dyn ConsensusController> {
        Box::new(self.clone())
    }
}
