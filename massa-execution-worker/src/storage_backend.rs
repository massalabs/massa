use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use massa_models::{config::THREAD_COUNT, slot::Slot};
use rocksdb::{DBCompressionType, Options, WriteBatch};

/// Parse a block-dump file name of the form `block_slot_{thread}_{period}.bin`
/// back into its [`Slot`]. Returns `None` for names that don't match.
fn parse_block_dump_file_name(name: &str) -> Option<Slot> {
    let rest = name.strip_prefix("block_slot_")?.strip_suffix(".bin")?;
    let (thread_str, period_str) = rest.split_once('_')?;
    let thread: u8 = thread_str.parse().ok()?;
    let period: u64 = period_str.parse().ok()?;
    if thread >= THREAD_COUNT {
        None
    } else {
        Some(Slot::new(period, thread))
    }
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

/// Oldest slot to keep when `slot` is written and only the last `max_blocks`
/// slots are retained. Returns `None` while the window still reaches genesis.
fn oldest_kept_slot(slot: &Slot, max_blocks: u64) -> Option<Slot> {
    let index = slot
        .period
        .checked_mul(THREAD_COUNT as u64)?
        .checked_add(slot.thread as u64)?;
    let oldest = index.checked_sub(max_blocks.saturating_sub(1))?;
    Some(Slot::new(
        oldest / THREAD_COUNT as u64,
        (oldest % THREAD_COUNT as u64) as u8,
    ))
}

/// A storage backend that uses RocksDB as the underlying storage engine.
///
/// Every final slot is dumped (missed slots with an empty value), so retention
/// keeps the last `max_blocks` slots: this needs no scan of the store on startup.
pub struct RocksDBStorageBackend {
    db: rocksdb::DB,
    /// oldest slot kept, as of the last write since startup
    oldest_kept: Option<Slot>,
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
            oldest_kept: None,
            max_blocks,
        }
    }
}

impl StorageBackend for RocksDBStorageBackend {
    fn write(&mut self, slot: &Slot, value: &[u8]) {
        let mut batch = WriteBatch::default();
        if let Some(oldest_kept) = oldest_kept_slot(slot, self.max_blocks) {
            match self.oldest_kept {
                // window did not move forward
                Some(prev) if prev >= oldest_kept => {}
                // steady state: exactly one slot fell out of the window
                Some(prev) if prev.get_next_slot(THREAD_COUNT).ok() == Some(oldest_kept) => {
                    batch.delete(prev.to_bytes_key());
                    self.oldest_kept = Some(oldest_kept);
                }
                // first write since startup (entries left from before a restart,
                // a downtime gap or a lowered `max_blocks`) or a gap in written
                // slots: drop everything older than the window in one range delete
                _ => {
                    batch.delete_range(Slot::new(0, 0).to_bytes_key(), oldest_kept.to_bytes_key());
                    self.oldest_kept = Some(oldest_kept);
                }
            }
        }
        batch.put(slot.to_bytes_key(), value);
        self.db.write(&batch).expect("Unable to write block to db");
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
        let dir = tempfile::tempdir().unwrap();
        let slot = Slot::new(1, 1);
        let slot_2 = Slot::new(1, 2);
        let slot_3 = Slot::new(1, 3);
        let value = vec![1, 2, 3];

        let mut storage = RocksDBStorageBackend::new(dir.path().to_path_buf(), 2);
        storage.write(&slot, &value);
        storage.write(&slot_2, &value);
        storage.write(&slot_3, &value);
        drop(storage);

        let storage = RocksDBStorageBackend::new(dir.path().to_path_buf(), 2);
        let data = storage.read(&slot);
        assert_eq!(data, None);
        let data = storage.read(&slot_2);
        assert_eq!(data, Some(value.clone()));
        let data = storage.read(&slot_3);
        assert_eq!(data, Some(value.clone()));
    }

    #[test]
    fn rocksdb_backend_window_slides_across_restart_and_period_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let value = vec![1, 2, 3];
        let last_thread = THREAD_COUNT - 1;
        let s1 = Slot::new(1, last_thread - 1);
        let s2 = Slot::new(1, last_thread);
        let s3 = Slot::new(2, 0);
        let s4 = Slot::new(2, 1);

        {
            let mut storage = RocksDBStorageBackend::new(dir.path().to_path_buf(), 2);
            storage.write(&s1, &value);
            storage.write(&s2, &value);
        }
        // restart: retention keeps applying to dumps written before it
        let mut storage = RocksDBStorageBackend::new(dir.path().to_path_buf(), 2);
        storage.write(&s3, &value);
        assert_eq!(storage.read(&s1), None);
        assert_eq!(storage.read(&s2), Some(value.clone()));
        storage.write(&s4, &value);
        assert_eq!(storage.read(&s2), None);
        assert_eq!(storage.read(&s3), Some(value.clone()));
        assert_eq!(storage.read(&s4), Some(value.clone()));
    }

    #[test]
    fn rocksdb_backend_drops_dumps_older_than_window_after_gap() {
        let dir = tempfile::tempdir().unwrap();
        let value = vec![1, 2, 3];
        let old = [Slot::new(1, 0), Slot::new(1, 1), Slot::new(1, 2)];
        let recent = Slot::new(10, 0);

        {
            let mut storage = RocksDBStorageBackend::new(dir.path().to_path_buf(), 100);
            for slot in &old {
                storage.write(slot, &value);
            }
        }
        // restart after downtime, with a lowered cap: the old dumps fall out of the window
        let mut storage = RocksDBStorageBackend::new(dir.path().to_path_buf(), 3);
        storage.write(&recent, &value);
        for slot in &old {
            assert_eq!(storage.read(slot), None);
        }
        assert_eq!(storage.read(&recent), Some(value.clone()));
    }

    #[test]
    fn oldest_kept_slot_window() {
        assert_eq!(oldest_kept_slot(&Slot::new(0, 1), 3), None);
        assert_eq!(oldest_kept_slot(&Slot::new(0, 2), 3), Some(Slot::new(0, 0)));
        assert_eq!(
            oldest_kept_slot(&Slot::new(1, 0), 2),
            Some(Slot::new(0, THREAD_COUNT - 1))
        );
        assert_eq!(oldest_kept_slot(&Slot::new(5, 3), 1), Some(Slot::new(5, 3)));
        assert_eq!(oldest_kept_slot(&Slot::new(5, 3), 0), Some(Slot::new(5, 3)));
    }
}
