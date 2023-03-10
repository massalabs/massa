use massa_sc_runtime::{GasCosts, RuntimeModule};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

/// NOTE: previous type
pub type ModuleInfo = (RuntimeModule, Option<u64>);

/// NOTE: this will replace ModuleInfo
pub enum ModuleInfoBis {
    Invalid,
    Module(RuntimeModule),
    ModuleAndDelta((RuntimeModule, u64)),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
enum ModuleInfoId {
    Invalid = 0u64,
    Module = 1u64,
    ModuleAndDelta = 2u64,
}

/// Serializer
pub struct ModuleInfoSerializer {
    u64_ser: U64VarIntSerializer,
}

impl ModuleInfoSerializer {
    pub fn new() -> Self {
        Self {
            u64_ser: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<ModuleInfoBis> for ModuleInfoSerializer {
    fn serialize(&self, value: &ModuleInfoBis, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            ModuleInfoBis::Invalid => self
                .u64_ser
                .serialize(&u64::from(ModuleInfoId::Invalid), buffer)?,
            ModuleInfoBis::Module(rt_module) => {
                self.u64_ser
                    .serialize(&u64::from(ModuleInfoId::Module), buffer)?;
                let mut ser_module = rt_module
                    .serialize()
                    .map_err(|e| SerializeError::GeneralError(e.to_string()))?;
                buffer.append(&mut ser_module);
            }
            ModuleInfoBis::ModuleAndDelta((rt_module, delta)) => {
                self.u64_ser
                    .serialize(&u64::from(ModuleInfoId::ModuleAndDelta), buffer)?;
                self.u64_ser.serialize(delta, buffer)?;
                let mut ser_module = rt_module
                    .serialize()
                    .map_err(|e| SerializeError::GeneralError(e.to_string()))?;
                buffer.append(&mut ser_module);
            }
        }
        Ok(())
    }
}

/// Deserializer
pub struct ModuleInfoDeserializer {
    u64_deser: U64VarIntDeserializer,
    module_deser: RuntimeModuleDeserializer,
}

impl ModuleInfoDeserializer {
    pub fn new(limit: u64, gas_costs: GasCosts) -> Self {
        Self {
            u64_deser: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            module_deser: RuntimeModuleDeserializer::new(limit, gas_costs),
        }
    }
}

impl Deserializer<ModuleInfoBis> for ModuleInfoDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ModuleInfoBis, E> {
        context("ModuleInfo", |buffer| {
            let (input, id) = context("ModuleInfoId", |input| self.u64_deser.deserialize(input))
                .map(|id| {
                    ModuleInfoId::try_from(id).map_err(|_| {
                        nom::Err::Error(ParseError::from_error_kind(
                            buffer,
                            nom::error::ErrorKind::Eof,
                        ))
                    })
                })
                .parse(buffer)?;
            match id? {
                ModuleInfoId::Invalid => Ok((input, ModuleInfoBis::Invalid)),
                ModuleInfoId::Module => context("RuntimeModule", |input| {
                    self.module_deser.deserialize(input)
                })
                .map(|module| ModuleInfoBis::Module(module))
                .parse(input),
                ModuleInfoId::ModuleAndDelta => tuple((
                    context("Delta", |input| self.u64_deser.deserialize(input)),
                    context("RuntimeModule", |input| {
                        self.module_deser.deserialize(input)
                    }),
                ))
                .map(|(delta, module)| ModuleInfoBis::ModuleAndDelta((module, delta)))
                .parse(input),
            }
        })
        .parse(buffer)
    }
}

/// Sub Deserializer
pub struct RuntimeModuleDeserializer {
    limit: u64,
    gas_costs: GasCosts,
}

impl RuntimeModuleDeserializer {
    pub fn new(limit: u64, gas_costs: GasCosts) -> Self {
        Self { limit, gas_costs }
    }
}

impl Deserializer<RuntimeModule> for RuntimeModuleDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], RuntimeModule, E> {
        RuntimeModule::deserialize(buffer, self.limit, self.gas_costs.clone())
            .map(|module| (buffer, module))
            .map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })
    }
}
