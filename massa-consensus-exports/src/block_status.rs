use massa_models::{
    active_block::ActiveBlock,
    address::Address,
    block::{Block, SecureShareBlock},
    block_header::SecuredHeader,
    block_id::BlockId,
    prehash::PreHashSet,
    slot::Slot,
};
use massa_storage::Storage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum HeaderOrBlock {
    Header(SecuredHeader),
    Block {
        id: BlockId,
        slot: Slot,
        storage: Storage,
    },
}

impl HeaderOrBlock {
    /// Gets slot for that header or block
    pub fn get_slot(&self) -> Slot {
        match self {
            HeaderOrBlock::Header(header) => header.content.slot,
            HeaderOrBlock::Block { slot, .. } => *slot,
        }
    }
}

/// Something can be discarded
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardReason {
    /// Block is invalid, either structurally, or because of some incompatibility. The String contains the reason for info or debugging.
    Invalid(String),
    /// Block is incompatible with a final block.
    Stale,
    /// Block has enough fitness.
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockStatusId {
    Incoming = 0,
    WaitingForSlot = 1,
    WaitingForDependencies = 2,
    Active = 3,
    Discarded = 4,
}

impl From<&BlockStatus> for BlockStatusId {
    fn from(status: &BlockStatus) -> Self {
        match status {
            BlockStatus::Incoming(_) => BlockStatusId::Incoming,
            BlockStatus::WaitingForSlot(_) => BlockStatusId::WaitingForSlot,
            BlockStatus::WaitingForDependencies { .. } => BlockStatusId::WaitingForDependencies,
            BlockStatus::Active { .. } => BlockStatusId::Active,
            BlockStatus::Discarded { .. } => BlockStatusId::Discarded,
        }
    }
}

/// A structure defining whether we keep a full block with operations in storage
/// or just the raw signed block without operations
#[derive(Debug, Clone)]
pub enum StorageOrBlock {
    /// Keep a full storage with operations
    Storage(Storage),
    /// Keep only the block header and list of ops (but not the ops)
    Block(Box<SecureShareBlock>),
}

impl StorageOrBlock {
    /// Return a clone of the underlying block
    pub fn clone_block(&self, block_id: &BlockId) -> SecureShareBlock {
        match self {
            StorageOrBlock::Storage(storage) => storage
                .read_blocks()
                .get(block_id)
                .expect("block absent from its own storage")
                .clone(),
            StorageOrBlock::Block(block) => *block.clone(),
        }
    }

    /// Convert any StorageOrBlock variant into a StorageOrBlock::Block variant.
    /// This effectively drops the operations of the block.
    pub fn strip_to_block(&mut self, block_id: &BlockId) {
        let block = if let StorageOrBlock::Storage(storage) = self {
            storage
                .read_blocks()
                .get(block_id)
                .expect("block absent from its own storage")
                .clone()
        } else {
            return;
        };
        *self = StorageOrBlock::Block(Box::new(block));
    }
}

/// Enum used in `BlockGraph`'s state machine
#[derive(Debug, Clone)]
pub enum BlockStatus {
    /// The block/header has reached consensus but no consensus-level check has been performed.
    /// It will be processed during the next iteration
    Incoming(HeaderOrBlock),
    /// The block's or header's slot is too much in the future.
    /// It will be processed at the block/header slot
    WaitingForSlot(HeaderOrBlock),
    /// The block references an unknown Block id
    WaitingForDependencies {
        /// Given header/block
        header_or_block: HeaderOrBlock,
        /// includes self if it's only a header
        unsatisfied_dependencies: PreHashSet<BlockId>,
        /// Used to limit and sort the number of blocks/headers waiting for dependencies
        sequence_number: u64,
    },
    /// The block was checked and included in the blockgraph
    Active {
        a_block: Box<ActiveBlock>,
        storage_or_block: StorageOrBlock,
    },
    /// The block was discarded and is kept to avoid reprocessing it
    Discarded {
        /// Just the slot of that block
        slot: Slot,
        /// Address of the creator of the block
        creator: Address,
        /// Ids of parents blocks
        parents: Vec<BlockId>,
        /// why it was discarded
        reason: DiscardReason,
        /// Used to limit and sort the number of blocks/headers waiting for dependencies
        sequence_number: u64,
    },
}

/// Block status in the graph that can be exported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportBlockStatus {
    /// received but not yet graph processed
    Incoming,
    /// waiting for its slot
    WaitingForSlot,
    /// waiting for a missing dependency
    WaitingForDependencies,
    /// valid and not yet final
    Active(Block),
    /// immutable
    Final(Block),
    /// not part of the graph
    Discarded(DiscardReason),
}

/// The block version that can be exported.
/// Note that the detailed list of operation is not exported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCompiledBlock {
    /// Header of the corresponding block.
    pub header: SecuredHeader,
    /// For (i, set) in children,
    /// set contains the headers' hashes
    /// of blocks referencing exported block as a parent,
    /// in thread i.
    pub children: Vec<PreHashSet<BlockId>>,
    /// Active or final
    pub is_final: bool,
}

/// Status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    /// without enough fitness to be part of immutable history
    Active,
    /// with enough fitness to be part of immutable history
    Final,
}
