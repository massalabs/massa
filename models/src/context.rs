// Copyright (c) 2021 MASSA LABS <info@massa.net>

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
    pub max_block_operations: u32,
    pub max_block_endorsements: u32,
    pub parent_count: u8,
    pub max_block_size: u32,
    pub max_peer_list_length: u32,
    pub max_message_size: u32,
    pub max_bootstrap_blocks: u32,
    pub max_bootstrap_cliques: u32,
    pub max_bootstrap_deps: u32,
    pub max_bootstrap_children: u32,
    pub max_bootstrap_pos_cycles: u32,
    pub max_bootstrap_pos_entries: u32,
    pub max_bootstrap_message_size: u32,
    pub max_ask_blocks_per_message: u32,
    pub max_operations_per_message: u32,
    pub max_endorsements_per_message: u32,
}
