use std::collections::HashMap;
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

use crate::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U32VarIntDeserializer,
    U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};

// TODO: add more items here
/// Versioning component enum
#[derive(Clone, Debug, PartialEq, Eq, Hash, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
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
    pub(crate) start: u64,
    /// a timestamp at which the deployment is considered failed (timeout > start)
    pub(crate) timeout: u64,
}

machine!(
    /// State machine for a Versioning component that tracks the deployment state
    #[derive(Clone, Debug, PartialEq)]
    enum VersioningState {
        /// Initial state
        Defined,
        /// Past start
        Started { threshold: Amount },
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

const THRESHOLD_TRANSITION_ACCEPTED: &str = "75.0";

#[derive(Clone, Debug, PartialEq)]
pub struct Advance {
    /// from VersioningInfo.start
    start_timestamp: u64,
    /// from VersioningInfo.timeout
    timeout: u64,
    /// % of past blocks with this version
    threshold: Amount,
    /// Current time (timestamp)
    now: u64,
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
            n if n > input.start_timestamp => VersioningState::Started(Started {
                threshold: Amount::zero(),
            }),
            _ => VersioningState::Defined(Defined {}),
        }
    }
}

impl Started {
    pub fn new(threshold: Amount) -> Self {
        Self { threshold }
    }

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
    pub fn new() -> Self {
        Self {}
    }
    pub fn on_advance(self, _input: Advance) -> Failed {
        Failed {}
    }
}

// Let's define it if needed

#[derive(Debug, Clone)]
pub struct VersioningStore(pub Arc<RwLock<VersioningStoreRaw>>);

/// Store of all versioning info
#[derive(Debug, Clone, PartialEq)]
pub struct VersioningStoreRaw {
    // TODO: rename to store?
    pub versioning_info: HashMap<VersioningInfo, VersioningState>,
}

/// Ser / Der

/// Serializer for `VersioningInfo`
#[derive(Clone)]
pub struct VersioningInfoSerializer {
    u32_serializer: U32VarIntSerializer,
    len_serializer: U64VarIntSerializer, // name length
    amount_serializer: AmountSerializer, // start / timeout
}

impl VersioningInfoSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        VersioningInfoSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            len_serializer: U64VarIntSerializer::new(),
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
        // TODO: add max size for name?
        self.len_serializer
            .serialize(&(value.name.len() as u64), buffer)?;
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
    u64_deserializer: U64VarIntDeserializer,
    u32_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl VersioningInfoDeserializer {
    /// Creates a new `VersioningInfoDeserializer`
    pub fn new() -> Self {
        Self {
            // FIXME
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(u32::MAX)),
            // FIXME
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Excluded(u64::MAX)),
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
                    let (input_, len_) = self.u64_deserializer.deserialize(input)?;
                    // FIXME: check
                    let len = len_ as usize;
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
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed start deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed timeout deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(name, version, component, start, timeout)| {
            // TODO: no unwrap() / default()
            VersioningInfo {
                name,
                version,
                component: VersioningComponent::try_from(component).unwrap(),
                start: start.to_raw(),
                timeout: timeout.to_raw(),
            }
        })
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
            VersioningState::Error => (0, None),
            VersioningState::Defined(_) => (1, None),
            VersioningState::Started(Started { threshold }) => (2, Some(*threshold)),
            VersioningState::LockedIn(_) => (3, None),
            VersioningState::Active(_) => (4, None),
            VersioningState::Failed(_) => (5, None),
        };
        self.u32_serializer.serialize(&state, buffer)?;
        if let Some(threshold) = threshold_ {
            self.amount_serializer.serialize(&threshold, buffer)?;
        }
        Ok(())
    }
}

pub struct VersioningStateDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl VersioningStateDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            // FIXME
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(u32::MAX)),
            // // FIXME
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
        let (rem, enum_value) = context("Failed enum value der", |input| {
            self.u32_deserializer.deserialize(input)
        })
        .parse(buffer)?;

        let (rem2, state): (&[u8], VersioningState) = match enum_value {
            1 => (rem, VersioningState::Defined(Defined::new())),
            2 => {
                let (rem2, threshold) = context("Failed threshold value der", |input| {
                    self.amount_deserializer.deserialize(input)
                })
                .parse(rem)?;
                (rem2, VersioningState::Started(Started::new(threshold)))
            }
            3 => (rem, VersioningState::LockedIn(LockedIn::new())),
            4 => (rem, VersioningState::Active(Active::new())),
            5 => (rem, VersioningState::Failed(Failed::new())),
            _ => (rem, VersioningState::Error),
        };

        IResult::Ok((rem2, state))
    }
}

/// Serializer for `VersioningStoreRaw`
#[derive(Clone)]
pub struct VersioningStoreRawSerializer {
    u64_serializer: U64VarIntSerializer,
    info_serializer: VersioningInfoSerializer,
    state_serializer: VersioningStateSerializer,
}

impl VersioningStoreRawSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        VersioningStoreRawSerializer {
            u64_serializer: U64VarIntSerializer::new(),
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
        let entry_count: u64 = value.versioning_info.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in VersioningStoreRaw: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (key, value) in value.versioning_info.iter() {
            self.info_serializer.serialize(key, buffer)?;
            self.state_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// A Deserializer for `VersioningStoreRaw
pub struct VersioningStoreRawDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    info_deserializer: VersioningInfoDeserializer,
    state_deserializer: VersioningStateDeserializer,
}

impl VersioningStoreRawDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            // FIXME
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Excluded(u64::MAX)),
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
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed items der", |input| {
                    let (rem, vi) = self.info_deserializer.deserialize(input)?;
                    let (rem2, vs) = self.state_deserializer.deserialize(rem)?;
                    IResult::Ok((rem2, (vi, vs)))
                }),
            ),
        )
        .map(|items| VersioningStoreRaw {
            versioning_info: items.into_iter().collect(),
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    // use crate::streaming_step::StreamingStep::Started;
    use chrono::{NaiveDate, NaiveDateTime};

    fn get_default_version_info() -> VersioningInfo {
        // A default VersioningInfo used in many tests
        // Models a Massa Improvments Proposal (MIP-0002), transitioning component address to v2
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
            versioning_info: HashMap::from([(vi, state)]),
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
