use massa_hash::HASH_SIZE_BYTES;

pub(crate) const LEDGER_CF: &str = "ledger";
pub(crate) const METADATA_CF: &str = "metadata";
pub(crate) const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
pub(crate) const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
pub(crate) const CF_ERROR: &str = "critical: rocksdb column family operation failed";
pub(crate) const LEDGER_HASH_ERROR: &str = "critical: saved ledger hash is corrupted";
pub(crate) const KEY_LEN_SER_ERROR: &str = "critical: key length serialization failed";
pub(crate) const SLOT_KEY: &[u8; 1] = b"s";
pub(crate) const LEDGER_HASH_KEY: &[u8; 1] = b"h";
pub(crate) const LEDGER_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];
