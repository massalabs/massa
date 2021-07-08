use crate::error::ConsensusError;
use crypto::hash::Hash;
use models::block::Block;
use std::collections::{hash_map, HashMap, HashSet, VecDeque};
use std::iter::FromIterator;

pub struct FutureIncomingBlocks {
    struct_deque: VecDeque<((u64, u8), Hash)>,
    struct_map: HashMap<Hash, Block>,
    max_size: usize,
}

impl FutureIncomingBlocks {
    pub fn new(max_size: usize) -> Self {
        FutureIncomingBlocks {
            struct_deque: VecDeque::with_capacity(max_size.saturating_add(1)),
            struct_map: HashMap::with_capacity(max_size),
            max_size,
        }
    }

    /// insert the block in the structure
    /// returns a discarded block if max_size has been reached -> used to update deps
    /// NB: the block we are trying to insert can be discarded
    pub fn insert(
        &mut self,
        hash: Hash,
        block: Block,
    ) -> Result<Option<(Hash, Block)>, ConsensusError> {
        if self.max_size == 0 {
            return Ok(Some((hash, block)));
        }
        let map_entry = match self.struct_map.entry(hash) {
            hash_map::Entry::Occupied(_) => return Ok(None), // already present
            hash_map::Entry::Vacant(vac) => vac,
        };
        // add into queue
        let slot = (block.header.period_number, block.header.thread_number);
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
                .struct_map
                .remove(&rem_hash)
                .ok_or(ConsensusError::ContainerInconsistency)?;
            Ok(Some((rem_hash, rem_block)))
        } else {
            Ok(None)
        }
    }

    pub fn pop_until(
        &mut self,
        until_slot: (u64, u8),
    ) -> Result<Vec<(Hash, Block)>, ConsensusError> {
        let mut res: Vec<(Hash, Block)> = Vec::new();
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
                self.struct_map
                    .remove(&hash)
                    .ok_or(ConsensusError::ContainerInconsistency)?,
            ));
        }
        Ok(res)
    }

    pub fn contains(&self, hash: &Hash) -> bool {
        return self.struct_map.contains_key(hash);
    }
}

pub struct DependencyWaitingBlocks {
    dep_to_blocked: HashMap<Hash, HashSet<Hash>>, // dep, hashes_blocked
    blocked_to_dep: HashMap<Hash, (u64, Block, HashSet<Hash>)>, // hash_blocked, (block_blocked, deps)
    vec_blocked: VecDeque<(u64, Hash)>,
    max_size: usize,
    counter: u64,
}

impl DependencyWaitingBlocks {
    pub fn new(max_size: usize) -> Self {
        DependencyWaitingBlocks {
            dep_to_blocked: HashMap::new(),
            blocked_to_dep: HashMap::with_capacity(max_size.saturating_add(1)),
            max_size,
            vec_blocked: VecDeque::with_capacity(max_size.saturating_add(1)),
            counter: 0,
        }
    }

