use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use machine::{machine, transitions};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::amount::Amount;
use massa_time::MassaTime;

// const
// TODO: to constants.rs
const VERSIONING_THRESHOLD_TRANSITION_ACCEPTED: &str = "75.0";

// TODO: add more items here
/// Versioning component enum
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum VersioningComponent {
    Address,
    Block,
    VM,
}

/// Version info per component
#[derive(Clone, Debug)]
pub struct VersioningInfo {
    /// brief description of the versioning
    pub name: String,
    /// version
    pub version: u32,
    /// Component concerned by this versioning (e.g. a new Block version)
    pub component: VersioningComponent,
    /// a timestamp at which the version gains its meaning (e.g. accepted in block header)
    pub start: MassaTime,
    /// a timestamp at which the deployment is considered failed (timeout > start)
    pub timeout: MassaTime,
}

// Need Ord / PartialOrd so it is properly sorted in BTreeMap

impl Ord for VersioningInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.start, &self.timeout).cmp(&(other.start, &other.timeout))
    }
}

impl PartialOrd for VersioningInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for VersioningInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.component == other.component
            && self.start == other.start
            && self.timeout == other.timeout
    }
}

impl Eq for VersioningInfo {}

// Need to impl this manually otherwise clippy is angry :-P
impl Hash for VersioningInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.version.hash(state);
        self.component.hash(state);
        self.start.hash(state);
        self.timeout.hash(state);
    }
}

machine!(
    /// State machine for a Versioning component that tracks the deployment state
    #[allow(missing_docs)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum VersioningState {
        /// Initial state
        Defined,
        /// Past start
        Started { threshold: Amount },
        /// Wait for some time before going to active (to let user the time to upgrade)
        LockedIn,
        /// After LockedIn, deployment is considered successful
        Active,
        /// Past the timeout, if LockedIn is not reach
        Failed,
    }
);

impl Default for VersioningState {
    fn default() -> Self {
        Self::Defined(Defined {})
    }
}

#[derive(IntoPrimitive, Debug, Clone, Eq, PartialEq, TryFromPrimitive, PartialOrd, Ord)]
#[repr(u32)]
enum VersioningStateTypeId {
    Error = 0,
    Defined = 1,
    Started = 2,
    LockedIn = 3,
    Active = 4,
    Failed = 5,
}

impl From<&VersioningState> for VersioningStateTypeId {
    fn from(value: &VersioningState) -> Self {
        match value {
            VersioningState::Error => VersioningStateTypeId::Error,
            VersioningState::Defined(_) => VersioningStateTypeId::Defined,
            VersioningState::Started(_) => VersioningStateTypeId::Started,
            VersioningState::LockedIn(_) => VersioningStateTypeId::LockedIn,
            VersioningState::Active(_) => VersioningStateTypeId::Active,
            VersioningState::Failed(_) => VersioningStateTypeId::Failed,
        }
    }
}

/// A message to update the `VersioningState`
#[derive(Clone, Debug, PartialEq)]
pub struct Advance {
    /// from VersioningInfo.start
    pub start_timestamp: MassaTime,
    /// from VersioningInfo.timeout
    pub timeout: MassaTime,
    /// % of past blocks with this version
    pub threshold: Amount,
    /// Current time (timestamp)
    pub now: MassaTime,
}

transitions!(VersioningState,
    [
        (Defined, Advance) => [Defined, Started, Failed],
        (Started, Advance) => [Started, LockedIn, Failed],
        (LockedIn, Advance) => [LockedIn, Active],
        (Active, Advance) => Active,
        (Failed, Advance) => Failed
    ]
);

impl Defined {
    /// Update state from state Defined
    pub fn on_advance(self, input: Advance) -> VersioningState {
        match input.now {
            n if n > input.timeout => VersioningState::failed(),
            n if n > input.start_timestamp => VersioningState::started(Amount::zero()),
            _ => VersioningState::Defined(Defined {}),
        }
    }
}

impl Started {
    /// Update state from state Started
    pub fn on_advance(self, input: Advance) -> VersioningState {
        if input.now > input.timeout {
            return VersioningState::failed();
        }

        // Safe to unwrap as we assume the constant is well defined & ok for Amount::from_str
        if input.threshold > Amount::from_str(VERSIONING_THRESHOLD_TRANSITION_ACCEPTED).unwrap() {
            VersioningState::locked_in()
        } else {
            VersioningState::started(input.threshold)
        }
    }
}

