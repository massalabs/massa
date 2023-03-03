use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use machine::{machine, transitions};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use parking_lot::RwLock;
use thiserror::Error;

use massa_models::{amount::Amount, config::VERSIONING_THRESHOLD_TRANSITION_ACCEPTED};
use massa_time::MassaTime;

// TODO: add more items here
/// Versioning component enum
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum MipComponent {
    Address,
    Block,
    VM,
}

/// MIP info (name,  version,
#[derive(Clone, Debug)]
pub struct MipInfo {
    /// MIP name or descriptive name
    pub name: String,
    /// Network (or global) version
    pub version: u32,
    /// Component concerned by this versioning (e.g. a new Block version)
    pub component: MipComponent,
    /// Component version
    pub component_version: u32,
    /// a timestamp at which the version gains its meaning (e.g. accepted in block header)
    pub start: MassaTime,
    /// a timestamp at which the deployment is considered failed (timeout > start)
    pub timeout: MassaTime,
}

// Need Ord / PartialOrd so it is properly sorted in BTreeMap

impl Ord for MipInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.start, &self.timeout).cmp(&(other.start, &other.timeout))
    }
}

impl PartialOrd for MipInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MipInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.component == other.component
            && self.start == other.start
            && self.timeout == other.timeout
    }
}

impl Eq for MipInfo {}

// Need to impl this manually otherwise clippy is angry :-P
impl Hash for MipInfo {
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
    enum MipState {
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

impl Default for MipState {
    fn default() -> Self {
        Self::Defined(Defined {})
    }
}

#[allow(missing_docs)]
#[derive(IntoPrimitive, Debug, Clone, Eq, PartialEq, TryFromPrimitive, PartialOrd, Ord)]
#[repr(u32)]
pub enum MipStateTypeId {
    Error = 0,
    Defined = 1,
    Started = 2,
    LockedIn = 3,
    Active = 4,
    Failed = 5,
}

impl From<&MipState> for MipStateTypeId {
    fn from(value: &MipState) -> Self {
        match value {
            MipState::Error => MipStateTypeId::Error,
            MipState::Defined(_) => MipStateTypeId::Defined,
            MipState::Started(_) => MipStateTypeId::Started,
            MipState::LockedIn(_) => MipStateTypeId::LockedIn,
            MipState::Active(_) => MipStateTypeId::Active,
            MipState::Failed(_) => MipStateTypeId::Failed,
        }
    }
}

/// A message to update the `MipState`
#[derive(Clone, Debug)]
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

impl Default for Advance {
    fn default() -> Self {
        Self {
            start_timestamp: MassaTime::from(0),
            timeout: MassaTime::from(0),
            threshold: Default::default(),
            now: MassaTime::from(0),
        }
    }
}

// Need Ord / PartialOrd so it is properly sorted in BTreeMap

impl Ord for Advance {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.now).cmp(&other.now)
    }
}

