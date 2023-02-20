use std::collections::HashMap;
use std::ops::Bound::{Excluded, Included};
use std::sync::Arc;
use std::time::Duration;

use machine::{machine, transitions};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use num::ToPrimitive; // for function: to_u64()
use num_enum::{IntoPrimitive, TryFromPrimitive};
use parking_lot::RwLock;
use rust_decimal::Decimal;

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
        let start = value.start.as_secs_f32();
        let start_as_decimal =
            Decimal::from_f32_retain(start).ok_or(SerializeError::GeneralError(format!(
                "Could not convert start from f32: {:?} to Decimal",
                start
            )))?;
        let start_as_u64 = start_as_decimal
            .to_u64()
            .ok_or(SerializeError::GeneralError(format!(
                "Could not convert Decimal: {:?} to u64",
                start_as_decimal
            )))?;
        let amount = Amount::from_raw(start_as_u64);
        self.amount_serializer.serialize(&amount, buffer)?;
        // timeout
        let timeout = value.timeout.as_secs_f32();
        let timeout_as_decimal =
            Decimal::from_f32_retain(timeout).ok_or(SerializeError::GeneralError(format!(
                "Could not convert timeout from f32: {:?} to Decimal",
                start
            )))?;
        let timeout_as_u64 = timeout_as_decimal
            .to_u64()
            .ok_or(SerializeError::GeneralError(format!(
                "Could not convert Decimal: {:?} to u64",
                start_as_decimal
            )))?;
        let amount = Amount::from_raw(timeout_as_u64);
        self.amount_serializer.serialize(&amount, buffer)?;
        Ok(())
    }
}

pub struct VersioningInfoDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    u32_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl VersioningInfoDeserializer {
    /// Creates a new `EndorsementDeserializerLW`
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
                    let len = self.u64_deserializer.deserialize(input)?;
                    todo!()
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
                name: name,
                version: version,
                component: VersioningComponent::try_from(component).unwrap(),
                start: Default::default(),
                timeout: Default::default(),
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
        let (state, threshold_): (u32, Option<f32>) = match value {
            VersioningState::Error => (0, None),
            VersioningState::Defined(_) => (1, None),
            VersioningState::Started(Started { threshold }) => (2, Some(*threshold)),
            VersioningState::LockedIn(_) => (3, None),
            VersioningState::Active(_) => (4, None),
            VersioningState::Failed(_) => (5, None),
        };

        self.u32_serializer.serialize(&state, buffer)?;
        if let Some(threshold) = threshold_ {
            let start_as_decimal = Decimal::from_f32_retain(threshold).unwrap();
            let amount = Amount::from_raw(start_as_decimal.to_u64().unwrap());
            self.amount_serializer.serialize(&amount, buffer)?;
        }
        Ok(())
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

    #[test]
    fn test_versioning_store_ser_der() {
        let vi = get_default_version_info();
        let state = VersioningState::Started(Started::new(25.7));
        let v_store = VersioningStoreRaw {
            versioning_info: HashMap::from([(vi, state)]),
        };

        let mut buffer: Vec<u8> = Vec::new();
        let serializer = VersioningStoreRawSerializer::new();
        serializer.serialize(&v_store, &mut buffer).unwrap();

        // TODO: der
    }
}
