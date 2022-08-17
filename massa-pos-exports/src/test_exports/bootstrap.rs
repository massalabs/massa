use crate::PoSFinalState;

/// Compare two PoS States
pub fn assert_eq_pos_state(s1: &PoSFinalState, s2: &PoSFinalState) {
    // println!("HERE A: {:#?} \n\n HERE B: {:#?}", s1.cycle_history, s2.cycle_history);
    assert_eq!(s1.cycle_history.len(), s2.cycle_history.len(), "PoS cycle_history mismatching len");
    assert_eq!(s1.deferred_credits.len(), s2.deferred_credits.len(), "PoS deferred_credits mismatching len");
}