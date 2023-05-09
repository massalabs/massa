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
            genesis_address: Address::from_public_key(&KeyPair::generate().get_public_key()),
            channel_size: CHANNEL_SIZE,
        }
    }
}

/// Compare two PoS States
pub(crate)  fn assert_eq_pos_state(s1: &PoSFinalState, s2: &PoSFinalState) {
    assert_eq!(
        s1.cycle_history.len(),
        s2.cycle_history.len(),
        "PoS cycle_history len mismatching"
    );
    assert_eq!(
        s1.cycle_history, s2.cycle_history,
        "PoS cycle_history mismatching"
    );
    assert_eq!(
        s1.deferred_credits.credits.len(),
        s2.deferred_credits.credits.len(),
        "PoS deferred_credits len mismatching"
    );
    assert_eq!(
        s1.deferred_credits.credits, s2.deferred_credits.credits,
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
pub(crate)  fn assert_eq_pos_selection(
    s1: &VecDeque<(u64, HashMap<Slot, Selection>)>,
    s2: &VecDeque<(u64, HashMap<Slot, Selection>)>,
) {
    assert_eq!(s1.len(), s2.len(), "PoS selections len do not match");
    assert_eq!(s1, s2, "PoS selections do not match");
}
