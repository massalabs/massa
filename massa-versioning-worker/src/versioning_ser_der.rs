use std::mem;
use std::ops::Bound::{Excluded, Included};

use nom::{
    error::context,
    error::{ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};

use crate::versioning::{ComponentState, ComponentStateTypeId, MipComponent, MipInfo, Started};

use massa_models::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_time::{MassaTimeDeserializer, MassaTimeSerializer};

/// Ser / Der

const VERSIONING_INFO_NAME_LEN_MAX: u32 = 255;
const COMPONENT_STATE_VARIANT_COUNT: u32 = mem::variant_count::<ComponentState>() as u32;
// const VERSIONING_STORE_ENTRIES_MAX: u32 = 2048;

/// Serializer for `MipInfo`
pub struct MipInfoSerializer {
    u32_serializer: U32VarIntSerializer,
    time_serializer: MassaTimeSerializer, // start / timeout
}

impl MipInfoSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        MipInfoSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            time_serializer: MassaTimeSerializer::new(),
        }
    }
}

impl Default for MipInfoSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<MipInfo> for MipInfoSerializer {
    fn serialize(&self, value: &MipInfo, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // TODO: StringSerializer
        // name
        let name_len_ = value.name.len();
        if name_len_ > VERSIONING_INFO_NAME_LEN_MAX as usize {
            return Err(SerializeError::StringTooBig(format!(
                "MIP info name len is {}, max: {}",
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
        let component_ = value.component.clone();
        let component: u32 = component_.into();

        self.u32_serializer.serialize(&component, buffer)?;
        // component version
        self.u32_serializer
            .serialize(&value.component_version, buffer)?;
        // start
        self.time_serializer.serialize(&value.start, buffer)?;
        // timeout
        self.time_serializer.serialize(&value.timeout, buffer)?;
        Ok(())
    }
}

/// Deserializer for `MipInfo`
pub struct MipInfoDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    len_deserializer: U32VarIntDeserializer,
    time_deserializer: MassaTimeDeserializer,
}

impl MipInfoDeserializer {
    /// Creates a new `MipInfoDeserializer`
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(u32::MAX)),
            len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(VERSIONING_INFO_NAME_LEN_MAX),
            ),
            time_deserializer: MassaTimeDeserializer::new((
                Included(0.into()),
                Included(u64::MAX.into()),
            )),
        }
    }
}

// Make clippy happy again!
impl Default for MipInfoDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<MipInfo> for MipInfoDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MipInfo, E> {
        context(
            "Failed MipInfo deserialization",
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
                    let component = MipComponent::try_from(component_).map_err(|_| {
                        nom::Err::Error(ParseError::from_error_kind(
                            input,
                            nom::error::ErrorKind::Fail,
                        ))
                    })?;
                    IResult::Ok((rem, component))
                }),
                context("Failed component version deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed start deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
                context("Failed timeout deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(name, version, component, component_version, start, timeout)| MipInfo {
                name,
                version,
                component,
                component_version,
                start,
                timeout,
            },
        )
        .parse(buffer)
    }
}

// End MipInfo

// ComponentState

/// Serializer for `ComponentState`
#[derive(Clone)]
pub struct ComponentStateSerializer {
    u32_serializer: U32VarIntSerializer,
    amount_serializer: AmountSerializer,
}

impl ComponentStateSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Default for ComponentStateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<ComponentState> for ComponentStateSerializer {
    fn serialize(
        &self,
        value: &ComponentState,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let (state, threshold_): (u32, Option<Amount>) = match value {
            ComponentState::Error => (u32::from(ComponentStateTypeId::Error), None),
            ComponentState::Defined(_) => (u32::from(ComponentStateTypeId::Defined), None),
            ComponentState::Started(Started { threshold }) => {
                (u32::from(ComponentStateTypeId::Started), Some(*threshold))
            }
            ComponentState::LockedIn(_) => (u32::from(ComponentStateTypeId::LockedIn), None),
            ComponentState::Active(_) => (u32::from(ComponentStateTypeId::Active), None),
            ComponentState::Failed(_) => (u32::from(ComponentStateTypeId::Failed), None),
        };
        self.u32_serializer.serialize(&state, buffer)?;
        if let Some(threshold) = threshold_ {
            self.amount_serializer.serialize(&threshold, buffer)?;
        }
        Ok(())
    }
}

/// A Deserializer for ComponentState`
pub struct ComponentStateDeserializer {
    state_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl ComponentStateDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            state_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(COMPONENT_STATE_VARIANT_COUNT + 1),
            ),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
        }
    }
}

impl Default for ComponentStateDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<ComponentState> for ComponentStateDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ComponentState, E> {
        let (rem, enum_value_) = context("Failed enum value der", |input| {
            self.state_deserializer.deserialize(input)
        })
        .parse(buffer)?;

        let enum_value = ComponentStateTypeId::try_from(enum_value_).map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Eof,
            ))
        })?;
        let (rem2, state): (&[u8], ComponentState) = match enum_value {
            ComponentStateTypeId::Defined => (rem, ComponentState::defined()),
            ComponentStateTypeId::Started => {
                let (rem2, threshold) = context("Failed threshold value der", |input| {
                    self.amount_deserializer.deserialize(input)
                })
                .parse(rem)?;
                (rem2, ComponentState::started(threshold))
            }
            ComponentStateTypeId::LockedIn => (rem, ComponentState::locked_in()),
            ComponentStateTypeId::Active => (rem, ComponentState::active()),
            ComponentStateTypeId::Failed => (rem, ComponentState::failed()),
            _ => (rem, ComponentState::Error),
        };

        IResult::Ok((rem2, state))
    }
}

// End ComponentState

#[cfg(test)]
mod test {
    use super::*;

    use std::str::FromStr;

    use massa_serialization::DeserializeError;
    use massa_time::MassaTime;

    #[test]
    fn test_mip_info_ser_der() {
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
        };

        let mut buf = Vec::new();
        let mip_info_ser = MipInfoSerializer::new();
        mip_info_ser.serialize(&vi_1, &mut buf).unwrap();

        let mip_info_der = MipInfoDeserializer::new();

        let (rem, vi_1_der) = mip_info_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(vi_1, vi_1_der);
    }

    #[test]
    fn test_component_state_ser_der() {
        let st_1 = ComponentState::failed();
        let st_2 = ComponentState::Started(Started {
            threshold: Amount::from_str("98.42").unwrap(),
        });

        let mut buf = Vec::new();
        let state_ser = ComponentStateSerializer::new();
        state_ser.serialize(&st_1, &mut buf).unwrap();

        let state_der = ComponentStateDeserializer::new();

        let (rem, st_1_der) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(st_1, st_1_der);

        buf.clear();
        state_ser.serialize(&st_2, &mut buf).unwrap();
        let (rem, st_2_der) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(st_2, st_2_der);
    }
}
