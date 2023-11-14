use massa_channel::receiver::MassaReceiver;
use massa_consensus_exports::{
    block_status::{BlockStatus, StorageOrBlock},
    bootstrapable_graph::BootstrapableGraph,
    error::ConsensusError,
    ConsensusConfig,
};
use massa_execution_exports::ExecutionBlockMetadata;
use massa_hash::Hash;
use massa_models::{
    active_block::ActiveBlock,
    address::Address,
    block::{Block, BlockSerializer, SecureShareBlock},
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    prehash::PreHashMap,
    secure_share::SecureShareContent,
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
};
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tracing::info;

use crate::{commands::ConsensusCommand, state::ConsensusState};

use super::ConsensusWorker;

/// Creates genesis block in given thread.
///
/// # Arguments
/// * `cfg`: consensus configuration
/// * `thread_number`: thread in which we want a genesis block
///
/// # Returns
/// A genesis block
pub fn create_genesis_block(
    cfg: &ConsensusConfig,
    thread_number: u8,
) -> Result<SecureShareBlock, ConsensusError> {
    let keypair = &cfg.genesis_key;
    let header = BlockHeader::new_verifiable(
        BlockHeader {
            current_version: 0,
            announced_version: None,
            slot: Slot::new(cfg.last_start_period, thread_number),
            parents: Vec::new(),
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements: Vec::new(),
            denunciations: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        keypair,
    )?;

    Ok(Block::new_verifiable(
        Block {
            header,
            operations: Default::default(),
        },
        BlockSerializer::new(),
        keypair,
    )?)
}

impl ConsensusWorker {
    /// Creates a new consensus worker.
    ///
    /// # Arguments
    /// * `config`: consensus configuration
    /// * `command_receiver`: channel to receive commands from controller
    /// * `channels`: channels to communicate with other workers
    /// * `shared_state`: shared state with the controller
    /// * `init_graph`: Optional graph of blocks to initiate the worker
    /// * `storage`: shared storage
    ///
    /// # Returns:
    /// A `ConsensusWorker`, to interact with it use the `ConsensusController`
    pub fn new(
        config: ConsensusConfig,
        command_receiver: MassaReceiver<ConsensusCommand>,
        shared_state: Arc<RwLock<ConsensusState>>,
        init_graph: Option<BootstrapableGraph>,
        storage: Storage,
    ) -> Result<Self, ConsensusError> {
        let now = MassaTime::now();
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
            let block = create_genesis_block(&config, thread).map_err(|err| {
                ConsensusError::GenesisCreationError(format!("genesis error {}", err))
            })?;
            let mut storage = storage.clone_without_refs();
            storage.store_block(block.clone());
            genesis_block_ids.push(block.id);
            block_statuses.insert(
                block.id,
                BlockStatus::Active {
                    a_block: Box::new(ActiveBlock {
                        creator_address: block.content_creator_address,
                        parents: Vec::new(),
                        children: vec![PreHashMap::default(); config.thread_count as usize],
                        descendants: Default::default(),
                        is_final: true,
                        block_id: block.id,
                        slot: block.content.header.content.slot,
                        fitness: block.get_fitness(),
                        same_thread_parent_creator: None,
                    }),
                    storage_or_block: StorageOrBlock::Storage(storage),
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
        .estimate_instant()?;

        info!(
            "Started node at time {}, cycle {}, period {}, thread {}",
            now.format_instant(),
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

        if config.last_start_period > 0
            && config
                .genesis_timestamp
                .checked_add(config.t0.checked_mul(config.last_start_period)?)?
                > now
        {
            let (days, hours, mins, secs) = config
                .genesis_timestamp
                .checked_add(config.t0.checked_mul(config.last_start_period)?)?
                .saturating_sub(now)
                .days_hours_mins_secs()?;
            info!(
                "{} days, {} hours, {} minutes, {} seconds remaining to network restart",
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
                    Slot::new(config.last_start_period, thread),
                )?,
                genesis_addr,
                false,
            ))
        }

        let mut res_consensus = ConsensusWorker {
            config: config.clone(),
            command_receiver,
            shared_state,
            previous_slot,
            next_slot,
            next_instant,
        };

        // If the node starts after the genesis timestamp then it has to initialize its graph
        // with already produced blocks received from the bootstrap.
        if let Some(BootstrapableGraph { final_blocks }) = init_graph {
            // load final blocks
            let final_blocks: Vec<(ActiveBlock, StorageOrBlock)> = final_blocks
                .into_iter()
                .map(|export_b| export_b.to_active_block(config.thread_count))
                .collect::<Result<_, ConsensusError>>()?;

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
            // Initialize the shared state between the worker and the interface used by the other modules.
            {
                let mut write_shared_state = res_consensus.shared_state.write();
                write_shared_state.genesis_hashes = genesis_block_ids;
                write_shared_state.best_parents = latest_final_blocks_periods.clone();
                write_shared_state.latest_final_blocks_periods = latest_final_blocks_periods;
                for (b, storage_or_block) in final_blocks {
                    write_shared_state.blocks_state.transition_map(
                        &(b.block_id.clone()),
                        |_, _| {
                            Some(BlockStatus::Active {
                                a_block: Box::new(b),
                                storage_or_block,
                            })
                        },
                    );
                }
                write_shared_state.final_block_stats = final_block_stats;
            }

            res_consensus.claim_parent_refs()?;
        } else {
            // Initialize the shared state between the worker and the interface used by the other modules.
            {
                let mut write_shared_state = res_consensus.shared_state.write();
                write_shared_state.latest_final_blocks_periods =
                    genesis_block_ids.iter().map(|h| (*h, 0)).collect();
                write_shared_state.best_parents =
                    genesis_block_ids.iter().map(|v| (*v, 0)).collect();
                write_shared_state.genesis_hashes = genesis_block_ids;
                for (b, s) in block_statuses {
                    write_shared_state
                        .blocks_state
                        .transition_map(&b, |_, _| Some(s));
                }
                write_shared_state.final_block_stats = final_block_stats;
            }
        }

        // Notify execution module of current blockclique and all final blocks.
        // we need to do this because the bootstrap snapshots of the executor vs the consensus may not have been taken in sync
        // because the two modules run concurrently and out of sync.
        {
            let mut write_shared_state = res_consensus.shared_state.write();
            let mut block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata> =
                Default::default();
            let notify_finals: HashMap<Slot, BlockId> = write_shared_state
                .get_all_final_blocks()
                .into_iter()
                .map(|(b_id, (b_slot, b_metadata))| {
                    block_metadata.insert(b_id, b_metadata);
                    (b_slot, b_id)
                })
                .collect();
            let notify_blockclique: HashMap<Slot, BlockId> = write_shared_state
                .get_blockclique()
                .iter()
                .map(|b_id| {
                    let (a_block, storage_or_block) = write_shared_state
                        .get_full_active_block(b_id)
                        .expect("active block missing from block_db");
                    let storage = match storage_or_block {
                        StorageOrBlock::Storage(storage) => Some(storage.clone()),
                        _ => None,
                    };
                    block_metadata.insert(
                        *b_id,
                        ExecutionBlockMetadata {
                            same_thread_parent_creator: a_block.same_thread_parent_creator,
                            storage,
                        },
                    );
                    (a_block.slot, *b_id)
                })
                .collect();
            write_shared_state.prev_blockclique =
                notify_blockclique.iter().map(|(k, v)| (*v, *k)).collect();
            write_shared_state
                .channels
                .execution_controller
                .update_blockclique_status(notify_finals, Some(notify_blockclique), block_metadata);
        }

        Ok(res_consensus)
    }

    /// Internal function used at initialization of the `ConsensusWorker` to link blocks with their parents
    fn claim_parent_refs(&mut self) -> Result<(), ConsensusError> {
        let mut write_shared_state = self.shared_state.write();

        // list active block parents
        let active_blocks_map: PreHashMap<BlockId, (Slot, Vec<BlockId>)> = write_shared_state
            .blocks_state
            .iter()
            .filter_map(|(h, s)| {
                if let BlockStatus::Active { a_block: a, .. } = s {
                    return Some((*h, (a.slot, a.parents.iter().map(|(ph, _)| *ph).collect())));
                }
                None
            })
            .collect();

        for (b_id, (b_slot, b_parents)) in active_blocks_map.into_iter() {
            // update aprenthood relationships
            write_shared_state.insert_parents_descendants(b_id, b_slot, b_parents);
        }
        Ok(())
    }
}
