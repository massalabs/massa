use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::mem;
use std::ops::Bound::{Excluded, Included};
use std::str::FromStr;
use std::sync::Arc;

use machine::{machine, transitions};
use nom::multi::length_count;
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use parking_lot::RwLock;
#[cfg(test)]
use thiserror::Error;

use crate::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};

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
#[derive(Clone, Debug, Hash)]
pub struct VersioningInfo {
    /// brief description of the versioning
    pub name: String,
    /// version
    pub version: u32,
    /// Component concerned by this versioning (e.g. a new Block version)
    pub component: VersioningComponent,
    /// a timestamp at which the version gains its meaning (e.g. accepted in block header)
    pub start: u64,
    /// a timestamp at which the deployment is considered failed (timeout > start)
    pub timeout: u64,
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

impl VersioningState {
    /// Check if the state is coherent with the Versioning info time range
    fn is_coherent_with(&self, v_info: &VersioningInfo, now: u64) -> bool {
        // All we need is trying to update the state and check if it has changed

        let t = match self {
            VersioningState::Started(Started { threshold }) => threshold.clone(),
            _ => Amount::zero(),
        };

        let advance_msg = Advance {
            start_timestamp: v_info.start,
            timeout: v_info.timeout,
            threshold: t,
            now,
        };

        let new_state = self.clone();
        let new_state = new_state.on_advance(advance_msg);
        new_state == *self
    }
}

impl From<&str> for VersioningState {
    fn from(state: &str) -> Self {
        match state {
            "Defined" => VersioningState::Defined(Defined::new()),
            "Started" => {
                let amount = Amount::zero();
                VersioningState::Started(Started::new(amount))
            }
            "LockedIn" => VersioningState::LockedIn(LockedIn::new()),
            "Active" => VersioningState::Active(Active::new()),
            "Failed" => VersioningState::Failed(Failed::new()),
            _ => panic!("Unknown variant ofr VersioningState: {}", state),
        }
    }
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
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

const THRESHOLD_TRANSITION_ACCEPTED: &str = "75.0";

/// A message to update the `VersioningState`
#[derive(Clone, Debug, PartialEq)]
pub struct Advance {
    /// from VersioningInfo.start
    pub start_timestamp: u64,
    /// from VersioningInfo.timeout
    pub timeout: u64,
    /// % of past blocks with this version
    pub threshold: Amount,
    /// Current time (timestamp)
    pub now: u64,
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
    ///
    pub fn new() -> Self {
        Self {}
    }

    /// Update state from state Defined
    pub fn on_advance(self, input: Advance) -> VersioningState {
        match input.now {
            n if n > input.timeout => VersioningState::Failed(Failed {}),
            n if n > input.start_timestamp => VersioningState::Started(Started {
                threshold: Amount::zero(),
            }),
            _ => VersioningState::Defined(Defined {}),
        }
    }
}

impl Started {
    ///
    pub fn new(threshold: Amount) -> Self {
        Self { threshold }
    }

    /// Update state from state Started
    pub fn on_advance(self, input: Advance) -> VersioningState {
        if input.now > input.timeout {
            return VersioningState::Failed(Failed {});
        }

        if input.threshold > Amount::from_str(THRESHOLD_TRANSITION_ACCEPTED).unwrap() {
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
        return Self {
            threshold: Amount::zero(),
        };
    }
}

impl LockedIn {
    ///
    pub fn new() -> Self {
        Self {}
    }

    /// Update state from state LockedIn ...
    pub fn on_advance(self, input: Advance) -> VersioningState {
        if input.now > input.timeout {
            VersioningState::Active(Active {})
        } else {
            VersioningState::LockedIn(LockedIn {})
        }
    }
}

impl Active {
    ///
    pub fn new() -> Self {
        Self {}
    }
    /// Update state (will always stay in state Active)
    pub fn on_advance(self, _input: Advance) -> Active {
        Active {}
    }
}

impl Failed {
    ///
    pub fn new() -> Self {
        Self {}
    }

