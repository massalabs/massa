use massa_hash::HASH_SIZE_BYTES;

// Commons
pub const LSMTREE_NODES_CF: &str = "lsmtree_nodes";
pub const LSMTREE_VALUES_CF: &str = "lsmtree_values";
pub const METADATA_CF: &str = "metadata";
pub const STATE_CF: &str = "state";

pub const STATE_HASH_KEY: &[u8; 1] = b"h";
pub const CHANGE_ID_KEY: &[u8; 1] = b"c";
pub const CHANGE_ID_DESER_ERROR: &str = "critical: change_id deserialization failed";
pub const CHANGE_ID_SER_ERROR: &str = "critical: change_id serialization failed";

// Errors
pub const CF_ERROR: &str = "critical: rocksdb column family operation failed";
pub const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
pub const LSMTREE_ERROR: &str = "critical: lsmtree insert / remove open operation failed";
pub const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
pub const STATE_HASH_ERROR: &str = "critical: saved state hash is corrupted";
pub const STATE_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

// Prefixes
// TODO_PR: See if the indexes can be removed (I needed the cycle history to be the first streamed at some point)
pub const CYCLE_HISTORY_PREFIX: &str = "0_cycle_history/";
pub const DEFERRED_CREDITS_PREFIX: &str = "1_deferred_credits/";
pub const ASYNC_POOL_PREFIX: &str = "2_async_pool/";
pub const EXECUTED_OPS_PREFIX: &str = "3_executed_ops/";
pub const EXECUTED_DENUNCIATIONS_PREFIX: &str = "4_executed_denunciations/";
pub const LEDGER_PREFIX: &str = "5_ledger/";

// Async Pool
pub const ASYNC_POOL_HASH_ERROR: &str = "critical: saved async pool hash is corrupted";
pub const ASYNC_POOL_HASH_KEY: &[u8; 4] = b"ap_h";
pub const ASYNC_POOL_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

pub const MESSAGE_DESER_ERROR: &str = "critical: message deserialization failed";
pub const MESSAGE_SER_ERROR: &str = "critical: message serialization failed";
pub const MESSAGE_ID_DESER_ERROR: &str = "critical: message_id deserialization failed";
pub const MESSAGE_ID_SER_ERROR: &str = "critical: message_id serialization failed";

// PosState
pub const CYCLE_HISTORY_HASH_ERROR: &str = "critical: saved cycle_history hash is corrupted";
pub const CYCLE_HISTORY_HASH_KEY: &[u8; 4] = b"ch_h";
pub const CYCLE_HISTORY_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];
pub const CYCLE_HISTORY_DESER_ERROR: &str = "critical: cycle_history deserialization failed";
pub const CYCLE_HISTORY_SER_ERROR: &str = "critical: cycle_history serialization failed";

pub const DEFERRED_CREDITS_HASH_ERROR: &str = "critical: saved deferred_credits hash is corrupted";
pub const DEFERRED_CREDITS_HASH_KEY: &[u8; 4] = b"dc_h";
pub const DEFERRED_CREDITS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];
pub const DEFERRED_CREDITS_DESER_ERROR: &str = "critical: deferred_credits deserialization failed";
pub const DEFERRED_CREDITS_SER_ERROR: &str = "critical: deferred_credits serialization failed";

// Executed Ops

pub const EXECUTED_OPS_HASH_ERROR: &str = "critical: saved executed_ops hash is corrupted";
pub const EXECUTED_OPS_HASH_KEY: &[u8; 4] = b"eo_h";
pub const EXECUTED_OPS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];
pub const EXECUTED_OPS_ID_DESER_ERROR: &str = "critical: executed_ops_id deserialization failed";
pub const EXECUTED_OPS_ID_SER_ERROR: &str = "critical: executed_ops_id serialization failed";

// Executed Denunciations

pub const EXECUTED_DENUNCIATIONS_HASH_ERROR: &str =
    "critical: saved executed_denunciations hash is corrupted";
pub const EXECUTED_DENUNCIATIONS_HASH_KEY: &[u8; 4] = b"ed_h";
pub const EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];
pub const EXECUTED_DENUNCIATIONS_INDEX_DESER_ERROR: &str =
    "critical: executed_denunciations_index deserialization failed";
pub const EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR: &str =
    "critical: executed_denunciations_index serialization failed";

// Ledger

pub const LEDGER_HASH_ERROR: &str = "critical: saved ledger hash is corrupted";
pub const LEDGER_HASH_KEY: &[u8; 3] = b"l_h";
pub const LEDGER_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

pub const KEY_DESER_ERROR: &str = "critical: key deserialization failed";
pub const KEY_SER_ERROR: &str = "critical: key serialization failed";
pub const KEY_LEN_SER_ERROR: &str = "critical: key length serialization failed";
