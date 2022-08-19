use std::collections::{BTreeMap, HashMap};

use massa_models::Slot;

use crate::{PoSFinalState, Selection};

/// Compare two PoS States
pub fn assert_eq_pos_state(s1: &PoSFinalState, s2: &PoSFinalState) {
    let mut s1_cleared = s1.cycle_history.clone();
    // remove bootstrap safety cycle from s1
    s1_cleared.pop_back();
    assert_eq!(
        s1_cleared, s2.cycle_history,
        "PoS cycle_history mismatching"
    );
    assert_eq!(
        s1.deferred_credits.0, s2.deferred_credits.0,
        "PoS deferred_credits mismatching"
    );
}

/// Compare two PoS Selections
pub fn assert_eq_pos_selection(
    s1: &BTreeMap<u64, HashMap<Slot, Selection>>,
    s2: &BTreeMap<u64, HashMap<Slot, Selection>>,
) {
    assert_eq!(s1.len(), s2.len(), "PoS selections len do not match");
    for (key, value) in s2 {
        if let Some(s1_value) = s1.get(key) {
            for (slot, b) in value {
                let a = s1_value.get(slot).unwrap();
                assert_eq!(a, b, "Selection mismatching for {:?}", slot);
            }
        } else {
            panic!("missing key in first selection");
        }
    }
    assert_eq!(s1, s2, "PoS selections do not match");
}
