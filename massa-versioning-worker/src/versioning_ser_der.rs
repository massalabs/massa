use std::collections::BTreeMap;
use std::mem;
use std::ops::Bound::{Excluded, Included};

use nom::{
    error::context,
    error::{ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};

use crate::versioning::{
    Advance, ComponentState, ComponentStateTypeId, MipComponent, MipInfo, MipState, MipStoreRaw,
    Started,
};

use massa_models::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_time::{MassaTimeDeserializer, MassaTimeSerializer};

/// Ser / Der

const MIP_INFO_NAME_MAX_LEN: u32 = 255;
const COMPONENT_STATE_VARIANT_COUNT: u32 = mem::variant_count::<ComponentState>() as u32;
const COMPONENT_STATE_ID_VARIANT_COUNT: u32 = mem::variant_count::<ComponentStateTypeId>() as u32;
const MIP_STORE_MAX_ENTRIES: u32 = 4096;
const MIP_STORE_MAX_SIZE: usize = 2097152;

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
        // name
        let name_len_ = value.name.len();
        if name_len_ > MIP_INFO_NAME_LEN_MAX as usize {
            return Err(SerializeError::StringTooBig(format!(
                "MIP info name len is {}, max: {}",
                name_len_, MIP_INFO_NAME_LEN_MAX
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
                Excluded(MIP_INFO_NAME_MAX_LEN),
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
                    // Note: this is bounded to MIP_INFO_NAME_MAX_LEN
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

// Advance

/// Serializer for `Advance`
pub struct AdvanceSerializer {
    amount_serializer: AmountSerializer,
    time_serializer: MassaTimeSerializer,
}

impl AdvanceSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            amount_serializer: AmountSerializer::new(),
            time_serializer: MassaTimeSerializer::new(),
        }
    }
}

impl Default for AdvanceSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Advance> for AdvanceSerializer {
    fn serialize(&self, value: &Advance, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.time_serializer
            .serialize(&value.start_timestamp, buffer)?;
        self.time_serializer.serialize(&value.timeout, buffer)?;
        self.amount_serializer.serialize(&value.threshold, buffer)?;
        self.time_serializer.serialize(&value.now, buffer)?;
        Ok(())
    }
}

/// A Deserializer for `Advance`
pub struct AdvanceDeserializer {
    amount_deserializer: AmountDeserializer,
    time_deserializer: MassaTimeDeserializer,
}

impl AdvanceDeserializer {
    /// Creates a new `AdvanceDeserializer`
    pub fn new() -> Self {
        Self {
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
            time_deserializer: MassaTimeDeserializer::new((
                Included(0.into()),
                Included(u64::MAX.into()),
            )),
        }
    }
}

impl Default for AdvanceDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<Advance> for AdvanceDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Advance, E> {
        context(
            "Failed Advance deserialization",
            tuple((
                context("Failed start_timestamp deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
                context("Failed timeout deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
                context("Failed threshold deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed now deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(start_timestamp, timeout, threshold, now)| Advance {
            start_timestamp,
            timeout,
            threshold,
            now,
        })
        .parse(buffer)
    }
}

// End Advance

// MipState

/// Serializer for `MipState`
pub struct MipStateSerializer {
    state_serializer: ComponentStateSerializer,
    advance_serializer: AdvanceSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl MipStateSerializer {
    /// Creates a new `MipStateSerializer`
    pub fn new() -> Self {
        Self {
            state_serializer: Default::default(),
            advance_serializer: Default::default(),
            u32_serializer: U32VarIntSerializer::default(),
        }
    }
}

impl Default for MipStateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<MipState> for MipStateSerializer {
    fn serialize(&self, value: &MipState, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.state_serializer.serialize(&value.inner, buffer)?;
        // history len
        self.u32_serializer.serialize(
            &value.history.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many history: {}", err))
            })?,
            buffer,
        )?;
        for (advance, state_id) in value.history.iter() {
            self.advance_serializer.serialize(advance, buffer)?;
            self.u32_serializer
                .serialize(&u32::from(state_id.clone()), buffer)?;
        }
        Ok(())
    }
}

/// A Deserializer for `MipState`
pub struct MipStateDeserializer {
    state_deserializer: ComponentStateDeserializer,
    advance_deserializer: AdvanceDeserializer,
    state_id_deserializer: U32VarIntDeserializer,
    u32_deserializer: U32VarIntDeserializer,
}

impl MipStateDeserializer {
    /// Creates a new `MipStateDeserializer`
    pub fn new() -> Self {
        Self {
            state_deserializer: ComponentStateDeserializer::new(),
            advance_deserializer: AdvanceDeserializer::new(),
            state_id_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(COMPONENT_STATE_ID_VARIANT_COUNT),
            ),
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(u32::MAX)),
        }
    }
}

impl Default for MipStateDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<MipState> for MipStateDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MipState, E> {
        // Der component state
        let (rem, component_state) = context("Failed component state deserialization", |input| {
            self.state_deserializer.deserialize(input)
        })
        .parse(buffer)?;

        // Der history
        let (rem2, history) = context(
            "Failed Vec<OperationId> deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context(
                    "Failed deserialization",
                    tuple((
                        context("Failed advance deserialization", |input| {
                            self.advance_deserializer.deserialize(input)
                        }),
                        context("Failed state id deserialization", |input| {
                            let (res, state_id_) = self.state_id_deserializer.deserialize(input)?;

                            let state_id =
                                ComponentStateTypeId::try_from(state_id_).map_err(|_e| {
                                    nom::Err::Error(ParseError::from_error_kind(
                                        buffer,
                                        nom::error::ErrorKind::Fail,
                                    ))
                                })?;

                            IResult::Ok((res, state_id))
                        }),
                    )),
                ),
            ),
        )
        .map(|items| {
            items
                .into_iter()
                .collect::<BTreeMap<Advance, ComponentStateTypeId>>()
        })
        .parse(rem)?;

        IResult::Ok((
            rem2,
            MipState {
                inner: component_state,
                history,
            },
        ))
    }
}

