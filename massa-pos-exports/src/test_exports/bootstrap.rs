use std::collections::{HashMap, VecDeque};

use massa_models::{
    address::Address,
    config::{CHANNEL_SIZE, ENDORSEMENT_COUNT, PERIODS_PER_CYCLE, THREAD_COUNT},
    slot::Slot,
};
use massa_signature::KeyPair;

use crate::{PoSFinalState, Selection, SelectorConfig};

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            thread_count: THREAD_COUNT,
            endorsement_count: ENDORSEMENT_COUNT,
            max_draw_cache: 10,
            periods_per_cycle: PERIODS_PER_CYCLE,
            genesis_address: Address::from_public_key(
                &KeyPair::generate(0).unwrap().get_public_key(),
            ),
            channel_size: CHANNEL_SIZE,
            last_start_period: 0
        }
    }
}

/// Compare two PoS States
pub fn assert_eq_pos_state(s1: &PoSFinalState, s2: &PoSFinalState) {
    assert_eq!(
        s1.cycle_history_cache.len(),
        s2.cycle_history_cache.len(),
        "PoS cycle_history_cache len mismatching"
    );
    assert_eq!(
        s1.cycle_history_cache, s2.cycle_history_cache,
        "PoS cycle_history_cache mismatching"
    );
    for cycle in s1.cycle_history_cache.clone() {
        assert_eq!(
            s1.get_cycle_info(cycle.0).expect("cycle missing"),
            s2.get_cycle_info(cycle.0).expect("cycle missing"),
            "PoS cycle_history mismatching"
        );
    }
    let deferred_credits_s1 = s1.get_deferred_credits();
    let deferred_credits_s2 = s2.get_deferred_credits();
    assert_eq!(
        deferred_credits_s1.credits.len(),
        deferred_credits_s2.credits.len(),
        "PoS deferred_credits len mismatching"
    );
    assert_eq!(
        deferred_credits_s1.credits, deferred_credits_s2.credits,
        "PoS deferred_credits mismatching"
    );
    assert_eq!(
        s1.initial_rolls, s2.initial_rolls,
        "PoS initial_rolls mismatching"
    );
    assert_eq!(
        s1.initial_seeds, s2.initial_seeds,
        "PoS initial_seeds mismatching"
    );
}

/// Compare two PoS Selections
pub fn assert_eq_pos_selection(
    s1: &VecDeque<(u64, HashMap<Slot, Selection>)>,
    s2: &VecDeque<(u64, HashMap<Slot, Selection>)>,
) {
    assert_eq!(s1.len(), s2.len(), "PoS selections len do not match");
    assert_eq!(s1, s2, "PoS selections do not match");
}
