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
    Advance, ComponentState, ComponentStateTypeId, LockedIn, MipComponent, MipInfo, MipState,
    MipStatsConfig, MipStoreRaw, MipStoreStats, Started,
};

use massa_models::amount::{Amount, AmountDeserializer, AmountSerializer};
use massa_models::config::{MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_time::{MassaTimeDeserializer, MassaTimeSerializer};

/// Ser / Der

const MIP_INFO_NAME_MAX_LEN: u32 = 255;
const MIP_INFO_COMPONENTS_MAX_ENTRIES: u32 = 8;
const COMPONENT_STATE_VARIANT_COUNT: u32 = mem::variant_count::<ComponentState>() as u32;
const COMPONENT_STATE_ID_VARIANT_COUNT: u32 = mem::variant_count::<ComponentStateTypeId>() as u32;
const MIP_STORE_MAX_ENTRIES: u32 = 4096;
#[allow(dead_code)]
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
        if name_len_ > MIP_INFO_NAME_MAX_LEN as usize {
            return Err(SerializeError::StringTooBig(format!(
                "MIP info name len is {}, max: {}",
                name_len_, MIP_INFO_NAME_MAX_LEN
            )));
        }
        let name_len = u32::try_from(name_len_).map_err(|_| {
            SerializeError::GeneralError(format!(
                "Cannot convert to name_len: {} to u32",
                name_len_
            ))
        })?;
        self.u32_serializer.serialize(&name_len, buffer)?;
        buffer.extend(value.name.as_bytes());
        // version
        self.u32_serializer.serialize(&value.version, buffer)?;

        // Components
        let components_len_ = value.components.len();
        if components_len_ > MIP_INFO_COMPONENTS_MAX_ENTRIES as usize {
            return Err(SerializeError::NumberTooBig(format!(
                "MIP info cannot have more than {} components, got: {}",
                MIP_STORE_MAX_ENTRIES, components_len_
            )));
        }
        let components_len = u32::try_from(components_len_).map_err(|_| {
            SerializeError::GeneralError(format!(
                "Cannot convert to component_len: {} to u32",
                name_len_
            ))
        })?;
        // ser hashmap len
        self.u32_serializer.serialize(&components_len, buffer)?;
        // ser hashmap items
        for (component, component_version) in value.components.iter() {
            // component
            self.u32_serializer
                .serialize(&component.clone().into(), buffer)?;
            // component version
            self.u32_serializer.serialize(component_version, buffer)?;
        }

        // start
        self.time_serializer.serialize(&value.start, buffer)?;
        // timeout
        self.time_serializer.serialize(&value.timeout, buffer)?;
        // activation delay
        self.time_serializer
            .serialize(&value.activation_delay, buffer)?;
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
                context(
                    "Failed components deserialization",
                    length_count(
                        context("Failed components length deserialization", |input| {
                            self.u32_deserializer.deserialize(input)
                        }),
                        tuple((
                            context("Failed component deserialization", |input| {
                                let (rem, component_) = self.u32_deserializer.deserialize(input)?;
                                let component =
                                    MipComponent::try_from(component_).map_err(|_| {
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
                        )),
                    ),
                ),
                context("Failed start deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
                context("Failed timeout deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
                context("Failed activation delay deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(name, version, components, start, timeout, activation_delay)| MipInfo {
                name,
                version,
                components: components.into_iter().collect(),
                start,
                timeout,
                activation_delay,
            },
        )
        .parse(buffer)
    }
}

// End MipInfo

// ComponentState

/// Serializer for `ComponentState`
pub struct ComponentStateSerializer {
    u32_serializer: U32VarIntSerializer,
    amount_serializer: AmountSerializer,
    time_serializer: MassaTimeSerializer,
}

impl ComponentStateSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            amount_serializer: AmountSerializer::new(),
            time_serializer: MassaTimeSerializer::new(),
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
        match value {
            ComponentState::Started(Started { threshold }) => {
                let state_id = u32::from(ComponentStateTypeId::from(value));
                self.u32_serializer.serialize(&state_id, buffer)?;
                self.amount_serializer.serialize(threshold, buffer)?;
            }
            ComponentState::LockedIn(LockedIn { at }) => {
                let state_id = u32::from(ComponentStateTypeId::from(value));
                self.u32_serializer.serialize(&state_id, buffer)?;
                self.time_serializer.serialize(at, buffer)?;
            }
            _ => {
                let state_id = u32::from(ComponentStateTypeId::from(value));
                self.u32_serializer.serialize(&state_id, buffer)?;
            }
        }
        Ok(())
    }
}

/// A Deserializer for ComponentState`
pub struct ComponentStateDeserializer {
    state_deserializer: U32VarIntDeserializer,
    amount_deserializer: AmountDeserializer,
    time_deserializer: MassaTimeDeserializer,
}

impl ComponentStateDeserializer {
    /// Creates a new ``
    pub fn new() -> Self {
        Self {
            state_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(COMPONENT_STATE_VARIANT_COUNT),
            ),
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
            ComponentStateTypeId::LockedIn => {
                let (rem2, at) = context("Failed delay value der", |input| {
                    self.time_deserializer.deserialize(input)
                })
                .parse(rem)?;
                (rem2, ComponentState::locked_in(at))
            }
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
        // start
        self.time_serializer
            .serialize(&value.start_timestamp, buffer)?;
        // timeout
        self.time_serializer.serialize(&value.timeout, buffer)?;
        // threshold
        self.amount_serializer.serialize(&value.threshold, buffer)?;
        // now
        self.time_serializer.serialize(&value.now, buffer)?;
        // activation delay
        self.time_serializer
            .serialize(&value.activation_delay, buffer)?;
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
                context("Failed activation delay deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(start_timestamp, timeout, threshold, now, activation_delay)| Advance {
                start_timestamp,
                timeout,
                threshold,
                now,
                activation_delay,
            },
        )
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
        self.state_serializer.serialize(&value.state, buffer)?;
        // history len
        self.u32_serializer.serialize(
            &value.history.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many history: {}", err))
            })?,
            buffer,
        )?;
        // history
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
            "Failed history deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context(
                    "Failed history items deserialization",
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
                state: component_state,
                history,
            },
        ))
    }
}