    /// Update state (will always stay in state Failed)
    pub fn on_advance(self, _input: Advance) -> Failed {
        Failed {}
    }
}

// Let's define it if needed

/// Database for all versioning info
#[derive(Debug, Clone)]
pub struct VersioningStore(pub Arc<RwLock<VersioningStoreRaw>>);

impl Default for VersioningStore {
    fn default() -> Self {
        Self {
            0: Arc::new(RwLock::new(Default::default())),
        }
    }
}

impl VersioningStore {
    /// Used to check if a received block header is valid or not
    /// The given timestamp is the slot time of the block
    /// It is valid if the version is the
    pub fn get_active_version_at_timestamp(&self, timestamp: u64) -> u32 {
        let store = self.0.read().data.clone();

        // We filter the versions that were not active back in timestamp.
        let mut all_active_versions: Vec<_> = store
            .iter()
            .filter(|&(k, v)| {
                v == &VersioningState::Active(Active::new()) && k.timeout <= timestamp
            })
            .map(|(k, _v)| k)
            .collect();

        // Check the sort here
        // Currently, we consider that the most active is the last "timeout date", but there may be edge cases?
        all_active_versions.sort_by_key(|&k| k.timeout);
        match all_active_versions.len() {
            0 => 0,
            n => all_active_versions[n - 1].version,
        }
    }

    ///
    pub fn get_current_active_version(&self) -> u32 {
        let store = (*self.0.read()).data.clone();

        let mut all_active_versions: Vec<_> = store
            .iter()
            .filter(|&(_k, v)| v == &VersioningState::Active(Active::new()))
            .map(|(k, _v)| k)
            .collect();

        // Check the sort here
        // Currently, we consider that the most active is the last "timeout date", but there may be edge cases?
        all_active_versions.sort_by_key(|&k| k.timeout);
        match all_active_versions.len() {
            0 => 0,
            n => all_active_versions[n - 1].version,
        }
    }