impl PartialOrd for Advance {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Advance {
    fn eq(&self, other: &Self) -> bool {
        self.start_timestamp == other.start_timestamp
            && self.timeout == other.timeout
            && self.threshold == other.threshold
            && self.now == other.now
    }
}

impl Eq for Advance {}

transitions!(MipState,
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
    pub fn on_advance(self, input: Advance) -> MipState {
        match input.now {
            n if n > input.timeout => MipState::failed(),
            n if n > input.start_timestamp => MipState::started(Amount::zero()),
            _ => MipState::Defined(Defined {}),
        }
    }
}

impl Started {
    /// Update state from state Started
    pub fn on_advance(self, input: Advance) -> MipState {
        if input.now > input.timeout {
            return MipState::failed();
        }

        if input.threshold >= VERSIONING_THRESHOLD_TRANSITION_ACCEPTED {
            MipState::locked_in()
        } else {
            MipState::started(input.threshold)
        }
    }
}

impl LockedIn {
    /// Update state from state LockedIn ...
    pub fn on_advance(self, input: Advance) -> MipState {
        if input.now > input.timeout {
            MipState::active()
        } else {
            MipState::locked_in()
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

/// Wrapper of MipState (in order to keep state history)
#[derive(Debug, Clone, PartialEq)]
pub struct MipStateHistory {
    pub(crate) state: MipState,
    history: BTreeMap<Advance, MipStateTypeId>,
}

impl MipStateHistory {
    /// Create
    pub fn new(defined: MassaTime) -> Self {
        let state: MipState = Default::default();
        let state_id = MipStateTypeId::from(&state);
        // let mut advance = Advance::default();
        // advance.now = defined;
        let advance = Advance {
            now: defined,
            ..Default::default()
        };
        let history = BTreeMap::from([(advance, state_id)]);
        Self {
            state: Default::default(),
            history,
        }
    }

    /// Advance state
    pub fn on_advance(&mut self, input: &Advance) {
        let now = input.now;
        // Check that input.now is not after last item in history
        // We don't want to go backward
        let is_forward = self
            .history
            .last_key_value()
            .map(|(adv, _)| adv.now < now)
            .unwrap_or(false);

        if is_forward {
            // machines crate (for state machine) does not support passing ref :-/
            let state = self.state.on_advance(input.clone());
            // Update history as well
            if state != self.state {
                let state_id = MipStateTypeId::from(&state);
                self.history.insert(input.clone(), state_id);
                self.state = state;
            }
        }
    }

    /// Given a corresponding VersioningInfo, check if state is coherent
    /// it is coherent
    ///   if state can be at this position (e.g. can it be at state "Started" according to given time range)
    ///   if history is coherent with current state
    /// Return false for state == MipState::Error
    pub fn is_coherent_with(&self, versioning_info: &MipInfo) -> bool {
        // Always return false for state Error or if history is empty
        if matches!(&self.state, &MipState::Error) || self.history.is_empty() {
            return false;
        }

        // safe to unwrap (already tested if empty or not)
        let (initial_ts, initial_state_id) = self.history.first_key_value().unwrap();
        if *initial_state_id != MipStateTypeId::Defined {
            // self.history does not start with Defined -> (always) false
            return false;
        }

        // Build a new VersionStateHistory from initial state, replaying the whole history
        // but with given versioning info then compare
        let mut vsh = MipStateHistory::new(initial_ts.now);
        let mut advance_msg = Advance {
            start_timestamp: versioning_info.start,
            timeout: versioning_info.timeout,
            threshold: Amount::zero(),
            now: initial_ts.now,
        };

        for (adv, _state) in self.history.iter().skip(1) {
            advance_msg.now = adv.now;
            advance_msg.threshold = adv.threshold;
            vsh.on_advance(&advance_msg);
        }

        // XXX: is there always full eq? Can we have slight variation here?
        vsh == *self
    }

    /// Query state at given timestamp
    pub fn state_at(
        &self,
        ts: MassaTime,
        start: MassaTime,
        timeout: MassaTime,
    ) -> Result<MipStateTypeId, StateAtError> {
        if self.history.is_empty() {
            return Err(StateAtError::EmptyHistory);
        }

        // Optim: this avoids iterating over history (cheap to retrieve first item)
        let first = self.history.first_key_value().unwrap(); // safe to unwrap
        if ts < first.0.now {
            // Before initial state
            return Err(StateAtError::BeforeInitialState(first.1.clone(), ts));
        }

        // At this point, we are >= the first state in history
        let mut lower_bound = None;
        let mut higher_bound = None;
        let mut is_after_last = false;

        // Optim: this avoids iterating over history (cheap to retrieve first item)
        let last = self.history.last_key_value().unwrap(); // safe to unwrap
        if ts > last.0.now {
            lower_bound = Some(last);
            is_after_last = true;
        }

        if !is_after_last {
            // We are in between two states in history, find bounds
            for (adv, state_id) in self.history.iter() {
                if adv.now <= ts {
                    lower_bound = Some((adv, state_id));
                }
                if adv.now >= ts && higher_bound.is_none() {
                    higher_bound = Some((adv, state_id));
                    break;
                }
            }
        }

        match (lower_bound, higher_bound) {
            (Some((_adv_1, st_id_1)), Some((_adv_2, _st_id_2))) => {
                // Between 2 states (e.g. between Defined && Started) -> return Defined
                Ok(st_id_1.clone())
            }
            (Some((adv, st_id)), None) => {
                // After the last state in history -> need to advance the state and return
                let threshold_for_transition = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
                // Note: Please update this if MipState transitions change as it might not hold true
                if *st_id == MipStateTypeId::Started
                    && adv.threshold < threshold_for_transition
                    && ts < adv.timeout
                {
                    Err(StateAtError::Unpredictable)
                } else {
                    let msg = Advance {
                        start_timestamp: start,
                        timeout,
                        threshold: adv.threshold,
                        now: ts,
                    };
                    // Return the resulting state after advance
                    let state = self.state.on_advance(msg);
                    Ok(MipStateTypeId::from(&state))
                }
            }
            _ => {
                // 1. Before the first state in history: already covered
                // 2. None, None: already covered - empty history
                Err(StateAtError::EmptyHistory)
            }
        }
    }
}

/// Error returned by MipStateHistory::state_at
#[allow(missing_docs)]
#[derive(Error, Debug, PartialEq)]
pub enum StateAtError {
    #[error("Initial state ({0:?}) only defined after timestamp: {1}")]
    BeforeInitialState(MipStateTypeId, MassaTime),
    #[error("Empty history, should never happen")]
    EmptyHistory,
    #[error("Cannot predict in the future (~ threshold not reach yet)")]
    Unpredictable,
}

// Store

/// Database for all MIP info
#[derive(Debug, Clone)]
pub struct MipStore(pub Arc<RwLock<MipStoreRaw>>);

/*
impl Default for MipStore {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(Default::default())))
    }
}
*/

impl MipStore {
    // TODO: merge get_version_current / get_version_to_announce so we don't iter twice?
    //       & rename to get_network_versions(...) -> (u32, u32)
    //       Move all others funcs from Factory and named them like:
    //       get_component_version_XXX (best, all, ...)

    /// Retrieve the current "global" version to set in block header
    pub fn get_version_current(&self) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        // Current version == last active
        store
            .0
            .iter()
            .rev()
            .find_map(|(k, v)| (v.state == MipState::active()).then_some(k.version))
            .unwrap_or(0)
    }

    /// Retrieve the "global" version number to announce in block header
    /// return 0 is there is nothing to announce
    pub fn get_version_to_announce(&self) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        // Announce the latest versioning info in Started / LockedIn state
        // Defined == Not yet ready to announce
        // Active == current version
        store
            .0
            .iter()
            .rev()
            .find_map(|(k, v)| {
                matches!(&v.state, &MipState::Started(_) | &MipState::LockedIn(_))
                    .then_some(k.version)
            })
            .unwrap_or(0)
    }
}

impl<const N: usize> TryFrom<[(MipInfo, MipStateHistory); N]> for MipStore {
    type Error = ();

