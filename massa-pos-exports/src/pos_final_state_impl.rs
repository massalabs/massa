use std::{collections::BTreeMap, path::PathBuf};

use massa_hash::Hash;
use massa_models::{prehash::Map, Address, Amount, Slot};

use crate::{
    CycleInfo, PoSChanges, PoSFinalState, PosError, PosResult, ProductionStats, SelectorController,
};

impl PoSFinalState {
    /// create a new PoSFinalState
    pub fn new(
        initial_seed_string: &String,
        initial_rolls_path: &PathBuf,
        selector: Box<dyn SelectorController>,
    ) -> Result<Self, PosError> {
        // load get initial rolls from file
        let initial_rolls = serde_json::from_str::<BTreeMap<Address, u64>>(
            &std::fs::read_to_string(initial_rolls_path).map_err(|err| {
                PosError::RollsFileLoadingError(format!("error while deserializing: {}", err))
            })?,
        )
        .map_err(|err| PosError::RollsFileLoadingError(format!("error opening file: {}", err)))?;

        // Seeds used as the initial seeds for negative cycles (-2 and -1 respectively)
        let init_seed = Hash::compute_from(initial_seed_string.as_bytes());
        let initial_seeds = vec![Hash::compute_from(init_seed.to_bytes()), init_seed];

        Ok(Self {
            cycle_history: Default::default(),
            deferred_credits: Default::default(),
            selector,
            initial_rolls,
            initial_seeds,
        })
    }

    /// Create the initial cycle based off the initial rolls.
    ///
    /// This should be called only if bootstrap did not happen.
    pub fn create_initial_cycle(&mut self) {
        self.cycle_history.push_back(CycleInfo {
            cycle: 0,
            // TODO: Feed with genesis block hashes
            rng_seed: Default::default(),
            production_stats: Default::default(),
            roll_counts: self.initial_rolls.clone(),
            complete: false,
        });
    }

    /// Sends the current draw inputs (initial or bootstrapped) to the selector.
    /// Waits for the initial draws to be performed.
    pub fn compute_initial_draws(&mut self) -> PosResult<()> {
        // if cycle_history starts at a cycle that is strictly higher than 0, do not feed cycles 0, 1 to selector
        let history_starts_late = self
            .cycle_history
            .front()
            .map(|c_info| c_info.cycle > 0)
            .unwrap_or(false);

        let mut max_cycle = None;

        // feed cycles 0, 1 to selector if necessary
        if !history_starts_late {
            for draw_cycle in 0u64..=1 {
                self.feed_selector(draw_cycle)?;
                max_cycle = Some(draw_cycle);
            }
        }

        // feed cycles available from history
        for (idx, hist_item) in self.cycle_history.iter().enumerate() {
            if !hist_item.complete {
                break;
            }
            if history_starts_late && idx == 0 {
                // If the history starts late, the first RNG seed cannot be used to draw
                // because the roll distribution which should be provided by the previous element is absent.
                continue;
            }
            let draw_cycle = hist_item.cycle.checked_add(2).ok_or_else(|| {
                PosError::OverflowError("cycle overflow in give_selector_controller".into())
            })?;
            self.feed_selector(draw_cycle)?;
            max_cycle = Some(draw_cycle);
        }

        // wait for all fed cycles to be drawn
        if let Some(wait_cycle) = max_cycle {
            self.selector.as_mut().wait_for_draws(wait_cycle)?;
        }
        Ok(())
    }

