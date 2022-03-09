// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::{BLOCK_ID_SIZE_BYTES, SLOT_KEY_SIZE};
use crate::prehash::{Map, PreHashed, Set};
use crate::signed::{Id, Signable, Signed};
use crate::{
    array_from_slice, u8_from_slice, with_serialization_context, Address, DeserializeCompact,
    DeserializeMinBEInt, DeserializeVarInt, Endorsement, EndorsementId, ModelsError, Operation,
    OperationId, SerializeCompact, SerializeMinBEInt, SerializeVarInt, SignedEndorsement,
    SignedOperation, Slot,
};
use massa_hash::hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_signature::{PublicKey, PUBLIC_KEY_SIZE_BYTES};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt::Formatter;
use std::str::FromStr;

const BLOCK_ID_STRING_PREFIX: &str = "BLO";

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct BlockId(pub Hash);

impl PreHashed for BlockId {}
impl Id for BlockId {
    fn new(hash: Hash) -> Self {
        BlockId(hash)
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", BLOCK_ID_STRING_PREFIX, self.0.to_bs58_check())
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl std::fmt::Debug for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", BLOCK_ID_STRING_PREFIX, self.0.to_bs58_check())
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl FromStr for BlockId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(BlockId(Hash::from_str(s)?))
            } else if v[0] != BLOCK_ID_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    BLOCK_ID_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(BlockId(Hash::from_str(v[1])?))
            }
        } else {
            Ok(BlockId(Hash::from_str(s)?))
        }
    }
}

