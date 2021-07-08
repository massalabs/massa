# Architecture idea for consensus blocks and headers management
WIP

## Introduction

The goal is to describe how data will be processed in consensus, and have a place to discuss what is possible.

We want to unify all existent data structures with blocks and/or headers inside one big hashmap. It will store `ConsensusBlock`s

```rust
struct BlockDatabase{
    blocks: HashMap<Hash, ConsensusBlock>,
    current_sequence_number: u64,
}

enum ConsensusBlock {
    HeaderOnly(Header, PendingAckBlockState),
    FullBlock(Block, PendingAckBlockState),
    Discarded(Header, DiscardReason),
    Genesis(Block),
    Active(CompiledBlock),
}

struct PendingAckBlockState {
    i_am_waiting_for: HashSet<Hash>,
    they_are_waiting_for_me: HashSet<Hash>,
    slot: Slot,
    sequence_number: u64,
}
```

## Consensus worker tokio select loop

### Slot tick

* if it is our turn, create block and rec_acknowledge it
* loop on block_db.full_blocks().filter(|block| block.1.1_am_waiting_for.len()==0 and block.1.slot.is_in_the_present_enough()) and rec_acknowledge these blocks
* reset timer

### Protocol event

### Detailed methods and functions

#### rec_acknowledge block
* start with the block to acknowledge in to_ack hashmap
* while there is a next block to acknowledge
    * remove that block from to_ack
    * acknoledge block
    * extend to_ack with to retry blocks
* if there is a storage command sender, send it blocks that became final

#### acknowledge block in consensus worker
* check if already in block_db, update sequence_number if needed
* go to block graph acknowledge block
    * if it is now an active block
        * discard waiting blocks that are older than latest_final_blocks_periods
        * propagate block
        * unlock dependencies that were relying on that block -> a hash map of blocks to retry
    * else
        * update dependencies and future waiting blocks
        * or do nothing if the block is too much in the future
        * or discarded it if it was already discarded, has invalid fields, draw mismathc, invalid parents, too old
        * or return an error if crypto, time, consensus or container inconsistency error occured.
* return blocks we could retry and blocks that became final


#### acknowledge block in block graph

#### unlock dependencies