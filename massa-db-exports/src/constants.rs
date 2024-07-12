// Commons
pub const METADATA_CF: &str = "metadata";
pub const STATE_CF: &str = "state";
pub const VERSIONING_CF: &str = "versioning";

// Hash
pub const STATE_HASH_BYTES_LEN: usize = 512;
pub const STATE_HASH_KEY: &[u8; 1] = b"h";
pub const STATE_HASH_INITIAL_BYTES: &[u8; STATE_HASH_BYTES_LEN] = &[0; STATE_HASH_BYTES_LEN];

// Change_id
pub const CHANGE_ID_KEY: &[u8; 1] = b"c";
pub const CHANGE_ID_DESER_ERROR: &str = "critical: change_id deserialization failed";
pub const CHANGE_ID_SER_ERROR: &str = "critical: change_id serialization failed";

// Errors
pub const CF_ERROR: &str = "critical: rocksdb column family operation failed";
pub const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
pub const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
pub const STATE_HASH_ERROR: &str = "critical: saved state hash is corrupted";

// Prefixes
pub const CYCLE_HISTORY_PREFIX: &str = "cycle_history/";
pub const DEFERRED_CREDITS_PREFIX: &str = "deferred_credits/";
pub const ASYNC_POOL_PREFIX: &str = "async_pool/";
pub const EXECUTED_OPS_PREFIX: &str = "executed_ops/";
pub const EXECUTED_DENUNCIATIONS_PREFIX: &str = "executed_denunciations/";
pub const LEDGER_PREFIX: &str = "ledger/";
pub const MIP_STORE_PREFIX: &str = "versioning/";
pub const MIP_STORE_STATS_PREFIX: &str = "versioning_stats/";
pub const EXECUTION_TRAIL_HASH_PREFIX: &str = "execution_trail_hash/";
pub const DEFERRED_CALLS_SLOT_PREFIX: &str = "deferred_calls/";

// Async Pool
pub const MESSAGE_DESER_ERROR: &str = "critical: message deserialization failed";
pub const MESSAGE_SER_ERROR: &str = "critical: message serialization failed";
pub const MESSAGE_ID_DESER_ERROR: &str = "critical: message_id deserialization failed";
pub const MESSAGE_ID_SER_ERROR: &str = "critical: message_id serialization failed";

// PosState
pub const CYCLE_HISTORY_DESER_ERROR: &str = "critical: cycle_history deserialization failed";
pub const CYCLE_HISTORY_SER_ERROR: &str = "critical: cycle_history serialization failed";
pub const DEFERRED_CREDITS_DESER_ERROR: &str = "critical: deferred_credits deserialization failed";
pub const DEFERRED_CREDITS_SER_ERROR: &str = "critical: deferred_credits serialization failed";

// Executed Ops
pub const EXECUTED_OPS_ID_DESER_ERROR: &str = "critical: executed_ops_id deserialization failed";
pub const EXECUTED_OPS_ID_SER_ERROR: &str = "critical: executed_ops_id serialization failed";

// Executed Denunciations
pub const EXECUTED_DENUNCIATIONS_INDEX_DESER_ERROR: &str =
    "critical: executed_denunciations_index deserialization failed";
pub const EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR: &str =
    "critical: executed_denunciations_index serialization failed";

// Ledger
pub const KEY_DESER_ERROR: &str = "critical: key deserialization failed";
pub const KEY_SER_ERROR: &str = "critical: key serialization failed";
pub const KEY_LEN_SER_ERROR: &str = "critical: key length serialization failed";
