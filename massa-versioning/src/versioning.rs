use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::iter;
use std::ops::Deref;
use std::sync::Arc;

use machine::{machine, transitions};
use num::{rational::Ratio, Zero};
use num_enum::{FromPrimitive, IntoPrimitive, TryFromPrimitive};
use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, warn};

use massa_db_exports::{
    DBBatch, ShareableMassaDBController, MIP_STORE_PREFIX, MIP_STORE_STATS_PREFIX, STATE_CF,
    VERSIONING_CF,
};
use massa_models::config::MIP_STORE_STATS_BLOCK_CONSIDERED;
#[allow(unused_imports)]
use massa_models::config::VERSIONING_ACTIVATION_DELAY_MIN;
use massa_models::config::VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
use massa_models::error::ModelsError;
use massa_models::slot::Slot;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_serialization::{DeserializeError, Deserializer, SerializeError, Serializer};
use massa_time::MassaTime;
use variant_count::VariantCount;

use crate::versioning_ser_der::{
    MipInfoDeserializer, MipInfoSerializer, MipStateDeserializer, MipStateSerializer,
    MipStoreStatsDeserializer, MipStoreStatsSerializer,
};

/// Versioning component enum
#[allow(missing_docs)]
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, FromPrimitive, IntoPrimitive, VariantCount,
)]
#[repr(u32)]
pub enum MipComponent {
    // Address and KeyPair versions are directly related
    Address,
    KeyPair,
    Block,
    VM,
    FinalStateHashKind,
    Execution,
    FinalState,
    #[doc(hidden)]
    #[num_enum(default)]
    __Nonexhaustive,
}

/// MIP info (name & versions & time range for a MIP)
#[derive(Clone, Debug)]
pub struct MipInfo {
    /// MIP name or descriptive name
    pub name: String,
    /// Network (or global) version (to be included in block header)
    pub version: u32,
    /// Components concerned by this versioning (e.g. a new Block version), and the associated component_version
    pub components: BTreeMap<MipComponent, u32>,
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
        (
            self.start,
            self.timeout,
            self.activation_delay,
            &self.name,
            &self.version,
            &self.components,
        )
            .cmp(&(
                other.start,
                other.timeout,
                other.activation_delay,
                &other.name,
                &other.version,
                &other.components,
            ))
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

machine!(
    /// State machine for a Versioning component that tracks the deployment state
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum ComponentState {
        /// Initial state
        Defined,
        /// Past start, can only go to LockedIn after the threshold is above a given value
        Started { pub(crate) vote_ratio: Ratio<u64> },
        /// Locked but wait for some time before going to active (to let users the time to upgrade)
        /// 'at' is the timestamp where the transition from 'Started' to 'LockedIn' happened
        LockedIn { pub(crate) at: MassaTime },
        /// After LockedIn, deployment is considered successful (after activation delay)
        /// 'at' is the timestamp where the transition from 'LockedIn' to 'Active' happened
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

impl ComponentState {
    fn is_final(&self) -> bool {
        matches!(
            self,
            ComponentState::Active(..) | ComponentState::Failed(..) | ComponentState::Error
        )
    }
}

#[allow(missing_docs)]
#[derive(
    IntoPrimitive, Debug, Clone, Eq, PartialEq, TryFromPrimitive, PartialOrd, Ord, VariantCount,
)]
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
    pub threshold: Ratio<u64>,
    /// Current time (timestamp)
    pub now: MassaTime,
}

impl PartialEq for Advance {
    fn eq(&self, other: &Self) -> bool {
        self.start_timestamp == other.start_timestamp
            && self.timeout == other.timeout
            && self.threshold == other.threshold
            && self.now == other.now
            && self.activation_delay == other.activation_delay
    }
}

impl Eq for Advance {}

// A Lightweight version of 'Advance' (used in MipState history)
#[derive(Clone, Debug)]
pub struct AdvanceLW {
    /// % of past blocks with this version
    pub threshold: Ratio<u64>,
    /// Current time (timestamp)
    pub now: MassaTime,
}

impl From<&Advance> for AdvanceLW {
    fn from(value: &Advance) -> Self {
        Self {
            threshold: value.threshold,
            now: value.now,
        }
    }
}

impl Ord for AdvanceLW {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.now, self.threshold).cmp(&(other.now, other.threshold))
    }
}

impl PartialOrd for AdvanceLW {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for AdvanceLW {
    fn eq(&self, other: &Self) -> bool {
        self.threshold == other.threshold && self.now == other.now
    }
}

impl Eq for AdvanceLW {}

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
            n if n >= input.start_timestamp => ComponentState::started(Ratio::zero()),
            _ => ComponentState::Defined(Defined {}),
        }
    }
}

impl Started {
    /// Update state from state Started
    pub fn on_advance(self, input: Advance) -> ComponentState {
        if input.now >= input.timeout {
            return ComponentState::failed();
        }

        if input.threshold >= VERSIONING_THRESHOLD_TRANSITION_ACCEPTED {
            debug!("(VERSIONING LOG) transition accepted, locking in");
            ComponentState::locked_in(input.now)
        } else {
            ComponentState::started(input.threshold)
        }
    }
}

