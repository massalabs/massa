use crate::versioning::{Advance, MipInfo, MipState, MipStateHistory};

use massa_models::config::VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
use massa_time::MassaTime;

pub fn advance_state_until(at_state: MipState, versioning_info: &MipInfo) -> MipStateHistory {
    // A helper function to advance a state
    // Assume enough time between versioning info start & timeout
    // TODO: allow to give a threshold as arg?

    let start = versioning_info.start;
    let timeout = versioning_info.timeout;

    if matches!(at_state, MipState::Error) {
        todo!()
    }

    let mut state = MipStateHistory::new(start.saturating_sub(MassaTime::from(1)));

    if matches!(at_state, MipState::Defined(_)) {
        return state;
    }

    let mut advance_msg = Advance {
        start_timestamp: start,
        timeout,
        threshold: Default::default(),
        now: start.saturating_add(MassaTime::from(1)),
    };
    state.on_advance(&advance_msg);

    if matches!(at_state, MipState::Started(_)) {
        return state;
    }

    if matches!(at_state, MipState::Failed(_)) {
        advance_msg.now = timeout.saturating_add(MassaTime::from(1));
        state.on_advance(&advance_msg);
        return state;
    }

    advance_msg.now = start.saturating_add(MassaTime::from(2));
    advance_msg.threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
    state.on_advance(&advance_msg);

    if matches!(at_state, MipState::LockedIn(_)) {
        return state;
    }

    advance_msg.now = timeout.saturating_add(MassaTime::from(1));
    state.on_advance(&advance_msg);
    // Active
    return state;
}