    /// Technical specification of apply_changes:
    ///
    /// set self.last_final_slot = C
    /// if cycle C is absent from self.cycle_history:
    ///     push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
    ///     pop_front from cycle_history until front() represents cycle C-4 or later (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
    /// for the cycle C entry of cycle_history:
    ///     extend seed_bits with changes.seed_bits
    ///     extend roll_counts with changes.roll_changes
    ///         delete all entries from roll_counts for which the roll count is zero
    ///     add each element of changes.production_stats to the cycle's production_stats
    /// for each changes.deferred_credits targeting cycle Ct:
    ///     overwrite self.deferred_credits entries of cycle Ct in cycle_history with the ones from change
    ///         remove entries for which Amount = 0
    /// if slot S was the last of cycle C:
    ///     set complete=true for cycle C in the history
    ///     compute the seed hash and notifies the PoSDrawer for cycle C+3
    ///
    pub fn settle_slot(
        &mut self,
        changes: PoSChanges,
        slot: Slot,
        periods_per_cycle: u64,
        thread_count: u8,
    ) -> PosResult<()> {
        // compute the current cycle from the given slot
        let cycle = slot.get_cycle(periods_per_cycle);

        // if cycle C is absent from self.cycle_history:
        // push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
        // pop_front from cycle_history until front() represents cycle C-4 or later
        // (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
        if let Some(info) = self.cycle_history.iter().last() {
            if info.cycle != cycle {
                self.cycle_history.push_back(CycleInfo {
                    cycle,
                    roll_counts: info.roll_counts.clone(),
                    ..Default::default()
                });
                // add 1 for the current cycle and 1 for bootstrap safety
                while self.cycle_history.len() as u64 > 6 {
                    self.cycle_history.pop_front();
                }
            }
        } else {
            self.cycle_history.push_back(CycleInfo {
                cycle,
                roll_counts: self.initial_rolls.clone(),
                ..Default::default()
            });
        }

        // update cycle data
        let cycle_completed: bool;
        {
            let current = self
                .cycle_history
                .back_mut()
                .expect("cycle history should be non-empty"); // because if was filled above

            // extend seed_bits with changes.seed_bits
            current.rng_seed.extend(changes.seed_bits);

            // extend roll counts
            current.roll_counts.extend(changes.roll_changes);
            current.roll_counts.retain(|_, &mut count| count != 0);

            // extend production stats
            for (addr, stats) in changes.production_stats {
                current
                    .production_stats
                    .entry(addr)
                    .and_modify(|cur| cur.extend(&stats))
                    .or_insert(stats);
            }

            // check for completion
            current.complete = slot.is_last_of_cycle(periods_per_cycle, thread_count);
            cycle_completed = current.complete;
        }

        // extent deferred_credits with changes.deferred_credits
        // remove zero-valued credits
        self.deferred_credits
            .nested_extend(changes.deferred_credits);
        self.deferred_credits.remove_zeros();

        // feed the cycle if it is complete
        // notify the PoSDrawer about the newly ready draw data
        // to draw cycle + 2, we use the rng data from cycle - 1 and the seed from cycle
        if cycle_completed {
            self.feed_selector(cycle.checked_add(2).ok_or_else(|| {
                PosError::OverflowError("cycle overflow when feeding selector".into())
            })?)
        } else {
            Ok(())
        }
    }

    /// Feeds the selector targeting a given draw cycle
    fn feed_selector(&self, draw_cycle: u64) -> PosResult<()> {
        // get roll lookback
        let lookback_rolls = match draw_cycle.checked_sub(3) {
            // looking back in history
            Some(c) => {
                let index = self
                    .get_cycle_index(c)
                    .ok_or(PosError::CycleUnavailable(c))?;
                let cycle_info = &self.cycle_history[index];
                if !cycle_info.complete {
                    return Err(PosError::CycleUnfinalised(c));
                }
                cycle_info.roll_counts.clone()
            }
            // looking back to negative cycles
            None => self.initial_rolls.clone(),
        };

        // get seed lookback
        let lookback_seed = match draw_cycle.checked_sub(2) {
            // looking back in history
            Some(c) => {
                let index = self
                    .get_cycle_index(c)
                    .ok_or(PosError::CycleUnavailable(c))?;
                let cycle_info = &self.cycle_history[index];
                if !cycle_info.complete {
                    return Err(PosError::CycleUnfinalised(c));
                }
                Hash::compute_from(&cycle_info.rng_seed.clone().into_vec())
            }
            // looking back to negative cycles
            None => self.initial_seeds[draw_cycle as usize],
        };

        // feed selector
        self.selector
            .as_ref()
            .feed_cycle(draw_cycle, lookback_rolls, lookback_seed)
    }

    /// Retrieves the amount of rolls a given address has at the latest cycle
    pub fn get_rolls_for(&self, addr: &Address) -> u64 {
        self.cycle_history
            .back()
            .and_then(|info| info.roll_counts.get(addr).cloned())
            .unwrap_or_default()
    }

    /// Retrieves the amount of rolls a given address has at a given cycle
    pub fn get_address_active_rolls(&self, addr: &Address, cycle: u64) -> Option<u64> {
        // get lookback cycle index
        let lookback_cycle = cycle.checked_sub(3);
        if let Some(lookback_cycle) = lookback_cycle {
            let lookback_index = match self.get_cycle_index(lookback_cycle) {
                Some(idx) => idx,
                None => return None,
            };
            // get rolls
            self.cycle_history[lookback_index]
                .roll_counts
                .get(addr)
                .cloned()
        } else {
            self.initial_rolls.get(addr).cloned()
        }
    }

    /// Retrives every deferred credit of the given slot
    pub fn get_deferred_credits_at(&self, slot: &Slot) -> Map<Address, Amount> {
        self.deferred_credits
            .0
            .get(slot)
            .cloned()
            .unwrap_or_default()
    }

    /// Retrives the productions statistics for all addresses on a given cycle
    pub fn get_all_production_stats(&self, cycle: u64) -> Option<&Map<Address, ProductionStats>> {
        self.get_cycle_index(cycle)
            .map(|idx| &self.cycle_history[idx].production_stats)
    }

    /// Gets the index of a cycle in history
    pub fn get_cycle_index(&self, cycle: u64) -> Option<usize> {
        let first_cycle = match self.cycle_history.front() {
            Some(c) => c.cycle,
            None => return None, // history empty
        };
        if cycle < first_cycle {
            return None; // in the past
        }
        let index: usize = match (cycle - first_cycle).try_into() {
            Ok(v) => v,
            Err(_) => return None, // usize overflow
        };
        if index >= self.cycle_history.len() {
            return None; // in the future
        }
        Some(index)
    }
}
