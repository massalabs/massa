use crate::PoSFinalState;

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
        s1.deferred_credits, s2.deferred_credits,
        "PoS deferred_credits mismatching"
    );
}
