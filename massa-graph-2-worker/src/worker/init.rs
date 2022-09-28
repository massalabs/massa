use std::{
    collections::VecDeque,
    sync::{mpsc, Arc},
};

use massa_graph::{
    error::{GraphError, GraphResult},
    BootstrapableGraph,
};
use massa_graph_2_exports::{GraphChannels, GraphConfig, GraphState};
use massa_hash::Hash;
use massa_models::{
    active_block::ActiveBlock,
    address::Address,
    block::{Block, BlockHeader, BlockHeaderSerializer, BlockId, BlockSerializer, WrappedBlock},
    clique::Clique,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
    wrapped::WrappedContent,
};
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::RwLock;
use tracing::log::info;

use crate::{block_status::BlockStatus, commands::GraphCommand};

use super::GraphWorker;

/// Creates genesis block in given thread.
///
/// # Arguments
/// * `cfg`: consensus configuration
/// * `thread_number`: thread in which we want a genesis block
pub fn create_genesis_block(
    cfg: &GraphConfig,
    thread_number: u8,
) -> GraphResult<(BlockId, WrappedBlock)> {
    let keypair = &cfg.genesis_key;
    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot: Slot::new(0, thread_number),
            parents: Vec::new(),
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        keypair,
    )?;

    Ok((
        header.id,
        Block::new_wrapped(
            Block {
                header,
                operations: Default::default(),
            },
            BlockSerializer::new(),
            keypair,
        )?,
    ))
}

