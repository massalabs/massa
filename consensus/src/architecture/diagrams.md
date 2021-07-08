```mermaid
stateDiagram-v2
[*] --> HeaderCheck: phantom transition to make it pretty
HeaderCheck --> BlockCheck: phantom transition to make it pretty
BlockCheck --> ConsensusUpdate: phantom transition to make it pretty

    state HeaderCheck{
        [*] --> Header: Received Header
        Header --> Header: DB check
        Header --> InProcessingHeader: incomplete check
        InProcessingHeader --> [*]: Discarded
        InProcessingHeader --> [*]: Ask for parent
        [*] --> InProcessingHeader: Parent integrated in Consensus
        [*] --> InProcessingHeader: New slot
        InProcessingHeader --> InProcessingHeader: Check on event
        InProcessingHeader --> Header: ok
        Header --> CheckedHeader: ok
        CheckedHeader --> [*]: Ask for full block
        Header --> [*]: Discarded
        CheckedHeader --> [*]: Header ready
    }

state BlockCheck{    
    [*] --> Block: Received block
    Block --> [*]: Received Header
    Block --> [*]: Discarded
    [*] --> WaitingBlock: Header Ready
    Block --> WaitingBlock
    WaitingBlock --> WaitingBlock: Transaction check (see 0.3)
    WaitingBlock --> [*]: Discarded
    WaitingBlock --> CheckedBlock: transaction check ok
    CheckedBlock --> [*]: Block ready
}

state ConsensusUpdate  {
[*] --> CBlock: BlockChecked
    CBlock --> CBlock: Compute GP, Thread, etc incompatibilities
    CBlock --> [*]: Discarded
    CBlock --> Active: Integration
    Active --> [*]: Block integrated in Consensus
    Active --> Active: Update gi_head, cliques, final blocks, stale blocks
    Active --> [*]: Discarded 
    Active --> Final: finality reached
    Active --> Stale: incompatible with final block
    Stale --> [*]: Discarded
    Final --> Final: check if still needed
    Final --> [*]: Discarded
}

state Discard {
    [*] --> Still
    Still --> [*]
    Still --> Moving
    Moving --> Still
    Moving --> Crash
    Crash --> [*]
}
```