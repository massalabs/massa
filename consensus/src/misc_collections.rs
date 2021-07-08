use crate::error::ConsensusError;
use crypto::hash::Hash;
use models::{Block, BlockHeader, Slot};
use std::collections::{hash_map, HashMap, HashSet, VecDeque};
use std::iter::FromIterator;

pub enum QueuedBlock {
    HeaderOnly(BlockHeader),
    FullBLock(Block),
}

impl QueuedBlock {
    fn header(&self) -> &BlockHeader {
        match self {
            QueuedBlock::HeaderOnly(h) => h,
            QueuedBlock::FullBLock(b) => &b.header,
        }
    }
}

/// Sophisticated queue containing blocks with slotsd in the near future.
pub struct FutureIncomingBlocks {
    /// The queue as a vecdeque of (slot, header's hash).
    struct_deque: VecDeque<(Slot, Hash)>,
    /// The queue as a map linking header's hash and the whole block.
    map: HashMap<Hash, QueuedBlock>,
    /// The queue as a map linking header's hash and the block header.
    max_size: usize,
}

impl FutureIncomingBlocks {
    /// Creates a new queue.
    ///
    /// # Argument
    /// * max_size: Maximum number of blocks we allow in the queue .
    pub fn new(max_size: usize) -> Self {
        FutureIncomingBlocks {
            struct_deque: VecDeque::with_capacity(max_size.saturating_add(1)),
            map: HashMap::with_capacity(max_size),
            max_size,
        }
    }

    /// Inserts the block in the structure.
    /// Returns a discarded block if max_size has been reached -> used to update dependencies
    ///
    /// Note: the block we are trying to insert can be discarded
    ///
    /// # Arguments
    /// * hash: block's header hash
    /// * block: block to insert
    pub fn insert(
        &mut self,
        hash: Hash,
        block: QueuedBlock,
    ) -> Result<Option<(Hash, QueuedBlock)>, ConsensusError> {
        if self.max_size == 0 {
            return Ok(Some((hash, block)));
        }
        let map_entry = match self.map.entry(hash) {
            hash_map::Entry::Occupied(_) => return Ok(None), // already present
            hash_map::Entry::Vacant(vac) => vac,
        };
        // add into queue
        let slot = block.header().content.slot;
        let pos: usize = self
            .struct_deque
            .binary_search(&(slot, hash))
            .err()
            .ok_or(ConsensusError::ContainerInconsistency)?;
        if pos > self.max_size {
            // beyond capacity
            return Ok(Some((hash, block)));
        }
        self.struct_deque.insert(pos, (slot, hash));
        // add into map
        map_entry.insert(block);

        // remove over capacity
        if self.struct_deque.len() > self.max_size {
            let (_, rem_hash) = self
                .struct_deque
                .pop_back()
                .ok_or(ConsensusError::ContainerInconsistency)?;
            let rem_block = self
                .map
                .remove(&rem_hash)
                .ok_or(ConsensusError::ContainerInconsistency)?;
            Ok(Some((rem_hash, rem_block)))
        } else {
            Ok(None)
        }
    }

    /// Pops blocks out of the structure
    /// if their slot is older than given slot.
    ///
    /// Note: given slot is included.
    ///
    /// # Argument
    /// * until_slot: we want to pop blocks until this slot.
    pub fn pop_until(
        &mut self,
        until_slot: Slot,
    ) -> Result<Vec<(Hash, QueuedBlock)>, ConsensusError> {
        let mut res: Vec<(Hash, QueuedBlock)> = Vec::new();
        while let Some(&(slot, hash)) = self.struct_deque.front() {
            if slot > until_slot {
                break;
            }
            let _ = self
                .struct_deque
                .pop_front()
                .ok_or(ConsensusError::ContainerInconsistency)?;
            res.push((
                hash,
                self.map
                    .remove(&hash)
                    .ok_or(ConsensusError::ContainerInconsistency)?,
            ));
        }
        Ok(res)
    }

