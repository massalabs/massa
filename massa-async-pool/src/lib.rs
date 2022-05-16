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
//!     "sender": "xxxx",  // address that sent the message and spent max_gas*gas_price+coins on emission
//!     "slot": {"period": 123455, "thread": 11},  // slto at which the message was emitted
//!     "emission_index": 212,  // index of the message emitted in this slot
//!     "destination": "xxxx",  // target address
//!     "handler": "handle_message",  // name of the handler function to call in the target SC
//!     "validity_start": {"period": 123456, "thread": 12},  // the message can be handled starting from the validity_start slot (included)
//!     "validity_end": {"period": 123457, "thread": 16},  // the message can be handled until the validity_end slot (excluded)
//!     "max_gas": 12334,  // max gas available when the handler is called
//!     "gas_price": "124.23",  // gas price for the handler call
//!     "coins": "1111.11",  // amount of coins to transfer to the destination address when calling its handler
//!     "data": { ... any object ... }  // data payload of the message, passed as the sole parameter of the destination handler when called
//! }
//! ```
//!
//! ## How to send a message during bytecode execution
//!
//! * messages are sent using an ABI: `send_message(target_address, target_handler, validity_start, validity_end, max_gas, gas_price, coins, data: JSON string) -> Result<(), ABIReturnError>`. Note that data has a configuration defined `max_async_message_data_size`.
//! * when called, this ABI does this:
//!   * it consumes `compute_gas_cost_of_message_storage(context.current_slot, validity_end_slot)` of gas in the current execution. This allows making the message emission more gas-consuming when it requires storing the message in queue for longer
//!   * it consumes `max_gas * gas_price + coins` coins from the sender
//!   * it generates an `AsyncMessage` and stores it in an asynchronous pool
//!
//! Note that `max_gas*gas_price` coins are burned when sending the message.
//!
//! ## How is the `AsyncPool` handled
//! ```md
//! * In the AsyncPool, Messages are kept sorted by `priority = AsyncMessageId(msg.max_gas * msg.gas_price, rev(msg.slot), rev(msg.emission_index))`
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
//!       * gas_price = M.gas_price
//!       * slot = S
//!       * call_stack = [M.target_address, M.sender_address]
//!   * on any failure, cancel all the effects of execution and credit M.coins back to the sender
//!   * if there is a block at slot S, the execution of the block happens here
//!
//! ## How to receive a message (inside the smart contract)
//!
//! * define a public exported handler function taking 1 parameter (the message data)
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
//! When the crate feature `testing` is enabled, tooling useful for testing purposes is exported.
//! See `test_exports/mod.rs` for details.

#![feature(map_first_last)]
#![feature(btree_drain_filter)]
#![feature(drain_filter)]

mod bootstrap;
mod changes;
mod config;
mod message;
mod pool;

pub use bootstrap::AsyncPoolBootstrap;
pub use changes::AsyncPoolChanges;
pub use config::AsyncPoolConfig;
pub use message::{AsyncMessage, AsyncMessageId};
pub use pool::AsyncPool;

#[cfg(test)]
mod tests;

#[cfg(feature = "testing")]
pub mod test_exports;
