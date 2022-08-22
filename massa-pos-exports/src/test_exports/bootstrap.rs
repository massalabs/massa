use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
};

use massa_models::{
    constants::{ENDORSEMENT_COUNT, PERIODS_PER_CYCLE},
    Address, Slot, THREAD_COUNT,
};
use massa_signature::KeyPair;

use crate::{PoSFinalState, Selection, SelectorConfig};

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            thread_count: THREAD_COUNT,
            endorsement_count: ENDORSEMENT_COUNT,
            max_draw_cache: 00,
            periods_per_cycle: PERIODS_PER_CYCLE,
            genesis_address: Address::from_public_key(&KeyPair::generate().get_public_key()),
            initial_rolls_path: PathBuf::default(),
            initial_draw_seed: String::default(),
        }
    }
}

/// Compare two PoS States
pub fn assert_eq_pos_state(s1: &PoSFinalState, s2: &PoSFinalState) {
    assert_eq!(
        s1.cycle_history, s2.cycle_history,
        "PoS cycle_history mismatching"
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
