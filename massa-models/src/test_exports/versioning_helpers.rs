use std::str::FromStr;

use crate::amount::Amount;
use crate::versioning::{Advance, VersioningInfo, VersioningState, VersioningStateHistory};

use crate::config::VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
use massa_time::MassaTime;

pub fn advance_state_until(
    at_state: VersioningState,
    versioning_info: &VersioningInfo,
) -> VersioningStateHistory {
    // A helper function to advance a state
    // Assume enough time between versioning info start & timeout
    // TODO: allow to give a threshold as arg?

    let start = versioning_info.start;
    let timeout = versioning_info.timeout;

    if matches!(at_state, VersioningState::Error) {
        todo!()
    }

    let mut state = VersioningStateHistory::new(start.saturating_sub(MassaTime::from(1)));

    if matches!(at_state, VersioningState::Defined(_)) {
        return state;
    }

    let mut advance_msg = Advance {
        start_timestamp: start,
        timeout,
        threshold: Default::default(),
        now: start.saturating_add(MassaTime::from(1)),
    };
    state.on_advance(&advance_msg);

    if matches!(at_state, VersioningState::Started(_)) {
        return state;
    }

    if matches!(at_state, VersioningState::Failed(_)) {
        advance_msg.now = timeout.saturating_add(MassaTime::from(1));
        state.on_advance(&advance_msg);
        return state;
    }

    advance_msg.now = start.saturating_add(MassaTime::from(2));
    advance_msg.threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
    state.on_advance(&advance_msg);

    if matches!(at_state, VersioningState::LockedIn(_)) {
        return state;
    }

    advance_msg.now = timeout.saturating_add(MassaTime::from(1));
    state.on_advance(&advance_msg);
    // Active
    return state;
}
