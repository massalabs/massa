use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use machine::{machine, transitions};
use parking_lot::RwLock;

// TODO: add more items here
/// Versioning component enum
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VersioningComponent {
    Address,
    Block,
    VM,
}

/// Version info per component
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VersioningInfo {
    /// brief description of the versioning
    pub(crate) name: String,
    /// version
    pub(crate) version: u32,
    /// Component concerned by this versioning (e.g. a new Block version)
    pub(crate) component: VersioningComponent,
    /// a timestamp at which the version gains its meaning (e.g. accepted in block header)
    pub(crate) start: Duration,
    /// a timestamp at which the deployment is considered failed (timeout > start)
    pub(crate) timeout: Duration,
}

machine!(
    /// State machine for a Versioning component that tracks the deployment state
    #[derive(Clone, Debug, PartialEq)]
    enum VersioningState {
        /// Initial state
        Defined,
        /// Past start
        Started { threshold: f32 },
        /// TODO
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

const THRESHOLD_TRANSITION_ACCEPTED: f32 = 75.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Advance {
    /// from VersioningInfo.start
    start_timestamp: Duration,
    /// from VersioningInfo.timeout
    timeout: Duration,
    /// % of past blocks with this version
    threshold: f32,
    /// Current time (timestamp)
    now: Duration,
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
    pub fn new() -> Self {
        Self {}
    }

    pub fn on_advance(self, input: Advance) -> VersioningState {
        match input.now {
            n if n > input.timeout => VersioningState::Failed(Failed {}),
            n if n > input.start_timestamp => VersioningState::Started(Started { threshold: 0.0 }),
            _ => VersioningState::Defined(Defined {}),
        }
    }
}

impl Started {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    pub fn on_advance(self, input: Advance) -> VersioningState {
        if input.now > input.timeout {
            return VersioningState::Failed(Failed {});
        }

        if input.threshold > THRESHOLD_TRANSITION_ACCEPTED {
            VersioningState::LockedIn(LockedIn {})
        } else {
            VersioningState::Started(Started {
                threshold: input.threshold,
            })
        }
    }
}

impl Default for Started {
    fn default() -> Self {
        return Self { threshold: 0.0 };
    }
}

impl LockedIn {
    pub fn new() -> Self {
        Self {}
    }

    pub fn on_advance(self, input: Advance) -> VersioningState {
        if input.now > input.timeout {
            VersioningState::Active(Active {})
        } else {
            VersioningState::LockedIn(LockedIn {})
        }
    }
}

impl Active {
    pub fn new() -> Self {
        Self {}
    }
    pub fn on_advance(self, _input: Advance) -> Active {
        Active {}
    }
}

impl Failed {
    pub fn on_advance(self, _input: Advance) -> Failed {
        Failed {}
    }
}

// Let's define it if needed

#[derive(Debug, Clone)]
pub struct VersioningStore(pub Arc<RwLock<VersioningStoreRaw>>);

/// Store of all versioning info
#[derive(Debug, Clone)]
pub struct VersioningStoreRaw {
    pub versioning_info: HashMap<VersioningInfo, VersioningState>,
}

#[cfg(test)]
mod test {
    use super::*;

    fn get_default_version_info() -> VersioningInfo {
        // A default VersioningInfo used in many tests
        // Models a Massa Improvments Proposal (MIP-0002), transitioning component address to v2
        return VersioningInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: Default::default(),
            timeout: Duration::from_secs(9999),
        };
    }

    #[test]
    fn test_state_advance_from_defined() {
        // Test Versioning state transition (from state: Defined)
        let vi = get_default_version_info();
        let mut state: VersioningState = Default::default();
        assert_eq!(state, VersioningState::Defined(Defined::new()));

        let now = vi.start;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: 0.0,
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Defined(Defined::new()));

        let now = vi.start + Duration::from_secs(5);
        advance_msg.now = now;
        state = state.on_advance(advance_msg);

        // println!("state: {:?}", state);
        assert_eq!(state, VersioningState::Started(Started { threshold: 0.0 }));
    }

    #[test]
    fn test_state_advance_from_started() {
        // Test Versioning state transition (from state: Started)
        let vi = get_default_version_info();
        let mut state: VersioningState = VersioningState::Started(Default::default());

        let now = vi.start;
        let threshold_too_low = 74.9;
        let threshold_ok = 82.42;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: threshold_too_low,
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(
            state,
            VersioningState::Started(Started::new(threshold_too_low))
        );
        advance_msg.threshold = threshold_ok;
        state = state.on_advance(advance_msg);
        assert_eq!(state, VersioningState::LockedIn(LockedIn::new()));
    }

    #[test]
    fn test_state_advance_from_lockedin() {
        // Test Versioning state transition (from state: LockedIn)
        let vi = get_default_version_info();
        let mut state: VersioningState = VersioningState::LockedIn(LockedIn::new());

        let now = vi.start;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: 0.0,
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::LockedIn(LockedIn::new()));

        advance_msg.now = advance_msg.timeout + Duration::from_secs(1);
        state = state.on_advance(advance_msg);
        assert_eq!(state, VersioningState::Active(Active::new()));
    }

    #[test]
    fn test_state_advance_from_active() {
        // Test Versioning state transition (from state: Active)
        let vi = get_default_version_info();
        let mut state = VersioningState::Active(Active {});
        let now = vi.start;
        let advance = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: 0.0,
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, VersioningState::Active(Active {}));
    }

    #[test]
    fn test_state_advance_from_failed() {
        // Test Versioning state transition (from state: Failed)
        let vi = get_default_version_info();
        let mut state = VersioningState::Failed(Failed {});
        let now = vi.start;
        let advance = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: 0.0,
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, VersioningState::Failed(Failed {}));
    }

    #[test]
    fn test_state_advance_to_failed() {
        // Test Versioning state transition (to state: Failed)
        let vi = get_default_version_info();
        let now = vi.start + Duration::from_secs(1);
        let advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.start,
            threshold: 0.0,
            now,
        };

        let mut state: VersioningState = Default::default();
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Failed(Failed {}));

        let mut state: VersioningState = VersioningState::Started(Default::default());
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Failed(Failed {}));
    }
}