impl BlockId {
    pub fn to_bytes(&self) -> [u8; BLOCK_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    pub fn into_bytes(self) -> [u8; BLOCK_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    pub fn from_bytes(data: &[u8; BLOCK_ID_SIZE_BYTES]) -> Result<BlockId, ModelsError> {
        Ok(BlockId(
            Hash::from_bytes(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
    pub fn from_bs58_check(data: &str) -> Result<BlockId, ModelsError> {
        Ok(BlockId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    pub fn get_first_bit(&self) -> bool {
        Hash::compute_from(&self.to_bytes()).to_bytes()[0] >> 7 == 1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: SignedHeader,
    pub operations: Vec<SignedOperation>,
}

impl Block {
    pub fn contains_operation(&self, op: SignedOperation) -> Result<bool, ModelsError> {
        let op_id = op.content.compute_id()?;
        Ok(self.operations.iter().any(|o| {
            o.content
                .compute_id()
                .map(|id| id == op_id)
                .unwrap_or(false)
        }))
    }

    pub fn bytes_count(&self) -> Result<u64, ModelsError> {
        Ok(self.to_bytes_compact()?.len() as u64)
    }

    /// Retrieve roll involving addresses
    pub fn get_roll_involved_addresses(&self) -> Result<Set<Address>, ModelsError> {
        let mut roll_involved_addrs = Set::<Address>::default();
        for op in self.operations.iter() {
            roll_involved_addrs.extend(op.content.get_roll_involved_addresses()?);
        }
        Ok(roll_involved_addrs)
    }

    /// retrieves a mapping of addresses to the list of operation IDs they are involved with in terms of ledger
    pub fn involved_addresses(
        &self,
        operation_set: &Map<OperationId, (usize, u64)>,
    ) -> Result<Map<Address, Set<OperationId>>, ModelsError> {
        let mut addresses_to_operations: Map<Address, Set<OperationId>> =
            Map::<Address, Set<OperationId>>::default();
        operation_set
            .iter()
            .try_for_each::<_, Result<(), ModelsError>>(|(op_id, (op_idx, _op_expiry))| {
                let op = &self.operations[*op_idx];
                let addrs = op.content.get_ledger_involved_addresses().map_err(|err| {
                    ModelsError::DeserializeError(format!(
                        "could not get involved addresses: {}",
                        err
                    ))
                })?;
                for ad in addrs.into_iter() {
                    if let Some(entry) = addresses_to_operations.get_mut(&ad) {
                        entry.insert(*op_id);
                    } else {
                        let mut set = Set::<OperationId>::default();
                        set.insert(*op_id);
                        addresses_to_operations.insert(ad, set);
                    }
                }
                Ok(())
            })?;
        Ok(addresses_to_operations)
    }

    pub fn addresses_to_endorsements(
        &self,
        _endo: &Map<EndorsementId, u32>,
    ) -> Result<Map<Address, Set<EndorsementId>>, ModelsError> {
        let mut res: Map<Address, Set<EndorsementId>> = Map::default();
        self.header
            .content
            .endorsements
            .iter()
            .try_for_each::<_, Result<(), ModelsError>>(|e| {
                let address = Address::from_public_key(&e.content.sender_public_key);
                if let Some(old) = res.get_mut(&address) {
                    old.insert(e.content.compute_id()?);
                } else {
                    let mut set = Set::<EndorsementId>::default();
                    set.insert(e.content.compute_id()?);
                    res.insert(address, set);
                }
                Ok(())
            })?;
        Ok(res)
    }
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.header)?;
        writeln!(
            f,
            "Operations: {}",
            self.operations
                .iter()
                .map(|op| format!("{}", op))
                .collect::<Vec<String>>()
                .join(" ")
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub creator: PublicKey,
    pub slot: Slot,
    pub parents: Vec<BlockId>,
    pub operation_merkle_root: Hash, // all operations hash
    pub endorsements: Vec<SignedEndorsement>,
}

impl Signable<BlockId> for BlockHeader {
    fn get_signature_message(&self) -> Result<Hash, ModelsError> {
        let hash = self.compute_hash()?;
        let mut res = [0u8; SLOT_KEY_SIZE + BLOCK_ID_SIZE_BYTES];
        res[..SLOT_KEY_SIZE].copy_from_slice(&self.slot.to_bytes_key());
        res[SLOT_KEY_SIZE..].copy_from_slice(&hash.to_bytes());
        // rehash for safety
        Ok(Hash::compute_from(&res))
    }
}

pub type SignedHeader = Signed<BlockHeader, BlockId>;

impl std::fmt::Display for BlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let pk = self.creator.to_string();
        writeln!(f, "\tCreator: {}", pk)?;
        writeln!(
            f,
            "\t(period: {}, thread: {})",
            self.slot.period, self.slot.thread,
        )?;
        writeln!(f, "\tMerkle root: {}", self.operation_merkle_root,)?;
        writeln!(f, "\tParents: ")?;
        for id in self.parents.iter() {
            let str_id = id.to_string();
            writeln!(f, "\t\t{}", str_id)?;
        }
        if self.parents.is_empty() {
            writeln!(f, "No parents found: This is a genesis header")?;
        }
        writeln!(f, "\tEndorsements:")?;
        for ed in self.endorsements.iter() {
            writeln!(f, "\t\t-----")?;
            writeln!(
                f,
                "\t\tId: {}",
                ed.content.compute_id().map_err(|_| std::fmt::Error)?
            )?;
            writeln!(f, "\t\tIndex: {}", ed.content.index)?;
            writeln!(f, "\t\tEndorsed slot: {}", ed.content.slot)?;
            writeln!(
                f,
                "\t\tEndorser's public key: {}",
                ed.content.sender_public_key
            )?;
            writeln!(f, "\t\tEndorsed block: {}", ed.content.endorsed_block)?;
            writeln!(f, "\t\tSignature: {}", ed.signature)?;
        }
        if self.endorsements.is_empty() {
            writeln!(f, "\tNo endorsements found")?;
        }
        Ok(())
    }
}

/// Checks performed:
/// - Validity of header.
/// - Number of operations.
/// - Validity of operations.
impl SerializeCompact for Block {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // header
        res.extend(self.header.to_bytes_compact()?);

        let max_block_operations =
            with_serialization_context(|context| context.max_operations_per_block);

        // operations
        let operation_count: u32 =
            self.operations.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!("too many operations: {}", err))
            })?;
        res.extend(operation_count.to_be_bytes_min(max_block_operations)?);
        for operation in self.operations.iter() {
            res.extend(operation.to_bytes_compact()?);
        }

        Ok(res)
    }
}

/// Checks performed:
/// - Validity of header.
/// - Size of block.
/// - Operation count.
/// - Validity of operation.
impl DeserializeCompact for Block {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (max_block_size, max_block_operations) = with_serialization_context(|context| {
            (context.max_block_size, context.max_operations_per_block)
        });

        // header
        let (header, delta) =
            Signed::<BlockHeader, BlockId>::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;
        if cursor > (max_block_size as usize) {
            return Err(ModelsError::DeserializeError("block is too large".into()));
        }

        // operations
        let (operation_count, delta) =
            u32::from_be_bytes_min(&buffer[cursor..], max_block_operations)?;
        cursor += delta;
        if cursor > (max_block_size as usize) {
            return Err(ModelsError::DeserializeError("block is too large".into()));
        }
        let mut operations: Vec<SignedOperation> = Vec::with_capacity(operation_count as usize);
        for _ in 0..(operation_count as usize) {
            let (operation, delta) =
                Signed::<Operation, OperationId>::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            if cursor > (max_block_size as usize) {
                return Err(ModelsError::DeserializeError("block is too large".into()));
            }
            operations.push(operation);
        }

        Ok((Block { header, operations }, cursor))
    }
}

impl BlockHeader {
    pub fn compute_hash(&self) -> Result<Hash, ModelsError> {
        Ok(Hash::compute_from(&self.to_bytes_compact()?))
    }
}

/// Checks performed:
/// - Validity of slot.
/// - Valid length of included endorsements.
/// - Validity of included endorsements.
impl SerializeCompact for BlockHeader {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // creator public key
        res.extend(&self.creator.to_bytes());

        // slot
        res.extend(self.slot.to_bytes_compact()?);

        // parents (note: there should be none if slot period=0)
        if self.parents.is_empty() {
            res.push(0);
        } else {
            res.push(1);
        }
        for parent_h in self.parents.iter() {
            res.extend(&parent_h.0.to_bytes());
        }

        // operations merkle root
        res.extend(&self.operation_merkle_root.to_bytes());

        // endorsements
        let endorsements_count: u32 = self.endorsements.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many endorsements: {}", err))
        })?;
        res.extend(endorsements_count.to_varint_bytes());
        for endorsement in self.endorsements.iter() {
            res.extend(endorsement.to_bytes_compact()?);
        }