    /// Checks if given hash is in the structure.
    ///
    /// # Argument
    /// * hash: we want to know if this hash is in the structure.
    pub fn contains(&self, hash: &Hash) -> bool {
        return self.map.contains_key(hash);
    }
}

/// The perfect mix between a queue and a dependencies double chained graph, for dependency waiting blocks.
pub struct DependencyWaitingBlocks {
    /// Maps a dependency's hash to the set of hashes of the blocks that are blocked, wainting for this dependency.
    dep_to_blocked: HashMap<Hash, HashSet<Hash>>, // dep, hashes_blocked
    /// Maps a header's hash to a number in the queue, the whole block and the set of dependencies that block is waiting for.
    blocked_to_dep: HashMap<Hash, (u64, BlockHeader, HashSet<Hash>)>, // hash_blocked, (block_blocked, deps)
    /// VecDeque of (number in the queue, hash).
    /// Note that the number in the queue is not the index in vecdeque
    /// due to update method.
    vec_blocked: VecDeque<(u64, Hash)>,
    /// headers waiting for a dependency
    // headers: HashMap<Hash, BlockHeader>,
    /// blocks waiting for a dependency
    blocks: HashMap<Hash, Block>,
    /// Maximum number of blocked blocks we allow in the structure.
    max_size: usize,
    /// Current next number in the queue.
    counter: u64,
}

impl DependencyWaitingBlocks {
    /// Creates a new DependencyWaitingBlocks.
    ///
    /// # Argument
    /// * max_size: Maximum number of blocked blocks we allow in the structure.
    pub fn new(max_size: usize) -> Self {
        DependencyWaitingBlocks {
            dep_to_blocked: HashMap::new(),
            blocked_to_dep: HashMap::with_capacity(max_size.saturating_add(1)),
            max_size,
            //headers: HashMap::with_capacity(max_size.saturating_add(1)),
            blocks: HashMap::with_capacity(max_size.saturating_add(1)),
            vec_blocked: VecDeque::with_capacity(max_size.saturating_add(1)),
            counter: 0,
        }
    }