impl LockedIn {
    /// Update state from state LockedIn ...
    pub fn on_advance(self, input: Advance) -> VersioningState {
        if input.now > input.timeout {
            VersioningState::active()
        } else {
            VersioningState::locked_in()
        }
    }
}

impl Active {
    /// Update state (will always stay in state Active)
    pub fn on_advance(self, _input: Advance) -> Active {
        Active {}
    }
}

impl Failed {
    /// Update state (will always stay in state Failed)
    pub fn on_advance(self, _input: Advance) -> Failed {
        Failed {}
    }
}

/// Wrapper of VersioningState (in order to keep state history)
#[derive(Debug)]
struct VersioningStateHistory {
    state: VersioningState,
    history: BTreeMap<MassaTime, VersioningStateTypeId>,
}

impl VersioningStateHistory {
    /// Create
    pub fn new(defined: MassaTime) -> Self {
        let state: VersioningState = Default::default();
        let state_id = VersioningStateTypeId::from(&state);
        let mut history: BTreeMap<MassaTime, VersioningStateTypeId> = Default::default();
        history.insert(defined, state_id);
        Self {
            state: Default::default(),
            history,
        }
    }

    /// Advance state
    pub fn on_advance(&mut self, input: Advance) {
        let now = input.now;
        let state = self.state.on_advance(input);
        if state != self.state {
            let state_id = VersioningStateTypeId::from(&state);
            self.history.insert(now, state_id);
            self.state = state;
        }
    }

    pub fn state_at(
        &self,
        ts: MassaTime,
        start: MassaTime,
        timeout: MassaTime,
    ) -> VersioningStateTypeId {
        // Optim: Avoid iterating over history
        let first = self.history.first_key_value();
        let value_first = first.map(|i| i.0);
        if Some(&ts) < value_first {
            // TODO: should we return Result + proper error msg?
            // Before initial state
            return VersioningStateTypeId::Error;
        }

        // TODO: Optim - use value_last instead of iterating
        // At this point, we are >= the first state in history
        let mut lower_bound = None;
        let mut higher_bound = None;
        for (k, v) in self.history.iter() {
            if *k <= ts {
                lower_bound = Some((k, v));
            }
            if *k >= ts && higher_bound.is_none() {
                higher_bound = Some((k, v));
                break;
            }
        }

        match (lower_bound, higher_bound) {
            (Some((_k1, v1)), Some((_k2, _v2))) => {
                // Between 2 states (e.g. between Defined && Started) -> return Defined
                (*v1).clone()
            }
            (Some((_k, _v)), None) => {
                // After the last state in history -> need to advance the state and return
                // FIXME: threshold save in history?
                let msg = Advance {
                    start_timestamp: start,
                    timeout: timeout,
                    threshold: Default::default(),
                    now: ts,
                };
                // Return the resulting state after advance
                // So we keep the state internal update in VersioningState
                VersioningStateTypeId::from(&self.state.on_advance(msg))
            }
            _ => VersioningStateTypeId::Error,
        }
    }
}

impl PartialEq<VersioningState> for VersioningStateHistory {
    fn eq(&self, other: &VersioningState) -> bool {
        self.state == *other
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime};
    use massa_serialization::DeserializeError;

