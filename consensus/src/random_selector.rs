use crypto::hash::Hash;
use models::slot::Slot;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_xoshiro::rand_core::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::convert::TryInto;

use crate::error::ConsensusError;

pub const RANDOM_SELECTOR_MIN_SEED_LENGTH: usize = 32;

/// Used to select staker for slot.
pub struct RandomSelector {
    thread_generators: Vec<Xoshiro256PlusPlus>,
    distribution: WeightedIndex<u64>,
    cache: Vec<Vec<u32>>,
}

impl RandomSelector {
    /// Creates a new selector.
    ///
    /// # Arguments
    /// * seed: intitiation seed.
    /// * thread_count: number of threads
    /// * participant_weights: participants' weights.
    ///
    /// Note: we must know every possible staker.
    pub fn new(
        seed: &Vec<u8>,
        thread_count: u8,
        participant_weights: Vec<u64>,
    ) -> Result<Self, ConsensusError> {
        if seed.len() < RANDOM_SELECTOR_MIN_SEED_LENGTH {
            return Err(ConsensusError::SmallSeedError);
        }
        let mut thread_generators = Vec::with_capacity(thread_count as usize);
        thread_generators.push(Xoshiro256PlusPlus::from_seed(
            Hash::hash(&seed)
                .to_bytes()
                .try_into()
                .map_err(|_| ConsensusError::HashConversionError)?,
        ));
        for i in 1..(thread_count as usize) {
            let mut derived_generator = thread_generators[i - 1].clone();
            derived_generator.long_jump();
            thread_generators.push(derived_generator);
        }
        Ok(RandomSelector {
            thread_generators,
            distribution: WeightedIndex::new(participant_weights)?,
            cache: vec![Vec::new(); thread_count as usize],
        })
    }

    /// Draws a staker for given slot.
    /// For given selector and node, the result is deterministic.
    ///
    /// # Argument
    /// * slot: we want to know which staker is selected for that slot.
    pub fn draw(&mut self, slot: Slot) -> u32 {
        while slot.period >= (self.cache[slot.thread as usize].len() as u64) {
            self.cache[slot.thread as usize].push(
                self.distribution
                    .sample(&mut self.thread_generators[slot.thread as usize])
                    as u32,
            );
        }
        self.cache[slot.thread as usize][slot.period as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_3thread_4participant_random_selector() -> RandomSelector {
        let seed = vec![0u8; RANDOM_SELECTOR_MIN_SEED_LENGTH];
        let distrib = vec![1u64, 2u64, 1u64, 3u64];
        RandomSelector::new(&seed, 3, distrib).unwrap() // in a test
    }

    #[test]
    fn test_same_node_multiple_init() {
        let mut selector1 = get_3thread_4participant_random_selector();
        let mut selector2 = get_3thread_4participant_random_selector();

        for index in 0..1000 {
            let rnd1 = selector1.draw(Slot::new(1210 + index, 1));
            let rnd2 = selector2.draw(Slot::new(1210 + index, 1));
            assert_eq!(rnd1, rnd2, "determinism problem")
        }
    }

    #[test]
    fn test_selected_node_diff_between_threads() {
        let mut selector = get_3thread_4participant_random_selector();

        let mut overlaps = 0;
        let trials: u64 = 1000;
        for index in 0..trials {
            let rnd1 = selector.draw(Slot::new(1210 + index, 0));
            let rnd2 = selector.draw(Slot::new(1210 + index, 1));
            if rnd1 == rnd2 {
                overlaps += 1;
            }
        }
        assert_eq!(
            overlaps, 297,
            "unexpected overlap between the sequences of two threads"
        );
    }

    #[test]
    fn test_consecutive_draws() {
        let mut selector = get_3thread_4participant_random_selector();
        let mut overlaps = 0;
        let trials: u64 = 1000;
        let mut prev = selector.draw(Slot::new(1210, 2));
        for index in 1..trials {
            let new = selector.draw(Slot::new(1210 + index, 2));
            if new == prev {
                overlaps += 1;
            }
            prev = new;
        }
        assert_eq!(
            overlaps, 342,
            "unexpected overlap between the consecutive draws"
        );
    }

    #[test]
    fn test_same_draw() {
        let mut selector = get_3thread_4participant_random_selector();
        let trials: u64 = 1000;
        let prev = selector.draw(Slot::new(1210, 2));
        for _ in 1..trials {
            let new = selector.draw(Slot::new(1210, 2));
            assert_eq!(
                prev, new,
                "consecutive draws of the same iteration are not be the same"
            );
        }
    }
}
