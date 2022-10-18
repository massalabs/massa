use massa_graph::BootstrapableGraph;
use massa_graph_2_exports::{GraphChannels, GraphConfig, GraphController, GraphManager};
use massa_models::block::BlockId;
use massa_models::clique::Clique;
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use crate::commands::GraphCommand;
use crate::controller::GraphControllerImpl;
use crate::manager::GraphManagerImpl;
use crate::state::GraphState;

/// The graph worker structure that contains all informations and tools for the graph worker thread.
pub struct GraphWorker {
    /// Channel to receive command from the controller
    command_receiver: mpsc::Receiver<GraphCommand>,
    /// Configuration of the graph
    config: GraphConfig,
    /// State shared with the controller
    shared_state: Arc<RwLock<GraphState>>,
    /// Previous slot.
    previous_slot: Option<Slot>,
    /// Next slot
    next_slot: Slot,
    /// Next slot instant
    next_instant: Instant,
}

mod init;
mod main_loop;

/// Create a new graph worker thread.
///
/// # Arguments:
/// * `config`: Configuration of the graph
/// * `channels`: Channels to communicate with others modules
/// * `init_graph`: Optional initial graph to bootstrap the graph. if None, the graph will have only genesis blocks.
/// * `storage`: Storage to use for the graph
///
/// # Returns:
/// * The graph controller to communicate with the graph worker thread
/// * The graph manager to manage the graph worker thread
pub fn start_graph_worker(
    config: GraphConfig,
    channels: GraphChannels,
    init_graph: Option<BootstrapableGraph>,
    storage: Storage,
) -> (Box<dyn GraphController>, Box<dyn GraphManager>) {
    let (tx, rx) = mpsc::sync_channel(10);
    // desync detection timespan
    let stats_desync_detection_timespan =
        config.t0.checked_mul(config.periods_per_cycle * 2).unwrap();
    let shared_state = Arc::new(RwLock::new(GraphState {
        storage: storage.clone(),
        config: config.clone(),
        channels,
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
        save_final_periods: Default::default(),
        latest_final_blocks_periods: Default::default(),
        best_parents: Default::default(),
        block_statuses: Default::default(),
        genesis_hashes: Default::default(),
        gi_head: Default::default(),
        final_block_stats: Default::default(),
        stale_block_stats: Default::default(),
        protocol_blocks: Default::default(),
        wishlist: Default::default(),
        launch_time: MassaTime::now(config.clock_compensation_millis).unwrap(),
        stats_desync_detection_timespan,
        stats_history_timespan: std::cmp::max(
            stats_desync_detection_timespan,
            config.stats_timespan,
        ),
        prev_blockclique: Default::default(),
    }));

    let shared_state_cloned = shared_state.clone();
    let thread_graph = thread::Builder::new()
        .name("graph worker".into())
        .spawn(move || {
            let mut graph_worker =
                GraphWorker::new(config, rx, shared_state_cloned, init_graph, storage).unwrap();
            graph_worker.run()
        })
        .expect("Can't spawn thread graph.");

    let manager = GraphManagerImpl {
        thread_graph: Some((tx.clone(), thread_graph)),
    };

    let controller = GraphControllerImpl::new(tx, shared_state);

    (Box::new(controller), Box::new(manager))
}
