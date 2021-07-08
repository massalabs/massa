# Introduction

The goal is to describe how data will be processed in consensus, and have a place to discuss what is possible.

We want to unify all existent data structures with blocks and/or headers inside one big hashmap. It will store `ConsensusBlock`s

```rust
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