impl LockedIn {
    /// Update state from state LockedIn ...
    pub fn on_advance(self, input: Advance) -> ComponentState {
        if input.now > self.at.saturating_add(input.activation_delay) {
            debug!("(VERSIONING LOG) locked version has become active");
            ComponentState::active(input.now)
        } else {
            ComponentState::locked_in(self.at)
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

/// Error returned by `MipState::is_consistent_with`
#[derive(Error, Debug, PartialEq)]
pub enum IsConsistentError {
    // State is not consistent with associated MipInfo, ex: State is active but MipInfo.start was not reach yet
    #[error("MipState history is empty")]
    EmptyHistory,
    #[error("MipState is at state Error")]
    AtError,
    #[error("History must start at state 'Defined' and not {0:?}")]
    InvalidHistory(ComponentStateTypeId),
    #[error("Non consistent state: {0:?} versus rebuilt state: {1:?}")]
    NonConsistent(ComponentState, ComponentState),
    #[error("Invalid data in MIP info, start >= timeout")]
    Invalid,
}

/// Wrapper of ComponentState (in order to keep state history)
#[derive(Debug, Clone, PartialEq)]
pub struct MipState {
    pub(crate) state: ComponentState,
    pub(crate) history: BTreeMap<AdvanceLW, ComponentStateTypeId>,
}

impl MipState {
    /// Create
    pub fn new(defined: MassaTime) -> Self {
        let state: ComponentState = Default::default(); // Default is Defined
        let state_id = ComponentStateTypeId::from(&state);
        // Build a 'dummy' advance lw msg for state Defined, this is to avoid using an
        // Option<AdvanceLW> in MipStateHistory::history
        let advance = AdvanceLW {
            threshold: Default::default(),
            now: defined,
            // activation_delay: MassaTime::from_millis(0),
        };

        let history = BTreeMap::from([(advance, state_id)]);
        Self { state, history }
    }

    /// Create a new state from an existing state - resulting state will be at state "Defined"
    pub fn reset_from(&self) -> Option<Self> {
        match self.history.first_key_value() {
            Some((advance, state_id)) if *state_id == ComponentStateTypeId::Defined => {
                Some(MipState::new(advance.now))
            }
            _ => None,
        }
    }

    /// Advance the state
    /// Can be called as multiple times as it will only store what changes the state in history
    pub fn on_advance(&mut self, input: &Advance) {
        // Check that input.now is after last item in history
        // We don't want to go backward
        let is_forward = self
            .history
            .last_key_value()
            .map(|(adv, _)| adv.now < input.now)
            .unwrap_or(false);

        if is_forward {
            // machines crate (for state machine) does not support passing ref :-/
            let state = self.state.on_advance(input.clone());
            // Update history as well
            if state != self.state {
                // Avoid storing too much things in history
                // Here we avoid storing for every threshold update
                if !(matches!(state, ComponentState::Started(Started { .. }))
                    && matches!(self.state, ComponentState::Started(Started { .. })))
                {
                    self.history
                        .insert(input.into(), ComponentStateTypeId::from(&state));
                }
                self.state = state;
            }
        }
    }

    /// Given a corresponding MipInfo, check if state is consistent
    /// it is consistent
    ///   if state can be at this position (e.g. can it be at state "Started" according to given time range)
    ///   if history is consistent with current state
    /// Return false for state == ComponentState::Error
    pub fn is_consistent_with(&self, mip_info: &MipInfo) -> Result<(), IsConsistentError> {
        // Always return false for state Error or if history is empty
        if matches!(&self.state, &ComponentState::Error) {
            return Err(IsConsistentError::AtError);
        }

        if mip_info.start >= mip_info.timeout {
            return Err(IsConsistentError::Invalid);
        }

        if self.history.is_empty() {
            return Err(IsConsistentError::EmptyHistory);
        }

        // safe to unwrap (already tested if empty or not)
        let (initial_ts, initial_state_id) = self.history.first_key_value().unwrap();
        if *initial_state_id != ComponentStateTypeId::Defined {
            // self.history does not start with Defined -> (always) false
            return Err(IsConsistentError::InvalidHistory(initial_state_id.clone()));
        }

        if mip_info.start < initial_ts.now || mip_info.timeout < initial_ts.now {
            // MIP info start (or timeout) is before Defined timestamp??
            return Err(IsConsistentError::InvalidHistory(initial_state_id.clone()));
        }

        // Build a new MipStateHistory from initial state, replaying the whole history
        // but with given versioning info then compare
        let mut vsh = MipState::new(initial_ts.now);
        let mut advance_msg = Advance {
            start_timestamp: mip_info.start,
            timeout: mip_info.timeout,
            threshold: Ratio::zero(),
            now: initial_ts.now,
            activation_delay: mip_info.activation_delay,
        };

        for (adv, _state) in self.history.iter().skip(1) {
            advance_msg.now = adv.now;
            advance_msg.threshold = adv.threshold;
            vsh.on_advance(&advance_msg);
        }

        // Advance state if both are at 'Started' (to have the same threshold)
        // Note: because in history we do not add entries for every threshold update
        if let (
            ComponentState::Started(Started {
                vote_ratio: threshold,
            }),
            ComponentState::Started(Started {
                vote_ratio: threshold_2,
            }),
        ) = (vsh.state, self.state)
        {
            if threshold_2 != threshold {
                advance_msg.threshold = threshold_2;
                // Need to advance now timestamp otherwise it will be ignored
                advance_msg.now = advance_msg.now.saturating_add(MassaTime::from_millis(1));
                vsh.on_advance(&advance_msg);
            }
        }

        if vsh == *self {
            Ok(())
        } else {
            Err(IsConsistentError::NonConsistent(self.state, vsh.state))
        }
    }

    /// Query state at given timestamp
    /// TODO: add doc for start & timeout parameter? why do we need them?
    pub fn state_at(
        &self,
        ts: MassaTime,
        start: MassaTime,
        timeout: MassaTime,
        activation_delay: MassaTime,
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
                    && ts < timeout
                // adv.timeout - TODO: test this
                {
                    Err(StateAtError::Unpredictable)
                } else {
                    let msg = Advance {
                        start_timestamp: start,
                        timeout,
                        threshold: adv.threshold,
                        now: ts,
                        // activation_delay: adv.activation_delay,
                        activation_delay,
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

    /// Return the time when state will go from LockedIn to Active, None if not already LockedIn
    pub fn activation_at(&self, mip_info: &MipInfo) -> Option<MassaTime> {
        match self.state {
            ComponentState::LockedIn(LockedIn { at }) => {
                Some(at.saturating_add(mip_info.activation_delay))
            }
            _ => None,
        }
    }

    /// Return True if state can not change anymore (e.g. Active, Failed or Error)
    pub fn is_final(&self) -> bool {
        self.state.is_final()
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
    #[error("Cannot predict value: threshold not reached yet")]
    Unpredictable,
}

// Store

/// Database for all MIP info
#[derive(Debug, Clone)]
pub struct MipStore(pub Arc<RwLock<MipStoreRaw>>);

impl MipStore {
    /// Retrieve the current network version to set in block header
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

    /// Retrieve the last active version at the given timestamp
    pub fn get_network_version_active_at(&self, ts: MassaTime) -> u32 {
        let lock = self.0.read();
        let store = lock.deref();
        store
            .store
            .iter()
            .rev()
            .find_map(|(k, v)| match v.state {
                ComponentState::Active(Active { at }) if at <= ts => Some(k.version),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Retrieve the network version number to announce in block header
    /// return 0 is there is nothing to announce
    pub fn get_network_version_to_announce(&self) -> Option<u32> {
        let lock = self.0.read();
        let store = lock.deref();
        // Announce the latest versioning info in Started / LockedIn state
        // Defined == Not yet ready to announce
        // Active == current version
        store.store.iter().rev().find_map(|(k, v)| {
            matches!(
                &v.state,
                &ComponentState::Started(_) | &ComponentState::LockedIn(_)
            )
            .then_some(k.version)
        })
    }

    pub fn update_network_version_stats(
        &mut self,
        slot_timestamp: MassaTime,
        network_versions: Option<(u32, Option<u32>)>,
    ) {
        let mut lock = self.0.write();
        lock.update_network_version_stats(slot_timestamp, network_versions);
    }

    #[allow(clippy::result_large_err)]
    pub fn update_with(
        &mut self,
        mip_store: &MipStore,
    ) -> Result<(Vec<MipInfo>, BTreeMap<MipInfo, MipState>), UpdateWithError> {
        let mut lock = self.0.write();
        let lock_other = mip_store.0.read();
        lock.update_with(lock_other.deref())
    }

    // Query

    /// Get latest version at given timestamp (e.g. slot) for the given MipComponent
    pub fn get_latest_component_version_at(&self, component: &MipComponent, ts: MassaTime) -> u32 {
        let guard = self.0.read();
        guard.get_latest_component_version_at(component, ts)
    }

    /// Get all versions in 'Active state' for the given MipComponent
    pub(crate) fn get_all_active_component_versions(&self, component: &MipComponent) -> Vec<u32> {
        let guard = self.0.read();
        guard.get_all_active_component_versions(component)
    }

    /// Get all versions (at any state) for the given MipComponent
    pub(crate) fn get_all_component_versions(
        &self,
        component: &MipComponent,
    ) -> BTreeMap<u32, ComponentStateTypeId> {
        let guard = self.0.read();
        guard.get_all_component_versions(component)
    }

    // GRPC

    /// Retrieve a list of MIP info with their corresponding state (as id) - used for grpc API
    pub fn get_mip_status(&self) -> BTreeMap<MipInfo, ComponentStateTypeId> {
        let guard = self.0.read();
        guard
            .store
            .iter()
            .map(|(mip_info, mip_state)| {
                (
                    mip_info.clone(),
                    ComponentStateTypeId::from(&mip_state.state),
                )
            })
            .collect()
    }

    // Network restart
    pub fn is_consistent_with_shutdown_period(
        &self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<(), IsConsistentWithShutdownPeriodError> {
        let guard = self.0.read();
        guard.is_consistent_with_shutdown_period(
            shutdown_start,
            shutdown_end,
            thread_count,
            t0,
            genesis_timestamp,
        )
    }

    #[allow(dead_code)]
    pub fn update_for_network_shutdown(
        &mut self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<(), ModelsError> {
        let mut guard = self.0.write();
        guard.update_for_network_shutdown(
            shutdown_start,
            shutdown_end,
            thread_count,
            t0,
            genesis_timestamp,
        )
    }

    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        let guard = self.0.read();
        guard.is_key_value_valid(serialized_key, serialized_value)
    }

    // DB

    pub fn update_batches(
        &self,
        db_batch: &mut DBBatch,
        db_versioning_batch: &mut DBBatch,
        between: Option<(&MassaTime, &MassaTime)>,
    ) -> Result<(), SerializeError> {
        let guard = self.0.read();
        guard.update_batches(db_batch, db_versioning_batch, between)
    }

    pub fn extend_from_db(
        &mut self,
        db: ShareableMassaDBController,
    ) -> Result<(Vec<MipInfo>, BTreeMap<MipInfo, MipState>), ExtendFromDbError> {
        let mut guard = self.0.write();
        guard.extend_from_db(db)
    }

    pub fn reset_db(&self, db: ShareableMassaDBController) {
        {
            let mut guard = db.write();
            guard.delete_prefix(MIP_STORE_PREFIX, STATE_CF, None);
            guard.delete_prefix(MIP_STORE_PREFIX, VERSIONING_CF, None);
            guard.delete_prefix(MIP_STORE_STATS_PREFIX, VERSIONING_CF, None);
        }
    }

    /// Create a MIP store with what is written on the disk
    pub fn try_from_db(
        db: ShareableMassaDBController,
        cfg: MipStatsConfig,
    ) -> Result<Self, ExtendFromDbError> {
        MipStoreRaw::try_from_db(db, cfg).map(|store_raw| Self(Arc::new(RwLock::new(store_raw))))
    }

    // debug
    // pub fn len(&self) -> usize {
    //     let guard = self.0.read();
    //     guard.store.len()
    // }
}

impl<const N: usize> TryFrom<([(MipInfo, MipState); N], MipStatsConfig)> for MipStore {
    type Error = UpdateWithError;

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
    pub warn_announced_version_ratio: Ratio<u64>,
}

/// In order for a MIP to be accepted, we compute statistics about other node 'network' version announcement
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MipStoreStats {
    // config for block count to consider when computing the vote ratio
    pub(crate) config: MipStatsConfig,
    // Last network version announcements (in last block header)
    // Used to clean up the field: network_version_counters (pop the oldest then subtract matching counter)
    pub(crate) latest_announcements: VecDeque<u32>,
    // A map where key: network version, value: announcement for this network version count
    // Note: to avoid various attacks, we have as many counters as version announcements
    //       + if a counter reset to 0, it is removed from the hash map
    pub(crate) network_version_counters: HashMap<u32, u64>,
}

impl MipStoreStats {
    pub(crate) fn new(config: MipStatsConfig) -> Self {
        Self {
            config: config.clone(),
            latest_announcements: VecDeque::with_capacity(config.block_count_considered),
            network_version_counters: HashMap::with_capacity(config.block_count_considered),
        }
    }

    // reset stats - used in `update_for_network_shutdown` function
    fn reset(&mut self) {
        self.latest_announcements.clear();
        self.network_version_counters.clear();
    }
}

/// Error returned by `MipStoreRaw::update_with`
#[derive(Error, Debug, PartialEq)]
pub enum UpdateWithError {
    // State is not consistent with associated MipInfo, ex: State is active but MipInfo.start was not reach yet
    #[error("MipInfo {0:#?} is not consistent with state: {1:#?}, error: {2}")]
    NonConsistent(MipInfo, MipState, IsConsistentError),
    // ex: State is already started but received state is only defined
    #[error("For MipInfo {0:?}, trying to downgrade from state {1:?} to {2:?}")]
    Downgrade(MipInfo, ComponentState, ComponentState),
    // ex: MipInfo 2 start is before MipInfo 1 timeout (MipInfo timings should only be sequential)
    #[error("MipInfo {0:?} has overlapping data of MipInfo {1:?}")]
    Overlapping(MipInfo, MipInfo),
    #[error("MipInfo {0:?} has an invalid activation delay value: {1}, min allowed: {2}")]
    InvalidActivationDelay(MipInfo, MassaTime, MassaTime),
}

/// Error returned by 'extend_from_db`
#[derive(Error, Debug)]
pub enum ExtendFromDbError {
    #[error("Unable to get an handle over db column: {0}")]
    UnknownDbColumn(String),
    #[error("{0}")]
    Update(#[from] UpdateWithError),
    #[error("{0}")]
    Deserialize(String),
}

/// Error returned by 'is_consistent_with_shutdown_period`
#[derive(Error, Debug)]
pub enum IsConsistentWithShutdownPeriodError {
    #[error("{0}")]
    Update(#[from] ModelsError),
    #[error("MipInfo: {0:?} (state: {1:?}) is not consistent with shutdown: {2} {3}")]
    NonConsistent(MipInfo, ComponentState, MassaTime, MassaTime),
}

#[derive(Error, Debug)]
pub enum IsKVValidError {
    #[error("{0}")]
    Deserialize(String),
    #[error("Invalid prefix for key")]
    InvalidPrefix,
}

/// Store of all versioning info
#[derive(Debug, Clone, PartialEq)]
pub struct MipStoreRaw {
    pub(crate) store: BTreeMap<MipInfo, MipState>,
    pub(crate) stats: MipStoreStats,
}

impl MipStoreRaw {
    /// Update our store with another (usually after a bootstrap where we received another store)
    /// Return list of updated / added if successful, UpdateWithError otherwise
    #[allow(clippy::result_large_err)]
    pub fn update_with(
        &mut self,
        store_raw: &MipStoreRaw,
    ) -> Result<(Vec<MipInfo>, BTreeMap<MipInfo, MipState>), UpdateWithError> {
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
        let mut names: HashSet<String> = self.store.keys().map(|mi| mi.name.clone()).collect();
        let mut to_update: BTreeMap<MipInfo, MipState> = Default::default();
        let mut to_add: BTreeMap<MipInfo, MipState> = Default::default();
        let mut has_error: Option<UpdateWithError> = None;

        for (m_info, m_state) in store_raw.store.iter() {
            if let Err(e) = m_state.is_consistent_with(m_info) {
                // As soon as we found one non consistent state we abort the merge
                has_error = Some(UpdateWithError::NonConsistent(
                    m_info.clone(),
                    m_state.clone(),
                    e,
                ));
                break;
            }

            if let Some(m_state_orig) = self.store.get(m_info) {
                // Given MIP info is already in self
                // Need to check if we add it to 'to_update' list
                let m_state_id: u32 = ComponentStateTypeId::from(&m_state.state).into();
                let m_state_orig_id: u32 = ComponentStateTypeId::from(&m_state_orig.state).into();

                // Note: we do not check for state: active OR failed OR error as they cannot change
                if matches!(
                    m_state_orig.state,
                    ComponentState::Defined(_)
                        | ComponentState::Started(_)
                        | ComponentState::LockedIn(_)
                ) {
                    // Only accept 'higher' state
                    // (e.g. 'started' if 'defined', 'locked in' if 'started'...)
                    if m_state_id >= m_state_orig_id {
                        to_update.insert(m_info.clone(), m_state.clone());
                    } else {
                        // Trying to downgrade state' (e.g. trying to go from 'active' -> 'defined')
                        has_error = Some(UpdateWithError::Downgrade(
                            m_info.clone(),
                            m_state_orig.state,
                            m_state.state,
                        ));
                        break;
                    }
                }
            } else {
                // Given MIP info is not in self
                // Need to check if we add it to 'to_add' list

                let last_m_info_ = to_add
                    .last_key_value()
                    .map(|(mi, _)| mi)
                    .or(self.store.last_key_value().map(|(mi, _)| mi));

                if let Some(last_m_info) = last_m_info_ {
                    // check for versions of all components in v_info
                    let mut component_version_compatible = true;
                    for (component, component_version) in m_info.components.iter() {
                        if component_version <= component_versions.get(component).unwrap_or(&0) {
                            component_version_compatible = false;
                            break;
                        }
                    }

                    #[cfg(not(any(test, feature = "test-exports")))]
                    if m_info.activation_delay < VERSIONING_ACTIVATION_DELAY_MIN {
                        has_error = Some(UpdateWithError::InvalidActivationDelay(
                            m_info.clone(),
                            m_info.activation_delay,
                            VERSIONING_ACTIVATION_DELAY_MIN,
                        ));
                        break;
                    }

                    if m_info.start > last_m_info.timeout
                        && m_info.timeout > m_info.start
                        && m_info.version > last_m_info.version
                        && !names.contains(&m_info.name)
                        && component_version_compatible
                    {
                        // Time range is ok / version is ok / name is unique, let's add it
                        to_add.insert(m_info.clone(), m_state.clone());
                        names.insert(m_info.name.clone());
                        for (component, component_version) in m_info.components.iter() {
                            component_versions.insert(component.clone(), *component_version);
                        }
                    } else {
                        // Something is wrong (time range not ok? / version not incr? / names?
                        // or component version not incr?)
                        has_error = Some(UpdateWithError::Overlapping(
                            m_info.clone(),
                            last_m_info.clone(),
                        ));
                        break;
                    }
                } else {
                    // to_add is empty && self.0 is empty
                    to_add.insert(m_info.clone(), m_state.clone());
                    names.insert(m_info.name.clone());
                }
            }
        }

        match has_error {
            None => {
                let updated: Vec<MipInfo> = to_update.keys().cloned().collect();

                // Note: we only update the store with to_update collection
                //       having something in the to_add collection means that we need to update
                //       the Massa node software
                self.store.append(&mut to_update);
                Ok((updated, to_add))
            }
            Some(e) => Err(e),
        }
    }

    fn update_network_version_stats(
        &mut self,
        slot_timestamp: MassaTime,
        network_versions: Option<(u32, Option<u32>)>,
    ) {
        if let Some((_current_network_version, announced_network_version_)) = network_versions {
            let announced_network_version = announced_network_version_.unwrap_or(0);

            let removed_version_ = match self.stats.latest_announcements.len() {
                n if n >= self.stats.config.block_count_considered => {
                    self.stats.latest_announcements.pop_front()
                }
                _ => None,
            };
            self.stats
                .latest_announcements
                .push_back(announced_network_version);

            // We update the count of the received version (example: update counter for version 1)
            let mut network_version_count = *self
                .stats
                .network_version_counters
                .entry(announced_network_version)
                .and_modify(|v| *v = v.saturating_add(1))
                .or_insert(1);

            // If we removed a version announcement, we decrement the corresponding counter
            // (example: remove a version 1, so decrement the corresponding counter)
            // As soon as a counter value is 0, we remove it
            if let Some(removed_version) = removed_version_ {
                if let Entry::Occupied(mut e) =
                    self.stats.network_version_counters.entry(removed_version)
                {
                    let entry_value = e.get_mut();
                    *entry_value = entry_value.saturating_sub(1);
                    network_version_count = *entry_value;
                    if *entry_value == 0 {
                        self.stats.network_version_counters.remove(&removed_version);
                    }
                }
            }

            if announced_network_version != 0 {
                let vote_ratio = Ratio::new(
                    network_version_count,
                    self.stats.config.block_count_considered as u64,
                );

                if vote_ratio > self.stats.config.warn_announced_version_ratio {
                    let last_key_value = self.store.last_key_value();
                    if let Some((mi, _ms)) = last_key_value {
                        if announced_network_version > mi.version {
                            // Vote ratio is > 30%
                            // announced version is not known (not in MIP store)
                            // announced version is > to the last known network version in MIP store
                            // -> Warn the user to update
                            warn!("{} our of {} last blocks advertised that they are willing to transition to version {}. You should update your node if you wish to move to that version.",
                                network_version_count,
                                self.stats.config.block_count_considered,
                                announced_network_version
                            );
                        }
                    }
                }
            }
        }

        debug!(
            "[VERSIONING STATS] stats have {} counters and {} announcements",
            self.stats.network_version_counters.len(),
            self.stats.latest_announcements.len()
        );

        // Even if stats did not move, update the states (e.g. LockedIn -> Active)
        self.advance_states_on_updated_stats(slot_timestamp);
    }

    /// Used internally by `update_network_version_stats`
    fn advance_states_on_updated_stats(&mut self, slot_timestamp: MassaTime) {
        for (mi, state) in self.store.iter_mut() {
            if state.is_final() {
                // State cannot change (ex: Active), no need to update
                continue;
            }

            let network_version_count = *self
                .stats
                .network_version_counters
                .get(&mi.version)
                .unwrap_or(&0);

            let vote_ratio = Ratio::new(
                network_version_count,
                self.stats.config.block_count_considered as u64,
            );

            debug!("[VERSIONING STATS] Vote counts / blocks considered = {} / {} (for MipInfo with network version {} - {})",
                network_version_count,
                self.stats.config.block_count_considered,
                mi.version,
                mi.name);

            let advance_msg = Advance {
                start_timestamp: mi.start,
                timeout: mi.timeout,
                threshold: vote_ratio,
                now: slot_timestamp,
                activation_delay: mi.activation_delay,
            };

            state.on_advance(&advance_msg.clone());
        }
    }

    // Query

    /// Get latest version at given timestamp (e.g. slot)
    fn get_latest_component_version_at(&self, component: &MipComponent, ts: MassaTime) -> u32 {
        let version = self
            .store
            .iter()
            .rev()
            .filter(|(mi, ms)| {
                mi.components.get(component).is_some()
                    && matches!(ms.state, ComponentState::Active(_))
            })
            .find_map(|(mi, ms)| {
                let res = ms.state_at(ts, mi.start, mi.timeout, mi.activation_delay);
                match res {
                    Ok(ComponentStateTypeId::Active) => mi.components.get(component).copied(),
                    _ => None,
                }
            })
            .unwrap_or(0);

        version
    }

    /// Get all versions in 'Active state' for the given MipComponent
    fn get_all_active_component_versions(&self, component: &MipComponent) -> Vec<u32> {
        let versions_iter = self.store.iter().filter_map(|(mi, ms)| {
            if matches!(ms.state, ComponentState::Active(_)) {
                mi.components.get(component).copied()
            } else {
                None
            }
        });
        let versions: Vec<u32> = iter::once(0).chain(versions_iter).collect();
        versions
    }

    /// Get all versions (at any state) for the given MipComponent
    fn get_all_component_versions(
        &self,
        component: &MipComponent,
    ) -> BTreeMap<u32, ComponentStateTypeId> {
        let versions_iter = self.store.iter().filter_map(|(mi, ms)| {
            mi.components
                .get(component)
                .copied()
                .map(|component_version| (component_version, ComponentStateTypeId::from(&ms.state)))
        });
        iter::once((0, ComponentStateTypeId::Active))
            .chain(versions_iter)
            .collect()
    }

    // Network restart

    /// Check if store is consistent with given last network shutdown
    /// On a network shutdown, the MIP infos will be edited but we still need to check if this is consistent
    fn is_consistent_with_shutdown_period(
        &self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<(), IsConsistentWithShutdownPeriodError> {
        // let mut is_consistent = true;
        let mut has_error: Result<(), IsConsistentWithShutdownPeriodError> = Ok(());

        let shutdown_start_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_start)?;
        let shutdown_end_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_end)?;
        let shutdown_range = shutdown_start_ts..=shutdown_end_ts;

        for (mip_info, mip_state) in &self.store {
            match mip_state.state {
                ComponentState::Defined(..) => {
                    // all good if it does not start / timeout during shutdown period
                    if shutdown_range.contains(&mip_info.start)
                        || shutdown_range.contains(&mip_info.timeout)
                    {
                        // is_consistent = false;
                        has_error = Err(IsConsistentWithShutdownPeriodError::NonConsistent(
                            mip_info.clone(),
                            mip_state.state,
                            shutdown_start_ts,
                            shutdown_end_ts,
                        ));
                        break;
                    }
                }
                ComponentState::Started(..) | ComponentState::LockedIn(..) => {
                    // assume this should have been reset
                    has_error = Err(IsConsistentWithShutdownPeriodError::NonConsistent(
                        mip_info.clone(),
                        mip_state.state,
                        shutdown_start_ts,
                        shutdown_end_ts,
                    ));
                    break;
                }
                _ => {
                    // active / failed, error, nothing to do
                    // locked in, nothing to do (might go from 'locked in' to 'active' during shutdown)
                }
            }
        }

        has_error
    }

    fn update_for_network_shutdown(
        &mut self,
        shutdown_start: Slot,
        shutdown_end: Slot,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> Result<(), ModelsError> {
        let shutdown_start_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_start)?;
        let shutdown_end_ts =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, shutdown_end)?;
        let shutdown_range = shutdown_start_ts..=shutdown_end_ts;

        let mut new_store: BTreeMap<MipInfo, MipState> = Default::default();
        let mut new_stats = self.stats.clone();
        new_stats.reset();

        let next_valid_start_ = shutdown_end.get_next_slot(thread_count)?;
        let next_valid_start =
            get_block_slot_timestamp(thread_count, t0, genesis_timestamp, next_valid_start_)?;

        let mut offset: Option<MassaTime> = None;

        for (mip_info, mip_state) in &self.store {
            match mip_state.state {
                ComponentState::Defined(..) => {
                    // Defined: offset start & timeout

                    let mut new_mip_info = mip_info.clone();

                    if shutdown_range.contains(&new_mip_info.start) {
                        let offset_ts = match offset {
                            Some(offset_ts) => offset_ts,
                            None => {
                                let offset_ts = next_valid_start.saturating_sub(mip_info.start);
                                offset = Some(offset_ts);
                                offset_ts
                            }
                        };

                        new_mip_info.start = new_mip_info.start.saturating_add(offset_ts);
                        new_mip_info.timeout = new_mip_info
                            .start
                            .saturating_add(mip_info.timeout.saturating_sub(mip_info.start));
                    }
                    new_store.insert(new_mip_info, mip_state.clone());
                }
                ComponentState::Started(..) | ComponentState::LockedIn(..) => {
                    // Started or LockedIn -> Reset to Defined, offset start & timeout

                    let mut new_mip_info = mip_info.clone();

                    let offset_ts = match offset {
                        Some(offset_ts) => offset_ts,
                        None => {
                            let offset_ts = next_valid_start.saturating_sub(mip_info.start);
                            offset = Some(offset_ts);
                            offset_ts
                        }
                    };

                    new_mip_info.start = new_mip_info.start.saturating_add(offset_ts);
                    new_mip_info.timeout = new_mip_info
                        .start
                        .saturating_add(mip_info.timeout.saturating_sub(mip_info.start));

                    // Need to reset state to 'Defined'
                    let new_mip_state = MipState::reset_from(mip_state)
                        .ok_or(ModelsError::from("Unable to reset state"))?;
                    // Note: statistics are already reset
                    new_store.insert(new_mip_info, new_mip_state.clone());
                }
                _ => {
                    // active / failed, error, nothing to do
                    new_store.insert(mip_info.clone(), mip_state.clone());
                }
            }
        }

        self.store = new_store;
        self.stats = new_stats;
        Ok(())
    }

    // Final state
    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        self._is_key_value_valid(serialized_key, serialized_value)
            .is_ok()
    }

    pub fn _is_key_value_valid(
        &self,
        serialized_key: &[u8],
        serialized_value: &[u8],
    ) -> Result<(), IsKVValidError> {
        let mip_info_deser = MipInfoDeserializer::new();
        let mip_state_deser = MipStateDeserializer::new();
        let mip_store_stats_deser = MipStoreStatsDeserializer::new(
            MIP_STORE_STATS_BLOCK_CONSIDERED,
            self.stats.config.warn_announced_version_ratio,
        );

        if serialized_key.starts_with(MIP_STORE_PREFIX.as_bytes()) {
            let (rem, _mip_info) = mip_info_deser
                .deserialize::<DeserializeError>(&serialized_key[MIP_STORE_PREFIX.len()..])
                .map_err(|e| IsKVValidError::Deserialize(e.to_string()))?;

            if !rem.is_empty() {
                return Err(IsKVValidError::Deserialize(
                    "Rem not empty after deserialization".to_string(),
                ));
            }

            let (rem2, _mip_state) = mip_state_deser
                .deserialize::<DeserializeError>(serialized_value)
                .map_err(|e| IsKVValidError::Deserialize(e.to_string()))?;

            if !rem2.is_empty() {
                return Err(IsKVValidError::Deserialize(
                    "Rem not empty after deserialization".to_string(),
                ));
            }
        } else if serialized_key.starts_with(MIP_STORE_STATS_PREFIX.as_bytes()) {
            let (rem, _mip_store_stats) = mip_store_stats_deser
                .deserialize::<DeserializeError>(serialized_value)
                .map_err(|e| IsKVValidError::Deserialize(e.to_string()))?;

            if !rem.is_empty() {
                return Err(IsKVValidError::Deserialize(
                    "Rem not empty after deserialization".to_string(),
                ));
            }
        } else {
            return Err(IsKVValidError::InvalidPrefix);
        }

        Ok(())
    }

    // DB methods

    /// Get MIP store changes between 2 timestamps - used by the db to update the disk
    fn update_batches(
        &self,
        batch: &mut DBBatch,
        versioning_batch: &mut DBBatch,
        between: Option<(&MassaTime, &MassaTime)>,
    ) -> Result<(), SerializeError> {
        let mip_info_ser = MipInfoSerializer::new();
        let mip_state_ser = MipStateSerializer::new();

        let bounds = match between {
            Some(between) => (*between.0)..(*between.1),
            None => MassaTime::from_millis(0)..MassaTime::max(),
        };
        let mut key = Vec::new();
        let mut value = Vec::new();

        for (mip_info, mip_state) in self.store.iter() {
            if let Some((advance, state_id)) = mip_state.history.last_key_value() {
                if bounds.contains(&advance.now) {
                    key.extend(MIP_STORE_PREFIX.as_bytes().to_vec());
                    mip_info_ser.serialize(mip_info, &mut key)?;
                    mip_state_ser.serialize(mip_state, &mut value)?;
                    match state_id {
                        ComponentStateTypeId::Active => {
                            batch.insert(key.clone(), Some(value.clone()));
                            // + "Remove key" in VERSIONING_CF
                            versioning_batch.insert(key.clone(), None);
                        }
                        _ => {
                            versioning_batch.insert(key.clone(), Some(value.clone()));
                        }
                    }
                    key.clear();
                    value.clear();
                }
            }
        }

        value.clear();
        let mip_stats_ser = MipStoreStatsSerializer::new();
        mip_stats_ser.serialize(&self.stats, &mut value)?;
        versioning_batch.insert(
            MIP_STORE_STATS_PREFIX.as_bytes().to_vec(),
            Some(value.clone()),
        );

        Ok(())
    }

    /// Extend MIP store with what is written on the disk
    fn extend_from_db(
        &mut self,
        db: ShareableMassaDBController,
    ) -> Result<(Vec<MipInfo>, BTreeMap<MipInfo, MipState>), ExtendFromDbError> {
        let mip_info_deser = MipInfoDeserializer::new();
        let mip_state_deser = MipStateDeserializer::new();
        let mip_store_stats_deser = MipStoreStatsDeserializer::new(
            MIP_STORE_STATS_BLOCK_CONSIDERED,
            self.stats.config.warn_announced_version_ratio,
        );

        let db = db.read();

        // Get data from state cf handle
        let mut update_data: BTreeMap<MipInfo, MipState> = Default::default();
        for (ser_mip_info, ser_mip_state) in
            db.prefix_iterator_cf(STATE_CF, MIP_STORE_PREFIX.as_bytes())
        {
            if !ser_mip_info.starts_with(MIP_STORE_PREFIX.as_bytes()) {
                break;
            }

            // deser
            let (_, mip_info) = mip_info_deser
                .deserialize::<DeserializeError>(&ser_mip_info[MIP_STORE_PREFIX.len()..])
                .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

            let (_, mip_state) = mip_state_deser
                .deserialize::<DeserializeError>(&ser_mip_state)
                .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

            update_data.insert(mip_info, mip_state);
        }

        let (mut updated, mut added) = match update_data.is_empty() {
            true => (vec![], BTreeMap::new()),
            false => {
                let store_raw_ = MipStoreRaw {
                    store: update_data,
                    stats: MipStoreStats {
                        config: MipStatsConfig {
                            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
                            warn_announced_version_ratio: self
                                .stats
                                .config
                                .warn_announced_version_ratio,
                        },
                        latest_announcements: Default::default(),
                        network_version_counters: Default::default(),
                    },
                };
                // Only call update_with if update_data is not empty
                self.update_with(&store_raw_)?
            }
        };

        let mut update_data: BTreeMap<MipInfo, MipState> = Default::default();

        // Get data from state cf handle
        for (ser_mip_info, ser_mip_state) in
            db.prefix_iterator_cf(VERSIONING_CF, MIP_STORE_PREFIX.as_bytes())
        {
            // deser

            match &ser_mip_info {
                key if key.starts_with(MIP_STORE_PREFIX.as_bytes()) => {
                    let (_, mip_info) = mip_info_deser
                        .deserialize::<DeserializeError>(&ser_mip_info[MIP_STORE_PREFIX.len()..])
                        .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

                    let (_, mip_state) = mip_state_deser
                        .deserialize::<DeserializeError>(&ser_mip_state)
                        .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

                    update_data.insert(mip_info, mip_state);
                }
                key if key.starts_with(MIP_STORE_STATS_PREFIX.as_bytes()) => {
                    let (_, mip_store_stats) = mip_store_stats_deser
                        .deserialize::<DeserializeError>(&ser_mip_state)
                        .map_err(|e| ExtendFromDbError::Deserialize(e.to_string()))?;

                    self.stats = mip_store_stats;
                }
                _ => {
                    break;
                }
            }
        }

        if !update_data.is_empty() {
            let store_raw_ = MipStoreRaw {
                store: update_data,
                stats: MipStoreStats {
                    config: MipStatsConfig {
                        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
                        warn_announced_version_ratio: self
                            .stats
                            .config
                            .warn_announced_version_ratio,
                    },
                    latest_announcements: Default::default(),
                    network_version_counters: Default::default(),
                },
            };
            // Only call update_with if update_data is not empty
            let (updated_2, added_2) = self.update_with(&store_raw_)?;
            updated.extend(updated_2);
            added.extend(added_2);
        }

        Ok((updated, added))
    }

    /// Create a MIP store raw with what is written on the disk
    fn try_from_db(
        db: ShareableMassaDBController,
        cfg: MipStatsConfig,
    ) -> Result<Self, ExtendFromDbError> {
        let mut store_raw = MipStoreRaw {
            store: Default::default(),
            stats: MipStoreStats {
                config: cfg,
                latest_announcements: Default::default(),
                network_version_counters: Default::default(),
            },
        };

        let (_updated, mut added) = store_raw.extend_from_db(db)?;
        store_raw.store.append(&mut added);
        Ok(store_raw)
    }
}

impl<const N: usize> TryFrom<([(MipInfo, MipState); N], MipStatsConfig)> for MipStoreRaw {
    type Error = UpdateWithError;

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
            Ok((_updated, mut added)) => {
                store.store.append(&mut added);
                Ok(store)
            }
            Err(e) => Err(e),
        }
    }
}

// End Store

#[cfg(test)]
mod test {
    use super::*;

    use assert_matches::assert_matches;
    use massa_db_exports::{MassaDBConfig, MassaDBController, MassaIteratorMode};
    use massa_db_worker::MassaDB;
    use more_asserts::{assert_gt, assert_le};
    use parking_lot::RwLock;
    use std::ops::{Add, Sub};
    use std::sync::Arc;
    use tempfile::tempdir;

    use crate::test_helpers::versioning_helpers::advance_state_until;

    use massa_models::config::{MIP_STORE_STATS_BLOCK_CONSIDERED, T0, THREAD_COUNT};
    use massa_models::timeslots::get_closest_slot_to_timestamp;

    // Only for unit tests
    impl PartialEq<ComponentState> for MipState {
        fn eq(&self, other: &ComponentState) -> bool {
            self.state == *other
        }
    }

    // helper
    impl From<(&MipInfo, &Ratio<u64>, &MassaTime)> for Advance {
        fn from((mip_info, threshold, now): (&MipInfo, &Ratio<u64>, &MassaTime)) -> Self {
            Self {
                start_timestamp: mip_info.start,
                timeout: mip_info.timeout,
                threshold: *threshold,
                now: *now,
                activation_delay: mip_info.activation_delay,
            }
        }
    }

    fn get_a_version_info() -> (MassaTime, MassaTime, MipInfo) {
        // A helper function to provide a default MipInfo

        // Models a Massa Improvements Proposal (MIP-0002), transitioning component address to v2

        let start = MassaTime::from_utc_ymd_hms(2017, 11, 1, 7, 33, 44).unwrap();
        let timeout = MassaTime::from_utc_ymd_hms(2017, 11, 11, 7, 33, 44).unwrap();

        (
            start,
            timeout,
            MipInfo {
                name: "MIP-0002".to_string(),
                version: 2,
                components: BTreeMap::from([(MipComponent::Address, 1)]),
                start,
                timeout,
                activation_delay: MassaTime::from_millis(20),
            },
        )
    }

    #[test]
    fn test_state_advance_from_defined() {
        // Test Versioning state transition (from state: Defined)
        let (_, _, mi) = get_a_version_info();
        let mut state: ComponentState = Default::default();
        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_sub(MassaTime::from_millis(1));
        let mut advance_msg = Advance::from((&mi, &Ratio::zero(), &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_add(MassaTime::from_millis(5));
        advance_msg.now = now;
        state = state.on_advance(advance_msg);

        // println!("state: {:?}", state);
        assert_eq!(
            state,
            ComponentState::Started(Started {
                vote_ratio: Ratio::zero()
            })
        );
    }

    #[test]
    fn test_state_advance_from_started() {
        // Test Versioning state transition (from state: Started)
        let (_, _, mi) = get_a_version_info();
        let mut state: ComponentState = ComponentState::started(Default::default());

        let now = mi.start;
        let threshold_too_low =
            VERSIONING_THRESHOLD_TRANSITION_ACCEPTED.sub(Ratio::new_raw(10, 100));
        let threshold_ok = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED.add(Ratio::new_raw(1, 100));
        assert_le!(threshold_ok, Ratio::from_integer(1));
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

        let locked_in_at = mi.start.saturating_add(MassaTime::from_millis(1));
        let mut state: ComponentState = ComponentState::locked_in(locked_in_at);

        let now = mi.start;
        let mut advance_msg = Advance::from((&mi, &Ratio::zero(), &now));

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, ComponentState::locked_in(locked_in_at));

        advance_msg.now = advance_msg
            .timeout
            .saturating_add(MassaTime::from_millis(1));
        state = state.on_advance(advance_msg);
        assert!(matches!(state, ComponentState::Active(_)));
    }

    #[test]
    fn test_state_advance_from_active() {
        // Test Versioning state transition (from state: Active)
        let (start, _, mi) = get_a_version_info();
        let mut state = ComponentState::active(start);
        let now = mi.start;
        let advance = Advance::from((&mi, &Ratio::zero(), &now));

        state = state.on_advance(advance);
        assert!(matches!(state, ComponentState::Active(_)));
    }

    #[test]
    fn test_state_advance_from_failed() {
        // Test Versioning state transition (from state: Failed)
        let (_, _, mi) = get_a_version_info();
        let mut state = ComponentState::failed();
        let now = mi.start;
        let advance = Advance::from((&mi, &Ratio::zero(), &now));
        state = state.on_advance(advance);
        assert_eq!(state, ComponentState::failed());
    }

    #[test]
    fn test_state_advance_to_failed() {
        // Test Versioning state transition (to state: Failed)
        let (_, _, mi) = get_a_version_info();
        let now = mi.timeout.saturating_add(MassaTime::from_millis(1));
        let advance_msg = Advance::from((&mi, &Ratio::zero(), &now));

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
        let now_0 = start;
        let mut state = MipState::new(now_0);

        assert_eq!(state, ComponentState::defined());

        let now = mi.start.saturating_add(MassaTime::from_millis(15));
        let mut advance_msg = Advance::from((&mi, &Ratio::zero(), &now));

        // Move from Defined -> Started
        state.on_advance(&advance_msg);
        assert_eq!(state, ComponentState::started(Ratio::zero()));

        // Check history
        assert_eq!(state.history.len(), 2);
        assert!(matches!(
            state.history.first_key_value(),
            Some((&AdvanceLW { .. }, &ComponentStateTypeId::Defined))
        ));
        assert!(matches!(
            state.history.last_key_value(),
            Some((&AdvanceLW { .. }, &ComponentStateTypeId::Started))
        ));

        // Query with timestamp

        // Before Defined
        let state_id_ = state.state_at(
            mi.start.saturating_sub(MassaTime::from_millis(5)),
            mi.start,
            mi.timeout,
            mi.activation_delay,
        );
        assert!(matches!(
            state_id_,
            Err(StateAtError::BeforeInitialState(_, _))
        ));
        // After Defined timestamp
        let state_id = state
            .state_at(mi.start, mi.start, mi.timeout, mi.activation_delay)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Defined);
        // At Started timestamp
        let state_id = state
            .state_at(now, mi.start, mi.timeout, mi.activation_delay)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Started);

        // After Started timestamp but before timeout timestamp
        let after_started_ts = now.saturating_add(MassaTime::from_millis(15));
        let state_id_ = state.state_at(after_started_ts, mi.start, mi.timeout, mi.activation_delay);
        assert_eq!(state_id_, Err(StateAtError::Unpredictable));

        // After Started timestamp and after timeout timestamp
        let after_timeout_ts = mi.timeout.saturating_add(MassaTime::from_millis(15));
        let state_id = state
            .state_at(after_timeout_ts, mi.start, mi.timeout, mi.activation_delay)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Failed);

        // Move from Started to LockedIn
        let threshold = VERSIONING_THRESHOLD_TRANSITION_ACCEPTED;
        advance_msg.threshold = threshold.add(Ratio::from_integer(1));
        advance_msg.now = now.saturating_add(MassaTime::from_millis(1));
        state.on_advance(&advance_msg);
        assert_eq!(state, ComponentState::locked_in(advance_msg.now));

        // Query with timestamp
        // After LockedIn timestamp and before timeout timestamp
        let after_locked_in_ts = now.saturating_add(MassaTime::from_millis(10));
        let state_id = state
            .state_at(
                after_locked_in_ts,
                mi.start,
                mi.timeout,
                mi.activation_delay,
            )
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::LockedIn);
        // After LockedIn timestamp and after timeout timestamp
        let state_id = state
            .state_at(after_timeout_ts, mi.start, mi.timeout, mi.activation_delay)
            .unwrap();
        assert_eq!(state_id, ComponentStateTypeId::Active);
    }

    #[test]
    fn test_versioning_store_announce_current() {
        // Test VersioningInfo::get_version_to_announce() & ::get_version_current()

        let (start, timeout, mi) = get_a_version_info();

        let mut mi_2 = mi.clone();
        mi_2.version += 1;
        mi_2.start = timeout
            .checked_add(MassaTime::from_millis(1000 * 60 * 60 * 24 * 2))
            .unwrap(); // Add 2 days
        mi_2.timeout = timeout
            .checked_add(MassaTime::from_millis(1000 * 60 * 60 * 24 * 5))
            .unwrap(); // Add 5 days

        // Can only build such object in test - history is empty :-/
        let vs_1 = MipState {
            state: ComponentState::active(start),
            history: Default::default(),
        };
        let vs_2 = MipState {
            state: ComponentState::started(Ratio::zero()),
            history: Default::default(),
        };

        // TODO: Have VersioningStore::from ?
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let vs_raw = MipStoreRaw {
            store: BTreeMap::from([(mi.clone(), vs_1), (mi_2.clone(), vs_2)]),
            stats: MipStoreStats::new(mip_stats_cfg.clone()),
        };
        // let vs_raw = MipStoreRaw::try_from([(vi.clone(), vs_1), (vi_2.clone(), vs_2)]).unwrap();
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));

        assert_eq!(vs.get_network_version_current(), mi.version);
        assert_eq!(vs.get_network_version_to_announce(), Some(mi_2.version));

        // Test also an empty versioning store
        let vs_raw = MipStoreRaw {
            store: Default::default(),
            stats: MipStoreStats::new(mip_stats_cfg),
        };
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), None);
    }

    #[test]
    fn test_is_consistent_with() {
        // Test MipStateHistory::is_consistent_with (consistency of MIP state against its MIP info)

        // Given the following MIP info, we expect state
        // Defined @ time <= 2
        // Started @ time > 2 && <= 5
        // LockedIn @ time > time(Started) && <= 5
        // Active @time > 5
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        // Another versioning info (from an attacker) for testing
        let vi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(7),
            timeout: MassaTime::from_millis(10),
            activation_delay: MassaTime::from_millis(2),
        };

        let vsh = MipState {
            state: ComponentState::Error,
            history: Default::default(),
        };
        // At state Error -> (always) false
        assert_eq!(
            vsh.is_consistent_with(&vi_1),
            Err(IsConsistentError::AtError)
        );

        let vsh = MipState {
            state: ComponentState::defined(),
            history: Default::default(),
        };
        // At state Defined but no history -> false
        assert!(vsh.is_consistent_with(&vi_1).is_err());

        let mut vsh = MipState::new(MassaTime::from_millis(1));
        // At state Defined at time 1 -> true, given vi_1 @ time 1
        assert!(vsh.is_consistent_with(&vi_1).is_ok());
        // At state Defined at time 1 -> false given vi_1 @ time 3 (state should be Started)
        // assert_eq!(vsh.is_consistent_with(&vi_1, MassaTime::from_millis(3)), false);

        // Advance to Started
        let now = MassaTime::from_millis(3);
        let adv = Advance::from((&vi_1, &Ratio::zero(), &now));
        vsh.on_advance(&adv);
        let now = MassaTime::from_millis(4);
        let adv = Advance::from((&vi_1, &Ratio::new_raw(14, 100), &now));
        vsh.on_advance(&adv);

        // At state Started at time now -> true
        assert_eq!(vsh.state, ComponentState::started(Ratio::new_raw(14, 100)));
        assert!(vsh.is_consistent_with(&vi_1).is_ok());
        // Now with another versioning info
        assert!(vsh.is_consistent_with(&vi_2).is_err());

        // Advance to LockedIn
        let now = MassaTime::from_millis(4);
        let adv = Advance::from((&vi_1, &VERSIONING_THRESHOLD_TRANSITION_ACCEPTED, &now));
        vsh.on_advance(&adv);

        // At state LockedIn at time now -> true
        assert_eq!(vsh.state, ComponentState::locked_in(now));
        assert!(vsh.is_consistent_with(&vi_1).is_ok());

        // edge cases
        // TODO: history all good but does not start with Defined, start with Started
    }

