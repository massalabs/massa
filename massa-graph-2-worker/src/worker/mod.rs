use massa_graph::BootstrapableGraph;
use massa_graph_2_exports::{GraphChannels, GraphConfig, GraphController, GraphManager};
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
    stats_desync_detection_timespan: MassaTime,
    /// save latest final periods
    latest_final_periods: Vec<u64>,
    /// time at which the node was launched (used for desynchronization detection)
    launch_time: MassaTime,
    /// previous blockclique notified to Execution
    prev_blockclique: PreHashMap<BlockId, Slot>,
    /// Shared storage,
    storage: Storage,
}

mod init;
mod main_loop;
mod process_commands;
mod stats;
mod tick;

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
        channels: channels.clone(),
        max_cliques: vec![Clique {
            block_ids: PreHashSet::<BlockId>::default(),
            fitness: 0,
            is_blockclique: true,
        }],
        sequence_counter: 0,
        waiting_for_slot_index: Default::default(),
        waiting_for_dependencies_index: Default::default(),
        discarded_index: Default::default(),
        to_propagate: Default::default(),
        attack_attempts: Default::default(),
        new_final_blocks: Default::default(),
        new_stale_blocks: Default::default(),
        incoming_index: Default::default(),
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
            let mut graph_worker = GraphWorker::new(
                config,
                rx,
                channels,
                shared_state_cloned,
                init_graph,
                storage,
            )
            .unwrap();
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
