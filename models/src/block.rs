use crate::{
    array_from_slice, u8_from_slice, DeserializeCompact, DeserializeMinBEInt, ModelsError,
    Operation, SerializationContext, SerializeCompact, SerializeMinBEInt, Slot, SLOT_KEY_SIZE,
};
use crypto::{
    hash::{Hash, HASH_SIZE_BYTES},
    signature::{
        PrivateKey, PublicKey, Signature, SignatureEngine, PUBLIC_KEY_SIZE_BYTES,
        SIGNATURE_SIZE_BYTES,
    },
};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeaderContent {
    pub creator: PublicKey,
    pub slot: Slot,
    pub parents: Vec<Hash>,
    pub out_ledger_hash: Hash,
    pub operation_merkle_root: Hash, // all operations hash
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub content: BlockHeaderContent,
    pub signature: Signature,
}

impl SerializeCompact for Block {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // header
        res.extend(self.header.to_bytes_compact(&context)?);

        // operations
        let operation_count: u32 = self.operations.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many operations: {:?}", err))
        })?;
        res.extend(operation_count.to_be_bytes_min(context.max_block_operations)?);
        for operation in self.operations.iter() {
            res.extend(operation.to_bytes_compact(&context)?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for Block {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // header
        let (header, delta) = BlockHeader::from_bytes_compact(&buffer[cursor..], &context)?;
        cursor += delta;
        if cursor > (context.max_block_size as usize) {
            return Err(ModelsError::DeserializeError("block is too large".into()));
        }

        // operations
        let (operation_count, delta) =
            u32::from_be_bytes_min(&buffer[cursor..], context.max_block_operations)?;
        cursor += delta;
        if cursor > (context.max_block_size as usize) {
            return Err(ModelsError::DeserializeError("block is too large".into()));
        }
        let mut operations: Vec<Operation> = Vec::with_capacity(operation_count as usize);
        for _ in 0..(operation_count as usize) {
            let (operation, delta) = Operation::from_bytes_compact(&buffer[cursor..], &context)?;
            cursor += delta;
            if cursor > (context.max_block_size as usize) {
                return Err(ModelsError::DeserializeError("block is too large".into()));
            }
            operations.push(operation);
        }

        Ok((Block { header, operations }, cursor))
    }
}

impl BlockHeader {
    // Hash([slot, hash])
    fn get_signature_message(slot: &Slot, hash: &Hash) -> Hash {
        let mut res = [0u8; SLOT_KEY_SIZE + HASH_SIZE_BYTES];
        res[..SLOT_KEY_SIZE].copy_from_slice(&slot.to_bytes_key());
        res[SLOT_KEY_SIZE..].copy_from_slice(&hash.to_bytes());
        // rehash for safety
        Hash::hash(&res)
    }

    // check if a [slot, hash] pair was signed by a public_key
    pub fn verify_slot_hash_signature(
        slot: &Slot,
        hash: &Hash,
        signature: &Signature,
        sig_engine: &SignatureEngine,
        public_key: &PublicKey,
    ) -> Result<(), ModelsError> {
        Ok(sig_engine.verify(
            &BlockHeader::get_signature_message(slot, hash),
            signature,
            public_key,
        )?)
    }

    pub fn new_signed(
        sig_engine: &mut SignatureEngine,
        private_key: &PrivateKey,
        content: BlockHeaderContent,
        context: &SerializationContext,
    ) -> Result<(Hash, Self), ModelsError> {
        let hash = content.compute_hash(&context)?;
        let signature = sig_engine.sign(
            &BlockHeader::get_signature_message(&content.slot, &hash),
            private_key,
        )?;
        Ok((hash, BlockHeader { content, signature }))
    }

    pub fn verify_signature(
        &self,
        hash: &Hash,
        sig_engine: &SignatureEngine,
    ) -> Result<(), ModelsError> {
        BlockHeader::verify_slot_hash_signature(
            &self.content.slot,
            hash,
            &self.signature,
            sig_engine,
            &self.content.creator,
        )
    }
}

impl SerializeCompact for BlockHeader {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // signed content
        res.extend(self.content.to_bytes_compact(&context)?);

        // signature
        res.extend(&self.signature.to_bytes());

        Ok(res)
    }
}

