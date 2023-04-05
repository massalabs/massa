use crate::CycleDraws;
use massa_hash::Hash;
use massa_models::{address::Address, slot::Slot};
use massa_pos_exports::{PosError, PosResult, Selection, SelectorConfig};
use rand::{distributions::Distribution, SeedableRng};
use rand_distr::WeightedAliasIndex;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::collections::{BTreeMap, HashMap};
use tracing::info;

/// Draws block and creators for a given cycle.
///
/// Then prune the `cache` pointer if max cache is exceeded.
///
/// # Parameters
/// * `cycle`: Cycle to draw
/// * `lookback_rolls`: Roll counts at look back (`cycle-3`)
/// * `lookback_seed`: RNG seed at look back (`cycle-2`)
///
/// # Result
/// - The draws can throw the errors of the function `get_params` and from the
///   creation of a `Xoshiro256PlusPlus`.
/// - An inconsistency error is thrown if nobody has rolls
///
/// Otherwise, the draws return an empty success.
pub(crate) fn perform_draws(
    cfg: &SelectorConfig,
    cycle: u64,
    lookback_rolls: BTreeMap<Address, u64>,
    lookback_seed: Hash,
) -> PosResult<CycleDraws> {
    // get seeded RNG
    let mut rng = Xoshiro256PlusPlus::from_seed(*lookback_seed.to_bytes());

    let (addresses, roll_counts): (Vec<_>, Vec<_>) = lookback_rolls.into_iter().unzip();

    // prepare distribution
    let dist = WeightedAliasIndex::new(roll_counts).map_err(|err| {
        PosError::InvalidRollDistribution(format!(
            "could not initialize weighted roll distribution: {}",
            err
        ))
    })?;

    // perform cycle draws
    let mut cur_slot = Slot::new_first_of_cycle(cycle, cfg.periods_per_cycle).map_err(|err| {
        PosError::OverflowError(format!("start slot overflow in perform_draws: {}", err))
    })?;
    let last_slot = Slot::new_last_of_cycle(cycle, cfg.periods_per_cycle, cfg.thread_count)
        .map_err(|err| {
            PosError::OverflowError(format!("end slot overflow in perform_draws: {}", err))
        })?;
    let mut cycle_draws = CycleDraws {
        cycle,
        draws: HashMap::with_capacity(
            (cfg.periods_per_cycle as usize) * (cfg.thread_count as usize),
        ),
    };

    let mut five_first_slots: Vec<(Slot, Selection)> = Vec::new();
    let mut count = 0;
    loop {
        // draw block creator
        let producer = if cur_slot.period > 0 {
            addresses[dist.sample(&mut rng)]
        } else {
            // force draws for genesis blocks
            cfg.genesis_address
        };

        // draw endorsement creators
        let endorsements: Vec<_> = (0..cfg.endorsement_count)
            .map(|_index| addresses[dist.sample(&mut rng)])
            .collect();

        let selection = Selection {
            producer,
            endorsements,
        };
        if count < 5 {
            five_first_slots.push((cur_slot, selection.clone()));
            count += 1;
        }
        // add to draws
        cycle_draws.draws.insert(cur_slot, selection);

        if cur_slot == last_slot {
            break;
        }
        cur_slot = cur_slot.get_next_slot(cfg.thread_count).map_err(|err| {
            PosError::OverflowError(format!("iteration slot overflow in perform_draws: {}", err))
        })?;
    }

    info!(
        "Draws for cycle {} complete. Look_back seed was {:#?}. Five first selections is : {:#?}",
        cycle,
        lookback_seed.to_bytes(),
        five_first_slots
    );

    Ok(cycle_draws)
}