    fn try_from(value: [(MipInfo, MipStateHistory); N]) -> Result<Self, Self::Error> {
        MipStoreRaw::try_from(value)
            .and_then(|store_raw| Ok(Self(Arc::new(RwLock::new(store_raw)))))
    }
}

/// Store of all versioning info
#[derive(Debug, Clone, Default)]
pub struct MipStoreRaw(pub(crate) BTreeMap<MipInfo, MipStateHistory>);

impl MipStoreRaw {
    /// Update our store with another (usually after a bootstrap where we received another store)
    /// Return true if update is successful, false if something is wrong on given store raw
    fn update_with(&mut self, store_raw: &MipStoreRaw) -> bool {
        // iter over items in given store:
        // -> 2 cases: VersioningInfo is already in self store -> add to 'to_update' list
        //             VersioningInfo is not in self.store -> add to 'to_add' list
        //

        // TODO: Check component_version always increase...

        let mut names: BTreeSet<String> = self.0.iter().map(|i| i.0.name.clone()).collect();
        let mut to_update: BTreeMap<MipInfo, MipStateHistory> = Default::default();
        let mut to_add: BTreeMap<MipInfo, MipStateHistory> = Default::default();
        let mut should_merge = true;

        for (v_info, v_state) in store_raw.0.iter() {
            if !v_state.is_coherent_with(v_info) {
                // As soon as we found one non coherent state we abort the merge
                should_merge = false;
                break;
            }

            if let Some(v_state_orig) = self.0.get(v_info) {
                // Versioning info (from right) is already in self (left)
                // Need to check if we add this to 'to_update' list
                let v_state_id: u32 = MipStateTypeId::from(&v_state.state).into();
                let v_state_orig_id: u32 = MipStateTypeId::from(&v_state_orig.state).into();

                if matches!(
                    v_state_orig.state,
                    MipState::Defined(_) | MipState::Started(_) | MipState::LockedIn(_)
                ) {
                    // Only accept 'higher' state
                    // (e.g. 'started' if 'defined', 'locked in' if 'started'...)
                    if v_state_id > v_state_orig_id {
                        to_update.insert(v_info.clone(), v_state.clone());
                    } else {
                        // Trying to downgrade state' (e.g. trying to go from 'active' -> 'defined')
                        should_merge = false;
                        break;
                    }
                }
            } else {
                // Versioning info (from right) is not in self.0 (left)
                // Need to check if we add this to 'to_add' list

                let last_v_info_ = to_add
                    .last_key_value()
                    .map(|i| i.0)
                    .or(self.0.last_key_value().map(|i| i.0));

                if let Some(last_v_info) = last_v_info_ {
                    if v_info.start > last_v_info.timeout
                        && v_info.timeout > v_info.start
                        && v_info.version > last_v_info.version
                        && !names.contains(&v_info.name)
                    {
                        // Time range is ok / version is ok / name is unique, let's add it
                        to_add.insert(v_info.clone(), v_state.clone());
                        names.insert(v_info.name.clone());
                    }
                } else {
                    // to_add is empty && self.0 is empty
                    to_add.insert(v_info.clone(), v_state.clone());
                    names.insert(v_info.name.clone());
                }
            }
        }

        if should_merge {
            self.0.append(&mut to_update);
            self.0.append(&mut to_add);
            true
        } else {
            false
        }
    }
}

impl<const N: usize> TryFrom<[(MipInfo, MipStateHistory); N]> for MipStoreRaw {
    type Error = ();

