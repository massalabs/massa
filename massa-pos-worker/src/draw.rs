use crate::{worker::SelectorThread, CycleDraws};
use massa_hash::Hash;
use massa_models::{prehash::Map, Address, Slot};
use massa_pos_exports::{PosResult, Selection};
use rand::{
    distributions::{Distribution, WeightedIndex},
    SeedableRng,
};
use rand_xoshiro::Xoshiro256PlusPlus;
use std::collections::HashMap;

impl SelectorThread {
    /// Draws block and creators for a given cycle.
    ///
    /// Then prune the `cache` pointer if max cache is exceeded.
    ///
    /// # Parameters
    /// * cycle: Cycle to draw
    /// * lookback_rolls: Roll counts at lookback (`cycle-3`)
    /// * lookback_seed: RNG seed at lookback (`cycle-2`)
    ///
    /// # Result
    /// - The draws can throw the errors of the function [get_params] and from the
    ///   creation of a [Xoshiro256PlusPlus].
    /// - An inconsistency error is thrown if nobody has rolls
    ///
    /// Otherwise, the draws return an empty success.
    pub(crate) fn perform_draws(
        &mut self,
        cycle: u64,
        lookback_rolls: Map<Address, u64>,
        lookback_seed: Hash,
    ) -> PosResult<()> {
        // get seeded RNG
        let mut rng = Xoshiro256PlusPlus::from_seed(*lookback_seed.to_bytes());

        // sort lookback rolls into a vec
        let mut lookback_rolls: Vec<(Address, u64)> = lookback_rolls.into_iter().collect();
        lookback_rolls.sort_unstable_by_key(|(addr, _)| *addr);
        let (addresses, roll_counts): (Vec<_>, Vec<_>) = lookback_rolls.into_iter().unzip();

        // prepare distribution
        let dist =
            WeightedIndex::new(&roll_counts).expect("nobody has rolls, this is a critical error");

        // perform cycle draws
        let mut cur_slot = Slot::new_first_of_cycle(cycle, self.cfg.periods_per_cycle)
            .expect("unexpected start slot overflow in perform_draws");
        let last_slot =
            Slot::new_last_of_cycle(cycle, self.cfg.periods_per_cycle, self.cfg.thread_count)
                .expect("unexpected end slot overflow in perform_draws");
        let mut cycle_draws = CycleDraws {
            cycle,
            draws: HashMap::with_capacity(
                (self.cfg.periods_per_cycle as usize) * (self.cfg.thread_count as usize),
            ),
        };

        loop {
            // draw block creator
            let producer = if cur_slot.period > 0 {
                addresses[dist.sample(&mut rng)]
            } else {
                // force draws for genesis blocks
                self.cfg.genesis_address
            };

            // draw endorsement creators
            let endorsements: Vec<_> = (0..self.cfg.endorsement_count)
                .map(|_index| addresses[dist.sample(&mut rng)])
                .collect();

            // add to draws
            cycle_draws.draws.insert(
                cur_slot,
                Selection {
                    producer,
                    endorsements,
                },
            );

            if cur_slot == last_slot {
                break;
            }
            cur_slot = cur_slot
                .get_next_slot(self.cfg.thread_count)
                .expect("unexpected slot overflow in perform_draws");
        }

        {
            // write-lock the cache as shortly as possible
            let mut cache = self.cache.write();

            // add draws to cache
            if let Some(last_cycle) = cache.0.back() {
                if last_cycle.cycle.checked_add(1) != Some(cycle) {
                    panic!("discontinuity in selector cycles");
                }
            }
            cache.0.push_back(cycle_draws);

            // truncate cache to keep only the desired number of elements
            while cache.0.len() > self.cfg.max_draw_cache {
                cache.0.pop_front();
            }
        }

        Ok(())
    }
}