    #[test]
    fn test_update_with() {
        // Test MipStoreRaw.update_with method (e.g. update a store from another, used in bootstrap)

        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };

        let _time = MassaTime::now();
        let vs_1 = advance_state_until(ComponentState::active(_time), &vi_1);
        assert!(matches!(vs_1.state, ComponentState::Active(_)));

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: BTreeMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from_millis(17),
            timeout: MassaTime::from_millis(27),
            activation_delay: MassaTime::from_millis(2),
        };
        let vs_2 = advance_state_until(ComponentState::defined(), &vi_2);

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mut vs_raw_1 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
            mip_stats_cfg.clone(),
        ))
        .unwrap();

        let vs_2_2 = advance_state_until(ComponentState::active(_time), &vi_2);
        assert!(matches!(vs_2_2.state, ComponentState::Active(_)));

        let vs_raw_2 = MipStoreRaw::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2_2.clone())],
            mip_stats_cfg,
        ))
        .unwrap();

        let (updated, added) = vs_raw_1.update_with(&vs_raw_2).unwrap();

        // Check update_with result
        assert!(added.is_empty());
        assert_eq!(updated, vec![vi_2.clone()]);

        // Expect state 1 (for vi_1) no change, state 2 (for vi_2) updated to "Active"
        assert_eq!(vs_raw_1.store.get(&vi_1).unwrap().state, vs_1.state);
        assert_eq!(vs_raw_1.store.get(&vi_2).unwrap().state, vs_2_2.state);
    }

    #[test]
    fn test_update_with_invalid() {
        // Test updating a MIP store with another invalid one

        // part 0 - defines data for the test
        let mi_1 = MipInfo {
            name: "MIP-0001".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        let _time = MassaTime::now();
        let ms_1 = advance_state_until(ComponentState::active(_time), &mi_1);
        assert!(matches!(ms_1.state, ComponentState::Active(_)));

        let mi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 3,
            components: BTreeMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from_millis(17),
            timeout: MassaTime::from_millis(27),
            activation_delay: MassaTime::from_millis(2),
        };
        let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
        assert_eq!(ms_2, ComponentState::defined());

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };

        // case 1: overlapping time range
        {
            let mut store_1 = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1.clone()), (mi_2.clone(), ms_2.clone())],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            let mut mi_2_2 = mi_2.clone();
            // Make mip info invalid (because start == mi_1.timeout)
            mi_2_2.start = mi_1.timeout;
            let ms_2_2 = advance_state_until(ComponentState::defined(), &mi_2_2);
            let store_2 = MipStoreRaw {
                store: BTreeMap::from([
                    (mi_1.clone(), ms_1.clone()),
                    (mi_2_2.clone(), ms_2_2.clone()),
                ]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            assert_matches!(
                store_1.update_with(&store_2),
                Err(UpdateWithError::Overlapping(..))
            );
            assert_eq!(store_1.store.get(&mi_1).unwrap().state, ms_1.state);
            assert_eq!(store_1.store.get(&mi_2).unwrap().state, ms_2.state);

            // Check that try_from fails too (because it uses update_with internally)
            {
                let _store_2_ = MipStoreRaw::try_from((
                    [
                        (mi_1.clone(), ms_1.clone()),
                        (mi_2_2.clone(), ms_2_2.clone()),
                    ],
                    mip_stats_cfg.clone(),
                ));
                assert!(_store_2_.is_err());
            }
        }

        // case 2: overlapping versioning component
        {
            let mut store_1 = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1.clone()), (mi_2.clone(), ms_2.clone())],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            let mut mi_2_2 = mi_2.clone();
            // Make MIP invalid (has component version set to 1 - same as MIP 1)
            mi_2_2.components = mi_1.components.clone();

            let ms_2_2 = advance_state_until(ComponentState::defined(), &mi_2_2);
            let store_2 = MipStoreRaw {
                store: BTreeMap::from([
                    (mi_1.clone(), ms_1.clone()),
                    (mi_2_2.clone(), ms_2_2.clone()),
                ]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            // MIP-0003 in vs_raw_1 & vs_raw_2 has != components
            assert_matches!(
                store_1.update_with(&store_2),
                Err(UpdateWithError::Overlapping(..))
            );
        }

        // case 3: trying to downgrade network version
        {
            let mut store_1 =
                MipStoreRaw::try_from(([(mi_1.clone(), ms_1.clone())], mip_stats_cfg.clone()))
                    .unwrap();
            let mut mi_2_2 = mi_2.clone();
            // Make MIP 2 invalid (MIP 2 network version < MIP 1 network version)
            mi_2_2.version = mi_1.version - 1;

            let store_2 = MipStoreRaw {
                store: BTreeMap::from([(mi_2_2.clone(), ms_2.clone())]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            assert_matches!(
                store_1.update_with(&store_2),
                Err(UpdateWithError::Overlapping(..))
            );

            // Test again but with == network versions

            let mut mi_2_3 = mi_2.clone();
            // Make MIP 2 invalid (MIP 2 network version == MIP 1 network version)
            mi_2_3.version = mi_1.version;

            let store_2 = MipStoreRaw {
                store: BTreeMap::from([(mi_2_3.clone(), ms_2.clone())]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            assert_matches!(
                store_1.update_with(&store_2),
                Err(UpdateWithError::Overlapping(..))
            );
        }

        // case 4: non unique name
        {
            let mut store_1 =
                MipStoreRaw::try_from(([(mi_1.clone(), ms_1.clone())], mip_stats_cfg.clone()))
                    .unwrap();
            let mut mi_2_2 = mi_2.clone();
            // Make MIP 2 invalid (MIP 2 name == MIP 1 name)
            mi_2_2.name = mi_1.name.clone();

            let store_2 = MipStoreRaw {
                store: BTreeMap::from([(mi_2_2.clone(), ms_2.clone())]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            assert_matches!(
                store_1.update_with(&store_2),
                Err(UpdateWithError::Overlapping(..))
            );
        }

        // case 5: trying to downgrade state
        {
            let ms_1_1 =
                advance_state_until(ComponentState::locked_in(MassaTime::from_millis(0)), &mi_1);
            let ms_1_2 = advance_state_until(ComponentState::started(Ratio::zero()), &mi_1);
            let mut store_1 =
                MipStoreRaw::try_from(([(mi_1.clone(), ms_1_1.clone())], mip_stats_cfg.clone()))
                    .unwrap();

            // Try to downgrade from 'LockedIn' to 'Started'
            let store_2 = MipStoreRaw {
                store: BTreeMap::from([(mi_1.clone(), ms_1_2.clone())]),
                stats: MipStoreStats::new(mip_stats_cfg.clone()),
            };

            assert_matches!(
                store_1.update_with(&store_2),
                Err(UpdateWithError::Downgrade(..))
            );
        }
    }

    #[test]
    fn test_try_from_invalid() {
        // Test create a MIP store with invalid MIP info

        // part 0 - defines data for the test
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        let _time = MassaTime::now();
        let ms_1 = advance_state_until(ComponentState::active(_time), &mi_1);
        assert!(matches!(ms_1.state, ComponentState::Active(_)));
        {
            // make mi_1_1 invalid: start is after timeout
            let mut mi_1_1 = mi_1.clone();
            mi_1_1.start = MassaTime::from_millis(5);
            mi_1_1.timeout = MassaTime::from_millis(2);

            let mip_store =
                MipStoreRaw::try_from(([(mi_1_1, ms_1.clone())], mip_stats_cfg.clone()));
            assert_matches!(mip_store, Err(UpdateWithError::NonConsistent(..)));
        }
        {
            let ms_1_2 = MipState::new(MassaTime::from_millis(15));
            // make mi_1_1 invalid: start is after timeout
            let mut mi_1_2 = mi_1.clone();
            mi_1_2.start = MassaTime::from_millis(2);
            mi_1_2.timeout = MassaTime::from_millis(5);

            let mip_store = MipStoreRaw::try_from(([(mi_1_2, ms_1_2)], mip_stats_cfg.clone()));
            assert_matches!(mip_store, Err(UpdateWithError::NonConsistent(..)));
        }
        {
            let ms_1_2 = MipState::new(MassaTime::from_millis(15));
            // make mi_1_1 invalid: start is after timeout
            let mut mi_1_2 = mi_1.clone();
            mi_1_2.start = MassaTime::from_millis(16);
            mi_1_2.timeout = MassaTime::from_millis(5);

            let mip_store = MipStoreRaw::try_from(([(mi_1_2, ms_1_2)], mip_stats_cfg));
            assert_matches!(mip_store, Err(UpdateWithError::NonConsistent(..)));
        }
    }

    #[test]
    fn test_empty_mip_store() {
        // Test if we can init an empty MipStore

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };

        let mip_store = MipStore::try_from(([], mip_stats_config));
        assert!(mip_store.is_ok());
    }

    #[test]
    fn test_update_with_unknown() {
        // Test update_with with unknown MipComponent (can happen if a node software is outdated)

        // data
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };

        let mut mip_store_raw_1 = MipStoreRaw::try_from(([], mip_stats_config.clone())).unwrap();

        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::__Nonexhaustive, 1)]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };
        let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);
        assert_eq!(ms_1, ComponentState::defined());
        let mip_store_raw_2 = MipStoreRaw {
            store: BTreeMap::from([(mi_1.clone(), ms_1.clone())]),
            stats: MipStoreStats::new(mip_stats_config.clone()),
        };

        let (updated, added) = mip_store_raw_1.update_with(&mip_store_raw_2).unwrap();

        assert_eq!(updated.len(), 0);
        assert_eq!(added.len(), 1);
        assert_eq!(added.get(&mi_1).unwrap().state, ComponentState::defined());
    }

    #[test]
    fn test_mip_store_network_restart() {
        // Test if we can get a consistent MipStore after a network shutdown

        let genesis_timestamp = MassaTime::from_millis(0);

        // helper functions so the test code is easy to read
        let get_slot_ts =
            |slot| get_block_slot_timestamp(THREAD_COUNT, T0, genesis_timestamp, slot).unwrap();
        let is_consistent = |store: &MipStoreRaw, shutdown_start, shutdown_end| {
            store.is_consistent_with_shutdown_period(
                shutdown_start,
                shutdown_end,
                THREAD_COUNT,
                T0,
                genesis_timestamp,
            )
        };
        let update_store = |store: &mut MipStoreRaw, shutdown_start, shutdown_end| {
            store
                .update_for_network_shutdown(
                    shutdown_start,
                    shutdown_end,
                    THREAD_COUNT,
                    T0,
                    genesis_timestamp,
                )
                .unwrap()
        };
        let _dump_store = |store: &MipStoreRaw| {
            println!("Dump store:");
            for (mip_info, mip_state) in store.store.iter() {
                println!(
                    "mip_info {} {} - start: {} - timeout: {}: state: {:?}",
                    mip_info.name,
                    mip_info.version,
                    get_closest_slot_to_timestamp(
                        THREAD_COUNT,
                        T0,
                        genesis_timestamp,
                        mip_info.start
                    ),
                    get_closest_slot_to_timestamp(
                        THREAD_COUNT,
                        T0,
                        genesis_timestamp,
                        mip_info.timeout
                    ),
                    mip_state.state
                );
            }
        };
        // end helpers

        let shutdown_start = Slot::new(2, 0);
        let shutdown_end = Slot::new(8, 0);

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mut mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(100),
        };
        let mut mi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: BTreeMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from_millis(7),
            timeout: MassaTime::from_millis(11),
            activation_delay: MassaTime::from_millis(100),
        };

        // MipInfo 1 @ state 'Defined' should start during shutdown
        {
            mi_1.start = get_slot_ts(Slot::new(3, 7));
            mi_1.timeout = get_slot_ts(Slot::new(5, 7));
            mi_2.start = get_slot_ts(Slot::new(7, 7));
            mi_2.timeout = get_slot_ts(Slot::new(10, 7));

            let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            match is_consistent(&store, shutdown_start, shutdown_end) {
                Err(IsConsistentWithShutdownPeriodError::NonConsistent(mi, ..)) => {
                    assert_eq!(mi, mi_1);
                }
                _ => panic!("is_consistent expects a non consistent error"),
            }

            update_store(&mut store, shutdown_start, shutdown_end);
            assert!(is_consistent(&store, shutdown_start, shutdown_end).is_ok());
            // _dump_store(&store);
        }

        // MipInfo 1 @ state 'Defined' will start AFTER shutdown
        {
            mi_1.start = get_slot_ts(Slot::new(9, 7));
            mi_1.timeout = get_slot_ts(Slot::new(11, 7));
            mi_2.start = get_slot_ts(Slot::new(12, 7));
            mi_2.timeout = get_slot_ts(Slot::new(19, 7));

            let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();
            let store_orig = store.clone();

            // Already ok even with a shutdown but let's check it
            assert!(is_consistent(&store, shutdown_start, shutdown_end).is_ok());
            // _dump_store(&store);
            update_store(&mut store, shutdown_start, shutdown_end);
            assert!(is_consistent(&store, shutdown_start, shutdown_end).is_ok());
            // _dump_store(&store);

            // Check that nothing has changed
            assert_eq!(store_orig, store);
        }

        // MipInfo 1 @ state 'Started' before shutdown
        {
            mi_1.start = get_slot_ts(Slot::new(1, 7));
            mi_1.timeout = get_slot_ts(Slot::new(5, 7));
            mi_2.start = get_slot_ts(Slot::new(7, 7));
            mi_2.timeout = get_slot_ts(Slot::new(10, 7));

            let ms_1 = advance_state_until(ComponentState::started(Ratio::zero()), &mi_1);
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            assert_matches!(
                is_consistent(&store, shutdown_start, shutdown_end),
                Err(IsConsistentWithShutdownPeriodError::NonConsistent(..))
            );
            update_store(&mut store, shutdown_start, shutdown_end);
            assert!(is_consistent(&store, shutdown_start, shutdown_end).is_ok());
            // _dump_store(&store);
        }

        // MipInfo 1 @ state 'LockedIn' with transition during shutdown
        {
            let shutdown_range = shutdown_start..=shutdown_end;

            mi_1.start = get_slot_ts(Slot::new(1, 7));
            mi_1.timeout = get_slot_ts(Slot::new(5, 7));

            // Just before shutdown
            let locked_in_at = Slot::new(1, 9);
            assert!(locked_in_at < shutdown_start);
            let activate_at = Slot::new(4, 0);
            assert!(shutdown_range.contains(&activate_at));
            // MIP 1 in state 'LockedIn', should transition to 'Active' during shutdown period
            mi_1.activation_delay =
                get_slot_ts(activate_at).saturating_sub(get_slot_ts(locked_in_at));
            let ms_1 = advance_state_until(
                ComponentState::locked_in(get_slot_ts(Slot::new(1, 9))),
                &mi_1,
            );

            // MIP 2 in state 'Defined'
            mi_2.start = get_slot_ts(Slot::new(7, 7));
            mi_2.timeout = get_slot_ts(Slot::new(10, 7));
            let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);
            let mut store = MipStoreRaw::try_from((
                [(mi_1.clone(), ms_1), (mi_2.clone(), ms_2)],
                mip_stats_cfg.clone(),
            ))
            .unwrap();

            match is_consistent(&store, shutdown_start, shutdown_end) {
                Err(IsConsistentWithShutdownPeriodError::NonConsistent(mi, ..)) => {
                    assert_eq!(mi, mi_1);
                }
                _ => panic!("is_consistent expects a non consistent error"),
            }
            // _dump_store(&store);
            update_store(&mut store, shutdown_start, shutdown_end);
            assert!(is_consistent(&store, shutdown_start, shutdown_end).is_ok());
            // _dump_store(&store);

            // Update stats - so should force transitions if any
            store.update_network_version_stats(
                get_slot_ts(shutdown_end.get_next_slot(THREAD_COUNT).unwrap()),
                Some((1, None)),
            );

            let (first_mi_info, first_mi_state) = store.store.first_key_value().unwrap();
            assert_eq!(*first_mi_info.name, mi_1.name);
            // State was 'LockedIn' -> reset, start ts now defined right after network restart
            assert_eq!(
                ComponentStateTypeId::from(&first_mi_state.state),
                ComponentStateTypeId::Started
            );
            let (last_mi_info, last_mi_state) = store.store.last_key_value().unwrap();
            assert_eq!(*last_mi_info.name, mi_2.name);
            // State was 'Defined' -> start is set up after MIP 1 start & timeout
            assert_eq!(
                ComponentStateTypeId::from(&last_mi_state.state),
                ComponentStateTypeId::Defined
            );
        }
    }

    #[test]
    fn test_mip_store_db() {
        // Test interaction of MIP store with MassaDB
        // 1- init from db (empty disk)
        // 2- update state
        // 3- write changes to db
        // 4- init a new mip store from disk and compare
        // 5- test is_key_value_valid method

        let genesis_timestamp = MassaTime::from_millis(0);
        // helpers
        let get_slot_ts =
            |slot| get_block_slot_timestamp(THREAD_COUNT, T0, genesis_timestamp, slot).unwrap();

        // Db init

        let temp_dir = tempdir().expect("Unable to create a temp folder");
        // println!("Using temp dir: {:?}", temp_dir.path());

        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100_000,
            max_versioning_elements_size: 100_000,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));

        // MIP info / store init

        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: get_slot_ts(Slot::new(2, 0)),
            timeout: get_slot_ts(Slot::new(3, 0)),
            activation_delay: MassaTime::from_millis(10),
        };
        let ms_1 = advance_state_until(ComponentState::defined(), &mi_1);

        let mi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: BTreeMap::from([(MipComponent::Address, 2)]),
            start: get_slot_ts(Slot::new(4, 2)),
            timeout: get_slot_ts(Slot::new(7, 2)),
            activation_delay: MassaTime::from_millis(10),
        };
        let ms_2 = advance_state_until(ComponentState::defined(), &mi_2);

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mut mip_store = MipStore::try_from((
            [(mi_1.clone(), ms_1.clone()), (mi_2.clone(), ms_2.clone())],
            mip_stats_config.clone(),
        ))
        .expect("Cannot create an empty MIP store");

        // Step 1

        mip_store.extend_from_db(db.clone()).unwrap();
        // Check that we extend from an empty folder
        assert_eq!(mip_store.0.read().store.len(), 2);
        assert_eq!(
            mip_store.0.read().store.first_key_value(),
            Some((&mi_1, &ms_1))
        );
        assert_eq!(
            mip_store.0.read().store.last_key_value(),
            Some((&mi_2, &ms_2))
        );

        // Step 2
        let active_at = get_slot_ts(Slot::new(2, 5));
        let ms_1_ = advance_state_until(ComponentState::active(active_at), &mi_1);

        let mip_store_ =
            MipStore::try_from(([(mi_1.clone(), ms_1_.clone())], mip_stats_config.clone()))
                .expect("Cannot create an empty MIP store");

        let (updated, added) = mip_store.update_with(&mip_store_).unwrap();

        // Check update_with result - only 1 state should be updated
        assert_eq!(updated.len(), 1);
        assert_eq!(added.len(), 0);
        assert_eq!(mip_store.0.read().store.len(), 2);
        assert_eq!(
            mip_store.0.read().store.first_key_value(),
            Some((&mi_1, &ms_1_))
        );
        assert_eq!(
            mip_store.0.read().store.last_key_value(),
            Some((&mi_2, &ms_2))
        );

        // Step 3

        let mut db_batch = DBBatch::new();
        let mut db_versioning_batch = DBBatch::new();

        // FIXME: get slot right after active at - no hardcode
        let slot_bounds_ = (&Slot::new(1, 0), &Slot::new(4, 2));
        let between = (&get_slot_ts(*slot_bounds_.0), &get_slot_ts(*slot_bounds_.1));

        mip_store
            .update_batches(&mut db_batch, &mut db_versioning_batch, Some(between))
            .unwrap();

        assert_eq!(db_batch.len(), 1); // mi_1
        assert_eq!(db_versioning_batch.len(), 3); // mi_2 + mi_1 removal + stats

        let mut guard_db = db.write();
        // FIXME / TODO: no slot hardcoding?
        guard_db.write_batch(db_batch, db_versioning_batch, Some(Slot::new(3, 0)));
        drop(guard_db);

        // Step 4
        let mut mip_store_2 = MipStore::try_from((
            [(mi_1.clone(), ms_1.clone()), (mi_2.clone(), ms_2.clone())],
            mip_stats_config.clone(),
        ))
        .expect("Cannot create an empty MIP store");
        // assert_eq!(mip_store_2.0.read().store.len(), 0);

        mip_store_2.extend_from_db(db.clone()).unwrap();

        let guard_1 = mip_store.0.read();
        let guard_2 = mip_store_2.0.read();
        let st1_raw = guard_1.deref();
        let st2_raw = guard_2.deref();

        // println!("st1_raw: {:?}", st1_raw);
        // println!("st2_raw: {:?}", st2_raw);
        assert_eq!(st1_raw, st2_raw);

        // Step 5: test is_key_value_valid
        let mut count = 0;
        for (ser_key, ser_value) in db.read().iterator_cf(STATE_CF, MassaIteratorMode::Start) {
            assert!(mip_store.is_key_value_valid(&ser_key, &ser_value));
            count += 1;
        }
        assert_gt!(count, 0);

        let mut count2 = 0;
        for (ser_key, ser_value) in db
            .read()
            .iterator_cf(VERSIONING_CF, MassaIteratorMode::Start)
        {
            assert!(mip_store.is_key_value_valid(&ser_key, &ser_value));
            count2 += 1;
        }
        assert_gt!(count2, 0);
    }

    #[test]
    fn test_mip_store_stats() {
        // Test MipStoreRaw stats

        // helper functions so the test code is easy to read
        let genesis_timestamp = MassaTime::from_millis(0);
        let get_slot_ts =
            |slot| get_block_slot_timestamp(THREAD_COUNT, T0, genesis_timestamp, slot).unwrap();

        let mip_stats_config = MipStatsConfig {
            block_count_considered: 2,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let activation_delay = MassaTime::from_millis(100);
        let timeout = MassaTime::now().saturating_add(MassaTime::from_millis(50_000)); // + 50 seconds
        let mi_1 = MipInfo {
            name: "MIP-0001".to_string(),
            version: 1,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout,
            activation_delay,
        };
        let ms_1 = advance_state_until(ComponentState::started(Ratio::zero()), &mi_1);

        let mut mip_store =
            MipStoreRaw::try_from(([(mi_1.clone(), ms_1)], mip_stats_config)).unwrap();

        // Current network version is 0, next one is 1
        mip_store.update_network_version_stats(get_slot_ts(Slot::new(1, 0)), Some((0, Some(1))));
        assert_eq!(mip_store.stats.network_version_counters.len(), 1);
        assert_eq!(mip_store.stats.network_version_counters.get(&1), Some(&1));

        mip_store.update_network_version_stats(get_slot_ts(Slot::new(1, 0)), Some((0, Some(1))));
        assert_eq!(mip_store.stats.network_version_counters.len(), 1);
        assert_eq!(mip_store.stats.network_version_counters.get(&1), Some(&2));

        // Check that MipInfo is now
        let (mi_, ms_) = mip_store.store.last_key_value().unwrap();
        assert_eq!(*mi_, mi_1);
        assert_matches!(ms_.state, ComponentState::LockedIn(..));

        let mut at = MassaTime::now();
        at = at.saturating_add(activation_delay);
        assert_eq!(
            ms_.state_at(at, mi_1.start, mi_1.timeout, mi_1.activation_delay),
            Ok(ComponentStateTypeId::Active)
        );

        // Now network version is 1, next one is 2
        mip_store.update_network_version_stats(get_slot_ts(Slot::new(1, 0)), Some((1, Some(2))));
        // Counter for announced version: 1 & 2
        assert_eq!(mip_store.stats.network_version_counters.len(), 2);
        // First announced version 1 was removed and so the counter decremented
        assert_eq!(mip_store.stats.network_version_counters.get(&1), Some(&1));
        assert_eq!(mip_store.stats.network_version_counters.get(&2), Some(&1));
    }
}