impl DeserializeCompact for BlockHeader {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // signed content
        let (content, delta) = BlockHeaderContent::from_bytes_compact(&buffer[cursor..], &context)?;
        cursor += delta;

        // signature
        let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += SIGNATURE_SIZE_BYTES;

        Ok((BlockHeader { content, signature }, cursor))
    }
}

impl BlockHeaderContent {
    pub fn compute_hash(&self, context: &SerializationContext) -> Result<Hash, ModelsError> {
        Ok(Hash::hash(&self.to_bytes_compact(&context)?))
    }
}

impl SerializeCompact for BlockHeaderContent {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // creator public key
        res.extend(&self.creator.to_bytes());

        // slot
        res.extend(self.slot.to_bytes_compact(&context)?);

        // parents (note: there should be none if slot period=0)
        // parents (note: there should be none if slot period=0)
        if self.parents.len() == 0 {
            res.push(0);
        } else {
            res.push(1);
        }
        for parent_h in self.parents.iter() {
            res.extend(&parent_h.to_bytes());
        }

        // output ledger hash
        res.extend(&self.out_ledger_hash.to_bytes());

        // operations merkle root
        res.extend(&self.operation_merkle_root.to_bytes());

        Ok(res)
    }
}

impl DeserializeCompact for BlockHeaderContent {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // creator public key
        let creator = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += PUBLIC_KEY_SIZE_BYTES;

        // slot
        let (slot, delta) = Slot::from_bytes_compact(&buffer[cursor..], &context)?;
        cursor += delta;

        // parents
        let has_parents = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        let parents = if has_parents == 1 {
            let mut parents: Vec<Hash> = Vec::with_capacity(context.parent_count as usize);
            for _ in 0..context.parent_count {
                let parent_h = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                parents.push(parent_h);
            }
            parents
        } else if has_parents == 0 {
            Vec::new()
        } else {
            return Err(ModelsError::SerializeError(
                "BlockHeaderContent from_bytes_compact bad hasparents flags.".into(),
            ));
        };

        // output ledger hash
        let out_ledger_hash = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += HASH_SIZE_BYTES;

        // operation merkle tree root
        let operation_merkle_root = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += HASH_SIZE_BYTES;

        Ok((
            BlockHeaderContent {
                creator,
                slot,
                parents,
                out_ledger_hash,
                operation_merkle_root,
            },
            cursor,
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_block_serialization() {
        let ctx = SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 3,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_bootstrap_message_size: 100000000,
        };
        let mut sig_engine = SignatureEngine::new();
        let private_key = SignatureEngine::generate_random_private_key();
        let public_key = sig_engine.derive_public_key(&private_key);

        // create block header
        let (orig_hash, orig_header) = BlockHeader::new_signed(
            &mut sig_engine,
            &private_key,
            BlockHeaderContent {
                creator: public_key,
                slot: Slot::new(1, 2),
                parents: vec![
                    Hash::hash("abc".as_bytes()),
                    Hash::hash("def".as_bytes()),
                    Hash::hash("ghi".as_bytes()),
                ],
                out_ledger_hash: Hash::hash("jkl".as_bytes()),
                operation_merkle_root: Hash::hash("mno".as_bytes()),
            },
            &ctx,
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header,
            operations: vec![],
        };

        // serialize block
        let orig_bytes = orig_block.to_bytes_compact(&ctx).unwrap();

        // deserialize
        let (res_block, res_size) = Block::from_bytes_compact(&orig_bytes, &ctx).unwrap();
        assert_eq!(orig_bytes.len(), res_size);

        // check equality
        let res_hash = res_block.header.content.compute_hash(&ctx).unwrap();
        assert_eq!(orig_hash, res_hash);
        assert_eq!(res_block.header.signature, orig_block.header.signature);
    }
}
