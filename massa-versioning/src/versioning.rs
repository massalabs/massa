use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use machine::{machine, transitions};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::warn;

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

/// MIP info (name & versions & time range for a MIP)
#[derive(Clone, Debug)]
pub struct MipInfo {
    /// MIP name or descriptive name
    pub name: String,
    /// Network (or global) version (to be included in block header)
    pub version: u32,
    /// Components concerned by this versioning (e.g. a new Block version), and the associated component_version
    pub components: HashMap<MipComponent, u32>,
    /// a timestamp at which the version gains its meaning (e.g. announced in block header)
    pub start: MassaTime,
    /// a timestamp at the which the deployment is considered failed
    pub timeout: MassaTime,
    /// Once deployment has been locked, wait for this duration before deployment is considered active
    pub activation_delay: MassaTime,
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
            && self.components == other.components
            && self.start == other.start
            && self.timeout == other.timeout
            && self.activation_delay == other.activation_delay
    }
}

impl Eq for MipInfo {}

// Need to impl this manually otherwise clippy is angry :-P
impl Hash for MipInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.version.hash(state);
        self.components.iter().for_each(|c| c.hash(state));
        self.start.hash(state);
        self.timeout.hash(state);
    }
}

machine!(
    /// State machine for a Versioning component that tracks the deployment state
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum ComponentState {
        /// Initial state
        Defined,
        /// Past start, can only go to LockedIn after the threshold is above a given value
        Started { pub(crate) threshold: Amount },
        /// Wait for some time before going to active (to let user the time to upgrade)
        LockedIn { pub(crate) delay: MassaTime },
        /// After LockedIn, deployment is considered successful (after activation delay)
        Active { pub(crate) at: MassaTime },
        /// Past the timeout, if LockedIn is not reach
        Failed,
    }
);

impl Default for ComponentState {
    fn default() -> Self {
        Self::Defined(Defined {})
    }
}

#[allow(missing_docs)]
#[derive(IntoPrimitive, Debug, Clone, Eq, PartialEq, TryFromPrimitive, PartialOrd, Ord)]
#[repr(u32)]
pub enum ComponentStateTypeId {
    Error = 0,
    Defined = 1,
    Started = 2,
    LockedIn = 3,
    Active = 4,
    Failed = 5,
}

impl From<&ComponentState> for ComponentStateTypeId {
    fn from(value: &ComponentState) -> Self {
        match value {
            ComponentState::Error => ComponentStateTypeId::Error,
            ComponentState::Defined(_) => ComponentStateTypeId::Defined,
            ComponentState::Started(_) => ComponentStateTypeId::Started,
            ComponentState::LockedIn(_) => ComponentStateTypeId::LockedIn,
            ComponentState::Active(_) => ComponentStateTypeId::Active,
            ComponentState::Failed(_) => ComponentStateTypeId::Failed,
        }
    }
}

/// A message to update the `ComponentState`
#[derive(Clone, Debug)]
pub struct Advance {
    /// from MipInfo.start
    pub start_timestamp: MassaTime,
    /// from MipInfo.timeout
    pub timeout: MassaTime,
    /// from MipInfo.activation_delay
    pub activation_delay: MassaTime,