// End MipState

// MipStoreRaw

/// Serializer for `VersioningStoreRaw`
pub struct MipStoreRawSerializer {
    u32_serializer: U32VarIntSerializer,
    info_serializer: MipInfoSerializer,
    state_serializer: MipStateSerializer,
}

impl MipStoreRawSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            info_serializer: MipInfoSerializer::new(),
            state_serializer: MipStateSerializer::new(),
        }
    }
}

impl Default for MipStoreRawSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<MipStoreRaw> for MipStoreRawSerializer {
    fn serialize(&self, value: &MipStoreRaw, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let entry_count_ = value.0.len();
        let entry_count = u32::try_from(entry_count_).map_err(|e| {
            SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
        })?;
        if entry_count > MIP_STORE_MAX_ENTRIES {
            return Err(SerializeError::GeneralError(format!(
                "Too many entries in VersioningStoreRaw, max: {}",
                MIP_STORE_MAX_ENTRIES
            )));
        }
        self.u32_serializer.serialize(&entry_count, buffer)?;
        for (key, value) in value.0.iter() {
            self.info_serializer.serialize(key, buffer)?;
            self.state_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// A Deserializer for `VersioningStoreRaw
pub struct MipStoreRawDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    info_deserializer: MipInfoDeserializer,
    state_deserializer: MipStateDeserializer,
}

impl MipStoreRawDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MIP_STORE_MAX_ENTRIES),
            ),
            info_deserializer: MipInfoDeserializer::new(),
            state_deserializer: MipStateDeserializer::new(),
        }
    }
}

impl Default for MipStoreRawDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<MipStoreRaw> for MipStoreRawDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MipStoreRaw, E> {
        context(
            "Failed MipStoreRaw der",
            length_count(
                context("Failed len der", |input| {
                    let (rem, count) = self.u32_deserializer.deserialize(input)?;
                    IResult::Ok((rem, count))
                }),
                context("Failed items der", |input| {
                    let (rem, vi) = self.info_deserializer.deserialize(input)?;
                    let (rem2, vs) = self.state_deserializer.deserialize(rem)?;
                    IResult::Ok((rem2, (vi, vs)))
                }),
            ),
        )
        .map(|items| MipStoreRaw(items.into_iter().collect()))
        .parse(buffer)
    }
}

// End MipStoreRaw

#[cfg(test)]
mod test {
    use super::*;
    use std::mem::{size_of, size_of_val};

    use chrono::{NaiveDate, NaiveDateTime};
    use more_asserts::assert_lt;
    use std::str::FromStr;

    use crate::test_helpers::versioning_helpers::advance_state_until;
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

