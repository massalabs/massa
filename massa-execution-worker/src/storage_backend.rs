use std::{
    collections::VecDeque,
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
    fn write(&mut self, slot: &Slot, value: &[u8]);

    /// Reads the value from the storage backend.
    /// The slot is used as the key to the value.
    fn read(&self, slot: &Slot) -> Option<Vec<u8>>;
}

/// A storage backend that uses the file system as the underlying storage engine.
pub struct FileStorageBackend {
    folder: PathBuf,
    slots_saved: VecDeque<Slot>,
    max_blocks: u64,
}
impl FileStorageBackend {
    /// Creates a new instance of `FileStorageBackend` with the given path.
    pub fn new(path: PathBuf, max_blocks: u64) -> Self {
        Self {
            folder: path,
            slots_saved: VecDeque::new(),
            max_blocks,
        }
    }
}

impl StorageBackend for FileStorageBackend {
    fn write(&mut self, slot: &Slot, value: &[u8]) {
        if self.slots_saved.len() >= self.max_blocks as usize {
            let slot_to_remove = self.slots_saved.pop_front().unwrap();
            let block_file_path = self.folder.join(format!(
                "block_slot_{}_{}.bin",
                slot_to_remove.thread, slot_to_remove.period
            ));
            std::fs::remove_file(block_file_path).expect("Unable to delete block from disk");
        }
        let block_file_path = self
            .folder
            .join(format!("block_slot_{}_{}.bin", slot.thread, slot.period));

        let mut file = File::create(block_file_path.clone())
            .unwrap_or_else(|_| panic!("Cannot create file: {:?}", block_file_path));

        file.write_all(value).expect("Unable to write to disk");
        self.slots_saved.push_back(*slot);
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
    slots_saved: VecDeque<Slot>,
    max_blocks: u64,
}

impl RocksDBStorageBackend {
    /// Creates a new instance of `RocksDBStorageBackend` with the given path.
    pub fn new(path: PathBuf, max_blocks: u64) -> Self {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_compression_type(DBCompressionType::Lz4);
        opts.set_max_open_files(8);

        let db = rocksdb::DB::open(&opts, path.clone())
            .unwrap_or_else(|_| panic!("Failed to create storage db at {:?}", path));

        Self {
            db,
            slots_saved: VecDeque::new(),
            max_blocks,
        }
    }
}

impl StorageBackend for RocksDBStorageBackend {
    fn write(&mut self, slot: &Slot, value: &[u8]) {
        if self.slots_saved.len() >= self.max_blocks as usize {
            let slot_to_remove = self.slots_saved.pop_front().unwrap();
            self.db
                .delete(slot_to_remove.to_bytes_key())
                .expect("Unable to delete block from db");
        }
        self.db
            .put(slot.to_bytes_key(), value)
            .expect("Unable to write block to db");
        self.slots_saved.push_back(*slot);
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

        let mut storage = FileStorageBackend::new(PathBuf::from(""), 100);
        storage.write(&slot, &value);

        let storage = FileStorageBackend::new(PathBuf::from(""), 100);
        let data = storage.read(&slot);
        assert_eq!(data, Some(value));
    }

    #[test]
    fn test_rocksdb_storage_backend() {
        let slot = Slot {
            thread: 1,
            period: 1,
        };
        let slot_2 = Slot {
            thread: 1,
            period: 2,
        };
        let slot_3 = Slot {
            thread: 1,
            period: 3,
        };
        let value = vec![1, 2, 3];

        let mut storage = RocksDBStorageBackend::new(PathBuf::from("test_db"), 2);
        storage.write(&slot, &value);
        storage.write(&slot_2, &value);
        storage.write(&slot_3, &value);
        drop(storage);

        let storage = RocksDBStorageBackend::new(PathBuf::from("test_db"), 2);
        let data = storage.read(&slot);
        assert_eq!(data, None);
        let data = storage.read(&slot_2);
        assert_eq!(data, Some(value.clone()));
        let data = storage.read(&slot_3);
        assert_eq!(data, Some(value.clone()));
    }
}