    /// % of past blocks with this version
    pub threshold: Amount,
    /// Current time (timestamp)
    pub now: MassaTime,
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

transitions!(ComponentState,
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
    pub fn on_advance(self, input: Advance) -> ComponentState {
        match input.now {
            n if n >= input.timeout => ComponentState::failed(),
            n if n >= input.start_timestamp => ComponentState::started(Amount::zero()),
            _ => ComponentState::Defined(Defined {}),
        }
    }
}

impl Started {
    /// Update state from state Started
    pub fn on_advance(self, input: Advance) -> ComponentState {
        if input.now > input.timeout {
            return ComponentState::failed();
        }

        if input.threshold >= VERSIONING_THRESHOLD_TRANSITION_ACCEPTED {
            ComponentState::locked_in(input.now)
        } else {
            ComponentState::started(input.threshold)
        }
    }
}

impl LockedIn {
    /// Update state from state LockedIn ...
    pub fn on_advance(self, input: Advance) -> ComponentState {
        if input.now > self.delay.saturating_add(input.activation_delay) {
            ComponentState::active(input.now)
        } else {
            ComponentState::locked_in(self.delay)
        }
    }
}

impl Active {
    /// Update state (will always stay in state Active)
    pub fn on_advance(self, _input: Advance) -> Active {
        Active { at: self.at }
    }
}

impl Failed {
    /// Update state (will always stay in state Failed)
    pub fn on_advance(self, _input: Advance) -> Failed {
        Failed {}
    }
}

/// Wrapper of ComponentState (in order to keep state history)
#[derive(Debug, Clone, PartialEq)]
pub struct MipState {
    pub(crate) state: ComponentState,
    pub(crate) history: BTreeMap<Advance, ComponentStateTypeId>,
}

impl MipState {
    /// Create
    pub fn new(defined: MassaTime) -> Self {
        let state: ComponentState = Default::default(); // Default is Defined
        let state_id = ComponentStateTypeId::from(&state);
        // Build a 'dummy' advance msg for state Defined, this is to avoid using an
        // Option<Advance> in MipStateHistory::history
        let advance = Advance {
            start_timestamp: MassaTime::from(0),
            timeout: MassaTime::from(0),
            threshold: Default::default(),
            now: defined,
            activation_delay: MassaTime::from(0),
        };

        let history = BTreeMap::from([(advance, state_id)]);
        Self { state, history }
    }

    /// Advance the state
    /// Can be called as multiple times as it will only store what changes the state in history
    pub fn on_advance(&mut self, input: &Advance) {
        let now = input.now;
        // Check that input.now is after last item in history
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
                let state_id = ComponentStateTypeId::from(&state);

                // Avoid storing too much things in history
                // Here we avoid storing for every threshold update
                if !(matches!(state, ComponentState::Started(Started { .. }))
                    && matches!(self.state, ComponentState::Started(Started { .. })))
                {
                    self.history.insert(input.clone(), state_id);
                }
                self.state = state;
            }
        }
    }

    /// Given a corresponding VersioningInfo, check if state is coherent
    /// it is coherent
    ///   if state can be at this position (e.g. can it be at state "Started" according to given time range)
    ///   if history is coherent with current state
    /// Return false for state == ComponentState::Error
    pub fn is_coherent_with(&self, versioning_info: &MipInfo) -> bool {
        // TODO: rename versioning_info -> mip_info

        // Always return false for state Error or if history is empty
        if matches!(&self.state, &ComponentState::Error) || self.history.is_empty() {
            return false;
        }

        // safe to unwrap (already tested if empty or not)
        let (initial_ts, initial_state_id) = self.history.first_key_value().unwrap();
        if *initial_state_id != ComponentStateTypeId::Defined {
            // self.history does not start with Defined -> (always) false
            return false;
        }

        // Build a new VersionStateHistory from initial state, replaying the whole history
        // but with given versioning info then compare
        let mut vsh = MipState::new(initial_ts.now);
        let mut advance_msg = Advance {
            start_timestamp: versioning_info.start,
            timeout: versioning_info.timeout,
            threshold: Amount::zero(),
            now: initial_ts.now,
            activation_delay: versioning_info.activation_delay,
        };

        for (adv, _state) in self.history.iter().skip(1) {
            advance_msg.now = adv.now;
            advance_msg.threshold = adv.threshold;
            vsh.on_advance(&advance_msg);
        }

        vsh == *self
    }

    /// Query state at given timestamp
    /// TODO: add doc for start & timeout parameter? why do we need them?
    /// HERE
    pub fn state_at(
        &self,
        ts: MassaTime,
        start: MassaTime,
        timeout: MassaTime,
    ) -> Result<ComponentStateTypeId, StateAtError> {
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
                if *st_id == ComponentStateTypeId::Started
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
                        activation_delay: adv.activation_delay,
                    };
                    // Return the resulting state after advance
                    let state = self.state.on_advance(msg);
                    Ok(ComponentStateTypeId::from(&state))
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
    BeforeInitialState(ComponentStateTypeId, MassaTime),
    #[error("Empty history, should never happen")]
    EmptyHistory,
    #[error("Cannot predict in the future (~ threshold not reach yet)")]
    Unpredictable,
}

