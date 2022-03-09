// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

// The global context.
lazy_static! {
    static ref SERIALIZATION_CONTEXT: Arc<Mutex<Option<SerializationContext>>> =
        Arc::new(Mutex::new(None));
}

// The local context, a clone of the global one initialized on first use.
thread_local!(static TLS_CONTEXT: RefCell<Option<SerializationContext>> = RefCell::new(None));

/// Initialize the global context, should be called once at startup
/// or used at the beginning of a test.
pub fn init_serialization_context(context: SerializationContext) {
    *SERIALIZATION_CONTEXT
        .lock()
        .expect("Couldn't acquire mutex on SERIALIZATION_CONTEXT.") = Some(context);
}

/// Get a clone of the context. For tests only.
pub fn get_serialization_context() -> SerializationContext {
    SERIALIZATION_CONTEXT
        .lock()
        .expect("Couldn't acquire mutex on SERIALIZATION_CONTEXT.")
        .clone()
        .expect("uninitialized SERIALIZATION_CONTEXT.")
}

/// Use the tls context, should be called only after initializing the global context.
pub fn with_serialization_context<F, V>(f: F) -> V
where
    F: FnOnce(&SerializationContext) -> V,
{
    TLS_CONTEXT.with(|tls| {
        let mut local_context = tls.borrow_mut();

        // Init the local context if necessary.
        if local_context.is_none() {
            let global_context = SERIALIZATION_CONTEXT
                .lock()
                .expect("Couldn't acquire mutex on SERIALIZATION_CONTEXT.")
                .clone()
                .expect("uninitialized SERIALIZATION_CONTEXT.");
            *local_context = Some(global_context);
        }

        f(local_context.as_ref().unwrap())
    })
}

/// a context for model serialization/deserialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializationContext {
    pub max_operations_per_block: u32,
    pub endorsement_count: u32,
    pub thread_count: u8,
    pub max_block_size: u32,
    pub max_advertise_length: u32,
    pub max_message_size: u32,
    pub max_ask_blocks_per_message: u32,
    pub max_operations_per_message: u32,
    pub max_endorsements_per_message: u32,
    pub max_bootstrap_blocks: u32,
    pub max_bootstrap_cliques: u32,
    pub max_bootstrap_deps: u32,
    pub max_bootstrap_children: u32,
    pub max_bootstrap_pos_cycles: u32,
    pub max_bootstrap_pos_entries: u32,
    pub max_bootstrap_message_size: u32,
}

impl Default for SerializationContext {
    fn default() -> Self {
        SerializationContext::const_default()
    }
}

impl SerializationContext {
    pub const fn const_default() -> Self {
        #[cfg(feature = "testing")]
        // Overide normal constants on testing
        use crate::constants::default_testing::*;
        #[cfg(not(feature = "testing"))]
        use crate::constants::*;
        Self {
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            thread_count: THREAD_COUNT,
            max_block_size: MAX_BLOCK_SIZE,
            endorsement_count: ENDORSEMENT_COUNT,
            max_advertise_length: MAX_ADVERTISE_LENGTH,
            max_message_size: MAX_MESSAGE_SIZE,
            max_bootstrap_blocks: MAX_BOOTSTRAP_BLOCKS,
            max_bootstrap_cliques: MAX_BOOTSTRAP_CLIQUES,
            max_bootstrap_deps: MAX_BOOTSTRAP_DEPS,
            max_bootstrap_children: MAX_BOOTSTRAP_CHILDREN,
            max_ask_blocks_per_message: MAX_ASK_BLOCKS_PER_MESSAGE,
            max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
            max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
            max_bootstrap_message_size: MAX_BOOTSTRAP_MESSAGE_SIZE,
            max_bootstrap_pos_cycles: MAX_BOOTSTRAP_POS_CYCLES,
            max_bootstrap_pos_entries: MAX_BOOTSTRAP_POS_ENTRIES,
        }
    }
}
