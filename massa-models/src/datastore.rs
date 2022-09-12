use massa_serialization::{Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer};
use crate::serialization::{VecU8Deserializer, VecU8Serializer};
use nom::error::{context, ContextError, ParseError};
use nom::{IResult, Parser};
use nom::multi::length_count;
use nom::sequence::tuple;
use std::collections::BTreeMap;
use std::ops::Bound::Included;

pub type Datastore = BTreeMap<Vec<u8>, Vec<u8>>;

/// Serializer for `Datastore`
#[derive(Default)]
pub struct DatastoreSerializer {
    u64_serializer: U64VarIntSerializer,
    vec_u8_serializer: VecU8Serializer,
}

impl DatastoreSerializer {
    /// Creates a new `DatastoreSerializer`
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<Datastore> for DatastoreSerializer {
    /// ## Example
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_models::datastore::DatastoreSerializer;
    /// use massa_serialization::Serializer;
    ///
    /// let serializer = DatastoreSerializer::new();
    /// let mut buffer = Vec::new();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(vec![1, 2, 3], vec![4, 5, 6]);
    /// datastore.insert(vec![3, 4, 5], vec![6, 7, 8]);
    /// serializer.serialize(&datastore, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BTreeMap<Vec<u8>, Vec<u8>>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let entry_count: u64 = value.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!(
                "too many entries in ConsensusLedgerSubset: {}",
                err
            ))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (key, value) in value.iter() {
            self.vec_u8_serializer.serialize(key, buffer)?;
            self.vec_u8_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `Datastore` field in `LedgerEntry`
pub struct DatastoreDeserializer {
    length_deserializer: U64VarIntDeserializer,
    key_deserializer: VecU8Deserializer,
    value_deserializer: VecU8Deserializer,
}

impl DatastoreDeserializer {
    /// Creates a new `DatastoreDeserializer`
    pub fn new(
        max_datastore_entry_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
    ) -> Self {
        Self {
            length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_datastore_entry_count),
            ),
            key_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_key_length as u64),
            ),
            value_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_value_length),
            ),
        }
    }
}

impl Deserializer<Datastore> for DatastoreDeserializer {
    /// ## Example
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use massa_models::datastore::{DatastoreDeserializer, DatastoreSerializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let serializer = DatastoreSerializer::new();
    /// let deserializer = DatastoreDeserializer::new(10000, 255, 10000);
    /// let mut buffer = Vec::new();
    /// let mut datastore = BTreeMap::new();
    /// datastore.insert(vec![1, 2, 3], vec![4, 5, 6]);
    /// datastore.insert(vec![3, 4, 5], vec![6, 7, 8]);
    /// serializer.serialize(&datastore, &mut buffer).unwrap();
    /// let (rest, deserialized) = deserializer.deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized, datastore);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Vec<u8>, Vec<u8>>, E> {
        context(
            "Failed Datastore deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
                }),
                tuple((
                    context("Failed key deserialization", |input| {
                        self.key_deserializer.deserialize(input)
                    }),
                    context("Failed value deserialization", |input| {
                        self.value_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
            .map(|elements| elements.into_iter().collect())
            .parse(buffer)
    }
}