// Store

/// Database for all MIP info
#[derive(Debug, Clone)]
pub struct MipStore(pub Arc<RwLock<MipStoreRaw>>);

impl MipStore {
    /// Retrieve the current network version to set in block header
    /// HERE
    pub fn get_network_version_current(&self) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        // Current version == last active
        store
            .store
            .iter()
            .rev()
            .find_map(|(k, v)| (matches!(v.state, ComponentState::Active(_))).then_some(k.version))
            .unwrap_or(0)
    }

    /// Retrieve the network version number to announce in block header
    /// return 0 is there is nothing to announce
    pub fn get_network_version_to_announce(&self) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        // Announce the latest versioning info in Started / LockedIn state
        // Defined == Not yet ready to announce
        // Active == current version
        store
            .store
            .iter()
            .rev()
            .find_map(|(k, v)| {
                matches!(
                    &v.state,
                    &ComponentState::Started(_) | &ComponentState::LockedIn(_)
                )
                .then_some(k.version)
            })
            .unwrap_or(0)
    }

    pub fn update_network_version_stats(
        &mut self,
        slot_timestamp: MassaTime,
        network_versions: Option<(u32, u32)>,
    ) {
        let mut lock = self.0.write();
        lock.update_network_version_stats(slot_timestamp, network_versions);
    }

    #[allow(clippy::result_unit_err)]
    pub fn update_with(
        &mut self,
        mip_store: &MipStore,
    ) -> Result<(Vec<MipInfo>, Vec<MipInfo>), ()> {
        let mut lock = self.0.write();
        let lock_other = mip_store.0.read();
        lock.update_with(lock_other.deref())
    }
}

impl<const N: usize> TryFrom<([(MipInfo, MipState); N], MipStatsConfig)> for MipStore {
    type Error = ();

    fn try_from(
        (value, cfg): ([(MipInfo, MipState); N], MipStatsConfig),
    ) -> Result<Self, Self::Error> {
        MipStoreRaw::try_from((value, cfg)).map(|store_raw| Self(Arc::new(RwLock::new(store_raw))))
    }
}

/// Statistics in MipStoreRaw
#[derive(Debug, Clone, PartialEq)]
pub struct MipStatsConfig {
    pub block_count_considered: usize,
    pub counters_max: usize,
}

/// In order for a MIP to be accepted, we compute stats about other node 'network' version announcement
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MipStoreStats {
    pub(crate) config: MipStatsConfig,
    pub(crate) latest_announcements: VecDeque<u32>,
    pub(crate) network_version_counters: BTreeMap<u32, u64>,
}

impl MipStoreStats {
    pub(crate) fn new(config: MipStatsConfig) -> Self {
        Self {
            config: config.clone(),
            latest_announcements: VecDeque::with_capacity(config.block_count_considered),
            network_version_counters: Default::default(),
        }
    }
}

/// Store of all versioning info
/// HERE
#[derive(Debug, Clone, PartialEq)]
pub struct MipStoreRaw {
    pub(crate) store: BTreeMap<MipInfo, MipState>,
    pub(crate) stats: MipStoreStats,
}

