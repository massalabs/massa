use std::collections::{HashMap, VecDeque};

use massa_models::{
    config::default_testing::{CHANNEL_SIZE, ENDORSEMENT_COUNT, PERIODS_PER_CYCLE},
    Address, Slot, THREAD_COUNT,
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
pub fn assert_eq_pos_state(s1: &PoSFinalState, s2: &PoSFinalState) {
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
        s1.deferred_credits.0.len(),
        s2.deferred_credits.0.len(),
        "PoS deferred_credits len mismatching"
    );
    assert_eq!(
        s1.deferred_credits.0, s2.deferred_credits.0,
        "PoS deferred_credits mismatching"
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