    ///
    pub fn get_current_version_to_announce(&self) -> u32 {
        let store = (*self.0.read()).data.clone();

        // Check the filter and the sort here
        let mut filtered_versions: Vec<_> = store
            .iter()
            .filter(|&(_k, v)| {
                matches!(
                    v,
                    &VersioningState::Started(_)
                        | &VersioningState::LockedIn(_)
                        | &VersioningState::Active(_)
                )
            })
            .map(|(k, _v)| k)
            .collect();
        filtered_versions.sort_by_key(|&k| k.start);
        match filtered_versions.len() {
            0 => 0,
            _ => filtered_versions[0].version,
        }
    }
}

/// Store of all versioning info
#[derive(Debug, Clone, PartialEq)]
pub struct VersioningStoreRaw {
    // TODO: no need to name field?
    ///
    pub data: BTreeMap<VersioningInfo, VersioningState>,
}

impl VersioningStoreRaw {
    /// Merge our store with another (usually after a bootstrap where we received another store)
    fn merge_with(&mut self, store_raw: &VersioningStoreRaw, now: u64) {
        for (v_info, v_state) in store_raw.data.iter() {
            // Check if state is ok according to versioning info time range
            if !v_state.is_coherent_with(v_info, now) {
                continue;
            };

            if let Some(v_state_cur) = self.data.get_mut(v_info) {
                // Versioning info is already in store
                // Need to check state before merging
                let v_state_id: u32 = VersioningStateTypeId::from(v_state).into();
                let v_state_cur_ref: &VersioningState = v_state_cur;
                let v_state_cur_id: u32 = VersioningStateTypeId::from(v_state_cur_ref).into();

                // TODO: should update if started and threshold is >
                match v_state_cur {
                    VersioningState::Defined(_)
                    | VersioningState::Started(_)
                    | VersioningState::LockedIn(_) => {
                        // Only accept 'higher' state (e.g. started if defined, lockedin if started...)
                        if v_state_id > v_state_cur_id {
                            *v_state_cur = (*v_state).clone();
                        }
                    }
                    _ => {
                        // Nothing to do for already Active / Failed / Error
                    }
                }
            } else {
                // Versioning info not in store - usually a new one

                // Check if name & version are uniques
                if !self
                    .data
                    .iter()
                    .all(|(i, _)| i.version != v_info.version && i.name != v_info.name)
                {
                    continue;
                }

                // Should we also check for version >
                // Check for time range
                if let Some((last_vi, _)) = self.data.last_key_value() {
                    // TODO: should we add a min duration from start + timeout
                    if v_info.start > last_vi.timeout && v_info.timeout > v_info.start {
                        // Time range is ok, let's add it
                        self.data.insert(v_info.clone(), v_state.clone());
                    }
                } else {
                    self.data.insert(v_info.clone(), v_state.clone());
                }
            }
        }
    }
}

impl Default for VersioningStoreRaw {
    fn default() -> Self {
        Self {
            data: Default::default(),
        }
    }
}

/// Ser / Der

const VERSIONING_INFO_NAME_LEN_MAX: u32 = 255;
const VERSIONING_STATE_VARIANT_COUNT: u32 = mem::variant_count::<VersioningState>() as u32;
const VERSIONING_STORE_ENTRIES_MAX: u32 = 2048;

/// Serializer for `VersioningInfo`
#[derive(Clone)]
pub struct VersioningInfoSerializer {
    u32_serializer: U32VarIntSerializer,
    amount_serializer: AmountSerializer, // start / timeout
}

impl VersioningInfoSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        VersioningInfoSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Default for VersioningInfoSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<VersioningInfo> for VersioningInfoSerializer {
    fn serialize(
        &self,
        value: &VersioningInfo,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // name
        let name_len_ = value.name.len();
        if name_len_ > VERSIONING_INFO_NAME_LEN_MAX as usize {
            return Err(SerializeError::StringTooBig(format!(
                "Versioning info name len is {}, max: {}",
                name_len_, VERSIONING_INFO_NAME_LEN_MAX
            )));
        }
        let name_len = u32::try_from(name_len_).map_err(|_| {
            SerializeError::GeneralError(format!(
                "Cannot convert to name_len: {} to u64",
                name_len_
            ))
        })?;
        self.u32_serializer.serialize(&name_len, buffer)?;
        buffer.extend(value.name.as_bytes());
        // version
        self.u32_serializer.serialize(&value.version, buffer)?;
        // component
        let component = match &value.component {
            VersioningComponent::Address => u32::from(VersioningComponent::Address),
            VersioningComponent::Block => u32::from(VersioningComponent::Block),
            VersioningComponent::VM => u32::from(VersioningComponent::VM),
        };
        self.u32_serializer.serialize(&component, buffer)?;
        // start
        let amount = Amount::from_raw(value.start);
        self.amount_serializer.serialize(&amount, buffer)?;
        // timeout
        let amount = Amount::from_raw(value.timeout);
        self.amount_serializer.serialize(&amount, buffer)?;
        Ok(())
    }
}

/// Deserializer for VersioningInfo
pub struct VersioningInfoDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    len_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl VersioningInfoDeserializer {
    /// Creates a new `VersioningInfoDeserializer`
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(u32::MAX)),
            len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(VERSIONING_INFO_NAME_LEN_MAX),
            ),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Deserializer<VersioningInfo> for VersioningInfoDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], VersioningInfo, E> {
        context(
            "Failed VersioningInfo deserialization",
            tuple((
                context("Failed name deserialization", |input| {
                    let (input_, len_) = self.len_deserializer.deserialize(input)?;
                    // Safe to unwrap as it returns Result<usize, Infallible>
                    let len = usize::try_from(len_).unwrap();
                    let slice = &input_[..len];
                    let name = String::from_utf8(slice.to_vec()).map_err(|_| {
                        nom::Err::Error(ParseError::from_error_kind(
                            input_,
                            nom::error::ErrorKind::Fail,
                        ))
                    })?;
                    IResult::Ok((&input_[len..], name))
                }),
                context("Failed version deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed component deserialization", |input| {
                    let (rem, component_) = self.u32_deserializer.deserialize(input)?;
                    let component = VersioningComponent::try_from(component_).map_err(|_| {
                        nom::Err::Error(ParseError::from_error_kind(
                            input,
                            nom::error::ErrorKind::Fail,
                        ))
                    })?;
                    IResult::Ok((rem, component))
                }),
                context("Failed start deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed timeout deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(name, version, component, start, timeout)| VersioningInfo {
                name,
                version,
                component,
                start: start.to_raw(),
                timeout: timeout.to_raw(),
            },
        )
        .parse(buffer)
    }
}

/// Serializer for `VersioningState`
#[derive(Clone)]
pub struct VersioningStateSerializer {
    u32_serializer: U32VarIntSerializer,
    amount_serializer: AmountSerializer,
}

impl VersioningStateSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        VersioningStateSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Default for VersioningStateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<VersioningState> for VersioningStateSerializer {
    fn serialize(
        &self,
        value: &VersioningState,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let (state, threshold_): (u32, Option<Amount>) = match value {
            VersioningState::Error => (u32::from(VersioningStateTypeId::Error), None),
            VersioningState::Defined(_) => (u32::from(VersioningStateTypeId::Defined), None),
            VersioningState::Started(Started { threshold }) => {
                (u32::from(VersioningStateTypeId::Started), Some(*threshold))
            }
            VersioningState::LockedIn(_) => (u32::from(VersioningStateTypeId::LockedIn), None),
            VersioningState::Active(_) => (u32::from(VersioningStateTypeId::Active), None),
            VersioningState::Failed(_) => (u32::from(VersioningStateTypeId::Failed), None),
        };
        self.u32_serializer.serialize(&state, buffer)?;
        if let Some(threshold) = threshold_ {
            self.amount_serializer.serialize(&threshold, buffer)?;
        }
        Ok(())
    }
}

/// A Deserializer for VersioningState`
pub struct VersioningStateDeserializer {
    state_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl VersioningStateDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            state_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(VERSIONING_STATE_VARIANT_COUNT + 1),
            ),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Deserializer<VersioningState> for VersioningStateDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], VersioningState, E> {
        let (rem, enum_value_) = context("Failed enum value der", |input| {
            self.state_deserializer.deserialize(input)
        })
        .parse(buffer)?;

        let enum_value = VersioningStateTypeId::try_from(enum_value_).map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Eof,
            ))
        })?;
        let (rem2, state): (&[u8], VersioningState) = match enum_value {
            VersioningStateTypeId::Defined => (rem, VersioningState::Defined(Defined::new())),
            VersioningStateTypeId::Started => {
                let (rem2, threshold) = context("Failed threshold value der", |input| {
                    self.amount_deserializer.deserialize(input)
                })
                .parse(rem)?;
                (rem2, VersioningState::Started(Started::new(threshold)))
            }
            VersioningStateTypeId::LockedIn => (rem, VersioningState::LockedIn(LockedIn::new())),
            VersioningStateTypeId::Active => (rem, VersioningState::Active(Active::new())),
            VersioningStateTypeId::Failed => (rem, VersioningState::Failed(Failed::new())),
            _ => (rem, VersioningState::Error),
        };

        IResult::Ok((rem2, state))
    }
}

