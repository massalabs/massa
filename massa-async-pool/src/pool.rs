// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of async messages for use in the context of autonomous smart contracts

use std::collections::BTreeMap;

use crate::{config::AsyncPoolConfig, message::{AsyncMessage, AsyncMessageId}};


/// Represents a pool of deterministically sorted messages.
/// The final async pool is attached to the output of the latest final slot within the context of massa-final-state.
/// Nodes must bootstrap the final message pool when they join the network.
pub struct AsyncPool {
    /// async pool config
    config: AsyncPoolConfig,
    /// messages sorted by increasing ID (increasing priority)
    messages: BTreeMap<AsyncMessageId, AsyncMessage>,
}

impl AsyncPool {
    /// creates a new empty AsyncPool
    pub fn new(config: AsyncPoolConfig) -> AsyncPool {
        AsyncPool {
            config,
            messages: Default::default()
        }
    }

    /// applies AsyncPoolChanges to the pool
    /// 
    /// # returns
    /// the list of (message_id, message) that were eliminated from the pool after the new one was added
    /// 
    pub fn apply(changes: AsyncPoolChanges) -> Vec<(AsyncMessageId, AsyncMessage)> {

    }
}