    fn try_from(value: [(MipInfo, MipStateHistory); N]) -> Result<Self, Self::Error> {
        let mut store_: BTreeMap<MipInfo, MipStateHistory> = Default::default();
        let mut has_error = false;
        for (m_info, m_state) in value {
            if m_state.is_coherent_with(&m_info) {
                store_.insert(m_info, m_state);
            } else {
                has_error = true;
                break;
            }
        }

        return if has_error { Err(()) } else { Ok(Self(store_)) };
    }
}

// End Store

#[cfg(test)]
mod test {
    use super::*;

    use std::str::FromStr;

    use crate::test_helpers::versioning_helpers::advance_state_until;
    use chrono::{Days, NaiveDate, NaiveDateTime};

    // use crate::test_helpers::versioning_helpers::advance_state_until;

    // use massa_serialization::DeserializeError;

    // Only for unit tests
    impl PartialEq<MipState> for MipStateHistory {
        fn eq(&self, other: &MipState) -> bool {
            self.state == *other
        }
    }

    fn get_a_version_info() -> (NaiveDateTime, NaiveDateTime, MipInfo) {
        // A helper function to provide a  default VersioningInfo
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
            MipInfo {
                name: "MIP-0002".to_string(),
                version: 2,
                component: MipComponent::Address,
                component_version: 1,
                start: MassaTime::from(start.timestamp() as u64),
                timeout: MassaTime::from(timeout.timestamp() as u64),
            },
        );
    }