impl GraphWorker {
    pub fn new(
        command_receiver: mpsc::Receiver<GraphCommand>,
        config: GraphConfig,
        channels: GraphChannels,
        shared_state: Arc<RwLock<GraphState>>,
        init_graph: Option<BootstrapableGraph>,
        storage: Storage,
    ) -> GraphResult<Self> {
        let now = MassaTime::now(config.clock_compensation_millis)
            .expect("Couldn't init timer consensus");
        let previous_slot = get_latest_block_slot_at_timestamp(
            config.thread_count,
            config.t0,
            config.genesis_timestamp,
            now,
        )
        .expect("Couldn't get the init slot consensus.");
        // load genesis blocks

        let mut block_statuses = PreHashMap::default();
        let mut genesis_block_ids = Vec::with_capacity(config.thread_count as usize);
        for thread in 0u8..config.thread_count {
            let (block_id, block) = create_genesis_block(&config, thread).map_err(|err| {
                GraphError::GenesisCreationError(format!("genesis error {}", err))
            })?;
            let mut storage = storage.clone_without_refs();
            storage.store_block(block.clone());
            genesis_block_ids.push(block_id);
            block_statuses.insert(
                block_id,
                BlockStatus::Active {
                    a_block: Box::new(ActiveBlock {
                        creator_address: block.creator_address,
                        parents: Vec::new(),
                        children: vec![PreHashMap::default(); config.thread_count as usize],
                        descendants: Default::default(),
                        is_final: true,
                        block_id,
                        slot: block.content.header.content.slot,
                        fitness: block.get_fitness(),
                    }),
                    storage,
                },
            );
        }

        let next_slot = previous_slot.map_or(Ok(Slot::new(0u64, 0u8)), |s| {
            s.get_next_slot(config.thread_count)
        })?;
        let next_instant = get_block_slot_timestamp(
            config.thread_count,
            config.t0,
            config.genesis_timestamp,
            next_slot,
        )?
        .estimate_instant(config.clock_compensation_millis)?;

        info!(
            "Started node at time {}, cycle {}, period {}, thread {}",
            now.to_utc_string(),
            next_slot.get_cycle(config.periods_per_cycle),
            next_slot.period,
            next_slot.thread,
        );
        if config.genesis_timestamp > now {
            let (days, hours, mins, secs) = config
                .genesis_timestamp
                .saturating_sub(now)
                .days_hours_mins_secs()?;
            info!(
                "{} days, {} hours, {} minutes, {} seconds remaining to genesis",
                days, hours, mins, secs,
            )
        }

        // add genesis blocks to stats
        let genesis_addr = Address::from_public_key(&config.genesis_key.get_public_key());
        let mut final_block_stats = VecDeque::new();
        for thread in 0..config.thread_count {
            final_block_stats.push_back((
                get_block_slot_timestamp(
                    config.thread_count,
                    config.t0,
                    config.genesis_timestamp,
                    Slot::new(0, thread),
                )?,
                genesis_addr,
                false,
            ))
        }

        // desync detection timespan
        let stats_desync_detection_timespan =
            config.t0.checked_mul(config.periods_per_cycle * 2)?;

        //TODO: Factorize this code to create graph worker only one time a lot of fields are redundant.
        if let Some(BootstrapableGraph { final_blocks }) = init_graph {
            // load final blocks
            let final_blocks: Vec<(ActiveBlock, Storage)> = final_blocks
                .into_iter()
                .map(|export_b| export_b.to_active_block(&storage, config.thread_count))
                .collect::<Result<_, GraphError>>()?;

            // compute latest_final_blocks_periods
            let mut latest_final_blocks_periods: Vec<(BlockId, u64)> =
                genesis_block_ids.iter().map(|id| (*id, 0u64)).collect();
            for (b, _) in &final_blocks {
                if let Some(v) = latest_final_blocks_periods.get_mut(b.slot.thread as usize) {
                    if b.slot.period > v.1 {
                        *v = (b.block_id, b.slot.period);
                    }
                }
            }

            // generate graph
            let mut res_graph = GraphWorker {
                config: config.clone(),
                command_receiver,
                channels,
                shared_state,
                previous_slot,
                next_slot,
                next_instant,
                wishlist: Default::default(),
                final_block_stats,
                protocol_blocks: Default::default(),
                stale_block_stats: VecDeque::new(),
                stats_desync_detection_timespan,
                stats_history_timespan: std::cmp::max(
                    stats_desync_detection_timespan,
                    config.stats_timespan,
                ),
                launch_time: MassaTime::now(config.clock_compensation_millis)?,
                sequence_counter: 0,
                genesis_hashes: genesis_block_ids,
                active_index: final_blocks.iter().map(|(b, _)| b.block_id).collect(),
                incoming_index: Default::default(),
                waiting_for_slot_index: Default::default(),
                waiting_for_dependencies_index: Default::default(),
                discarded_index: Default::default(),
                best_parents: latest_final_blocks_periods.clone(),
                latest_final_blocks_periods,
                gi_head: Default::default(),
                max_cliques: vec![Clique {
                    block_ids: PreHashSet::<BlockId>::default(),
                    fitness: 0,
                    is_blockclique: true,
                }],
                to_propagate: Default::default(),
                attack_attempts: Default::default(),
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
                storage,
                block_statuses: final_blocks
                    .into_iter()
                    .map(|(b, s)| {
                        Ok((
                            b.block_id,
                            BlockStatus::Active {
                                a_block: Box::new(b),
                                storage: s,
                            },
                        ))
                    })
                    .collect::<GraphResult<_>>()?,
            };

            // claim parent refs
            for (_b_id, block_status) in res_graph.block_statuses.iter_mut() {
                if let BlockStatus::Active {
                    a_block,
                    storage: block_storage,
                } = block_status
                {
                    // claim parent refs
                    let n_claimed_parents = block_storage
                        .claim_block_refs(&a_block.parents.iter().map(|(p_id, _)| *p_id).collect())
                        .len();

                    if !a_block.is_final {
                        // note: parents of final blocks will be missing, that's ok, but it shouldn't be the case for non-finals
                        if n_claimed_parents != config.thread_count as usize {
                            return Err(GraphError::MissingBlock(
                                "block storage could not claim refs to all parent blocks".into(),
                            ));
                        }
                    }
                }
            }

            // list active block parents
            let active_blocks_map: PreHashMap<BlockId, (Slot, Vec<BlockId>)> = res_graph
                .block_statuses
                .iter()
                .filter_map(|(h, s)| {
                    if let BlockStatus::Active { a_block: a, .. } = s {
                        return Some((*h, (a.slot, a.parents.iter().map(|(ph, _)| *ph).collect())));
                    }
                    None
                })
                .collect();
            // deduce children and descendants
            for (b_id, (b_slot, b_parents)) in active_blocks_map.into_iter() {
                // deduce children
                for parent_id in &b_parents {
                    if let Some(BlockStatus::Active {
                        a_block: parent, ..
                    }) = res_graph.block_statuses.get_mut(parent_id)
                    {
                        parent.children[b_slot.thread as usize].insert(b_id, b_slot.period);
                    }
                }

                // deduce descendants
                let mut ancestors: VecDeque<BlockId> = b_parents.into_iter().collect();
                let mut visited: PreHashSet<BlockId> = Default::default();
                while let Some(ancestor_h) = ancestors.pop_back() {
                    if !visited.insert(ancestor_h) {
                        continue;
                    }
                    if let Some(BlockStatus::Active { a_block: ab, .. }) =
                        res_graph.block_statuses.get_mut(&ancestor_h)
                    {
                        ab.descendants.insert(b_id);
                        for (ancestor_parent_h, _) in ab.parents.iter() {
                            ancestors.push_front(*ancestor_parent_h);
                        }
                    }
                }
            }
            Ok(res_graph)
        } else {
            Ok(GraphWorker {
                config: config.clone(),
                command_receiver,
                channels,
                shared_state,
                previous_slot,
                next_slot,
                next_instant,
                wishlist: Default::default(),
                final_block_stats,
                protocol_blocks: Default::default(),
                stale_block_stats: VecDeque::new(),
                stats_desync_detection_timespan,
                stats_history_timespan: std::cmp::max(
                    stats_desync_detection_timespan,
                    config.stats_timespan,
                ),
                launch_time: MassaTime::now(config.clock_compensation_millis)?,
                sequence_counter: 0,
                block_statuses,
                incoming_index: Default::default(),
                waiting_for_slot_index: Default::default(),
                waiting_for_dependencies_index: Default::default(),
                active_index: genesis_block_ids.iter().copied().collect(),
                discarded_index: Default::default(),
                latest_final_blocks_periods: genesis_block_ids.iter().map(|h| (*h, 0)).collect(),
                best_parents: genesis_block_ids.iter().map(|v| (*v, 0)).collect(),
                genesis_hashes: genesis_block_ids,
                gi_head: PreHashMap::default(),
                max_cliques: vec![Clique {
                    block_ids: PreHashSet::<BlockId>::default(),
                    fitness: 0,
                    is_blockclique: true,
                }],
                to_propagate: Default::default(),
                attack_attempts: Default::default(),
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
                storage,
            })

            //TODO: Add notify execution
        }
    }
}
