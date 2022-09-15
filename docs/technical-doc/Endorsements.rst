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

Endorsements impact the clique convergence speed because we don't have to wait for `N` blocks to account for `N` proof-of-stake draws to happen, but only `E/()`.
This 
In the consensus algorithm, we choose the clique of highest fitness as the blockclique.
A block including `e` endorsements out of the maximum `E` contributes a fitness `e + 1` to the cliques it belongs to.
The fitness of a block is therefore reflected by the number of PoS draws that were involved in creating it.


TODO distrib coins


TODO 

TODO fitness

TODO double production

TODO future: choose best parents


How is deadlock prevented?
==========================