        Ok(res)
    }
}

/// Checks performed:
/// - Validity of slot.
/// - Presence of parent.
/// - Valid length of included endorsements.
/// - Validity of included endorsements.
impl DeserializeCompact for BlockHeader {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // creator public key
        let creator = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // slot
        let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // parents
        let has_parents = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        let parent_count = with_serialization_context(|context| context.thread_count);
        let parents = if has_parents == 1 {
            let mut parents: Vec<BlockId> = Vec::with_capacity(parent_count as usize);
            for _ in 0..parent_count {
                let parent_id = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += BLOCK_ID_SIZE_BYTES;
                parents.push(parent_id);
            }
            parents
        } else if has_parents == 0 {
            Vec::new()
        } else {
            return Err(ModelsError::SerializeError(
                "BlockHeader from_bytes_compact bad has parents flags.".into(),
            ));
        };

        // operation merkle tree root
        let operation_merkle_root = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += HASH_SIZE_BYTES;

        let max_block_endorsements =
            with_serialization_context(|context| context.endorsement_count);

        // endorsements
        let (endorsement_count, delta) =
            u32::from_varint_bytes_bounded(&buffer[cursor..], max_block_endorsements)?;
        cursor += delta;

        let mut endorsements = Vec::with_capacity(endorsement_count as usize);
        for _ in 0..endorsement_count {
            let (endorsement, delta) =
                Signed::<Endorsement, EndorsementId>::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            endorsements.push(endorsement);
        }

        Ok((
            BlockHeader {
                creator,
                slot,
                parents,
                operation_merkle_root,
                endorsements,
            },
            cursor,
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Endorsement;
    use massa_signature::{derive_public_key, generate_random_private_key};
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_block_serialization() {
        let ctx = crate::SerializationContext {
            max_block_size: 1024 * 1024,
            max_operations_per_block: 1024,
            thread_count: 3,
            max_advertise_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_bootstrap_pos_cycles: 1000,
            max_bootstrap_pos_entries: 1000,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_bootstrap_message_size: 100000000,
            endorsement_count: 8,
        };
        crate::init_serialization_context(ctx);
        let private_key = generate_random_private_key();
        let public_key = derive_public_key(&private_key);

        // create block header
        let (orig_id, orig_header) = Signed::new_signed(
            BlockHeader {
                creator: public_key,
                slot: Slot::new(1, 2),
                parents: vec![
                    BlockId(Hash::compute_from("abc".as_bytes())),
                    BlockId(Hash::compute_from("def".as_bytes())),
                    BlockId(Hash::compute_from("ghi".as_bytes())),
                ],
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![
                    Signed::new_signed(
                        Endorsement {
                            sender_public_key: public_key,
                            slot: Slot::new(1, 1),
                            index: 1,
                            endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                        },
                        &private_key,
                    )
                    .unwrap()
                    .1,
                    Signed::new_signed(
                        Endorsement {
                            sender_public_key: public_key,
                            slot: Slot::new(4, 0),
                            index: 3,
                            endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
                        },
                        &private_key,
                    )
                    .unwrap()
                    .1,
                ],
            },
            &private_key,
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header,
            operations: vec![],
        };

        // serialize block
        let orig_bytes = orig_block.to_bytes_compact().unwrap();

        // deserialize
        let (res_block, res_size) = Block::from_bytes_compact(&orig_bytes).unwrap();
        assert_eq!(orig_bytes.len(), res_size);

        // check equality
        let res_id = res_block.header.content.compute_id().unwrap();
        let generated_res_id = res_block.header.content.compute_id().unwrap();
        assert_eq!(orig_id, res_id);
        assert_eq!(orig_id, generated_res_id);
        assert_eq!(res_block.header.signature, orig_block.header.signature);
    }
}
