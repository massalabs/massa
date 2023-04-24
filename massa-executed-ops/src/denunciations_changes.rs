//! Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_models::denunciation::{
    DenunciationIndex, DenunciationIndexDeserializer, DenunciationIndexSerializer,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use std::collections::HashSet;
use std::ops::Bound::Included;

/// Speculative changes for ExecutedOps
pub type ProcessedDenunciationsChanges = HashSet<DenunciationIndex>;

/// `ExecutedOps` Serializer
pub struct ProcessedDenunciationsChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    de_idx_serializer: DenunciationIndexSerializer,
}

impl Default for ProcessedDenunciationsChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessedDenunciationsChangesSerializer {
    /// Create a new `ProcessedDenunciations` Serializer
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            de_idx_serializer: DenunciationIndexSerializer::new(),
        }
    }
}

impl Serializer<ProcessedDenunciationsChanges> for ProcessedDenunciationsChangesSerializer {
    fn serialize(
        &self,
        value: &ProcessedDenunciationsChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        for de_idx in value {
            self.de_idx_serializer.serialize(de_idx, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `ExecutedOps`
pub struct ProcessedDenunciationsChangesDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    de_idx_deserializer: DenunciationIndexDeserializer,
}

impl ProcessedDenunciationsChangesDeserializer {
    /// Create a new deserializer for `ExecutedOps`
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_de_changes_length: u64,
    ) -> ProcessedDenunciationsChangesDeserializer {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_de_changes_length),
            ),
            de_idx_deserializer: DenunciationIndexDeserializer::new(
                thread_count,
                endorsement_count,
            ),
        }
    }
}

impl Deserializer<ProcessedDenunciationsChanges> for ProcessedDenunciationsChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ProcessedDenunciationsChanges, E> {
        context(
            "ProcessedDenunciationsChanges",
            length_count(
                context("ProcessedDenunciationsChanges length", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((context("denunciation index", |input| {
                    self.de_idx_deserializer.deserialize(input)
                }),)),
            ),
        )
        .map(|items| {
            // TODO: remove tuple ret
            items.into_iter().map(|(de_idx,)| de_idx).collect()
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use massa_models::config::{ENDORSEMENT_COUNT, MAX_DENUNCIATION_CHANGES_LENGTH, THREAD_COUNT};
    use massa_models::denunciation::Denunciation;
    use massa_models::test_exports::{
        gen_block_headers_for_denunciation, gen_endorsements_for_denunciation,
    };
    use massa_serialization::DeserializeError;

    #[test]
    fn test_processed_denunciations_changes_ser_der() {
        let (_, _, s_block_header_1, s_block_header_2, _) = gen_block_headers_for_denunciation();
        let denunciation_1: Denunciation =
            (&s_block_header_1, &s_block_header_2).try_into().unwrap();
        let denunciation_index_1 = DenunciationIndex::from(&denunciation_1);

        let (_, _, s_endorsement_1, s_endorsement_2, _) =
            gen_endorsements_for_denunciation(None, None);
        let denunciation_2 = Denunciation::try_from((&s_endorsement_1, &s_endorsement_2)).unwrap();
        let denunciation_index_2 = DenunciationIndex::from(&denunciation_2);

        let p_de_changes: ProcessedDenunciationsChanges =
            HashSet::from([(denunciation_index_1), (denunciation_index_2)]);

        let mut buffer = Vec::new();
        let p_de_ser = ProcessedDenunciationsChangesSerializer::new();
        p_de_ser.serialize(&p_de_changes, &mut buffer).unwrap();

        let p_de_der = ProcessedDenunciationsChangesDeserializer::new(
            THREAD_COUNT,
            ENDORSEMENT_COUNT,
            MAX_DENUNCIATION_CHANGES_LENGTH,
        );
        let (rem, p_de_changes_der_res) =
            p_de_der.deserialize::<DeserializeError>(&buffer).unwrap();

        assert!(rem.is_empty());
        assert_eq!(p_de_changes, p_de_changes_der_res);
    }
}