/// Serializer for `VersioningStoreRaw`
#[derive(Clone)]
pub struct VersioningStoreRawSerializer {
    u32_serializer: U32VarIntSerializer,
    info_serializer: VersioningInfoSerializer,
    state_serializer: VersioningStateSerializer,
}

impl VersioningStoreRawSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        VersioningStoreRawSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            info_serializer: VersioningInfoSerializer::new(),
            state_serializer: VersioningStateSerializer::new(),
        }
    }
}

impl Default for VersioningStoreRawSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<VersioningStoreRaw> for VersioningStoreRawSerializer {
    fn serialize(
        &self,
        value: &VersioningStoreRaw,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let entry_count_ = value.data.len();
        let entry_count = u32::try_from(entry_count_).map_err(|e| {
            SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
        })?;
        if entry_count > VERSIONING_STORE_ENTRIES_MAX {
            return Err(SerializeError::GeneralError(format!(
                "Too many entries in VersioningStoreRaw, max: {}",
                VERSIONING_STORE_ENTRIES_MAX
            )));
        }
        self.u32_serializer.serialize(&entry_count, buffer)?;
        for (key, value) in value.data.iter() {
            self.info_serializer.serialize(key, buffer)?;
            self.state_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// A Deserializer for `VersioningStoreRaw
pub struct VersioningStoreRawDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    info_deserializer: VersioningInfoDeserializer,
    state_deserializer: VersioningStateDeserializer,
}

impl VersioningStoreRawDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(VERSIONING_STORE_ENTRIES_MAX),
            ),
            info_deserializer: VersioningInfoDeserializer::new(),
            state_deserializer: VersioningStateDeserializer::new(),
        }
    }
}