// End MipState

// MipStoreStats

/// Serializer for `VersioningStoreRaw`
pub struct MipStoreStatsSerializer {
    u32_serializer: U32VarIntSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl MipStoreStatsSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Default for MipStoreStatsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<MipStoreStats> for MipStoreStatsSerializer {
    fn serialize(&self, value: &MipStoreStats, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // config
        // let cfg_1 = u32::try_from(value.config.block_count_considered).map_err(|e| {
        //     SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
        // })?;
        // let cfg_2 = u32::try_from(value.config.counters_max).map_err(|e| {
        //     SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
        // })?;

        // self.u32_serializer.serialize(&cfg_1, buffer)?;
        // self.u32_serializer.serialize(&cfg_2, buffer)?;

        // stats data
        {
            let entry_count_ = value.latest_announcements.len();
            let entry_count = u32::try_from(entry_count_).map_err(|e| {
                SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
            })?;
            let entry_count_max = u32::try_from(MIP_STORE_STATS_BLOCK_CONSIDERED).map_err(|e| {
                SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
            })?;

            if entry_count > entry_count_max {
                return Err(SerializeError::GeneralError(format!(
                    "Too many entries in MipStoreStats latest announcements, max: {}",
                    MIP_STORE_STATS_BLOCK_CONSIDERED
                )));
            }
            self.u32_serializer.serialize(&entry_count, buffer)?;
            for v in value.latest_announcements.iter() {
                self.u32_serializer.serialize(v, buffer)?;
            }
        }

        {
            let entry_count_2_ = value.network_version_counters.len();
            let entry_count_2 = u32::try_from(entry_count_2_).map_err(|e| {
                SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
            })?;
            let entry_count_2_max = u32::try_from(MIP_STORE_STATS_COUNTERS_MAX).map_err(|e| {
                SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
            })?;

            if entry_count_2 > entry_count_2_max {
                return Err(SerializeError::GeneralError(format!(
                    "Too many entries in MipStoreStats version counters, max: {}",
                    MIP_STORE_STATS_COUNTERS_MAX
                )));
            }
            self.u32_serializer.serialize(&entry_count_2, buffer)?;
            for (v, c) in value.network_version_counters.iter() {
                self.u32_serializer.serialize(v, buffer)?;
                self.u64_serializer.serialize(c, buffer)?;
            }
        }

        Ok(())
    }
}

/// A Deserializer for `MipStoreStats
pub struct MipStoreStatsDeserializer {
    config: MipStatsConfig,
    u32_deserializer: U32VarIntDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl MipStoreStatsDeserializer {
    /// Creates a new ``
    pub fn new(block_count_considered: usize, counters_max: usize) -> Self {
        Self {
            config: MipStatsConfig {
                block_count_considered,
                counters_max,
            },
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        }
    }
}

/*
impl Default for MipStoreStatsDeserializer {
    fn default() -> Self {
        Self::new()
    }
}
*/

impl Deserializer<MipStoreStats> for MipStoreStatsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MipStoreStats, E> {
        // let (rem, cfg_1) = context("Failed cfg_1 der", |input| {
        //     self.u32_deserializer.deserialize(input)
        // })
        // .parse(buffer)?;

