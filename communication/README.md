Here is a proposition for block asking workflow.

For every node we have a list of blocks that we asked to that node, with a sequence number,
and a wishlist of blocks that node asked us.
To get next node sort them by
* if we already asked that block to that node
* if that node knows about that block
* number of time we asked that block to that node
* something else in case of tie ?

```mermaid
stateDiagram-v2
    state f2 <<fork>>
    [*] --> Ask: Ask for block
    [*] --> NodeDoesNotHaveBlock: received negative reply
    NodeDoesNotHaveBlock --> Asking: find next node
    [*] --> Timeout: Timeout expired while asking node for block
    [*] --> ReceivedBlock: Received block
    state f1 <<fork>>
    Ask --> f1: is it aready asked

    f1 --> [*]: yes - do nothing
    f1 --> Asking: no - get best node
    Asking --> [*]: Ask that node with given timout
    Asking --> [*]: insert hash to node's asked blocks
    Timeout --> f2: is it aready asked
    f2 --> [*]: no - do nothing
    f2 --> Asking: yes - find next node

    ReceivedBlock --> [*] : remove from already asked for every node
    ReceivedBlock --> [*] : add hash to known for that node

    [*] --> ReceivedHeader: Received header
    ReceivedHeader--> [*]: add hash to known for that node
```

When receiving a block from consensus for every node:
* if it is in node's wishlist, send it the full block
* if a node doesn't know about that block, send the header
* else do nothing

When a node ask us for a block:
* if we have it, send it
* else add it to node's wishlist, maybe ask it to someone else ?

What if a node ask us a block we consider stale, or invalid ?

```rust
struct NodeInfo {
    known_blocks: HashSet<Hash>,
    wanted_blocks: HashSet<Hash>,
    asked_blocks: HashMap<Hash, u64>,
}
```

## Second proposition
```mermaid
stateDiagram-v2
    NodeClosed --> UpdateAskBlock
    ReceivedBlock --> UpdateAskBlock
    ReceivedBlockNotFoundNotification -->ReceivedWishlist
    ReceivedWishlist --> UpdateAskBlock
    BlockAskTimeoutTick --> UpdateAskBlock

    state NodeClosed {
    [*] --> [*]: update ask block
    }

    state ReceivedBlock {
    [*] --> s1: update block knowledge
    s1 --> s1: ask and wait consensus wishlist
    s1 --> [*]: update ask block
    }

    state ReceivedBlockNotFoundNotification {
    [*] --> s2: update block knowledge
    s2 --> [*]: ask consensus wishlist
    }

    state ReceivedWishlist {
    [*] --> s3: if msg.seq_num > protocol.block_wishlist_seqnum 
    s3 --> s4: update protocol wishlist
    s4 --> [*]: update_ask_block
    }

    state UpdateAskBlock {
    [*] --> Node
    Node --> Node: Remove hashes that are not in wishlist
    Node --> Hash
    Hash --> MostRecentInstantAmongNodes: first this branch
    MostRecentInstantAmongNodes --> s5: if older than timeout
    s5 --> LowestNode: sort by Option<Instant>, knowledge
    LowestNode --> LowestNode: update asked block history
    LowestNode --> [*]: ask this node for that block
    Hash --> BiggestInstantAmongNodes: then, for every hash
    BiggestInstantAmongNodes --> NextTimeout: take the min for all hashes
    NextTimeout --> BlockAskTimeout: next_timeout + cfg.ask_block_timeout or none
    BlockAskTimeout --> [*]: sleep until block_ask_timeout
    }

    state BlockAskTimeoutTick {
    [*] --> [*]: update ask block
    }
```

Needed structures:
* protocol_worker.wishlist
* protocol_worker.wishlist_seq_number

```rust
struct NodeInfo {
    known_blocks: HashMap<Hash, bool>,
    wanted_blocks: HashSet<Hash>,
    asked_block_history: HashMap<Hash, Option<Instant>
}
```