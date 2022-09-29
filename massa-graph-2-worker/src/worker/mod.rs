use massa_graph::BootstrapableGraph;
use massa_graph_2_exports::{
    block_status::BlockStatus, GraphChannels, GraphConfig, GraphController, GraphManager,
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

use crate::commands::GraphCommand;
use crate::controller::GraphControllerImpl;
use crate::manager::GraphManagerImpl;
use crate::state::GraphState;

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

    /// Used to limit the number of waiting and discarded blocks
    sequence_counter: u64,
    /// Ids of incoming blocks/headers
    incoming_index: PreHashSet<BlockId>,
    /// ids of waiting for slot blocks/headers
    waiting_for_slot_index: PreHashSet<BlockId>,
    /// ids of waiting for dependencies blocks/headers
    waiting_for_dependencies_index: PreHashSet<BlockId>,
    /// ids of discarded blocks
    discarded_index: PreHashSet<BlockId>,
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
mod process_commands;

pub fn start_graph_worker(
    config: GraphConfig,
    channels: GraphChannels,
    init_graph: Option<BootstrapableGraph>,
    storage: Storage,
) -> (Box<dyn GraphController>, Box<dyn GraphManager>) {
    let (tx, rx) = mpsc::sync_channel(10);
    let shared_state = Arc::new(RwLock::new(GraphState {
        storage: storage.clone(),
        config: config.clone(),
        max_cliques: vec![Clique {
            block_ids: PreHashSet::<BlockId>::default(),
            fitness: 0,
            is_blockclique: true,
        }],
        active_index: Default::default(),
        latest_final_blocks_periods: Default::default(),
        best_parents: Default::default(),
        block_statuses: Default::default(),
        genesis_hashes: Default::default(),
        gi_head: Default::default(),
    }));

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
