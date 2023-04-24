use massa_hash::HASH_SIZE_BYTES;

// Commons
pub const METADATA_CF: &str = "metadata";
pub const SLOT_KEY: &[u8; 1] = b"s";

// Errors
pub const CF_ERROR: &str = "critical: rocksdb column family operation failed";
pub const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
pub const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
pub const WRONG_BATCH_TYPE_ERROR: &str = "critical: wrong batch type";

// Async Pool
pub const ASYNC_POOL_CF: &str = "async_pool";
pub const ASYNC_POOL_HASH_ERROR: &str = "critical: saved async pool hash is corrupted";
pub const ASYNC_POOL_HASH_KEY: &[u8; 4] = b"ap_h";
pub const ASYNC_POOL_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

pub const MESSAGE_DESER_ERROR: &str = "critical: message deserialization failed";
pub const MESSAGE_SER_ERROR: &str = "critical: message serialization failed";
pub const MESSAGE_ID_DESER_ERROR: &str = "critical: message_id deserialization failed";
pub const MESSAGE_ID_SER_ERROR: &str = "critical: message_id serialization failed";

// PosState
pub const CYCLE_HISTORY_CF: &str = "cycle_history";
pub const CYCLE_HISTORY_HASH_ERROR: &str = "critical: saved cycle_history hash is corrupted";
pub const CYCLE_HISTORY_HASH_KEY: &[u8; 4] = b"ch_h";

pub const DEFERRED_CREDITS_CF: &str = "deferred_credits";
pub const DEFERRED_CREDITS_HASH_ERROR: &str = "critical: saved deferred_credits hash is corrupted";
pub const DEFERRED_CREDITS_HASH_KEY: &[u8; 4] = b"dc_h";

// Executed Ops

pub const EXECUTED_OPS_CF: &str = "executed_ops";
pub const EXECUTED_OPS_HASH_ERROR: &str = "critical: saved executed_ops hash is corrupted";
pub const EXECUTED_OPS_HASH_KEY: &[u8; 4] = b"eo_h";
pub const EXECUTED_OPS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

// Ledger

pub const LEDGER_CF: &str = "ledger";
pub const LEDGER_HASH_ERROR: &str = "critical: saved ledger hash is corrupted";
pub const LEDGER_HASH_KEY: &[u8; 3] = b"l_h";
pub const LEDGER_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

pub const KEY_DESER_ERROR: &str = "critical: key deserialization failed";
pub const KEY_SER_ERROR: &str = "critical: key serialization failed";
pub const KEY_LEN_SER_ERROR: &str = "critical: key length serialization failed";