impl Deserializer<VersioningStoreRaw> for VersioningStoreRawDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], VersioningStoreRaw, E> {
        context(
            "Failed VersioningStoreRaw len der",
            length_count(
                context("Failed len der", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed items der", |input| {
                    let (rem, vi) = self.info_deserializer.deserialize(input)?;
                    let (rem2, vs) = self.state_deserializer.deserialize(rem)?;
                    IResult::Ok((rem2, (vi, vs)))
                }),
            ),
        )
        .map(|items| VersioningStoreRaw {
            data: items.into_iter().collect(),
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime};
    use massa_serialization::DeserializeError;

    fn get_default_version_info() -> VersioningInfo {
        // A default VersioningInfo used in many tests
        // Models a Massa Improvements Proposal (MIP-0002), transitioning component address to v2
        let timeout: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 12)
            .unwrap()
            .and_hms_opt(17, 33, 44)
            .unwrap();
        return VersioningInfo {
            name: "MIP-0002AAA".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: Default::default(),
            timeout: timeout.timestamp() as u64,
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
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Defined(Defined::new()));

        let now = vi.start + 5;
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
        let vi = get_default_version_info();
        let mut state: VersioningState = VersioningState::Started(Default::default());

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
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::LockedIn(LockedIn::new()));

        advance_msg.now = advance_msg.timeout + 1;
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
            threshold: Amount::zero(),
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
            threshold: Amount::zero(),
            now,
        };

        state = state.on_advance(advance);
        assert_eq!(state, VersioningState::Failed(Failed {}));
    }

    #[test]
    fn test_state_advance_to_failed() {
        // Test Versioning state transition (to state: Failed)
        let vi = get_default_version_info();
        let now = vi.start + 1;
        let advance_msg = Advance {
            start_timestamp: vi.start,
            timeout: vi.start,
            threshold: Amount::zero(),
            now,
        };

        let mut state: VersioningState = Default::default();
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Failed(Failed {}));

        let mut state: VersioningState = VersioningState::Started(Default::default());
        state = state.on_advance(advance_msg.clone());
        assert_eq!(state, VersioningState::Failed(Failed {}));
    }

    #[test]
    fn test_merge_with() {
        let vi_1 = VersioningInfo {
            name: "MIP-2".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: 0,
            timeout: 5,
        };
        let vs_1: VersioningState = "Active".into();

        let vi_2 = VersioningInfo {
            name: "MIP-3".to_string(),
            version: 3,
            component: VersioningComponent::Address,
            start: 17,
            timeout: 27,
        };
        let vs_2: VersioningState = "Defined".into();

        let mut vs_raw_1 = VersioningStoreRaw {
            data: BTreeMap::from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())]),
        };

        let vs_2_2: VersioningState = "LockedIn".into();
        let vs_raw_2 = VersioningStoreRaw {
            data: BTreeMap::from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2_2.clone())]),
        };

        let now = 7; // 7s after unix epoch - between vi_1.timeout && vi_2.start
        vs_raw_1.merge_with(&vs_raw_2, now);

        assert_eq!(vs_raw_1.data.get(&vi_1).unwrap(), &vs_1);
        assert_eq!(vs_raw_1.data.get(&vi_2).unwrap(), &vs_2_2);
    }

    #[test]
    fn test_merge_with_invalid() {
        let vi_1 = VersioningInfo {
            name: "MIP-2".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: 0,
            timeout: 5,
        };
        let vs_1: VersioningState = "Active".into();

        let vi_2 = VersioningInfo {
            name: "MIP-3".to_string(),
            version: 3,
            component: VersioningComponent::Address,
            start: 17,
            timeout: 27,
        };
        let vs_2: VersioningState = "Defined".into();

        let mut vs_raw_1 = VersioningStoreRaw {
            data: BTreeMap::from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())]),
        };

        let mut vi_2_2 = vi_2.clone();
        // Make versioning info invalid (because start == vi_1.timeout)
        vi_2_2.start = 5;
        let vs_2_2: VersioningState = "Defined".into();
        let vs_raw_2 = VersioningStoreRaw {
            data: BTreeMap::from([
                (vi_1.clone(), vs_1.clone()),
                (vi_2_2.clone(), vs_2_2.clone()),
            ]),
        };

        let now = 7; // 7s after unix epoch - between vi_1.timeout && vi_2.start
        vs_raw_1.merge_with(&vs_raw_2, now);

        assert_eq!(vs_raw_1.data.get(&vi_1).unwrap(), &vs_1);
        assert_eq!(vs_raw_1.data.get(&vi_2).unwrap(), &vs_2);
    }

    #[test]
    fn test_versioning_info_ser_der() {
        let vi = get_default_version_info();

        let mut buffer: Vec<u8> = Vec::new();
        let ser = VersioningInfoSerializer::new();
        ser.serialize(&vi, &mut buffer).unwrap();

        let der = VersioningInfoDeserializer::new();
        let (rem, vi_from_der) = der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert_eq!(vi, vi_from_der);
        assert!(rem.is_empty());
    }

    #[test]
    fn test_versioning_state_ser_der() {
        let vs = VersioningState::Defined(Defined::new());

        let mut buffer: Vec<u8> = Vec::new();
        let ser = VersioningStateSerializer::new();
        ser.serialize(&vs, &mut buffer).unwrap();

        let der = VersioningStateDeserializer::new();
        let (rem, vs_from_der) = der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert_eq!(vs, vs_from_der);
        assert!(rem.is_empty());

        let threshold = Amount::from_str("42.9876").unwrap();
        let vs = VersioningState::Started(Started::new(threshold));

        let mut buffer: Vec<u8> = Vec::new();
        let ser = VersioningStateSerializer::new();
        ser.serialize(&vs, &mut buffer).unwrap();

        let der = VersioningStateDeserializer::new();
        let (rem, vs_from_der) = der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert_eq!(vs, vs_from_der);
        assert!(rem.is_empty());
    }

    #[test]
    fn test_versioning_store_ser_der() {
        let vi = get_default_version_info();
        let state = VersioningState::Started(Started::new(Amount::from_str("25.7").unwrap()));
        let vs = VersioningStoreRaw {
            data: BTreeMap::from([(vi, state)]),
        };

        let mut buffer: Vec<u8> = Vec::new();
        let serializer = VersioningStoreRawSerializer::new();
        serializer.serialize(&vs, &mut buffer).unwrap();

        let der = VersioningStoreRawDeserializer::new();
        let (rem, vs_from_der) = der.deserialize::<DeserializeError>(&buffer).unwrap();
        assert_eq!(vs, vs_from_der);
        assert!(rem.is_empty());
    }
}
