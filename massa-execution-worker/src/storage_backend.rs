use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use massa_models::slot::{Slot, SLOT_KEY_SIZE};
use rocksdb::{DBCompressionType, Options};

/// Parse a block-dump file name of the form `block_slot_{thread}_{period}.bin`
/// back into its [`Slot`]. Returns `None` for names that don't match.
fn parse_block_dump_file_name(name: &str) -> Option<Slot> {
    let rest = name.strip_prefix("block_slot_")?.strip_suffix(".bin")?;
    let (thread_str, period_str) = rest.split_once('_')?;
    let thread: u8 = thread_str.parse().ok()?;
    let period: u64 = period_str.parse().ok()?;
    Some(Slot::new(period, thread))
}

/// Rebuild the retention queue (oldest first) from block dumps already present
/// on disk, so that the `max_blocks` cap keeps accounting for pre-restart entries.
fn load_saved_slots_from_folder(folder: &Path) -> VecDeque<Slot> {
    let mut slots: Vec<Slot> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|entry| parse_block_dump_file_name(entry.file_name().to_str()?))
            .collect(),
        // Folder missing/unreadable (e.g. fresh start): nothing persisted yet.
        Err(_) => Vec::new(),
    };
    slots.sort_unstable();
    VecDeque::from(slots)
}

/// Rebuild the retention queue (oldest first) from block dumps already present
/// in the RocksDB store, so that the `max_blocks` cap survives restarts.
fn load_saved_slots_from_db(db: &rocksdb::DB) -> VecDeque<Slot> {
    let mut slots: Vec<Slot> = db
        .iterator(rocksdb::IteratorMode::Start)
        .filter_map(|res| res.ok())
        .filter_map(|(key, _)| {
            let key: [u8; SLOT_KEY_SIZE] = key.as_ref().try_into().ok()?;
            Some(Slot::from_bytes_key(&key))
        })
        .collect();
    slots.sort_unstable();
    VecDeque::from(slots)
}

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
        let slots_saved = load_saved_slots_from_folder(&path);
        Self {
            folder: path,
            slots_saved,
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

        let slots_saved = load_saved_slots_from_db(&db);
        Self {
            db,
            slots_saved,
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
    fn file_backend_retention_survives_restart() {
        // Regression test for F59: after a restart the retention cap must keep
        // accounting for block dumps already present on disk.
        let dir = std::env::temp_dir().join(format!("massa_f59_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // start from a clean folder
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let _ = std::fs::remove_file(entry.path());
        }

        let value = vec![1, 2, 3];
        let s1 = Slot::new(1, 0);
        let s2 = Slot::new(2, 0);
        let s3 = Slot::new(3, 0);

        // First run: persist two dumps (cap = 2, so both are kept).
        {
            let mut storage = FileStorageBackend::new(dir.clone(), 2);
            storage.write(&s1, &value);
            storage.write(&s2, &value);
        }

        // Simulated restart: the queue must be rebuilt from the two files on disk,
        // so writing a third dump evicts the oldest one instead of bypassing the cap.
        {
            let mut storage = FileStorageBackend::new(dir.clone(), 2);
            storage.write(&s3, &value);
        }

        assert!(
            !dir.join("block_slot_0_1.bin").exists(),
            "oldest dump should have been evicted after restart"
        );
        assert!(dir.join("block_slot_0_2.bin").exists());
        assert!(dir.join("block_slot_0_3.bin").exists());

        // cleanup
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
        let _ = std::fs::remove_dir(&dir);
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