impl MipStoreRaw {
    /// Update our store with another (usually after a bootstrap where we received another store)
    /// Return list of updated / added if update is successful
    #[allow(clippy::result_unit_err)]
    pub fn update_with(
        &mut self,
        store_raw: &MipStoreRaw,
    ) -> Result<(Vec<MipInfo>, Vec<MipInfo>), ()> {
        // iter over items in given store:
        // -> 2 cases:
        // * MipInfo is already in self store -> add to 'to_update' list
        // * MipInfo is not in self.store -> We received a new MipInfo so we are running an out dated version
        //                                   of the software
        //                                   We then return the list of new MipInfo so we can warn and ask
        //                                   to update the software

        let mut component_versions: HashMap<MipComponent, u32> = self
            .store
            .iter()
            .flat_map(|c| {
                c.0.components
                    .iter()
                    .map(|(mip_component, component_version)| {
                        (mip_component.clone(), *component_version)
                    })
            })
            .collect();
        let mut names: BTreeSet<String> = self.store.iter().map(|i| i.0.name.clone()).collect();
        let mut to_update: BTreeMap<MipInfo, MipState> = Default::default();
        let mut to_add: BTreeMap<MipInfo, MipState> = Default::default();
        let mut should_merge = true;

        for (v_info, v_state) in store_raw.store.iter() {
            if !v_state.is_coherent_with(v_info) {
                // As soon as we found one non coherent state we abort the merge
                should_merge = false;
                break;
            }

            if let Some(v_state_orig) = self.store.get(v_info) {
                // Versioning info (from right) is already in self (left)
                // Need to check if we add this to 'to_update' list
                let v_state_id: u32 = ComponentStateTypeId::from(&v_state.state).into();
                let v_state_orig_id: u32 = ComponentStateTypeId::from(&v_state_orig.state).into();

                if matches!(
                    v_state_orig.state,
                    ComponentState::Defined(_)
                        | ComponentState::Started(_)
                        | ComponentState::LockedIn(_)
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
                    .or(self.store.last_key_value().map(|i| i.0));

                if let Some(last_v_info) = last_v_info_ {
                    // check for versions of all components in v_info
                    let mut component_version_compatible = true;
                    for component in v_info.components.iter() {
                        if component.1 <= component_versions.get(component.0).unwrap_or(&0) {
                            component_version_compatible = false;
                            break;
                        }
                    }

                    if v_info.start > last_v_info.timeout
                        && v_info.timeout > v_info.start
                        && v_info.version > last_v_info.version
                        && !names.contains(&v_info.name)
                        && component_version_compatible
                    {
                        // Time range is ok / version is ok / name is unique, let's add it
                        to_add.insert(v_info.clone(), v_state.clone());
                        names.insert(v_info.name.clone());
                        for component in v_info.components.iter() {
                            component_versions.insert(component.0.clone(), *component.1);
                        }
                    } else {
                        // Something is wrong (time range not ok? / version not incr? / names?
                        // or component version not incr?)
                        should_merge = false;
                        break;
                    }
                } else {
                    // to_add is empty && self.0 is empty
                    to_add.insert(v_info.clone(), v_state.clone());
                    names.insert(v_info.name.clone());
                }
            }
        }

        if should_merge {
            let added = to_add.keys().cloned().collect();
            let updated = to_update.keys().cloned().collect();

            self.store.append(&mut to_update);
            self.store.append(&mut to_add);
            Ok((updated, added))
        } else {
            Err(())
        }
    }

    fn update_network_version_stats(
        &mut self,
        slot_timestamp: MassaTime,
        network_versions: Option<(u32, u32)>,
    ) {
        if let Some((_current_network_version, announced_network_version)) = network_versions {
            let removed_version_ = match self.stats.latest_announcements.len() {
                n if n > self.stats.config.block_count_considered => {
                    self.stats.latest_announcements.pop_front()
                }
                _ => None,
            };
            self.stats
                .latest_announcements
                .push_back(announced_network_version);

            // We update the count of the received version
            *self
                .stats
                .network_version_counters
                .entry(announced_network_version)
                .or_default() += 1;

            if let Some(removed_version) = removed_version_ {
                *self
                    .stats
                    .network_version_counters
                    .entry(removed_version)
                    .or_insert(1) -= 1;
            }

            // Cleanup for the counters
            if self.stats.network_version_counters.len() > self.stats.config.counters_max {
                if let Some((version, count)) = self.stats.network_version_counters.pop_first() {
                    // TODO: return version / count for unit tests?
                    warn!(
                        "MipStoreStats removed counter for version {}, count was: {}",
                        version, count
                    )
                }
            }

            self.advance_states_on_updated_stats(slot_timestamp);
        }
    }

    /// Used internally by `update_network_version_stats`
    fn advance_states_on_updated_stats(&mut self, slot_timestamp: MassaTime) {
        for (mi, state) in self.store.iter_mut() {
            let network_version_count = *self
                .stats
                .network_version_counters
                .get(&mi.version)
                .unwrap_or(&0) as f32;
            let block_count_considered = self.stats.config.block_count_considered as f32;

            let vote_ratio_ = 100.0 * network_version_count / block_count_considered;

            let vote_ratio = Amount::from_mantissa_scale(vote_ratio_.round() as u64, 0);

            let advance_msg = Advance {
                start_timestamp: mi.start,
                timeout: mi.timeout,
                threshold: vote_ratio,
                now: slot_timestamp,
                activation_delay: mi.activation_delay,
            };

            // TODO / OPTIM: filter the store to avoid advancing on failed and active versions
            state.on_advance(&advance_msg.clone());
        }
    }
}

impl<const N: usize> TryFrom<([(MipInfo, MipState); N], MipStatsConfig)> for MipStoreRaw {
    type Error = ();

