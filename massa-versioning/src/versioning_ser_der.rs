use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Included};

use nom::{
    bytes::complete::take,
    error::context,
    error::{ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use num::rational::Ratio;

use crate::versioning::{
    Active, AdvanceLW, ComponentState, ComponentStateTypeId, LockedIn, MipComponent, MipInfo,
    MipState, MipStatsConfig, MipStoreRaw, MipStoreStats, Started,
};

use massa_models::config::MIP_STORE_STATS_BLOCK_CONSIDERED;
use massa_serialization::{
    Deserializer, RatioDeserializer, RatioSerializer, SerializeError, Serializer,
    U32VarIntDeserializer, U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_time::{MassaTime, MassaTimeDeserializer, MassaTimeSerializer};

/// Ser / Der

const MIP_INFO_NAME_MAX_LEN: u32 = 255;
const MIP_INFO_COMPONENTS_MAX_ENTRIES: u32 = 8;
const COMPONENT_STATE_VARIANT_COUNT: u32 = ComponentStateTypeId::VARIANT_COUNT as u32;
const COMPONENT_STATE_ID_VARIANT_COUNT: u32 = ComponentStateTypeId::VARIANT_COUNT as u32;
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
        let name_len = u32::try_from(name_len_).map_err(|_| {
            SerializeError::GeneralError(format!("Cannot convert name_len ({}) to u32", name_len_))
        })?;
        self.u32_serializer.serialize(&name_len, buffer)?;
        buffer.extend(value.name.as_bytes());
        // version
        self.u32_serializer.serialize(&value.version, buffer)?;

        // Components
        let components_len_ = value.components.len();
        let components_len = u32::try_from(components_len_).map_err(|_| {
            SerializeError::GeneralError(format!(
                "Cannot convert component_len ({}) to u32",
                components_len_
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
    name_len_deserializer: U32VarIntDeserializer,
    components_len_deserializer: U32VarIntDeserializer,
    time_deserializer: MassaTimeDeserializer,
}

impl MipInfoDeserializer {
    /// Creates a new `MipInfoDeserializer`
    pub fn new() -> Self {
        Self {
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(u32::MAX)),
            name_len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MIP_INFO_NAME_MAX_LEN),
            ),
            components_len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MIP_INFO_COMPONENTS_MAX_ENTRIES),
            ),
            time_deserializer: MassaTimeDeserializer::new((
                Included(MassaTime::from_millis(0)),
                Included(MassaTime::from_millis(u64::MAX)),
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
                    let (input_, len_) = self.name_len_deserializer.deserialize(input)?;
                    // Safe to unwrap as it returns Result<usize, Infallible>
                    let len = usize::try_from(len_).unwrap();
                    let (rem, slice) = take(len)(input_)?;
                    let name = String::from_utf8(slice.to_vec()).map_err(|_| {
                        nom::Err::Error(ParseError::from_error_kind(
                            input_,
                            nom::error::ErrorKind::Fail,
                        ))
                    })?;
                    IResult::Ok((rem, name))
                }),
                context("Failed version deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context(
                    "Failed components deserialization",
                    length_count(
                        context("Failed components length deserialization", |input| {
                            // Note: this is bounded to MIP_INFO_COMPONENTS_MAX_ENTRIES
                            self.components_len_deserializer.deserialize(input)
                        }),
                        tuple((
                            context("Failed component deserialization", |input| {
                                let (rem, component_) = self.u32_deserializer.deserialize(input)?;
                                let component = MipComponent::from(component_);
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
    ratio_serializer: RatioSerializer<u64, U64VarIntSerializer>,
    time_serializer: MassaTimeSerializer,
}

impl ComponentStateSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            ratio_serializer: RatioSerializer::new(U64VarIntSerializer::new()),
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
        let state_id = u32::from(ComponentStateTypeId::from(value));
        self.u32_serializer.serialize(&state_id, buffer)?;
        match value {
            ComponentState::Started(Started {
                vote_ratio: threshold,
            }) => {
                // self.amount_serializer.serialize(threshold, buffer)?;
                self.ratio_serializer.serialize(threshold, buffer)?;
            }
            ComponentState::LockedIn(LockedIn { at }) => {
                self.time_serializer.serialize(at, buffer)?;
            }
            ComponentState::Active(Active { at }) => {
                self.time_serializer.serialize(at, buffer)?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// A Deserializer for ComponentState`
pub struct ComponentStateDeserializer {
    state_deserializer: U32VarIntDeserializer,
    ratio_deserializer: RatioDeserializer<u64, U64VarIntDeserializer>,
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
            ratio_deserializer: RatioDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            )),
            time_deserializer: MassaTimeDeserializer::new((
                Included(MassaTime::from_millis(0)),
                Included(MassaTime::from_millis(u64::MAX)),
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
                    self.ratio_deserializer.deserialize(input)
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
            ComponentStateTypeId::Active => {
                let (rem2, at) = context("Failed at value der", |input| {
                    self.time_deserializer.deserialize(input)
                })
                .parse(rem)?;
                (rem2, ComponentState::active(at))
            }
            ComponentStateTypeId::Failed => (rem, ComponentState::failed()),
            _ => (rem, ComponentState::Error),
        };

        IResult::Ok((rem2, state))
    }
}

// End ComponentState

// AdvanceLW

/// Serializer for `AdvanceLW`
pub struct AdvanceLWSerializer {
    ratio_serializer: RatioSerializer<u64, U64VarIntSerializer>,
    time_serializer: MassaTimeSerializer,
}

impl AdvanceLWSerializer {
    /// Creates a new `Serializer`
    pub fn new() -> Self {
        Self {
            ratio_serializer: RatioSerializer::new(U64VarIntSerializer::new()),
            time_serializer: MassaTimeSerializer::new(),
        }
    }
}

impl Default for AdvanceLWSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<AdvanceLW> for AdvanceLWSerializer {
    fn serialize(&self, value: &AdvanceLW, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // vote ratio
        self.ratio_serializer.serialize(&value.threshold, buffer)?;
        // now
        self.time_serializer.serialize(&value.now, buffer)?;
        Ok(())
    }
}

/// A Deserializer for `AdvanceLW`
pub struct AdvanceLWDeserializer {
    ratio_deserializer: RatioDeserializer<u64, U64VarIntDeserializer>,
    time_deserializer: MassaTimeDeserializer,
}

impl AdvanceLWDeserializer {
    /// Creates a new `AdvanceLWDeserializer`
    pub fn new() -> Self {
        Self {
            ratio_deserializer: RatioDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            )),
            time_deserializer: MassaTimeDeserializer::new((
                Included(MassaTime::from_millis(0)),
                Included(MassaTime::from_millis(u64::MAX)),
            )),
        }
    }
}

impl Default for AdvanceLWDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<AdvanceLW> for AdvanceLWDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AdvanceLW, E> {
        context(
            "Failed Advance deserialization",
            tuple((
                context("Failed threshold deserialization", |input| {
                    self.ratio_deserializer.deserialize(input)
                }),
                context("Failed now deserialization", |input| {
                    self.time_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(threshold, now)| AdvanceLW { threshold, now })
        .parse(buffer)
    }
}

// End AdvanceLW

// MipState

/// Serializer for `MipState`
pub struct MipStateSerializer {
    state_serializer: ComponentStateSerializer,
    advance_serializer: AdvanceLWSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl MipStateSerializer {
    /// Creates a new `MipStateSerializer`
    pub fn new() -> Self {
        Self {
            state_serializer: Default::default(),
            advance_serializer: Default::default(),
            u32_serializer: U32VarIntSerializer,
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
    advance_deserializer: AdvanceLWDeserializer,
    state_id_deserializer: U32VarIntDeserializer,
    u32_deserializer: U32VarIntDeserializer,
}

impl MipStateDeserializer {
    /// Creates a new `MipStateDeserializer`
    pub fn new() -> Self {
        Self {
            state_deserializer: ComponentStateDeserializer::new(),
            advance_deserializer: AdvanceLWDeserializer::new(),
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
                            // TEST NOTE: deser fails here for last two tests
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
                .collect::<BTreeMap<AdvanceLW, ComponentStateTypeId>>()
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
                    "Too many entries in MipStoreStats latest announcements, max: {}, received: {}",
                    entry_count_max, entry_count
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
            let entry_count_2_max =
                u32::try_from(MIP_STORE_STATS_BLOCK_CONSIDERED).map_err(|e| {
                    SerializeError::GeneralError(format!("Could not convert to u32: {}", e))
                })?;

            if entry_count_2 > entry_count_2_max {
                return Err(SerializeError::GeneralError(format!(
                    "Too many entries in MipStoreStats version counters, max: {}, received: {}",
                    entry_count_2_max, entry_count_2
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
    pub fn new(block_count_considered: usize, warn_announced_version_ratio: Ratio<u64>) -> Self {
        Self {
            config: MipStatsConfig {
                block_count_considered,
                warn_announced_version_ratio,
            },
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        }
    }
}

impl Deserializer<MipStoreStats> for MipStoreStatsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], MipStoreStats, E> {
        let cfg_block_considered: u32 =
            u32::try_from(self.config.block_count_considered).map_err(|_e| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Fail,
                ))
            })?;

        let (rem3, latest_announcements_) = context(
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
                    if count > cfg_block_considered {
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
                latest_announcements: latest_announcements_.into_iter().collect(),
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
                "Too many entries in VersioningStoreRaw, max: {}, received: {}",
                MIP_STORE_MAX_ENTRIES, entry_count
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
    entry_count_deserializer: U32VarIntDeserializer,
    info_deserializer: MipInfoDeserializer,
    state_deserializer: MipStateDeserializer,
    stats_deserializer: MipStoreStatsDeserializer,
}

impl MipStoreRawDeserializer {
    /// Creates a new ``
    pub fn new(block_count_considered: usize, warn_announced_version_ratio: Ratio<u64>) -> Self {
        Self {
            entry_count_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MIP_STORE_MAX_ENTRIES),
            ),
            info_deserializer: MipInfoDeserializer::new(),
            state_deserializer: MipStateDeserializer::new(),
            stats_deserializer: MipStoreStatsDeserializer::new(
                block_count_considered,
                warn_announced_version_ratio,
            ),
        }
    }
}

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
                        self.entry_count_deserializer.deserialize(input)
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

    use assert_matches::assert_matches;
    use std::mem::{size_of, size_of_val};

    use more_asserts::assert_lt;
    use num::rational::Ratio;
    use num::FromPrimitive;

    use crate::test_helpers::versioning_helpers::advance_state_until;

    use massa_serialization::DeserializeError;
    use massa_time::MassaTime;

    #[test]
    fn test_mip_component_non_exhaustive() {
        let last_variant__ = MipComponent::VARIANT_COUNT - 2; // -1 for Nonexhaustive, -1 for index start at 0
        let last_variant_ = u32::try_from(last_variant__).unwrap();
        let last_variant = MipComponent::from(last_variant_);

        match last_variant {
            MipComponent::__Nonexhaustive => {
                panic!("Should be a known enum value")
            }
            _ => {
                // all good
                println!("last variant of MipComponent is: {:?}", last_variant);
            }
        }

        {
            let variant__ = MipComponent::VARIANT_COUNT - 1;
            let variant_ = u32::try_from(variant__).unwrap();
            assert_matches!(MipComponent::from(variant_), MipComponent::__Nonexhaustive);
        }

        {
            let variant__ = MipComponent::VARIANT_COUNT;
            let variant_ = u32::try_from(variant__).unwrap();
            assert_matches!(MipComponent::from(variant_), MipComponent::__Nonexhaustive);
        }
    }

    #[test]
    fn test_mip_info_ser_der() {
        let mi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };

        let mut buf = Vec::new();
        let mip_info_ser = MipInfoSerializer::new();
        mip_info_ser.serialize(&mi_1, &mut buf).unwrap();

        let mip_info_der = MipInfoDeserializer::new();

        let (rem, mi_1_der) = mip_info_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(mi_1, mi_1_der);
    }

    #[test]
    fn test_mip_info_ser_der_err() {
        {
            // A MIP info with too many MIP Component
            let mi_1 = MipInfo {
                name: "MIP-0002".to_string(),
                version: 2,
                components: BTreeMap::from([
                    (MipComponent::Address, 1),
                    (MipComponent::KeyPair, 2),
                    (MipComponent::Block, 3),
                    (MipComponent::VM, 4),
                    (MipComponent::FinalStateHashKind, 5),
                    (MipComponent::__Nonexhaustive, 6),
                ]),
                start: MassaTime::from_millis(2),
                timeout: MassaTime::from_millis(5),
                activation_delay: MassaTime::from_millis(2),
            };

            {
                let mut buf = Vec::new();
                let mip_info_ser = MipInfoSerializer::new();
                mip_info_ser.serialize(&mi_1, &mut buf).unwrap();

                let mut mip_info_der = MipInfoDeserializer::new();
                // Allow only a max of 2 components per MIP info
                mip_info_der.components_len_deserializer =
                    U32VarIntDeserializer::new(Included(0), Included(2));

                let res = mip_info_der.deserialize::<DeserializeError>(&buf);
                assert!(res.is_err());
            }
        }

        {
            // A MIP info with a very long name
            let mi_2 = MipInfo {
                name: "a".repeat(MIP_INFO_NAME_MAX_LEN as usize + 1),
                version: 2,
                components: BTreeMap::from([(MipComponent::Address, 1)]),
                start: MassaTime::from_millis(2),
                timeout: MassaTime::from_millis(5),
                activation_delay: MassaTime::from_millis(2),
            };

            {
                let mut buf = Vec::new();
                let mip_info_ser = MipInfoSerializer::new();
                mip_info_ser.serialize(&mi_2, &mut buf).unwrap();

                let mip_info_der = MipInfoDeserializer::new();

                let res = mip_info_der.deserialize::<DeserializeError>(&buf);
                assert!(res.is_err());
            }
        }

        {
            // A MIP info, tweak to have an incorrect length (for name)

            let mip_info_name = "MIP-0002".to_string();
            let mi_1 = MipInfo {
                name: mip_info_name.clone(),
                version: 2,
                components: BTreeMap::from([
                    (MipComponent::Address, 1),
                    (MipComponent::KeyPair, 2),
                    (MipComponent::Block, 3),
                ]),
                start: MassaTime::from_millis(2),
                timeout: MassaTime::from_millis(5),
                activation_delay: MassaTime::from_millis(2),
            };

            let mut buf = Vec::new();
            let mip_info_ser = MipInfoSerializer::new();
            mip_info_ser.serialize(&mi_1, &mut buf).unwrap();

            assert_eq!(buf[0], mip_info_name.len() as u8);
            // Now name len will be way too long
            buf[0] = buf.len() as u8 + 1;
            let mip_info_der = MipInfoDeserializer::new();
            let res = mip_info_der.deserialize::<DeserializeError>(&buf);
            assert!(res.is_err());
        }
    }

    #[test]
    fn test_component_state_ser_der() {
        let state_ser = ComponentStateSerializer::new();
        let state_der = ComponentStateDeserializer::new();

        {
            let st_1 = ComponentState::failed();

            let mut buf = Vec::new();
            state_ser.serialize(&st_1, &mut buf).unwrap();

            let (rem, st_1_der) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

            assert!(rem.is_empty());
            assert_eq!(st_1, st_1_der);
        }

        {
            let st_2 = ComponentState::Started(Started {
                vote_ratio: Ratio::from_f32(0.9842).unwrap(),
            });

            let mut buf = Vec::new();
            state_ser.serialize(&st_2, &mut buf).unwrap();
            let (rem, st_2_der) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

            assert!(rem.is_empty());
            assert_eq!(st_2, st_2_der);
        }
    }

    #[test]
    fn test_component_state_ser_der_err() {
        let state_ser = ComponentStateSerializer::new();
        let state_der = ComponentStateDeserializer::new();

        let st_2 = ComponentState::Started(Started {
            vote_ratio: Ratio::from_f32(0.9842).unwrap(),
        });

        let mut buf = Vec::new();
        state_ser.serialize(&st_2, &mut buf).unwrap();
        // ComponentState is encoded as a u32 varint
        // Here by modifying the first value of buf, we set the ComponentState encoded value to
        // a unknown value
        buf[0] = 99;

        let res = state_der.deserialize::<DeserializeError>(&buf);
        assert!(res.is_err());
    }

    #[test]
    fn test_advance_ser_der() {
        let now = MassaTime::from_utc_ymd_hms(2017, 5, 11, 11, 33, 44).unwrap();

        let adv_lw = AdvanceLW {
            threshold: Default::default(),
            now,
        };

        let mut buf = Vec::new();
        let adv_ser = AdvanceLWSerializer::new();
        adv_ser.serialize(&adv_lw, &mut buf).unwrap();

        let state_der = AdvanceLWDeserializer::new();

        let (rem, adv_der) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(adv_lw, adv_der);
    }

    #[test]
    fn test_mip_state_ser_der() {
        let state_1 = MipState::new(MassaTime::from_millis(100));

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
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };

        let state_2 =
            advance_state_until(ComponentState::locked_in(MassaTime::from_millis(3)), &mi_1);
        state_ser.serialize(&state_2, &mut buf).unwrap();
        let (rem2, state_der_res) = state_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem2.is_empty());
        assert_eq!(state_2, state_der_res);
    }

    #[test]
    fn test_mip_store_stats_ser_der() {
        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
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
            mip_stats_cfg.warn_announced_version_ratio,
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
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };

        let mi_2 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from_millis(2),
            timeout: MassaTime::from_millis(5),
            activation_delay: MassaTime::from_millis(2),
        };

        let mi_3 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 3,
            components: BTreeMap::from([(MipComponent::Block, 1)]),
            start: MassaTime::from_millis(12),
            timeout: MassaTime::from_millis(17),
            activation_delay: MassaTime::from_millis(2),
        };

        let _time = MassaTime::now();
        let state_2 = advance_state_until(ComponentState::active(_time), &mi_2);
        let state_3 = advance_state_until(ComponentState::started(Ratio::new_raw(42, 100)), &mi_3);

        let store_raw =
            MipStoreRaw::try_from(([(mi_2, state_2), (mi_3, state_3)], mip_stats_cfg.clone()))
                .unwrap();

        let mut buf = Vec::new();
        let store_raw_ser = MipStoreRawSerializer::new();
        store_raw_ser.serialize(&store_raw, &mut buf).unwrap();

        let store_raw_der = MipStoreRawDeserializer::new(
            mip_stats_cfg.block_count_considered,
            mip_stats_cfg.warn_announced_version_ratio,
        );
        let (rem, store_raw_der_res) = store_raw_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(store_raw, store_raw_der_res);
    }

    #[test]
    #[ignore]
    fn mip_store_raw_max_size() {
        let mut mi_base = MipInfo {
            name: "A".repeat(254),
            version: 0,
            components: BTreeMap::from([(MipComponent::Address, 0)]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(2),
            activation_delay: MassaTime::from_millis(2),
        };

        // Note: we did not add the name ptr and hashmap ptr, only the data inside
        let mi_base_size = size_of_val(&mi_base.name[..])
            + size_of_val(&mi_base.version)
            + mi_base.components.len() * size_of::<u32>() * 2
            + size_of_val(&mi_base.start)
            + size_of_val(&mi_base.timeout);

        let mut all_state_size = 0;

        let _time = MassaTime::now();
        let store_raw_: Vec<(MipInfo, MipState)> = (0..MIP_STORE_MAX_ENTRIES)
            .map(|_i| {
                mi_base.version += 1;
                mi_base
                    .components
                    .entry(MipComponent::Address)
                    .and_modify(|e| *e += 1);
                mi_base.start = mi_base.timeout.saturating_add(MassaTime::from_millis(1));
                mi_base.timeout = mi_base.start.saturating_add(MassaTime::from_millis(2));

                let state = advance_state_until(ComponentState::active(_time), &mi_base);

                all_state_size += size_of_val(&state.state);
                all_state_size += state.history.len() * (size_of::<AdvanceLW>() + size_of::<u32>());

                (mi_base.clone(), state)
            })
            .collect();

        // Cannot use update_with ou try_from here as the names are not uniques
        let store_raw = MipStoreRaw {
            store: BTreeMap::from_iter(store_raw_),
            stats: MipStoreStats::new(MipStatsConfig {
                block_count_considered: 10,
                warn_announced_version_ratio: Ratio::new(30, 100),
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

        let store_raw_der = MipStoreRawDeserializer::new(10, Ratio::new(30, 100));
        let (rem, store_raw_der_res) = store_raw_der.deserialize::<DeserializeError>(&buf).unwrap();

        assert!(rem.is_empty());
        assert_eq!(store_raw, store_raw_der_res);
    }
}
