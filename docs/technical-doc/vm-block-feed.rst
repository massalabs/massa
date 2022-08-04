====================================
Smart contract VM block feed process
====================================

Rationale
=========

This page describes how blocks and byte-code in the block graph are brought to the smart contract VM for execution.

Summary
=======

The general idea is that the Smart Contract Engine (SCE) takes an initial SCE ledger loaded from file and iteratively updates it by executing all Slots in order from (0,0) up to the current latest slot.
At each one of those slots, there might be a block (either final or in the active Blockclique) containing a certain number of ExecuteSC operations, or a miss (lack of block in the slot). ExecuteSC operations within a block are tentatively executed in the order they appear in the block.

Each one of those ExecuteSC operations has already spent `max_gas * gas_price + coins` from the CSS ledger so that the block creator is certain to get the gas fees (they cannot be spent in-between).
When an ExecuteSC operation is executed on the SCE side, it immediately credits the block producer with `max_gas * gas_price` in the SCE ledger, then it credits the target address with `coins` in the SCE ledger, then it executes the byte-code inside the ExecuteSC operation.
If byte-code execution fails (invalid byte-code or execution error), everything is reverted except the `max_gas * gas_price` credit to the block producer and `coins` are sent back to the sender on the SCE side.

This scheme works in theory but requires heavy optimizations in practice because not everything can be recomputed from genesis at every slot or Blockclique change, especially since old blocks are forgotten. Such optimizations include keeping only a final ledger on the SCE side and a list of changes caused by active blocks that can be applied to the final ledger on-the-fly.

Block sequencing
================

SCE vs CSS finality
-------------------

Block finality on the CSS side (CSS-finality) can happen in any order (see scientific paper). To ensure that final slots (blocks or misses) are taken into account in a fully sequential way on the SCE side, we introduce a stronger definition of finality on the SCE side (SCE-finality).

A slot S is SCE-final if-and-only-if all the following criteria are met:
* all slots before B are SCE-final`
* S either contains a CSS-final block or there is a CSS-final block in the same thread at a later period

That way, we ensure that once a slot becomes SCE-final, the final SCE ledger won't ever have to be rolled back because no new blocks can appear at that slot or before it.

The SCE-finality of a block implies its CSS-finality but the opposite may not always be true.

CSS: notifying SCE about Blockclique changes
--------------------------------------------

The Blockclique can change whenever a new block is added to the graph:
* another maximal clique can become the new Blockclique if its fitness becomes higher than the current Blockclique's
* a new active non-CSS-final block can be added to the Blockclique
* blocks are removed from the Blockclique when they become CSS-final

Every time the Blockclique changes, Consensus calls `ExecutionCommandSender::blockclique_changed(..)` with the list of blocks that became CSS-final, and the full list of blocks that belong to the updated Blockclique.

SCE setup
---------

The SCE (ExecutionWorker) carries:
* a VM (that also includes a CSS-final ledger and a active slot history)
* a `last_final_slot` cursor pointing to the latest executed SCE-final slot
* a `last_active_slot` cursor pointing to the latest executed slot
* a `pending_css_final_blocks` list of CSS-final blocks that are not yet SCE-final

SCE: processing Blockclique change notifications
------------------------------------------------

When the SCE receives a Blockclique change notification from the CSS with parameters `new_css_final_blocks` and `blockclique_blocks`:

**1 - reset the SCE state back to its latest final state**

Note that this is sub-optimal as only invalidated slots should be reverted.

* revert the VM to its latest SCE-final state by clearing its active slot history.
* set `last_active_slot = last_final_slot`

**2 - process CSS-final blocks**

* extend `pending_css_final_blocks` with `new_css_final_blocks`
* iterate over every slot S starting from `last_final_slot.get_next_slot()` up to the latest slot in `pending_css_final_blocks` (included)
  * if there is a block B at slot S in `pending_css_final_blocks`:
    * remove B from `pending_css_final_blocks`
    * call the VM to execute the SCE-final block B at slot S
    * set `last_active_slot = last_final_slot = S`
  * otherwise, if there is a block in `pending_css_final_blocks` at a later period of the same thread as S:
    * call the VM to execute an SCE-final miss at slot S
    * set `last_active_slot = last_final_slot = S`
  * otherwise it means that we have reached a non-SCE-final slot:
    * break

At this point, the VM has updated its final SCE ledger, and blocks that remain to be processed are in `blockclique_blocks UNION pending_css_final_blocks`.

**3 - process CSS-active blocks**

* define `sce_active_blocks = blockclique_blocks UNION pending_css_final_blocks`
* iterate over every slot S starting from `last_active_slot.get_next_slot()` up to the latest slot in `sce_active_blocks` (included)
  * if there is a block B at slot S in `sce_active_blocks`:
    * call the VM to execute the SCE-active block B at slot S
    * set `last_active_slot = S`
  * otherwise:
    * call the VM to execute an SCE-active miss at slot S
    * set `last_active_slot = S`
  
At this point, the SCE has executed all SCE-active blocks. It only needs to fill up the remaining slots until now with misses.

**4 - fill the remaining slots with misses**

* iterate over every slot S starting from `last_active_slot.get_next_slot()` up to the latest slot at the current timestamp (included):
  * call the VM to execute an SCE-active miss at slot S
  * set `last_active_slot = S`

Note that this miss-filling algorithm needs to be called again at every slot tick, even in the absence of Blockclique change notifications.