    fn try_from(
        (value, cfg): ([(MipInfo, MipState); N], MipStatsConfig),
    ) -> Result<Self, Self::Error> {
        // Build an empty store
        let mut store = Self {
            store: Default::default(),
            stats: MipStoreStats::new(cfg.clone()),
        };

        // Build another one with given value
        let other_store = Self {
            store: BTreeMap::from(value),
            stats: MipStoreStats::new(cfg),
        };

        // Use update_with ensuring that we have no overlapping time range, unique names & ...
        match store.update_with(&other_store) {
            Ok(_) => Ok(store),
            Err(_) => Err(()),
        }
    }
}

// End Store

#[cfg(test)]
mod test {
    use super::*;

    use std::str::FromStr;

    use chrono::{Days, NaiveDate, NaiveDateTime};

    use crate::test_helpers::versioning_helpers::advance_state_until;

    use massa_models::config::{MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX};

    // Only for unit tests
    impl PartialEq<ComponentState> for MipState {
        fn eq(&self, other: &ComponentState) -> bool {
            self.state == *other
        }
    }

    impl From<(&MipInfo, &Amount, &MassaTime)> for Advance {
        fn from((mip_info, threshold, now): (&MipInfo, &Amount, &MassaTime)) -> Self {
            Self {
                start_timestamp: mip_info.start,
                timeout: mip_info.timeout,
                threshold: *threshold,
                now: *now,
                activation_delay: mip_info.activation_delay,
            }
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
                components: HashMap::from([(MipComponent::Address, 1)]),
                start: MassaTime::from(start.timestamp() as u64),
                timeout: MassaTime::from(timeout.timestamp() as u64),
                activation_delay: MassaTime::from(20),
            },
        );
    }

    #[test]
    fn test_state_advance_from_defined() {
        // Test Versioning state transition (from state: Defined)
        let (_, _, mi) = get_a_version_info();
        let mut state: ComponentState = Default::default();
        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_sub(MassaTime::from(1));
        let mut advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_add(MassaTime::from(5));
        advance_msg.now = now;
        state = state.on_advance(advance_msg);

        // println!("state: {:?}", state);
        assert_eq!(
            state,
            ComponentState::Started(Started {
                threshold: Amount::zero()
            })
        );
    }

