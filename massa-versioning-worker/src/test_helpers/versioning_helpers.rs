use crate::versioning::{Advance, ComponentState, MipInfo, MipState};

use massa_models::config::constants::VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
use massa_time::MassaTime;

// TODO: rename versioning_info
pub(crate) fn advance_state_until(at_state: ComponentState, versioning_info: &MipInfo) -> MipState {
    // A helper function to advance a state
    // Assume enough time between versioning info start & timeout
    // TODO: allow to give a threshold as arg?

    let start = versioning_info.start;
    let timeout = versioning_info.timeout;

    if matches!(at_state, ComponentState::Error) {
        todo!()
    }

    let mut state = MipState::test_exp_new(start.saturating_sub(MassaTime::from(1)));

    if matches!(at_state, ComponentState::Defined(_)) {
        return state;
    }

    let mut advance_msg = Advance {
        start_timestamp: start,
        timeout,
        threshold: Default::default(),
        now: start.saturating_add(MassaTime::from(1)),
        activation_delay: versioning_info.activation_delay,
    };
    state.on_advance(&advance_msg);

    if matches!(at_state, ComponentState::Started(_)) {
        return state;
    }

    if matches!(at_state, ComponentState::Failed(_)) {
        advance_msg.now = timeout.saturating_add(MassaTime::from(1));
        state.on_advance(&advance_msg);
        return state;
    }

    advance_msg.now = start.saturating_add(MassaTime::from(2));
    advance_msg.threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
    state.on_advance(&advance_msg);

    if matches!(at_state, ComponentState::LockedIn(_)) {
        return state;
    }

    advance_msg.now = advance_msg
        .now
        .saturating_add(versioning_info.activation_delay)
        .saturating_add(MassaTime::from(1));

    state.on_advance(&advance_msg);
    // Active
    state
}
