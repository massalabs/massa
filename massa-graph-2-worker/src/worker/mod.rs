use massa_graph::BootstrapableGraph;
use massa_graph_2_exports::{
    GraphChannels, GraphConfig, GraphController, GraphManager, GraphState,
};
use massa_models::address::Address;
use massa_models::block::{BlockId, WrappedHeader};
use massa_models::clique::Clique;
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::slot::Slot;
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use crate::block_status::BlockStatus;
use crate::commands::GraphCommand;
use crate::controller::GraphControllerImpl;
use crate::manager::GraphManagerImpl;

pub struct GraphWorker {
    command_receiver: mpsc::Receiver<GraphCommand>,
    config: GraphConfig,
    channels: GraphChannels,
    shared_state: Arc<RwLock<GraphState>>,
    /// Previous slot.
    previous_slot: Option<Slot>,
    /// Next slot
    next_slot: Slot,
    /// Next slot instant
    next_instant: Instant,
    /// blocks we want
    wishlist: PreHashMap<BlockId, Option<WrappedHeader>>,
    /// Final block stats `(time, creator, is_from_protocol)`
    final_block_stats: VecDeque<(MassaTime, Address, bool)>,
    /// Blocks that come from protocol used for stats and ids are removed when inserted in `final_block_stats`
    protocol_blocks: VecDeque<(MassaTime, BlockId)>,
    /// Stale block timestamp
    stale_block_stats: VecDeque<MassaTime>,
    /// the time span considered for stats
    stats_history_timespan: MassaTime,
    /// the time span considered for desynchronization detection
    #[allow(dead_code)]
    stats_desync_detection_timespan: MassaTime,
    /// time at which the node was launched (used for desynchronization detection)
    launch_time: MassaTime,

    /// Block ids of genesis blocks
    genesis_hashes: Vec<BlockId>,
    /// Used to limit the number of waiting and discarded blocks
    sequence_counter: u64,
    /// Every block we know about
    block_statuses: PreHashMap<BlockId, BlockStatus>,
    /// Ids of incoming blocks/headers
    incoming_index: PreHashSet<BlockId>,
    /// ids of waiting for slot blocks/headers
    waiting_for_slot_index: PreHashSet<BlockId>,
    /// ids of waiting for dependencies blocks/headers
    waiting_for_dependencies_index: PreHashSet<BlockId>,
    /// ids of active blocks
    active_index: PreHashSet<BlockId>,
    /// ids of discarded blocks
    discarded_index: PreHashSet<BlockId>,
    /// One (block id, period) per thread
    latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// One `(block id, period)` per thread TODO not sure I understand the difference with `latest_final_blocks_periods`
    best_parents: Vec<(BlockId, u64)>,
    /// Incompatibility graph: maps a block id to the block ids it is incompatible with
    /// One entry per Active Block
    gi_head: PreHashMap<BlockId, PreHashSet<BlockId>>,
    /// All the cliques
    max_cliques: Vec<Clique>,
    /// Blocks that need to be propagated
    to_propagate: PreHashMap<BlockId, Storage>,
    /// List of block ids we think are attack attempts
    attack_attempts: Vec<BlockId>,
    /// Newly final blocks
    new_final_blocks: PreHashSet<BlockId>,
    /// Newly stale block mapped to creator and slot
    new_stale_blocks: PreHashMap<BlockId, (Address, Slot)>,
    /// Shared storage,
    storage: Storage,
}

mod init;
mod main_loop;

pub fn start_graph_worker(
    config: GraphConfig,
    channels: GraphChannels,
    init_graph: Option<BootstrapableGraph>,
    storage: Storage,
) -> (Box<dyn GraphController>, Box<dyn GraphManager>) {
    let (tx, rx) = mpsc::sync_channel(10);
    let shared_state = Arc::new(RwLock::new(GraphState {}));

    let shared_state_cloned = shared_state.clone();
    let thread_graph = thread::Builder::new()
        .name("graph worker".into())
        .spawn(move || {
            let mut graph_worker =
            //TODO: Better error management
                GraphWorker::new(rx, config, channels, shared_state_cloned, init_graph, storage).expect("Failed to initialize graph worker");
            graph_worker.run()
        })
        .expect("Can't spawn thread graph.");

    let manager = GraphManagerImpl {
        thread_graph: Some(thread_graph),
        graph_command_sender: tx.clone(),
    };

    let controller = GraphControllerImpl::new(tx, shared_state);

    (Box::new(controller), Box::new(manager))
}
