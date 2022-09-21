============
Endorsements
============

Intro
=====


Massa uses the Proof-of-Stake selection mechanism with Nakamoto consensus.
In that context, when there are multiple cliques in close competition,
we want all nodes to converge towards a single clique as fast as possible to minimize finality time and maximize the quality of the consensus.
To achieve this, we draw inspiration from Tezos and introduce the concept of Endorsement.

Basic principle
===============

At every slot `S`, we use the existing Proof-of-Stake selection mechanism to not only draw the block creator for that slot,
but also `E` other stakers that are responsible for creating Endorsements that can be included in the blocks of slot `S`.
Every endorsement that is selected to be included in blocks of slot `S` is signed by its creator `C`
and contains the "endorsed" hash of the block that `C` would choose as the parent of a block at slot `S` in thread `S.thread`.
Blocks in slot `S` can only include endorsements that endorse the block's parent block in the thread of `S`.
In other words, endorsement producers vote for which parent should be chosen by blocks in their own thread.

The likelihood of the attacker getting lucky and being selected for `N` consecutive PoS draws to attack/censor the system decays exponentially.
With endorsements, we don't have to wait for `N` blocks to account for `N` proof-of-stake draws to happen as `E+1` draws happen at every slot.
In the consensus algorithm, we choose the clique of highest fitness as the blockclique.
A block including `e` endorsements out of the maximum `E` contributes a fitness `e + 1` to the cliques it belongs to.
The fitness of a block is therefore reflected by the number of PoS draws that were involved in creating it.

The net effect of this mechanism is to increase safety and convergence speed
by allowing block producers to quickly choose the best clique to extend according to the "votes" provided by the endorsements.


Structure of an endorsement
===========================

.. code:: rust

  pub struct Endorsement {
    /// slot in which the endorsement can be included
    pub slot: Slot,
    /// endorsement index inside the including block
    pub index: u32,
    /// hash of endorsed block
    pub endorsed_block: BlockId,
  }


Note that the `WrappedEndorsement` structure includes the underlying `Endorsement` as well as the signature, and the public key of the endorsement producer.

Within a block, endorsements are fully included inside the header.

A header is invalidated if:

* it contains strictly more than `E` endorsements
* at least one of the endorsements fails deserialization or signature verification
* at least one of the endorsements endorses a block different than the parent of the including block within its own thread
* any of the endorsements should not have been produced at that `(endorsement.slot, endorsement.index)` according to the selector
* there is strictly more than one endorsement with a given `endorsement.index` 

Lifecycle of an endorsement
===========================

To produce endorsements for slot `S`, the Endorsement Factory wakes up at `timestamp(S) - t0/2` so that the previous block of thread `S.thread` had the time to propagate,
and so that the endorsement itself has the time to propagate to be included in blocks of slot `S`.
It then checks the endorsement producer draws for slot `S`. At every slot, there are `E` endorsement producer draws, one for each endorsement index from 0 (included) to `E-1` (included).
The factory will attempt to create all the endorsements that need to be produced by keypairs its wallet manages.
To choose the block to endorse, the factory asks Consensus for the ID of latest blockclique (or final) block `B` in thread `S.thread` that has a strictly lower period than `S.period`.
Every created endorsement is then sent to the Endorsement Pool for future inclusion in blocks, and to Protocol for propagation to other nodes.

In Protocol, endorsements can be received from other modules, in which case they are propagated.
They can also be received from other nodes, in which case they added to the Endorsement Pool and propagated.
Endorsements are propagated only to nodes that don't already know about them (including inside block headers).

The Endorsement Pool stores a finite number of endorsements that can potentially be included in future blocks created by the node.
Consensus notifies the Endorsement pool of newly finalized blocks,
which allows the pool to eliminate endorsements that can only be included in already-finalized slots and are therefore not useful anymore.

When the Block Factory produces a block and needs to fill its header with endorsements,
it asks the Endorsement Pool for the endorsements that can be included in the block's slot and that endorse the block's parent in its own thread.

Incentives and penalties
========================

There needs to be an incentive in:

* creating blocks that can be endorsed, and also avoid publishing them too late so that endorsers have the time to endorse them
* creating and propagating endorsements, also doing so not too early in order to endorse the most recent block, and not too late for subsequent blocks to be able to include the endorsement
* including endorsements in blocks being created, and also not publishing them too early to include as many endorsements as possible

To achieve this, we note `R` the total amount of coin revenue generated by the block: the sum of the per-block monetary creation, and all operation fees.
We then split `R` into `1+E` equal parts called `r = R/(1+E)`.

* `r` is given to the block creator to motivate block creation even if there are no endorsements available
* for each successfully included endorsement:

  - `r/3` is given to the block creator to motivate endorsement inclusion
  - `r/3` is given to the endorsement creator to motivate endorsement creation
  - `r/3` is given to the creator of the endorsed block to motivate the timely emission of endorsable blocks

Note that this split also massively increases the frequency at which stakers receive coins, which reduces the incentive to create staking pools.


Choosing the value of `E`
=========================


TODO  delta f0, choice of E justification


TODOS
=====

* check the PoS draw in Protocol to avoid propagating/storing endorsements with the wrong draw: https://github.com/massalabs/massa/issues/3020
* only store the public key and the signature (not the full endorsement) in block headers, everything else is already in the header: https://github.com/massalabs/massa/issues/3021
* use pool endorsements to choose the best parents in consensus: second part of https://github.com/massalabs/massa/issues/2976
* split off pools and factories into a separate binary: https://github.com/massalabs/massa/discussions/2895
* add denunciations and slashing when a staker produces two different endorsements for the same `(slot, index)`: https://github.com/massalabs/massa/issues/3022