    #[test]
    fn test_advance_ser_der() {
        let start: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 01)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        let timeout: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 11)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        let now: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 05, 11)
            .unwrap()
            .and_hms_opt(11, 33, 44)
            .unwrap();

        let adv = Advance {
            start_timestamp: MassaTime::from(start.timestamp() as u64),
            timeout: MassaTime::from(timeout.timestamp() as u64),
            threshold: Default::default(),
            now: MassaTime::from(now.timestamp() as u64),
        };

        let mut buf = Vec::new();
        let adv_ser = AdvanceSerializer::new();
        adv_ser.serialize(&adv, &mut buf).unwrap();

        let state_der = AdvanceDeserializer::new();

        let (rem, adv_der) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(adv, adv_der);
    }

    #[test]
    fn test_mip_state_ser_der() {
        let state_1 = MipState::new(MassaTime::from(100));

        let mut buf = Vec::new();
        let state_ser = MipStateSerializer::new();
        state_ser.serialize(&state_1, &mut buf).unwrap();

        let state_der = MipStateDeserializer::new();

        let (rem, state_der_res) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(state_1, state_der_res);
        buf.clear();

        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
        };

        let state_2 = advance_state_until(ComponentState::locked_in(), &mi_1);
        state_ser.serialize(&state_2, &mut buf).unwrap();
        let (rem, state_der_res) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(state_2, state_der_res);
    }

    #[test]
    fn test_mip_store_raw_ser_der() {
        let mi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: MipComponent::Address,
            component_version: 1,
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
        };

        let mi_3 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            component: MipComponent::Block,
            component_version: 1,
            start: MassaTime::from(12),
            timeout: MassaTime::from(17),
        };

        let state_2 = advance_state_until(ComponentState::active(), &mi_2);
        let state_3 = advance_state_until(
            ComponentState::started(Amount::from_str("42.4242").unwrap()),
            &mi_3,
        );

        let store_raw = MipStoreRaw::try_from([(mi_2, state_2), (mi_3, state_3)]).unwrap();

        let mut buf = Vec::new();
        let store_raw_ser = MipStoreRawSerializer::new();
        store_raw_ser.serialize(&store_raw, &mut buf).unwrap();

        let store_raw_der = MipStoreRawDeserializer::new();
        let (rem, store_raw_der_res) = store_raw_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(store_raw, store_raw_der_res);
    }

    #[test]
    fn mip_store_raw_max_size() {
        let mut mi_base = MipInfo {
            name: "A".repeat(254),
            version: 0,
            component: MipComponent::Address,
            component_version: 0,
            start: MassaTime::from(0),
            timeout: MassaTime::from(2),
        };

        let mi_base_size = size_of_val(&mi_base.name[..])
            + size_of_val(&mi_base.version)
            + size_of_val(&mi_base.component)
            + size_of_val(&mi_base.component_version)
            + size_of_val(&mi_base.start)
            + size_of_val(&mi_base.timeout);

        let mut all_state_size = 0;

        let store_raw_: Vec<(MipInfo, MipState)> = (0..MIP_STORE_MAX_ENTRIES)
            .map(|_i| {
                mi_base.version += 1;
                mi_base.component_version += 1;
                mi_base.start = mi_base.timeout.saturating_add(MassaTime::from(1));
                mi_base.timeout = mi_base.start.saturating_add(MassaTime::from(2));

                let state = advance_state_until(ComponentState::active(), &mi_base);

                all_state_size += size_of_val(&state.inner);
                all_state_size += state.history.len() * (size_of::<Advance>() + size_of::<u32>());

                (mi_base.clone(), state)
            })
            .collect();

        // Cannot use update_with ou try_from here as the names are not uniques
        let store_raw = MipStoreRaw {
            0: BTreeMap::from_iter(store_raw_.into_iter()),
        };
        assert_eq!(store_raw.0.len(), MIP_STORE_MAX_ENTRIES as usize);

        let store_raw_size = (store_raw.0.len() * mi_base_size) + all_state_size;
        assert_lt!(store_raw_size, MIP_STORE_MAX_SIZE);

        // Now check SER / DER with this huge store
        let mut buf = Vec::new();
        let store_raw_ser = MipStoreRawSerializer::new();

        store_raw_ser
            .serialize(&store_raw, &mut buf)
            .expect("Unable to serialize");

        let store_raw_der = MipStoreRawDeserializer::new();
        let (rem, store_raw_der_res) = store_raw_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(store_raw, store_raw_der_res);
    }
}
