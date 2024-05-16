use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

use massa_models::slot::Slot;
use rocksdb::{DBCompressionType, Options};

/// A trait that defines the interface for a storage backend for the dump-block
/// feature.
pub trait StorageBackend: Send + Sync {
    /// Writes the given value to the storage backend.
    /// The slot is used as the key to the value.
    fn write(&self, slot: &Slot, value: &[u8]);

    /// Reads the value from the storage backend.
    /// The slot is used as the key to the value.
    fn read(&self, slot: &Slot) -> Option<Vec<u8>>;
}

/// A storage backend that uses the file system as the underlying storage engine.
pub struct FileStorageBackend {
    folder: PathBuf,
}
impl FileStorageBackend {
    /// Creates a new instance of `FileStorageBackend` with the given path.
    pub fn new(path: PathBuf) -> Self {
        Self { folder: path }
    }
}

impl StorageBackend for FileStorageBackend {
    fn write(&self, slot: &Slot, value: &[u8]) {
        let block_file_path = self
            .folder
            .join(format!("block_slot_{}_{}.bin", slot.thread, slot.period));

        let mut file = File::create(block_file_path.clone())
            .unwrap_or_else(|_| panic!("Cannot create file: {:?}", block_file_path));

        file.write_all(value).expect("Unable to write to disk");
    }

    fn read(&self, slot: &Slot) -> Option<Vec<u8>> {
        let block_file_path = self
            .folder
            .join(format!("block_slot_{}_{}.bin", slot.thread, slot.period));

        let file = File::open(block_file_path.clone())
            .unwrap_or_else(|_| panic!("Cannot open file: {:?}", block_file_path));
        let mut reader = std::io::BufReader::new(file);
        let mut buffer = Vec::new();
        reader
            .read_to_end(&mut buffer)
            .expect("Unable to read from disk");

        Some(buffer)
    }
}

/// A storage backend that uses RocksDB as the underlying storage engine.
pub struct RocksDBStorageBackend {
    db: rocksdb::DB,
}

impl RocksDBStorageBackend {
    /// Creates a new instance of `RocksDBStorageBackend` with the given path.
    pub fn new(path: PathBuf) -> Self {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_compression_type(DBCompressionType::Lz4);

        let db = rocksdb::DB::open(&opts, path.clone())
            .unwrap_or_else(|_| panic!("Failed to create storage db at {:?}", path));

        Self { db }
    }
}

impl StorageBackend for RocksDBStorageBackend {
    fn write(&self, slot: &Slot, value: &[u8]) {
        self.db
            .put(slot.to_bytes_key(), value)
            .expect("Unable to write block to db");
    }

    fn read(&self, slot: &Slot) -> Option<Vec<u8>> {
        match self.db.get(slot.to_bytes_key()) {
            Ok(val) => val,
            Err(e) => {
                println!("Error: {} reading key {:?}", e, slot.to_bytes_key());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_storage_backend() {
        let slot = Slot {
            thread: 1,
            period: 1,
        };
        let value = vec![1, 2, 3];

        let storage = FileStorageBackend::new(PathBuf::from(""));
        storage.write(&slot, &value);

        let storage = FileStorageBackend::new(PathBuf::from(""));
        let data = storage.read(&slot);
        assert_eq!(data, Some(value));
    }

    #[test]
    fn test_rocksdb_storage_backend() {
        let slot = Slot {
            thread: 1,
            period: 1,
        };
        let value = vec![1, 2, 3];

        let storage = RocksDBStorageBackend::new(PathBuf::from("test_db"));
        storage.write(&slot, &value);
        drop(storage);

        let storage = RocksDBStorageBackend::new(PathBuf::from("test_db"));
        let data = storage.read(&slot);
        assert_eq!(data, Some(value));
    }
}