    #[test]
    fn test_state_advance_from_started() {
        // Test Versioning state transition (from state: Started)
        let (_, _, mi) = get_a_version_info();
        let mut state: ComponentState = ComponentState::started(Default::default());

        let now = mi.start;
        let threshold_too_low = Amount::from_str("74.9").unwrap();
        let threshold_ok = Amount::from_str("82.42").unwrap();
        let mut advance_msg = Advance::from((&mi, &threshold_too_low, &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::started(threshold_too_low));
        advance_msg.threshold = threshold_ok;
        state = state.on_advance(advance_msg);
        assert_eq!(state, ComponentState::locked_in(now));
    }

    #[test]
    fn test_state_advance_from_locked_in() {
        // Test Versioning state transition (from state: LockedIn)
        let (_, _, mi) = get_a_version_info();

        let locked_in_at = mi.start.saturating_add(MassaTime::from(1));
        let mut state: ComponentState = ComponentState::locked_in(locked_in_at);

        let now = mi.start;
        let mut advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::locked_in(locked_in_at));

        advance_msg.now = advance_msg.timeout.saturating_add(MassaTime::from(1));
        state = state.on_advance(advance_msg);
        assert_eq!(state, ComponentState::active());
    }

    #[test]
    fn test_state_advance_from_active() {
        // Test Versioning state transition (from state: Active)
        let (_, _, mi) = get_a_version_info();
        let mut state = ComponentState::active();
        let now = mi.start;
        let advance = Advance::from((&mi, &Amount::zero(), &now));

        state = state.on_advance(advance);
        assert_eq!(state, ComponentState::active());
    }

    #[test]
    fn test_state_advance_from_failed() {
        // Test Versioning state transition (from state: Failed)
        let (_, _, mi) = get_a_version_info();
        let mut state = ComponentState::failed();
        let now = mi.start;
        let advance = Advance::from((&mi, &Amount::zero(), &now));
        state = state.on_advance(advance);
        assert_eq!(state, ComponentState::failed());
    }

    #[test]
    fn test_state_advance_to_failed() {
        // Test Versioning state transition (to state: Failed)
        let (_, _, mi) = get_a_version_info();
        let now = mi.timeout.saturating_add(MassaTime::from(1));
        let advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        let mut state: ComponentState = Default::default(); // Defined
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::Failed(Failed {}));