    #[test]
    fn test_state_advance_from_defined() {
        // Test Versioning state transition (from state: Defined)
        let (_, _, vi) = get_a_version_info();
        let mut state: MipState = Default::default();
        assert_eq!(state, MipState::defined());

        let now = vi.start;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, MipState::defined());

        let now = vi.start.saturating_add(MassaTime::from(5));
        advance_msg.now = now;
        state = state.on_advance(advance_msg);

        // println!("state: {:?}", state);
        assert_eq!(
            state,
            MipState::Started(Started {
                threshold: Amount::zero()
            })
        );
    }

    #[test]
    fn test_state_advance_from_started() {
        // Test Versioning state transition (from state: Started)
        let (_, _, vi) = get_a_version_info();
        let mut state: MipState = MipState::started(Default::default());

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
        assert_eq!(state, MipState::started(threshold_too_low));
        advance_msg.threshold = threshold_ok;
        state = state.on_advance(advance_msg);
        assert_eq!(state, MipState::locked_in());
    }

    #[test]
    fn test_state_advance_from_locked_in() {
        // Test Versioning state transition (from state: LockedIn)
        let (_, _, vi) = get_a_version_info();
        let mut state: MipState = MipState::locked_in();

        let now = vi.start;
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, MipState::locked_in());

        advance_msg.now = advance_msg.timeout.saturating_add(MassaTime::from(1));
        state = state.on_advance(advance_msg);
        assert_eq!(state, MipState::active());
    }

    #[test]
    fn test_state_advance_from_active() {
        // Test Versioning state transition (from state: Active)
        let (_, _, vi) = get_a_version_info();
        let mut state = MipState::active();
        let now = vi.start;
        let advance = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, MipState::active());
    }

    #[test]
    fn test_state_advance_from_failed() {
        // Test Versioning state transition (from state: Failed)
        let (_, _, vi) = get_a_version_info();
        let mut state = MipState::failed();
        let now = vi.start;
        let advance = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, MipState::failed());
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

        let mut state: MipState = Default::default();
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, MipState::Failed(Failed {}));

        let mut state: MipState = MipState::started(Default::default());
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, MipState::Failed(Failed {}));
    }

    #[test]
    fn test_state_with_history() {
        // Test MipStateHistory::state_at() function

        let (start, _, vi) = get_a_version_info();
        let now_0 = MassaTime::from(start.timestamp() as u64);
        let mut state = MipStateHistory::new(now_0);

        assert_eq!(state, MipState::defined());

        let now = vi.start.saturating_add(MassaTime::from(15));
        let mut advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.timeout,
            threshold: Amount::zero(),
            now,
        };

        // Move from Defined -> Started
        state.on_advance(&advance_msg);
        assert_eq!(state, MipState::started(Amount::zero()));

        // Check history
        assert_eq!(state.history.len(), 2);
        assert!(matches!(
            state.history.first_key_value(),
            Some((&Advance { .. }, &MipStateTypeId::Defined))
        ));
        assert!(matches!(
            state.history.last_key_value(),
            Some((&Advance { .. }, &MipStateTypeId::Started))
        ));

        // Query with timestamp

        // Before Defined
        let state_id_ = state.state_at(
            vi.start.saturating_sub(MassaTime::from(5)),
            vi.start,
            vi.timeout,
        );
        assert!(matches!(
            state_id_,
            Err(StateAtError::BeforeInitialState(_, _))
        ));
        // After Defined timestamp
        let state_id = state.state_at(vi.start, vi.start, vi.timeout).unwrap();
        assert_eq!(state_id, MipStateTypeId::Defined);
        // At Started timestamp
        let state_id = state.state_at(now, vi.start, vi.timeout).unwrap();
        assert_eq!(state_id, MipStateTypeId::Started);

        // After Started timestamp but before timeout timestamp
        let after_started_ts = now.saturating_add(MassaTime::from(15));
        let state_id_ = state.state_at(after_started_ts, vi.start, vi.timeout);
        assert_eq!(state_id_, Err(StateAtError::Unpredictable));

        // After Started timestamp and after timeout timestamp
        let after_timeout_ts = vi.timeout.saturating_add(MassaTime::from(15));
        let state_id = state
            .state_at(after_timeout_ts, vi.start, vi.timeout)
            .unwrap();
        assert_eq!(state_id, MipStateTypeId::Failed);

        // Move from Started to LockedIn
        let threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
        advance_msg.threshold = threshold.saturating_add(Amount::from_str("1.0").unwrap());
        advance_msg.now = now.saturating_add(MassaTime::from(1));
        state.on_advance(&advance_msg);
        assert_eq!(state, MipState::locked_in());

        // Query with timestamp
        // After LockedIn timestamp and before timeout timestamp
        let after_locked_in_ts = now.saturating_add(MassaTime::from(10));
        let state_id = state
            .state_at(after_locked_in_ts, vi.start, vi.timeout)
            .unwrap();
        assert_eq!(state_id, MipStateTypeId::LockedIn);
        // After LockedIn timestamp and after timeout timestamp
        let state_id = state
            .state_at(after_timeout_ts, vi.start, vi.timeout)
            .unwrap();
        assert_eq!(state_id, MipStateTypeId::Active);
    }

    #[test]
    fn test_versioning_store_announce_current() {
        // Test VersioningInfo::get_version_to_announce() & ::get_version_current()

        let (_start, timeout, vi) = get_a_version_info();

        let mut vi_2 = vi.clone();
        vi_2.version += 1;
        vi_2.start =
            MassaTime::from(timeout.checked_add_days(Days::new(2)).unwrap().timestamp() as u64);
        vi_2.timeout =
            MassaTime::from(timeout.checked_add_days(Days::new(5)).unwrap().timestamp() as u64);

        // Can only build such object in test - history is empty :-/
        let vs_1 = MipStateHistory {
            state: MipState::active(),
            history: Default::default(),
        };
        let vs_2 = MipStateHistory {
            state: MipState::started(Amount::zero()),
            history: Default::default(),
        };

        // TODO: Have VersioningStore::from ?
        let vs_raw = MipStoreRaw(BTreeMap::from([(vi.clone(), vs_1), (vi_2.clone(), vs_2)]));
        // let vs_raw = MipStoreRaw::try_from([(vi.clone(), vs_1), (vi_2.clone(), vs_2)]).unwrap();
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));

        assert_eq!(vs.get_version_current(), vi.version);
        assert_eq!(vs.get_version_to_announce(), vi_2.version);

        // Test also an empty versioning store
        let vs_raw = MipStoreRaw(Default::default());
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));
        assert_eq!(vs.get_version_current(), 0);
        assert_eq!(vs.get_version_to_announce(), 0);
    }

    #[test]
    fn test_is_coherent_with() {
        // Test MipStateHistory::is_coherent_with

        // Given the following versioning info, we expect state
        // Defined @ time <= 2
        // Started @ time > 2 && <= 5
        // LockedIn @ time > time(Started) && <= 5
        // Active @time > 5
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
        };
        // Another versioning info (from an attacker) for testing
        let vi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(7),
            timeout: MassaTime::from(10),
        };

        let vsh = MipStateHistory {
            state: MipState::Error,
            history: Default::default(),
        };
        // At state Error -> (always) false
        assert_eq!(vsh.is_coherent_with(&vi_1), false);

        let vsh = MipStateHistory {
            state: MipState::defined(),
            history: Default::default(),
        };
        // At state Defined but no history -> false
        assert_eq!(vsh.is_coherent_with(&vi_1), false);

        let mut vsh = MipStateHistory::new(MassaTime::from(1));
        // At state Defined at time 1 -> true, given vi_1 @ time 1
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        // At state Defined at time 1 -> false given vi_1 @ time 3 (state should be Started)
        // assert_eq!(vsh.is_coherent_with(&vi_1, MassaTime::from(3)), false);

        // Advance to Started
        let now = MassaTime::from(3);
        vsh.on_advance(&Advance {
            start_timestamp: vi_1.start,
            timeout: vi_1.timeout,
            threshold: Amount::zero(),
            now,
        });

        // At state Started at time now -> true
        assert_eq!(vsh.state, MipState::started(Amount::zero()));
        assert_eq!(vsh.is_coherent_with(&vi_1), true);

        // Still coherent here (not after timeout)
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        // Not coherent anymore (after timeout) -> should be in state Failed
        // assert_eq!(vsh.is_coherent_with(&vi_1, MassaTime::from(6)), false);
        // Now with another versioning info
        assert_eq!(vsh.is_coherent_with(&vi_2), false);

        // Advance to LockedIn
        let now = MassaTime::from(4);
        vsh.on_advance(&Advance {
            start_timestamp: vi_1.start,
            timeout: vi_1.timeout,
            threshold: VERSIONING_THRESHOLD_TRANSITION_ACCEPTED,
            now,
        });

        // At state LockedIn at time now -> true
        assert_eq!(vsh.state, MipState::locked_in());
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        assert_eq!(vsh.is_coherent_with(&vi_1), true);

        // edge cases
        // TODO: history all good but does not start with Defined, start with Started
    }

    #[test]
    fn test_merge_with() {
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
        };

        let vs_1 = advance_state_until(MipState::active(), &vi_1);
        assert_eq!(vs_1, MipState::active());

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            component: MipComponent::Address,
            component_version: 2,
            start: MassaTime::from(17),
            timeout: MassaTime::from(27),
        };
        let vs_2 = advance_state_until(MipState::defined(), &vi_2);
        let mut vs_raw_1 = MipStoreRaw(BTreeMap::from([
            (vi_1.clone(), vs_1.clone()),
            (vi_2.clone(), vs_2.clone()),
        ]));

        let vs_2_2 = advance_state_until(MipState::locked_in(), &vi_2);
        assert_eq!(vs_2_2, MipState::locked_in());

        let vs_raw_2 =
            MipStoreRaw::try_from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2_2.clone())])
                .unwrap();

        vs_raw_1.update_with(&vs_raw_2);

        // Expect state 1 (for vi_1) no change, state 2 (for vi_2) updated to "LockedIn"
        assert_eq!(vs_raw_1.0.get(&vi_1).unwrap().state, vs_1.state);
        assert_eq!(vs_raw_1.0.get(&vi_2).unwrap().state, vs_2_2.state);
    }

    #[test]
    fn test_merge_with_invalid() {
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(0),
            timeout: MassaTime::from(5),
        };
        let vs_1 = advance_state_until(MipState::active(), &vi_1);

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            component: MipComponent::Address,
            component_version: 2,
            start: MassaTime::from(17),
            timeout: MassaTime::from(27),
        };
        let vs_2 = advance_state_until(MipState::defined(), &vi_2);

        let mut vs_raw_1 =
            MipStoreRaw::try_from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())])
                .unwrap();

        let mut vi_2_2 = vi_2.clone();
        // Make versioning info invalid (because start == vi_1.timeout)
        vi_2_2.start = MassaTime::from(5);
        let vs_2_2 = advance_state_until(MipState::defined(), &vi_2_2);
        let vs_raw_2 = MipStoreRaw(BTreeMap::from([
            (vi_1.clone(), vs_1.clone()),
            (vi_2_2.clone(), vs_2_2.clone()),
        ]));

        // FIXME: this should fail
        let _vs_raw_2_ =
            MipStoreRaw::try_from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2_2.clone())])
                .unwrap();

        vs_raw_1.update_with(&vs_raw_2);

        assert_eq!(vs_raw_1.0.get(&vi_1).unwrap().state, vs_1.state);
        assert_eq!(vs_raw_1.0.get(&vi_2).unwrap().state, vs_2.state);
    }
}