        // let (rem2, cfg_2) = context("Failed cfg_2 der", |input| {
        //     self.u32_deserializer.deserialize(input)
        // })
        // .parse(rem)?;

        // let config = MipStatsConfig {
        //     block_count_considered: cfg_1 as usize,
        //     counters_max: cfg_2 as usize,
        // };

        let cfg_block_considered: u32 =
            u32::try_from(self.config.block_count_considered).map_err(|_e| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Fail,
                ))
            })?;
        let cfg_counter_max: u32 = u32::try_from(self.config.counters_max).map_err(|_e| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            ))
        })?;

        let (rem3, latest_annoucements_) = context(
            "Failed MipStoreStats latest announcements der",
            length_count(
                context("Failed latest announcements count der", |input| {
                    let (rem, count) = self.u32_deserializer.deserialize(input)?;
                    if count > cfg_block_considered {
                        return IResult::Err(nom::Err::Error(ParseError::from_error_kind(
                            input,
                            nom::error::ErrorKind::Fail,
                        )));
                    }
                    IResult::Ok((rem, count))
                }),
                context("Failed latest announcement data der", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
            ),
        )
        .parse(buffer)?;

        let (rem4, network_version_counters) = context(
            "Failed MipStoreStats network version counters der",
            length_count(
                context("Failed counters len der", |input| {
                    let (rem, count) = self.u32_deserializer.deserialize(input)?;
                    if count > cfg_counter_max {
                        return IResult::Err(nom::Err::Error(ParseError::from_error_kind(
                            input,
                            nom::error::ErrorKind::Fail,
                        )));
                    }
                    IResult::Ok((rem, count))
                }),
                context("Failed counters data der", |input| {
                    let (rem, v) = self.u32_deserializer.deserialize(input)?;
                    let (rem2, c) = self.u64_deserializer.deserialize(rem)?;
                    IResult::Ok((rem2, (v, c)))
                }),
            ),
        )
        .parse(rem3)?;

        IResult::Ok((
            rem4,
            MipStoreStats {
                config: self.config.clone(),
                latest_announcements: latest_annoucements_.into_iter().collect(),
                network_version_counters: network_version_counters.into_iter().collect(),
            },
        ))
    }
}

// End MipStoreStats

// MipStoreRaw

