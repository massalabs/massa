use massa_models::{
    active_block::ActiveBlock, address::Address, block::Block, block_header::SecuredHeader,
    block_id::BlockId, prehash::PreHashSet, slot::Slot,
};
use massa_storage::Storage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate)  enum HeaderOrBlock {
    Header(SecuredHeader),
    Block {
        id: BlockId,
        slot: Slot,
        storage: Storage,
    },
}

impl HeaderOrBlock {
    /// Gets slot for that header or block
    pub(crate)  fn get_slot(&self) -> Slot {
        match self {
            HeaderOrBlock::Header(header) => header.content.slot,
            HeaderOrBlock::Block { slot, .. } => *slot,
        }
    }
}

/// Something can be discarded
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate)  enum DiscardReason {
    /// Block is invalid, either structurally, or because of some incompatibility. The String contains the reason for info or debugging.
    Invalid(String),
    /// Block is incompatible with a final block.
    Stale,
    /// Block has enough fitness.
    Final,
}

/// Enum used in `BlockGraph`'s state machine
#[derive(Debug, Clone)]
pub(crate)  enum BlockStatus {
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
        storage: Storage,
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
pub(crate)  enum ExportBlockStatus {
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
pub(crate)  struct ExportCompiledBlock {
    /// Header of the corresponding block.
    pub(crate)  header: SecuredHeader,
    /// For (i, set) in children,
    /// set contains the headers' hashes
    /// of blocks referencing exported block as a parent,
    /// in thread i.
    pub(crate)  children: Vec<PreHashSet<BlockId>>,
    /// Active or final
    pub(crate)  is_final: bool,
}

/// Status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate)  enum Status {
    /// without enough fitness to be part of immutable history
    Active,
    /// with enough fitness to be part of immutable history
    Final,
}