        let mut state = ComponentState::started(Default::default());
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::Failed(Failed {}));
    }

    #[test]
    fn test_state_with_history() {
        // Test MipStateHistory::state_at() function

        let (start, _, mi) = get_a_version_info();
        let now_0 = MassaTime::from(start.timestamp() as u64);
        let mut state = MipState::new(now_0);

        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_add(MassaTime::from(15));
        let mut advance_msg = Advance::from((&mi, &Amount::zero(), &now));

        // Move from Defined -> Started
        state.on_advance(&advance_msg);
        assert_eq!(state, ComponentState::started(Amount::zero()));

        // Check history
        assert_eq!(state.history.len(), 2);
        assert!(matches!(
            state.history.first_key_value(),
            Some((&Advance { .. }, &ComponentStateTypeId::Defined))
        ));
        assert!(matches!(
            state.history.last_key_value(),
            Some((&Advance { .. }, &ComponentStateTypeId::Started))
        ));

        // Query with timestamp

        // Before Defined
        let state_id_ = state.state_at(
            mi.start.saturating_sub(MassaTime::from(5)),
            mi.start,
            mi.timeout,
        );
        assert!(matches!(
            state_id_,
            Err(StateAtError::BeforeInitialState(_, _))
        ));
        // After Defined timestamp
        let state_id = state.state_at(mi.start, mi.start, mi.timeout).unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Defined);
        // At Started timestamp
        let state_id = state.state_at(now, mi.start, mi.timeout).unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Started);

        // After Started timestamp but before timeout timestamp
        let after_started_ts = now.saturating_add(MassaTime::from(15));
        let state_id_ = state.state_at(after_started_ts, mi.start, mi.timeout);
        assert_eq!(state_id_, Err(StateAtError::Unpredictable));

        // After Started timestamp and after timeout timestamp
        let after_timeout_ts = mi.timeout.saturating_add(MassaTime::from(15));
        let state_id = state
            .state_at(after_timeout_ts, mi.start, mi.timeout)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Failed);

        // Move from Started to LockedIn
        let threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
        advance_msg.threshold = threshold.saturating_add(Amount::from_str("1.0").unwrap());
        advance_msg.now = now.saturating_add(MassaTime::from(1));
        state.on_advance(&advance_msg);
        assert_eq!(state, ComponentState::locked_in(advance_msg.now));

        // Query with timestamp
        // After LockedIn timestamp and before timeout timestamp
        let after_locked_in_ts = now.saturating_add(MassaTime::from(10));
        let state_id = state
            .state_at(after_locked_in_ts, mi.start, mi.timeout)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::LockedIn);
        // After LockedIn timestamp and after timeout timestamp
        let state_id = state
            .state_at(after_timeout_ts, mi.start, mi.timeout)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Active);
    }

    #[test]
    fn test_versioning_store_announce_current() {
        // Test VersioningInfo::get_version_to_announce() & ::get_version_current()

        let (_start, timeout, mi) = get_a_version_info();

        let mut mi_2 = mi.clone();
        mi_2.version += 1;
        mi_2.start =
            MassaTime::from(timeout.checked_add_days(Days::new(2)).unwrap().timestamp() as u64);
        mi_2.timeout =
            MassaTime::from(timeout.checked_add_days(Days::new(5)).unwrap().timestamp() as u64);

        // Can only build such object in test - history is empty :-/
        let vs_1 = MipState {
            state: ComponentState::active(),
            history: Default::default(),
        };
        let vs_2 = MipState {
            state: ComponentState::started(Amount::zero()),
            history: Default::default(),
        };

        // TODO: Have VersioningStore::from ?
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };
        let vs_raw = MipStoreRaw {
            store: BTreeMap::from([(mi.clone(), vs_1), (mi_2.clone(), vs_2)]),
            stats: MipStoreStats::new(mip_stats_cfg.clone()),
        };
        // let vs_raw = MipStoreRaw::try_from([(vi.clone(), vs_1), (vi_2.clone(), vs_2)]).unwrap();
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));

        assert_eq!(vs.get_network_version_current(), mi.version);
        assert_eq!(vs.get_network_version_to_announce(), mi_2.version);

        // Test also an empty versioning store
        let vs_raw = MipStoreRaw {
            store: Default::default(),
            stats: MipStoreStats::new(mip_stats_cfg),
        };
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 0);
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
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
            activation_delay: MassaTime::from(2),
        };
        // Another versioning info (from an attacker) for testing
        let vi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(7),
            timeout: MassaTime::from(10),
            activation_delay: MassaTime::from(2),
        };

        let vsh = MipState {
            state: ComponentState::Error,
            history: Default::default(),
        };
        // At state Error -> (always) false
        assert_eq!(vsh.is_coherent_with(&vi_1), false);

        let vsh = MipState {
            state: ComponentState::defined(),
            history: Default::default(),
        };
        // At state Defined but no history -> false
        assert_eq!(vsh.is_coherent_with(&vi_1), false);

        let mut vsh = MipState::new(MassaTime::from(1));
        // At state Defined at time 1 -> true, given vi_1 @ time 1
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        // At state Defined at time 1 -> false given vi_1 @ time 3 (state should be Started)
        // assert_eq!(vsh.is_coherent_with(&vi_1, MassaTime::from(3)), false);

        // Advance to Started
        let now = MassaTime::from(3);
        let adv = Advance::from((&vi_1, &Amount::zero(), &now));
        vsh.on_advance(&adv);

        // At state Started at time now -> true
        assert_eq!(vsh.state, ComponentState::started(Amount::zero()));
        assert_eq!(vsh.is_coherent_with(&vi_1), true);
        // Now with another versioning info
        assert_eq!(vsh.is_coherent_with(&vi_2), false);

        // Advance to LockedIn
        let now = MassaTime::from(4);
        let adv = Advance::from((&vi_1, &VERSIONING_THRESHOLD_TRANSITION_ACCEPTED, &now));
        vsh.on_advance(&adv);

        // At state LockedIn at time now -> true
        assert_eq!(vsh.state, ComponentState::locked_in(now));
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
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
            activation_delay: MassaTime::from(2),
        };

        let vs_1 = advance_state_until(ComponentState::active(), &vi_1);
        assert_eq!(vs_1, ComponentState::active());

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from(17),
            timeout: MassaTime::from(27),
            activation_delay: MassaTime::from(2),
        };
        let vs_2 = advance_state_until(ComponentState::defined(), &vi_2);

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };
        let mut vs_raw_1 = MipStoreRaw {
            store: BTreeMap::from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())]),
            stats: MipStoreStats::new(mip_stats_cfg.clone()),
        };

        let vs_2_2 = advance_state_until(ComponentState::active(), &vi_2);
        assert_eq!(vs_2_2, ComponentState::active());

        let vs_raw_2 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2_2.clone())],
            mip_stats_cfg,
        ))
        .unwrap();

        vs_raw_1.update_with(&vs_raw_2).unwrap();

        // Expect state 1 (for vi_1) no change, state 2 (for vi_2) updated to "Active"
        assert_eq!(vs_raw_1.store.get(&vi_1).unwrap().state, vs_1.state);
        assert_eq!(vs_raw_1.store.get(&vi_2).unwrap().state, vs_2_2.state);
    }

    #[test]
    fn test_merge_with_invalid() {
        // Test updating a versioning store with another invalid:
        // 1- overlapping time range
        // 2- overlapping versioning component

        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(0),
            timeout: MassaTime::from(5),
            activation_delay: MassaTime::from(2),
        };
        let vs_1 = advance_state_until(ComponentState::active(), &vi_1);
        assert_eq!(vs_1, ComponentState::active());

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from(17),
            timeout: MassaTime::from(27),
            activation_delay: MassaTime::from(2),
        };
        let vs_2 = advance_state_until(ComponentState::defined(), &vi_2);
        assert_eq!(vs_2, ComponentState::defined());

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };

        let mut vs_raw_1 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
            mip_stats_cfg.clone(),
        ))
        .unwrap();

        let mut vi_2_2 = vi_2.clone();
        // Make versioning info invalid (because start == vi_1.timeout)
        vi_2_2.start = vi_1.timeout;
        let vs_2_2 = advance_state_until(ComponentState::defined(), &vi_2_2);
        let vs_raw_2 = MipStoreRaw {
            store: BTreeMap::from([
                (vi_1.clone(), vs_1.clone()),
                (vi_2_2.clone(), vs_2_2.clone()),
            ]),
            stats: MipStoreStats::new(mip_stats_cfg.clone()),
        };

        // This fails because try_from use update_with
        let _vs_raw_2_ = MipStoreRaw::try_from((
            [
                (vi_1.clone(), vs_1.clone()),
                (vi_2_2.clone(), vs_2_2.clone()),
            ],
            mip_stats_cfg.clone(),
        ));
        assert_eq!(_vs_raw_2_.is_err(), true);

        assert_eq!(vs_raw_1.update_with(&vs_raw_2), Err(()));
        assert_eq!(vs_raw_1.store.get(&vi_1).unwrap().state, vs_1.state);
        assert_eq!(vs_raw_1.store.get(&vi_2).unwrap().state, vs_2.state);

        // 2- overlapping component version
        let mut vs_raw_1 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
            mip_stats_cfg.clone(),
        ))
        .unwrap();

        let mut vi_2_2 = vi_2.clone();
        vi_2_2.components = vi_1.components.clone();

        let vs_2_2 = advance_state_until(ComponentState::defined(), &vi_2_2);
        let vs_raw_2 = MipStoreRaw {
            store: BTreeMap::from([
                (vi_1.clone(), vs_1.clone()),
                (vi_2_2.clone(), vs_2_2.clone()),
            ]),
            stats: MipStoreStats::new(mip_stats_cfg.clone()),
        };

        assert_eq!(vs_raw_1.update_with(&vs_raw_2), Err(()));
    }

    #[test]
    fn test_empty_mip_store() {
        // Test if we can init an empty MipStore

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };

        let mip_store = MipStore::try_from(([], mip_stats_config));
        assert_eq!(mip_store.is_ok(), true);
    }
}