    fn get_a_version_info() -> (NaiveDateTime, NaiveDateTime, VersioningInfo) {
        // A default VersioningInfo used in many tests
        // Models a Massa Improvements Proposal (MIP-0002), transitioning component address to v2

        let start: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 01)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        let timeout: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 11)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        return (
            start,
            timeout,
            VersioningInfo {
                name: "MIP-0002AAA".to_string(),
                version: 2,
                component: VersioningComponent::Address,
                start: MassaTime::from(start.timestamp() as u64),
                timeout: MassaTime::from(timeout.timestamp() as u64),
            },
        );
    }

    #[test]
    fn test_state_advance_from_defined() {
        // Test Versioning state transition (from state: Defined)
        let (_, _, vi) = get_a_version_info();
        let mut state: VersioningState = Default::default();
        assert_eq!(state, VersioningState::defined());

        let now = vi.start;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::defined());

        let now = vi.start.saturating_add(MassaTime::from(5));
        advance_msg.now = now;
        state = state.on_advance(advance_msg);

        // println!("state: {:?}", state);
        assert_eq!(
            state,
            VersioningState::Started(Started {
                threshold: Amount::zero()
            })
        );
    }

    #[test]
    fn test_state_advance_from_started() {
        // Test Versioning state transition (from state: Started)
        let (_, _, vi) = get_a_version_info();
        let mut state: VersioningState = VersioningState::started(Default::default());

        let now = vi.start;
        let threshold_too_low = Amount::from_str("74.9").unwrap();
        let threshold_ok = Amount::from_str("82.42").unwrap();
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: threshold_too_low,
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::started(threshold_too_low));
        advance_msg.threshold = threshold_ok;
        state = state.on_advance(advance_msg);
        assert_eq!(state, VersioningState::locked_in());
    }

    #[test]
    fn test_state_advance_from_locked_in() {
        // Test Versioning state transition (from state: LockedIn)
        let (_, _, vi) = get_a_version_info();
        let mut state: VersioningState = VersioningState::locked_in();

        let now = vi.start;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::locked_in());

        advance_msg.now = advance_msg.timeout.saturating_add(MassaTime::from(1));
        state = state.on_advance(advance_msg);
        assert_eq!(state, VersioningState::active());
    }

    #[test]
    fn test_state_advance_from_active() {
        // Test Versioning state transition (from state: Active)
        let (_, _, vi) = get_a_version_info();
        let mut state = VersioningState::active();
        let now = vi.start;
        let advance = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, VersioningState::active());
    }

    #[test]
    fn test_state_advance_from_failed() {
        // Test Versioning state transition (from state: Failed)
        let (_, _, vi) = get_a_version_info();
        let mut state = VersioningState::failed();
        let now = vi.start;
        let advance = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, VersioningState::failed());
    }

    #[test]
    fn test_state_advance_to_failed() {
        // Test Versioning state transition (to state: Failed)
        let (_, _, vi) = get_a_version_info();
        let now = vi.start.saturating_add(MassaTime::from(1));
        let advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.start,
            threshold: Amount::zero(),
            now,
        };

        let mut state: VersioningState = Default::default();
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Failed(Failed {}));

        let mut state: VersioningState = VersioningState::started(Default::default());
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Failed(Failed {}));
    }

    #[test]
    fn test_state_with_history() {
        let (start, _, vi) = get_a_version_info();
        let now = MassaTime::from(start.timestamp() as u64);
        let mut state = VersioningStateHistory::new(now);

        assert_eq!(state, VersioningState::defined());

        let now = vi.start.saturating_add(MassaTime::from(15));
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        // Move from Defined -> Started
        state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::started(Amount::zero()));

        // Check history
        assert_eq!(state.history.len(), 2);
        assert_eq!(
            state.history.last_key_value(),
            Some((&now, &VersioningStateTypeId::Started))
        );

        // Query with timestamp

        // Before Defined
        let state_id = state.state_at(
            vi.start.saturating_sub(MassaTime::from(5)),
            vi.start,
            vi.timeout,
        );
        assert_eq!(state_id, VersioningStateTypeId::Error);
        // After Defined timestamp
        let state_id = state.state_at(vi.start, vi.start, vi.timeout);
        assert_eq!(state_id, VersioningStateTypeId::Defined);
        // At Started timestamp
        let state_id = state.state_at(now, vi.start, vi.timeout);
        assert_eq!(state_id, VersioningStateTypeId::Started);
        // After Started timestamp but before timeout timestamp
        let after_started_ts = now.saturating_add(MassaTime::from(15));
        let state_id = state.state_at(after_started_ts, vi.start, vi.timeout);
        assert_eq!(state_id, VersioningStateTypeId::Started);
        // After Started timestamp and after timeout timestamp
        let after_timeout_ts = vi.timeout.saturating_add(MassaTime::from(15));
        let state_id = state.state_at(after_timeout_ts, vi.start, vi.timeout);
        assert_eq!(state_id, VersioningStateTypeId::Failed);

        // Move from Started to LockedIn
        let threshold = Amount::from_str(VERSIONING_THRESHOLD_TRANSITION_ACCEPTED).unwrap();
        advance_msg.threshold = threshold.saturating_add(Amount::from_str("1.0").unwrap());
        state.on_advance(advance_msg);
        assert_eq!(state, VersioningState::locked_in());

        // Query with timestamp
        // After LockedIn timestamp and before timeout timestamp
        let after_locked_in_ts = now.saturating_add(MassaTime::from(10));
        let state_id = state.state_at(after_locked_in_ts, vi.start, vi.timeout);
        assert_eq!(state_id, VersioningStateTypeId::LockedIn);
        // After LockedIn timestamp and after timeout timestamp
        let state_id = state.state_at(after_timeout_ts, vi.start, vi.timeout);
        assert_eq!(state_id, VersioningStateTypeId::Active);
    }
}