    /// Gets blocks that are older than implicitly given slots.
    ///
    /// # Argument
    /// * latest_final_periods: Vec of the lastest periods in each thread.
    ///   (i, period) in latest_final_periods.enumerate are the slots we consider.
    pub fn get_old(&mut self, latest_final_periods: Vec<u64>) -> HashSet<Hash> {
        // todo could be optimized (see issue #110)
        self.blocked_to_dep
            .iter()
            .filter_map(|(hash, (_, header, _))| {
                if header.content.slot.period
                    <= latest_final_periods[header.content.slot.thread as usize]
                {
                    Some(*hash)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Checks if block corresponding to given hash still has at least one dependency it is waiting for.
    ///
    /// # Arguments
    /// * hash: hash of the block we consider.
    pub fn has_missing_deps(&self, hash: &Hash) -> bool {
        if let Some((_, _, deps)) = self.blocked_to_dep.get(hash) {
            return !deps.is_empty();
        }
        return false;
    }

    /// Recursively put at the front of the queue blocks that given hash is waiting for.
    /// At the end of the day, given hash is at the front of the queue, then its parents,
    /// then its grand parents, and so on, and at the end alle the others, in the same order they were before.
    ///
    /// # Argument
    /// * hash: hash of the considered block.
    pub fn promote(&mut self, hash: &Hash) -> Result<(), ConsensusError> {
        // list promotable dep tree, remove them from vecdeque
        let mut stack: Vec<Hash> = self
            .blocked_to_dep
            .get(hash)
            .ok_or(ConsensusError::MissingBlock)?
            .2
            .iter()
            .copied()
            .chain([*hash].iter().copied())
            .collect();
        let mut to_promote: HashMap<Hash, (Slot, u64)> = HashMap::new();
        while let Some(pull_h) = stack.pop() {
            if to_promote.contains_key(&pull_h) {
                continue; // already traversed
            }
            if let Some((pull_seq, pull_header, pull_deps)) = self.blocked_to_dep.get(&pull_h) {
                stack.extend(pull_deps); // traverse dependencies
                to_promote.insert(pull_h, (pull_header.content.slot, *pull_seq));
                // remove from vecdeque
                self.vec_blocked
                    .remove(
                        self.vec_blocked
                            .binary_search(&(*pull_seq, pull_h))
                            .map_err(|_| ConsensusError::ContainerInconsistency)?,
                    )
                    .ok_or(ConsensusError::ContainerInconsistency)?;
            }
        }

        // sort promotion list by slot and original seq number, and insert it at the end of the vecdeque
        let mut sorted_promotions: Vec<(Slot, u64, Hash)> = to_promote
            .into_iter()
            .map(|(hash, (slot, sequence))| (slot, sequence, hash))
            .collect();
        sorted_promotions.sort_unstable();
        for (_, _, add_hash) in sorted_promotions.into_iter() {
            self.vec_blocked.push_back((self.counter, add_hash));
            self.blocked_to_dep
                .get_mut(&add_hash)
                .ok_or(ConsensusError::ContainerInconsistency)?
                .0 = self.counter;
            self.counter += 1;
        }

        Ok(())
    }

    /// Inserts block and dependencies in the structure, returns deleted blocks.
    ///
    /// # Arguments
    /// * hash: hash of the considered block
    /// * block: blocked block.
    /// * dependencies: dependencies that given block is waiting for
    pub fn insert(
        &mut self,
        hash: Hash,
        header: BlockHeader,
        block: Option<Block>,
        dependencies: HashSet<Hash>,
    ) -> Result<HashMap<Hash, (BlockHeader, Option<Block>)>, ConsensusError> {
        if self.max_size == 0 {
            return Ok(HashMap::new());
        }
        // add new block to structures
        dependencies.iter().for_each(|&dep_h| {
            self.dep_to_blocked
                .entry(dep_h)
                .and_modify(|dep_set| drop(dep_set.insert(hash)))
                .or_insert(HashSet::from_iter([hash].iter().copied()));
        });
        match self.blocked_to_dep.entry(hash) {
            hash_map::Entry::Vacant(vac) => {
                vac.insert((self.counter, header, dependencies.clone()));
                self.vec_blocked.push_back((self.counter, hash));
                self.counter += 1;
                if let Some(b) = block {
                    self.blocks.insert(hash, b);
                }
            }
            hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().2.extend(dependencies);
            }
        }

        // promote
        self.promote(&hash)?;

        // pruning
        let mut removed: HashMap<Hash, (BlockHeader, Option<Block>)> = HashMap::new();
        while self.vec_blocked.len() > self.max_size {
            let (_, rem_hash) = self
                .vec_blocked
                .front()
                .ok_or(ConsensusError::ContainerInconsistency)?
                .clone();
            removed.extend(self.cancel([rem_hash].iter().copied().collect())?);
        }

        Ok(removed)
    }

    /// Get reference to block.
    ///
    /// # Argument
    /// * hash : hash of the block we want to retrive.
    pub fn get(&self, hash: &Hash) -> (Option<BlockHeader>, Option<&Block>) {
        let header = self
            .blocked_to_dep
            .get(hash)
            .map(|(_, header, _)| header.clone());
        let block = self.blocks.get(hash);
        (header, block)
    }

    /// A valid block was obtained.
    /// It is removed from the structure.
    /// Returns block if removed and a list blocks that should be retried.
    ///
    /// # Arguments
    /// * obtained_h: hash of considered block.
    pub fn valid_block_obtained(
        &mut self,
        obtained_h: &Hash,
    ) -> Result<(Option<BlockHeader>, Option<Block>, HashSet<Hash>), ConsensusError> {
        // remove from blocked list
        let (ret_header, ret_block) =
            if let Some((seq, header, deps)) = self.blocked_to_dep.remove(obtained_h) {
                if !deps.is_empty() {
                    // if it is valid, it should have no missing deps
                    return Err(ConsensusError::ContainerInconsistency);
                }
                // remove from deque
                self.vec_blocked
                    .remove(
                        self.vec_blocked
                            .binary_search(&(seq, *obtained_h))
                            .map_err(|_| ConsensusError::ContainerInconsistency)?,
                    )
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                let block = self.blocks.remove(obtained_h);
                (Some(header), block)
            } else {
                (None, None)
            };

        // trigger blocks waiting for it
        let mut retry_blocks: HashSet<Hash> = HashSet::new();
        if let Some(unlock_block_hashes) = self.dep_to_blocked.remove(obtained_h) {
            for unlock_block_hash in unlock_block_hashes.iter() {
                self.blocked_to_dep
                    .get_mut(unlock_block_hash)
                    .ok_or(ConsensusError::ContainerInconsistency)?
                    .2
                    .remove(&obtained_h)
                    .then_some(())
                    .ok_or(ConsensusError::ContainerInconsistency)?;
            }
            retry_blocks.extend(unlock_block_hashes);
        }

        Ok((ret_header, ret_block, retry_blocks))
    }

    /// Cancels a set of blocks and all its depencendy tree.
    /// Returns the list of discarded blocks.
    ///
    /// # Arguments
    /// * cancel: set of blocks to cancel.
    pub fn cancel(
        &mut self,
        cancel: HashSet<Hash>,
    ) -> Result<HashMap<Hash, (BlockHeader, Option<Block>)>, ConsensusError> {
        let mut removed: HashMap<Hash, (BlockHeader, Option<Block>)> = HashMap::new();
        let mut stack: Vec<Hash> = cancel.into_iter().collect();
        while let Some(hash) = stack.pop() {
            // remove if blocked
            if let Some((seq, header, deps)) = self.blocked_to_dep.remove(&hash) {
                // remove from deque
                self.vec_blocked
                    .remove(
                        self.vec_blocked
                            .binary_search(&(seq, hash))
                            .map_err(|_| ConsensusError::ContainerInconsistency)?,
                    )
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                // remove from dep unlock lists
                for dep_h in deps.into_iter() {
                    if let hash_map::Entry::Occupied(mut entry) = self.dep_to_blocked.entry(dep_h) {
                        entry.get_mut().remove(&hash);
                        if entry.get().is_empty() {
                            entry.remove_entry();
                        }
                    }
                }
                let block = self.blocks.remove(&hash);
                // add to removed list
                removed.insert(hash, (header, block));
            }

            // drop the dependency and stack those that depend on it
            if let Some(depending) = self.dep_to_blocked.remove(&hash) {
                stack.extend(depending.into_iter());
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use crypto::signature::SignatureEngine;
    use models::{BlockHeader, BlockHeaderContent, SerializationContext};

    use crate::random_selector::RandomSelector;

    use super::*;

    fn create_standalone_block(slot: Slot) -> (Hash, Block) {
        let mut signature_engine = SignatureEngine::new();
        let private_key = SignatureEngine::generate_random_private_key();
        let public_key = signature_engine.derive_public_key(&private_key);
        let mut _selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let example_hash = Hash::hash("42".as_bytes());
        let parents = Vec::new();

        let (hash, header) = BlockHeader::new_signed(
            &mut signature_engine,
            &private_key,
            BlockHeaderContent {
                creator: public_key,
                slot,
                parents: parents.clone(),
                out_ledger_hash: example_hash,
                operation_merkle_root: example_hash,
            },
            &SerializationContext {
                max_block_size: 1024 * 1024,
                max_block_operations: 1024,
                parent_count: parents.len() as u8,
                max_peer_list_length: 128,
                max_message_size: 3 * 1024 * 1024,
            },
        )
        .unwrap();

        (
            hash,
            Block {
                header,
                operations: Vec::new(),
            },
        )
    }

    #[test]
    fn test_promote() {
        let mut deps = DependencyWaitingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 1));
        let (hash_b, _block_b) = create_standalone_block(Slot::new(2, 1));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 1));
        let (hash_d, _block_d) = create_standalone_block(Slot::new(4, 1));

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        deps.insert(hash_a, block_a.header.clone(), None, deps_a.clone())
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        deps.insert(hash_c, block_c.header.clone(), None, deps_c)
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        deps.promote(&hash_a).unwrap();
        assert_eq!(deps.vec_blocked, vec![(3, hash_c), (4, hash_a)]);
    }

    #[test]
    fn test_insert() {
        let mut deps = DependencyWaitingBlocks::new(2);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 1));
        let (hash_b, _block_b) = create_standalone_block(Slot::new(2, 1));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 1));
        let (hash_d, _block_d) = create_standalone_block(Slot::new(4, 1));
        let (hash_e, block_e) = create_standalone_block(Slot::new(5, 1));
        let (hash_f, _block_f) = create_standalone_block(Slot::new(6, 1));

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        deps.insert(hash_a, block_a.header.clone(), None, deps_a.clone())
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        deps.insert(hash_c, block_c.header.clone(), None, deps_c)
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        let mut deps_e = HashSet::new();
        deps_e.insert(hash_f);
        let removed = deps
            .insert(hash_e, block_e.header.clone(), None, deps_e)
            .unwrap();

        // block_a has been
        assert_eq!(deps.vec_blocked, vec![(3, hash_c), (5, hash_e)]);
        assert!(removed.contains_key(&hash_a));
        assert_eq!(removed.len(), 1)
    }

    #[test]
    fn test_insert_chain_dependencies() {
        let mut deps = DependencyWaitingBlocks::new(2);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 1));
        let (hash_b, _block_b) = create_standalone_block(Slot::new(2, 1));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 1));
        let (hash_e, block_e) = create_standalone_block(Slot::new(5, 1));

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        let removed = deps
            .insert(hash_a, block_a.header.clone(), None, deps_a.clone())
            .unwrap();
        assert_eq!(removed.len(), 0);

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_a);
        let removed = deps
            .insert(hash_c, block_c.header.clone(), None, deps_c)
            .unwrap();
        assert_eq!(removed.len(), 0);
        assert_eq!(deps.vec_blocked, vec![(3, hash_a), (4, hash_c)]);

        let mut deps_e = HashSet::new();
        deps_e.insert(hash_c);
        let removed = deps
            .insert(hash_e, block_e.header.clone(), None, deps_e)
            .unwrap();

        // block_a has been
        assert_eq!(deps.vec_blocked.len(), 0);
        assert_eq!(removed.len(), 3)
    }

    #[test]
    fn test_valid_block_obtained() {
        let mut deps = DependencyWaitingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 1));
        let (hash_b, _block_b) = create_standalone_block(Slot::new(2, 1));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 1));
        let (hash_d, _block_d) = create_standalone_block(Slot::new(4, 1));

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        deps.insert(hash_a, block_a.header.clone(), None, deps_a.clone())
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        deps.insert(hash_c, block_c.header.clone(), None, deps_c)
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        let (header, block, hashset) = deps.valid_block_obtained(&hash_b).unwrap();

        assert!(header.is_none()); //block_b was never in the structure
        assert!(hashset.contains(&hash_a));
        assert_eq!(hashset.len(), 1);
        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);
    }

    #[test]
    fn test_cancel() {
        let mut deps = DependencyWaitingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 1));
        let (hash_b, _block_b) = create_standalone_block(Slot::new(2, 1));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 1));
        let (hash_d, _block_d) = create_standalone_block(Slot::new(4, 1));
        let (hash_e, block_e) = create_standalone_block(Slot::new(5, 1));

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        let removed = deps
            .insert(hash_a, block_a.header.clone(), None, deps_a.clone())
            .unwrap();
        assert_eq!(removed.len(), 0);

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        let removed = deps
            .insert(hash_c, block_c.header.clone(), None, deps_c)
            .unwrap();
        assert_eq!(removed.len(), 0);
        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        let mut deps_e = HashSet::new();
        deps_e.insert(hash_a);
        let removed = deps
            .insert(hash_e, block_e.header.clone(), None, deps_e)
            .unwrap();
        assert_eq!(
            deps.vec_blocked,
            vec![(3, hash_c), (5, hash_a), (6, hash_e)]
        );
        assert_eq!(removed.len(), 0);

        let mut to_cancel = HashSet::new();
        to_cancel.insert(hash_b);

        let cancelled = deps.cancel(to_cancel).unwrap();
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.contains_key(&hash_a));
        assert!(cancelled.contains_key(&hash_e));
    }

    #[test]
    fn test_insert_future() {
        let mut future = FutureIncomingBlocks::new(3);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 0));
        let (hash_b, block_b) = create_standalone_block(Slot::new(2, 0));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 0));
        let (hash_d, block_d) = create_standalone_block(Slot::new(4, 0));

        assert!(future
            .insert(hash_a, QueuedBlock::FullBLock(block_a))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_b, QueuedBlock::FullBLock(block_b))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_c, QueuedBlock::FullBLock(block_c))
            .unwrap()
            .is_none());
        if let Some((removed_hash, removed_blockd)) = future
            .insert(hash_d, QueuedBlock::FullBLock(block_d))
            .unwrap()
        {
            assert_eq!(removed_hash, hash_d);
            assert_eq!(future.struct_deque.len(), 3);
        } else {
            panic!("max_size should have been reached")
        }
    }

    #[test]
    fn test_insert_future_order() {
        let mut future = FutureIncomingBlocks::new(3);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 0));
        let (hash_b, block_b) = create_standalone_block(Slot::new(2, 0));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 0));
        let (hash_d, block_d) = create_standalone_block(Slot::new(4, 0));

        assert!(future
            .insert(hash_d, QueuedBlock::FullBLock(block_d))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_b, QueuedBlock::FullBLock(block_b))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_c, QueuedBlock::FullBLock(block_c))
            .unwrap()
            .is_none());
        if let Some((removed_hash, _removed_block)) = future
            .insert(hash_a, QueuedBlock::FullBLock(block_a))
            .unwrap()
        {
            assert_eq!(removed_hash, hash_d);
            assert_eq!(future.struct_deque.len(), 3);
        } else {
            panic!("max_size should have been reached")
        }
    }

    #[test]
    fn test_pop_until() {
        let mut future = FutureIncomingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(Slot::new(1, 0));
        let (hash_b, block_b) = create_standalone_block(Slot::new(2, 0));
        let (hash_c, block_c) = create_standalone_block(Slot::new(3, 0));
        let (hash_d, block_d) = create_standalone_block(Slot::new(4, 0));

        assert!(future
            .insert(hash_a, QueuedBlock::FullBLock(block_a))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_b, QueuedBlock::FullBLock(block_b))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_c, QueuedBlock::FullBLock(block_c))
            .unwrap()
            .is_none());
        assert!(future
            .insert(hash_d, QueuedBlock::FullBLock(block_d))
            .unwrap()
            .is_none());

        let popped = future.pop_until(Slot::new(3, 0)).unwrap();
        assert_eq!(
            popped.iter().map(|(h, _)| h).collect::<Vec<&Hash>>(),
            vec![&hash_a, &hash_b, &hash_c]
        )
    }
}