    pub fn get_old(&mut self, latest_final_periods: Vec<u64>) -> HashSet<Hash> {
        // todo could be optimized (see issue #110)
        self.blocked_to_dep
            .iter()
            .filter_map(|(hash, (_, block, _))| {
                if block.header.period_number
                    <= latest_final_periods[block.header.thread_number as usize]
                {
                    Some(*hash)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn has_missing_deps(&self, hash: &Hash) -> bool {
        if let Some((_, _, deps)) = self.blocked_to_dep.get(hash) {
            return !deps.is_empty();
        }
        return false;
    }

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
        let mut to_promote: HashMap<Hash, (u64, u8, u64)> = HashMap::new();
        while let Some(pull_h) = stack.pop() {
            if to_promote.contains_key(&pull_h) {
                continue; // already traversed
            }
            if let Some((pull_seq, pull_block, pull_deps)) = self.blocked_to_dep.get(&pull_h) {
                stack.extend(pull_deps); // traverse dependencies
                to_promote.insert(
                    pull_h,
                    (
                        pull_block.header.period_number,
                        pull_block.header.thread_number,
                        *pull_seq,
                    ),
                );
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
        let mut sorted_promotions: Vec<(u64, u8, u64, Hash)> = to_promote
            .into_iter()
            .map(|(hash, (period, thread, sequence))| (period, thread, sequence, hash))
            .collect();
        sorted_promotions.sort_unstable();
        for (_, _, _, add_hash) in sorted_promotions.into_iter() {
            self.vec_blocked.push_back((self.counter, add_hash));
            self.blocked_to_dep
                .get_mut(&add_hash)
                .ok_or(ConsensusError::ContainerInconsistency)?
                .0 = self.counter;
            self.counter += 1;
        }

        Ok(())
    }

    // insert, return deleted blocks
    pub fn insert(
        &mut self,
        hash: Hash,
        block: Block,
        dependencies: HashSet<Hash>,
    ) -> Result<HashMap<Hash, Block>, ConsensusError> {
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
                vac.insert((self.counter, block, dependencies.clone()));
                self.vec_blocked.push_back((self.counter, hash));
                self.counter += 1;
            }
            hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().2.extend(dependencies);
            }
        }

        // promote
        self.promote(&hash)?;

        // pruning
        let mut removed: HashMap<Hash, Block> = HashMap::new();
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

    // get reference to block
    pub fn get(&self, hash: &Hash) -> Option<&Block> {
        self.blocked_to_dep.get(hash).map(|(_, block, _)| block)
    }

    /// a valid block was obtained.
    /// returns block if removed and a list blocks that should be retried
    pub fn valid_block_obtained(
        &mut self,
        obtained_h: &Hash,
    ) -> Result<(Option<Block>, HashSet<Hash>), ConsensusError> {
        // remove from blocked list
        let ret_block = if let Some((seq, block, deps)) = self.blocked_to_dep.remove(obtained_h) {
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
            Some(block)
        } else {
            None
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

        Ok((ret_block, retry_blocks))
    }

    // cancel a block and all its depencendy tree
    // returns the list of discarded blocks
    pub fn cancel(
        &mut self,
        cancel: HashSet<Hash>,
    ) -> Result<HashMap<Hash, Block>, ConsensusError> {
        let mut removed: HashMap<Hash, Block> = HashMap::new();
        let mut stack: Vec<Hash> = cancel.into_iter().collect();
        while let Some(hash) = stack.pop() {
            // remove if blocked
            if let Some((seq, block, deps)) = self.blocked_to_dep.remove(&hash) {
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
                // add to removed list
                removed.insert(hash, block);
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
    use models::block::BlockHeader;

    use crate::random_selector::RandomSelector;

    use super::*;

    fn create_standalone_block(period_number: u64, thread_number: u8) -> (Hash, Block) {
        let signature_engine = SignatureEngine::new();
        let private_key = SignatureEngine::generate_random_private_key();
        let public_key = signature_engine.derive_public_key(&private_key);
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let example_hash = Hash::hash("42".as_bytes());
        let parents = Vec::new();

        let header = BlockHeader {
            creator: public_key,
            thread_number,
            period_number,
            roll_number: selector.draw((period_number, thread_number)),
            parents,
            endorsements: Vec::new(),
            out_ledger_hash: example_hash,
            operation_merkle_root: example_hash,
        };

        let hash = header.compute_hash().expect("could not computte hash"); // in a test

        (
            hash,
            Block {
                header,
                operations: Vec::new(),
                signature: signature_engine
                    .sign(&hash, &private_key)
                    .expect("could not sign"), // in a test
            },
        )
    }

    #[test]
    fn test_promote() {
        let mut deps = DependencyWaitingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(1, 1);
        let (hash_b, block_b) = create_standalone_block(2, 1);
        let (hash_c, block_c) = create_standalone_block(3, 1);
        let (hash_d, block_d) = create_standalone_block(4, 1);

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        deps.insert(hash_a, block_a.clone(), deps_a.clone())
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        deps.insert(hash_c, block_c, deps_c).unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        deps.promote(&hash_a).unwrap();
        assert_eq!(deps.vec_blocked, vec![(3, hash_c), (4, hash_a)]);
    }

    #[test]
    fn test_insert() {
        let mut deps = DependencyWaitingBlocks::new(2);
        let (hash_a, block_a) = create_standalone_block(1, 1);
        let (hash_b, block_b) = create_standalone_block(2, 1);
        let (hash_c, block_c) = create_standalone_block(3, 1);
        let (hash_d, block_d) = create_standalone_block(4, 1);
        let (hash_e, block_e) = create_standalone_block(5, 1);
        let (hash_f, block_f) = create_standalone_block(6, 1);

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        deps.insert(hash_a, block_a.clone(), deps_a.clone())
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        deps.insert(hash_c, block_c, deps_c).unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        let mut deps_e = HashSet::new();
        deps_e.insert(hash_f);
        let removed = deps.insert(hash_e, block_e, deps_e).unwrap();

        // block_a has been
        assert_eq!(deps.vec_blocked, vec![(3, hash_c), (5, hash_e)]);
        assert!(removed.contains_key(&hash_a));
        assert_eq!(removed.len(), 1)
    }

    #[test]
    fn test_insert_chain_dependencies() {
        let mut deps = DependencyWaitingBlocks::new(2);
        let (hash_a, block_a) = create_standalone_block(1, 1);
        let (hash_b, block_b) = create_standalone_block(2, 1);
        let (hash_c, block_c) = create_standalone_block(3, 1);
        let (hash_e, block_e) = create_standalone_block(5, 1);

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        let removed = deps
            .insert(hash_a, block_a.clone(), deps_a.clone())
            .unwrap();
        assert_eq!(removed.len(), 0);

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_a);
        let removed = deps.insert(hash_c, block_c, deps_c).unwrap();
        assert_eq!(removed.len(), 0);
        assert_eq!(deps.vec_blocked, vec![(3, hash_a), (4, hash_c)]);

        let mut deps_e = HashSet::new();
        deps_e.insert(hash_c);
        let removed = deps.insert(hash_e, block_e, deps_e).unwrap();

        // block_a has been
        assert_eq!(deps.vec_blocked.len(), 0);
        assert_eq!(removed.len(), 3)
    }

    #[test]
    fn test_valid_block_obtained() {
        let mut deps = DependencyWaitingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(1, 1);
        let (hash_b, block_b) = create_standalone_block(2, 1);
        let (hash_c, block_c) = create_standalone_block(3, 1);
        let (hash_d, block_d) = create_standalone_block(4, 1);

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        deps.insert(hash_a, block_a.clone(), deps_a.clone())
            .unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        deps.insert(hash_c, block_c, deps_c).unwrap();

        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        let (block, hashset) = deps.valid_block_obtained(&hash_b).unwrap();

        assert!(block.is_none()); //block_b was never in the structure
        assert!(hashset.contains(&hash_a));
        assert_eq!(hashset.len(), 1);
        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);
    }

    #[test]
    fn test_cancel() {
        let mut deps = DependencyWaitingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(1, 1);
        let (hash_b, block_b) = create_standalone_block(2, 1);
        let (hash_c, block_c) = create_standalone_block(3, 1);
        let (hash_d, block_d) = create_standalone_block(4, 1);
        let (hash_e, block_e) = create_standalone_block(5, 1);

        let mut deps_a = HashSet::new();
        deps_a.insert(hash_b);
        let removed = deps
            .insert(hash_a, block_a.clone(), deps_a.clone())
            .unwrap();
        assert_eq!(removed.len(), 0);

        assert_eq!(deps.vec_blocked, vec![(1, hash_a)]);

        let mut deps_c = HashSet::new();
        deps_c.insert(hash_d);
        let removed = deps.insert(hash_c, block_c, deps_c).unwrap();
        assert_eq!(removed.len(), 0);
        assert_eq!(deps.vec_blocked, vec![(1, hash_a), (3, hash_c)]);

        let mut deps_e = HashSet::new();
        deps_e.insert(hash_a);
        let removed = deps.insert(hash_e, block_e, deps_e).unwrap();
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
        let (hash_a, block_a) = create_standalone_block(1, 0);
        let (hash_b, block_b) = create_standalone_block(2, 0);
        let (hash_c, block_c) = create_standalone_block(3, 0);
        let (hash_d, block_d) = create_standalone_block(4, 0);

        assert!(future.insert(hash_a, block_a).unwrap().is_none());
        assert!(future.insert(hash_b, block_b).unwrap().is_none());
        assert!(future.insert(hash_c, block_c).unwrap().is_none());
        if let Some((removed_hash, _removed_block)) = future.insert(hash_d, block_d).unwrap() {
            assert_eq!(removed_hash, hash_d);
            assert_eq!(future.struct_deque.len(), 3);
        } else {
            panic!("max_size should have been reached")
        }
    }

    #[test]
    fn test_insert_future_order() {
        let mut future = FutureIncomingBlocks::new(3);
        let (hash_a, block_a) = create_standalone_block(1, 0);
        let (hash_b, block_b) = create_standalone_block(2, 0);
        let (hash_c, block_c) = create_standalone_block(3, 0);
        let (hash_d, block_d) = create_standalone_block(4, 0);

        assert!(future.insert(hash_d, block_d).unwrap().is_none());
        assert!(future.insert(hash_b, block_b).unwrap().is_none());
        assert!(future.insert(hash_c, block_c).unwrap().is_none());
        if let Some((removed_hash, _removed_block)) = future.insert(hash_a, block_a).unwrap() {
            assert_eq!(removed_hash, hash_d);
            assert_eq!(future.struct_deque.len(), 3);
        } else {
            panic!("max_size should have been reached")
        }
    }

    #[test]
    fn test_pop_until() {
        let mut future = FutureIncomingBlocks::new(5);
        let (hash_a, block_a) = create_standalone_block(1, 0);
        let (hash_b, block_b) = create_standalone_block(2, 0);
        let (hash_c, block_c) = create_standalone_block(3, 0);
        let (hash_d, block_d) = create_standalone_block(4, 0);

        assert!(future.insert(hash_a, block_a).unwrap().is_none());
        assert!(future.insert(hash_b, block_b).unwrap().is_none());
        assert!(future.insert(hash_c, block_c).unwrap().is_none());
        assert!(future.insert(hash_d, block_d).unwrap().is_none());

        let popped = future.pop_until((3, 0)).unwrap();
        assert_eq!(
            popped.iter().map(|(h, _)| h).collect::<Vec<&Hash>>(),
            vec![&hash_a, &hash_b, &hash_c]
        )
    }
}
