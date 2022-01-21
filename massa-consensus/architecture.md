# Archicture of block processing

## Local production

```mermaid
  journey
    title Life of a block(local production)
    section Factory
      Produce block: 7: Consensus, Storage, POS
      Notify new block: 7: Consensus
    section Consensus
      Process new block: 7: Storage
      Notify new graph: 7: Executor, Network
    section Executor
      Execute graph: 7: Storage
      Update ledger: 7: Storage, POS
```

### Factory

1. Produce block:
    - Get best parents from Consensus.
    - Get draws from POS
2. Notify new block:
    - Notify Consensus.
    
## Incoming from network

```mermaid
  journey
    title Life of a block(incoming from network)
    section Network
      Process block: 7: Storage, POS
      Notify new block: 7: Consensus
    section Consensus
      Process new block: 7: Storage
      Notify new graph: 7: Executor, Network
    section Executor
      Execute graph: 7: Storage
      Update ledger: 7: Storage, POS
```

### Network

1. Process block:
    - Get draws from POS.
    - Read and write blocks, operations, endorsements, to Storage
2. Notify new block:
    - Notify Consensus

## Shared by both pipelines

### Consensus
1. Process new block:
    - Get new block from Storage.
    - Process with graph.
2. Notify new graph to Executor and Network(for progagation).

### Executor
1. Execute graph
    - Get blocks from Storage.
2. Update ledger/final blocks
    - Update storage
    - Notify POS.

