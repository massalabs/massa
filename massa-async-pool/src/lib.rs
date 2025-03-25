//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! This crate implements a consensual/deterministic pool of asynchronous messages (`AsyncPool`) within the context of autonomous smart contracts.
//!
//! `AsyncPool` is used in conjunction with `FinalLedger` within the `FinalState`, but also as a speculative copy for speculative execution.
//!
//! ## Goal
//!
//! Allow a smart contract to send a message to trigger another smart contract's handler asynchronously.
//!
//! Note that all the "coins" mentioned here are SCE coins.
//!
//! ## Message format
//!
//! ```json
//! {
//!     "sender": "xxxx",  // address that sent the message and spent fee + coins on emission
//!     "slot": {"period": 123455, "thread": 11},  // slot at which the message was emitted
//!     "emission_index": 212,  // index of the message emitted in this slot
//!     "destination": "xxxx",  // target address
//!     "function": "handle_message",  // name of the function to call in the target SC
//!     "validity_start": {"period": 123456, "thread": 12},  // the message can be handled starting from the validity_start slot (included)
//!     "validity_end": {"period": 123457, "thread": 16},  // the message can be handled until the validity_end slot (excluded)
//!     "max_gas": 12334,  // max gas available when the handler is called
//!     "coins": "1111.11",  // amount of coins to transfer to the destination address when calling its handler
//!     "function_params": { ... any object ... }  // parameters to call the function
//! }
//! ```
//!
//! ## How to send a message during bytecode execution
//!
//! * messages are sent using an ABI: `send_message(target_address, function, validity_start, validity_end, max_gas, fee, coins, function_params) -> Result<(), ABIReturnError>`.
//! * when called, this ABI does this:
//!   * it consumes `compute_gas_cost_of_message_storage(context.current_slot, validity_end_slot)` of gas in the current execution. This allows making the message emission more gas-consuming when it requires storing the message in queue for longer
//!   * it consumes `fee + coins` coins from the sender
//!   * it generates an `AsyncMessage` and stores it in an asynchronous pool
//!
//! Note that `fee + coins` coins are burned when sending the message.
//!
//! ## How is the `AsyncPool` handled
//! ```md
//! * In the AsyncPool, Messages are kept sorted by `priority = AsyncMessageId(rev(Ratio(msg.fee, max(msg.max_gas,1))), rev(msg.slot), rev(msg.emission_index))`
//!
//! * when an AsyncMessage is added to the AsyncPool:
//!   * if the AsyncPool length has exceeded config.max_async_pool_length:
//!     * remove the lowest-priority message and reimburse "coins" to the message sender
//!
//! * At every slot S :
//!   * expired messages are deleted, and "coins" are credited back to the message sender
//!   * messages that are valid at slot S (in terms of validity_start, validity end) are popped in highest-to-lowest priority order until they accumulate max_async_gas_per_slot. For each selected message M in decreasing priority order:
//!     * make sure that M.target_address exists and has a method called M.target_handler with the right signature, otherwise fail the execution
//!     * credit target_address with M.coins
//!     * run the target handler function with M.payload as parameter and the context:
//!       * max_gas = M.max_gas
//!       * fee = M.fee
//!       * slot = S
//!       * call_stack = [M.target_address, M.sender_address]
//!   * on any failure, cancel all the effects of execution and credit M.coins back to the sender
//!   * if there is a block at slot S, the execution of the block happens here
//!
//! ## How to receive a message (inside the smart contract)
//!
//! * define a public exported handler function taking 1 parameter
//! * this function will be called when a message is processed with the right `destination` and `handler`
//! ```
//!
//! # Architecture
//!
//! ## message.rs
//! Defines `AsyncMessage` that represents an asynchronous message.
//!
//! ## pool.rs
//! Defines the `AsyncPool` that manipulates a list of `AsyncMessages` sorted by priority.
//!
//! ## changes.rs
//! Represents and manipulates changes (message additions/deletions) in the `AsyncPool`.
//!
//! ## bootstrap.rs
//! Provides serializable structures and tools for bootstrapping the asynchronous pool.
//!
//! ## Test exports
//!
//! When the crate feature `test-exports` is enabled, tooling useful for test-exports purposes is exported.
//! See `test_exports/mod.rs` for details.

mod changes;
mod config;
mod pool;

pub use changes::{AsyncPoolChanges, AsyncPoolChangesDeserializer, AsyncPoolChangesSerializer};
pub use config::AsyncPoolConfig;
pub use pool::{AsyncPool, AsyncPoolDeserializer, AsyncPoolSerializer};

#[cfg(test)]
mod tests;

#[cfg(feature = "test-exports")]
pub mod test_exports;
