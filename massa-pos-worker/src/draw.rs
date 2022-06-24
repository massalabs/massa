use massa_hash::Hash;
use massa_models::{prehash::Map, Address, Slot};
use massa_pos_exports::{CycleInfo, PosError::*, PosResult, Selection};
use massa_signature::derive_public_key;

use rand::{distributions::Uniform, Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

use std::collections::HashMap;

use crate::worker::SelectorThread;

struct Seed {
    hash: Vec<u8>,
    cum_sum: Vec<(u64, Address)>,
}

impl SelectorThread {
    /// Regulary prune `cycle_states` and `cache` pointer
    pub(crate) fn prune_cache(&mut self) {
        // truncate cache to keep only the desired number of elements
        // we do it first to free memory space
        {
            let cache = &mut self.cache.write();
            if cache.len() >= self.cfg.max_draw_cache {
                let latest_cycle = *cache.last_key_value().unwrap().0 as usize;
                cache.drain_filter(|cycle, _| {
                    (*cycle as usize) < latest_cycle - self.cfg.max_draw_cache
                });
            }
        }
        if self.cycle_states.len() >= self.cfg.max_draw_cache {
            let latest_cycle = *self.cycle_states.last_key_value().unwrap().0 as usize;
            self.cycle_states.drain_filter(|cycle, _| {
                (*cycle as usize) < latest_cycle - self.cfg.max_draw_cache
            });
        }
    }

    /// Compute the RNG seed from the of a cycle from the final roll datas of that cycle.
    ///
    /// compute the RNG seed from the seed bits and return it.
    ///
    /// # Result
    /// The method can return an error when:
    /// - Tried to get an unavailable cycle or thread
    /// - Tried to get seed of a not finalized cycle
    fn get_seed_from_finals(&mut self, cycle_info: &CycleInfo) -> PosResult<Seed> {
        let cum_sum = match self.cycle_states.get(&(cycle_info.cycle - 1)) {
            Some(cum_sum) => cum_sum.clone(),
            _ => return Err(CycleUnavailable(cycle_info.cycle)),
        };
        self.cycle_states
            .insert(cycle_info.cycle, cumulate_sum(&cycle_info.roll_counts));
        Ok(Seed {
            hash: Hash::compute_from(&cycle_info.rng_seed.clone().into_vec())
                .to_bytes()
                .to_vec(),
            cum_sum,
        })
    }

    /// Compute a seed from the initial rools.
    fn get_seed_from_initials(&mut self, cycle_info: &CycleInfo) -> PosResult<Seed> {
        let init_rolls = match self.initial_rolls.get(cycle_info.cycle as usize) {
            Some(init_rolls) => init_rolls,
            _ => return Err(InitCycleUnavailable),
        };
        let cum_sum = cumulate_sum(init_rolls);
        self.cycle_states.insert(cycle_info.cycle, cum_sum.clone());
        let hash = match self.initial_seeds.get(cycle_info.cycle as usize) {
            Some(hash) => hash.clone(),
            _ => return Err(InitCycleUnavailable),
        };
        Ok(Seed { hash, cum_sum })
    }

    /// Get seed to compute the draw for C+2. Fill the `cycle_states` variable
    /// with the cumulative function of roll distribution for the current cycle
    /// info.
    ///
    /// # Seeds composition
    /// For a cycle, the full seed is composed of the rng_seed of the C-2 and
    /// the roll distribution of the C-3.
    ///
    /// # Description
    /// If cycle < `cfg.loopback_cycle` we use the predefined values for
    /// the hash and for the roll distribution.
    ///
    /// Otherwise, we use the given `cycle_info` for the seed and the roll
    /// distribution stored in `cycle_states` at index `cycle_info.cycle - 1`.
    fn get_seed(&mut self, cycle_info: &CycleInfo) -> PosResult<Seed> {
        if cycle_info.cycle as usize > self.cfg.lookback_cycles {
            self.get_seed_from_finals(cycle_info)
        } else {
            self.get_seed_from_initials(cycle_info)
        }
    }

    /// Perform the computation of the draws given a seed `rng`
    fn perform(
        &self,
        mut rng: Xoshiro256PlusPlus,
        cycle_info: &CycleInfo,
        cum_sum: &Vec<(u64, Address)>,
        cum_sum_max: u64,
    ) -> HashMap<Slot, Selection> {
        // perform draws
        let distribution = Uniform::new(0, cum_sum_max);
        let mut draws = HashMap::with_capacity(self.cfg.blocks_in_cycle);
        let mut cycle_first_period = cycle_info.cycle * self.cfg.periods_per_cycle;
        let cycle_last_period = (cycle_info.cycle + 1) * self.cfg.periods_per_cycle - 1;
        if cycle_first_period == 0 {
            // genesis slots: force block creator and endorsement creator address draw
            let genesis_addr = Address::from_public_key(&derive_public_key(&self.cfg.genesis_key));
            for draw_thread in 0..self.cfg.thread_count {
                draws.insert(
                    Slot::new(0, draw_thread),
                    Selection {
                        producer: genesis_addr,
                        endorsments: vec![genesis_addr; self.cfg.endorsement_count],
                    },
                );
            }
            // do not draw genesis again
            cycle_first_period += 1;
        }
        for draw_period in cycle_first_period..=cycle_last_period {
            for draw_thread in 0..self.cfg.thread_count {
                let mut res = Vec::with_capacity(self.cfg.endorsement_count + 1);
                // draw block creator and endorsers with the same probabilities
                for _ in 0..(self.cfg.endorsement_count + 1) {
                    let sample = rng.sample(&distribution);
                    // locate the draw in the cum_sum through binary search
                    let found_index =
                        match cum_sum.binary_search_by_key(&sample, |(c_sum, _)| *c_sum) {
                            Ok(idx) => idx + 1,
                            Err(idx) => idx,
                        };
                    let (_sum, found_addr) = cum_sum[found_index];
                    res.push(found_addr)
                }
                draws.insert(
                    Slot::new(draw_period, draw_thread),
                    Selection {
                        producer: res[0],
                        endorsments: res[1..].to_vec(),
                    },
                );
            }
        }
        draws
    }

    /// Draws the endorsements and the block creator for C + 2 and store in
    /// cache.
    ///
    /// # Parameters
    /// * cycle_info: Latest final cycle information.
    ///
    /// # Result
    /// - The draws can throw the errors of the function [get_seed] and from the
    ///   creation of a [Xoshiro256PlusPlus].
    /// - An inconsistency error is thrown when the cumulated sum of roll
    ///   distribution for C-1 is empty.
    /// - If the given parameter isn't notified as `complete`
    ///
    /// Otherwise, the draws return an empty success.
    pub(crate) fn draws(&mut self, cycle_info: CycleInfo) -> PosResult<()> {
        if !cycle_info.complete {
            return Err(CycleUnfinalised(cycle_info.cycle));
        }
        let seed = self.get_seed(&cycle_info)?;
        let draws = self.perform(
            Xoshiro256PlusPlus::from_seed(seed.hash.try_into().map_err(|_| CannotComputeSeed)?),
            &cycle_info,
            &seed.cum_sum,
            seed.cum_sum
                .last()
                .ok_or_else(|| EmptyContainerInconsistency)?
                .0,
        );
        self.cache.write().insert(cycle_info.cycle + 2, draws);
        Ok(())
    }
}

/// Compute the cumulative distribution function. It will be used in the
/// `perform` function for the selection's probability related to the number
/// of rolls by address.
fn cumulate_sum(roll_counts: &Map<Address, u64>) -> Vec<(u64, Address)> {
    let mut cum_sum_cursor = 0;
    let mut cum_sum = Vec::with_capacity(roll_counts.len());
    for (addr, &n_rolls) in roll_counts.iter() {
        if n_rolls == 0 {
            continue;
        }
        cum_sum_cursor += n_rolls;
        cum_sum.push((cum_sum_cursor, *addr));
    }
    cum_sum
}