/// Serializer for `VersioningStoreRaw`
pub struct MipStoreRawSerializer {
    u32_serializer: U32VarIntSerializer,
    info_serializer: MipInfoSerializer,
    state_serializer: MipStateSerializer,
    stats_serializer: MipStoreStatsSerializer,
}

impl MipStoreRawSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            info_serializer: MipInfoSerializer::new(),
            state_serializer: MipStateSerializer::new(),
            stats_serializer: MipStoreStatsSerializer::new(),
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
        let entry_count_ = value.store.len();
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
        for (key, value) in value.store.iter() {
            self.info_serializer.serialize(key, buffer)?;
            self.state_serializer.serialize(value, buffer)?;
        }
        self.stats_serializer.serialize(&value.stats, buffer)?;
        Ok(())
    }
}

/// A Deserializer for `VersioningStoreRaw
pub struct MipStoreRawDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    info_deserializer: MipInfoDeserializer,
    state_deserializer: MipStateDeserializer,
    stats_deserializer: MipStoreStatsDeserializer,
}

impl MipStoreRawDeserializer {
    /// Creates a new ``
    pub fn new(block_count_considered: usize, counters_max: usize) -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MIP_STORE_MAX_ENTRIES),
            ),
            info_deserializer: MipInfoDeserializer::new(),
            state_deserializer: MipStateDeserializer::new(),
            stats_deserializer: MipStoreStatsDeserializer::new(
                block_count_considered,
                counters_max,
            ),
        }
    }
}

/*
impl Default for MipStoreRawDeserializer {
    fn default() -> Self {
        Self::new()
    }
}
*/

impl Deserializer<MipStoreRaw> for MipStoreRawDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MipStoreRaw, E> {
        context(
            "Failed MipStoreRaw der",
            tuple((
                length_count(
                    context("Failed entry count der", |input| {
                        let (rem, count) = self.u32_deserializer.deserialize(input)?;
                        IResult::Ok((rem, count))
                    }),
                    context("Failed items der", |input| {
                        let (rem, vi) = self.info_deserializer.deserialize(input)?;
                        let (rem2, vs) = self.state_deserializer.deserialize(rem)?;
                        IResult::Ok((rem2, (vi, vs)))
                    }),
                ),
                context("Failed mip store stats der", |input| {
                    self.stats_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(items, stats)| MipStoreRaw {
            store: items.into_iter().collect(),
            stats,
        })
        .parse(buffer)
    }
}

// End MipStoreRaw

#[cfg(test)]
mod test {
    use super::*;

    use std::collections::HashMap;
    use std::mem::{size_of, size_of_val};
    use std::str::FromStr;

    use chrono::{NaiveDate, NaiveDateTime};
    use more_asserts::assert_lt;

    use crate::test_helpers::versioning_helpers::advance_state_until;

    use massa_serialization::DeserializeError;
    use massa_time::MassaTime;

    #[test]
    fn test_mip_info_ser_der() {
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
            activation_delay: MassaTime::from(2),
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
            activation_delay: MassaTime::from(20),
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
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
            activation_delay: MassaTime::from(2),
        };

        let state_2 = advance_state_until(ComponentState::locked_in(MassaTime::from(3)), &mi_1);
        state_ser.serialize(&state_2, &mut buf).unwrap();
        let (rem2, state_der_res) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem2.is_empty());
        assert_eq!(state_2, state_der_res);
    }

    #[test]
    fn test_mip_store_stats_ser_der() {
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };

        let mip_stats = MipStoreStats {
            config: mip_stats_cfg.clone(),
            latest_announcements: Default::default(),
            network_version_counters: Default::default(),
        };

        let mut buf = Vec::new();
        let store_stats_ser = MipStoreStatsSerializer::new();
        store_stats_ser.serialize(&mip_stats, &mut buf).unwrap();

        let store_stats_der = MipStoreStatsDeserializer::new(
            mip_stats_cfg.block_count_considered,
            mip_stats_cfg.counters_max,
        );
        let (rem, store_stats_der_res) = store_stats_der
            .deserialize::<DeserializeError>(&buf)
            .unwrap();

        assert!(rem.is_empty());
        assert_eq!(mip_stats, store_stats_der_res);
    }

