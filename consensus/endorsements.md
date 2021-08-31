## Goal 

Endorsments are included in a block's header. They are created by randomly selected endorsers (may or may not be the same as block's creator) to endorse the block's parent in the same thread. The block's total fee and reward are splitted between that block creator, endorsers, and endorsed block's creator. With that mecanism it becomes harder to gain control of the network (you now have to control endorsement_count + 1 draw to gain control over one block) and to reward stakers more frequently.

```rust

pub struct BlockHeaderContent {
    pub endorsements: Vec<Endorsement>,
    ..
}
```

## Endorsement structure

This is how an endorsement is defined : 
```rust
pub struct Endorsement {
    pub content: EndorsementContent,
    pub signature: Signature,
}

pub struct EndorsementContent {
    /// Public key of the endorser.
    pub sender_public_key: PublicKey,
    /// slot of endorsed block (= parent in the same thread) (can be different that previous slot in the same thread)
    pub slot: Slot,
    /// endorsement index inside the block
    pub index: u32,
    /// hash of endorsed block (= parent in the same thread)
    pub endorsed_block: BlockId,
}
```

The endorser is selected to create an endorsement at a specific (slot, endorsement_index). To be included in a specif block, endorsed_block has to match that block's parent in the same thread, and slot has to match that parent's slot. The signature is produced signing the header_content with the sender_public_key.

## Endorsement production

Endorsements are automatically produced at every slot tick if a staking address of the node has been selected.

## Endorsement propagation

Once an endorsement is created by consensus, it is sent to the endorsement pool, where it is stored waiting for a request from consensus. Then it is sent to protocol and to every node that we don't know if it already has that endorsement. On the other hand, when receiving an endorsement from the network, protocol notes which node sent it, checks the signature and send it to the endorsement pool.

The endorsement pool is pruned if it reaches a max size and when new slots are becoming final.

## Endorsement integration

When creating a block consensus asks pool for endorsements specifying :
- target_slot: the slot of the parent in the same thread of the block been produced
- parent : that parent's BlockId
- creators : the ordrered addresses that were selected to create endorsements in for that slot

Pool responds with a vec of endorsement that is endorsement_count long or less, as some endorsements may be missing. That vec is ordered by endorsement index and when there are endorsement_count endorsement, that index should match the vec index.

## Reward computation

For each block reward R (constant or fee):

- 1/(1 + config.endorsement_count) to the block creator
- For each included endorsement:
    - 1/(3*(1 + config.endorsement_count)) to the block creator
    - 1/(3*(1 + config.endorsement_count)) to the endorsed block's creator
    - 1/(3*(1 + config.endorsement_count)) to the creator of the endorsement


Lossless rounding is done in favor of the block creator

## Fitness

The fitness of a block is (1 + number_of_included_endorsements)