    #[test]
    fn test_mip_store_raw_ser_der() {
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };

        let mi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(2),
            timeout: MassaTime::from(5),
            activation_delay: MassaTime::from(2),
        };

        let mi_3 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: HashMap::from([(MipComponent::Block, 1)]),
            start: MassaTime::from(12),
            timeout: MassaTime::from(17),
            activation_delay: MassaTime::from(2),
        };

        let state_2 = advance_state_until(ComponentState::active(), &mi_2);
        let state_3 = advance_state_until(
            ComponentState::started(Amount::from_str("42.4242").unwrap()),
            &mi_3,
        );

        let store_raw =
            MipStoreRaw::try_from(([(mi_2, state_2), (mi_3, state_3)], mip_stats_cfg)).unwrap();

        let mut buf = Vec::new();
        let store_raw_ser = MipStoreRawSerializer::new();
        store_raw_ser.serialize(&store_raw, &mut buf).unwrap();

        let store_raw_der = MipStoreRawDeserializer::new(10, 5);
        let (rem, store_raw_der_res) = store_raw_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(store_raw, store_raw_der_res);
    }

    #[test]
    fn mip_store_raw_max_size() {
        let mut mi_base = MipInfo {
            name: "A".repeat(254),
            version: 0,
            components: HashMap::from([(MipComponent::Address, 0)]),
            start: MassaTime::from(0),
            timeout: MassaTime::from(2),
            activation_delay: MassaTime::from(2),
        };

        // Note: we did not add the name ptr and hashmap ptr, only the data inside
        let mi_base_size = size_of_val(&mi_base.name[..])
            + size_of_val(&mi_base.version)
            + mi_base.components.len() * size_of::<u32>() * 2
            + size_of_val(&mi_base.start)
            + size_of_val(&mi_base.timeout);

        let mut all_state_size = 0;

        let store_raw_: Vec<(MipInfo, MipState)> = (0..MIP_STORE_MAX_ENTRIES)
            .map(|_i| {
                mi_base.version += 1;
                mi_base
                    .components
                    .entry(MipComponent::Address)
                    .and_modify(|e| *e += 1);
                mi_base.start = mi_base.timeout.saturating_add(MassaTime::from(1));
                mi_base.timeout = mi_base.start.saturating_add(MassaTime::from(2));

                let state = advance_state_until(ComponentState::active(), &mi_base);

                all_state_size += size_of_val(&state.state);
                all_state_size += state.history.len() * (size_of::<Advance>() + size_of::<u32>());

                (mi_base.clone(), state)
            })
            .collect();

        // Cannot use update_with ou try_from here as the names are not uniques
        let store_raw = MipStoreRaw {
            store: BTreeMap::from_iter(store_raw_.into_iter()),
            stats: MipStoreStats::new(MipStatsConfig {
                block_count_considered: 10,
                counters_max: 5,
            }),
        };
        assert_eq!(store_raw.store.len(), MIP_STORE_MAX_ENTRIES as usize);

        let store_raw_size = (store_raw.store.len() * mi_base_size) + all_state_size;
        assert_lt!(store_raw_size, MIP_STORE_MAX_SIZE);

        // Now check SER / DER with this huge store
        let mut buf = Vec::new();
        let store_raw_ser = MipStoreRawSerializer::new();

        store_raw_ser
            .serialize(&store_raw, &mut buf)
            .expect("Unable to serialize");

        let store_raw_der = MipStoreRawDeserializer::new(10, 5);
        let (rem, store_raw_der_res) = store_raw_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(store_raw, store_raw_der_res);
    }
}
