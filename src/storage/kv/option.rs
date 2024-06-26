use std::path::PathBuf;

use crate::storage::{
    kv::error::{Error, Result},
    log::Metadata,
};

// Defining constants for metadata keys
const META_KEY_ISOLATION_LEVEL: &str = "isolation_level";
const META_KEY_MAX_KEY_SIZE: &str = "max_key_size";
const META_KEY_MAX_VALUE_SIZE: &str = "max_value_size";
const META_KEY_MAX_VALUE_THRESHOLD: &str = "max_value_threshold";
const META_KEY_MAX_ENTRIES_PER_TX: &str = "max_entries_per_txn";
const META_KEY_MAX_FILE_SIZE: &str = "max_file_size";
const META_KEY_MAX_VALUE_CACHE_SIZE: &str = "max_value_cache_size";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IsolationLevel {
    SnapshotIsolation = 1,
    SerializableSnapshotIsolation = 2,
}

impl IsolationLevel {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            1 => Some(IsolationLevel::SnapshotIsolation),
            2 => Some(IsolationLevel::SerializableSnapshotIsolation),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Options {
    // Required options.
    pub dir: PathBuf, // Directory path for storing the database files.

    // Usually modified options.
    pub isolation_level: IsolationLevel, // Isolation level for transactions.

    // Fine tuning options.
    pub max_key_size: u64,          // Maximum size in bytes for key.
    pub max_value_size: u64,        // Maximum size in bytes for value.
    pub max_value_threshold: usize, // Threshold to decide value should be stored and read from memory or from log value files.
    pub max_entries_per_txn: u32,   // Maximum entries in a transaction.
    pub max_segment_size: u64,      // Maximum size of a single segment.
    pub max_value_cache_size: u64,  // Maximum size of the value cache.

    // Field to indicate whether the data should be stored completely in memory
    pub disk_persistence: bool, // If false, data will be stored completely in memory. If true, data will be stored on disk too.
}

impl Default for Options {
    /// Creates a new set of options with default values.
    fn default() -> Self {
        Self {
            dir: PathBuf::from(""),
            max_key_size: 1024,
            max_value_size: 1024 * 1024,
            max_entries_per_txn: 1 << 12, // 4096 entries
            max_value_threshold: 64,      // 64 bytes
            isolation_level: IsolationLevel::SnapshotIsolation,
            max_segment_size: 1 << 29, // 512 MB
            max_value_cache_size: 100000,
            disk_persistence: true,
        }
    }
}

impl Options {
    /// Creates a new set of options with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert Options to Metadata.
    pub fn to_metadata(&self) -> Metadata {
        let mut metadata = Metadata::new(None);
        metadata.put_uint(META_KEY_ISOLATION_LEVEL, self.isolation_level as u64);
        metadata.put_uint(META_KEY_MAX_KEY_SIZE, self.max_key_size);
        metadata.put_uint(META_KEY_MAX_VALUE_SIZE, self.max_value_size);
        metadata.put_uint(
            META_KEY_MAX_VALUE_THRESHOLD,
            self.max_value_threshold as u64,
        );
        metadata.put_uint(META_KEY_MAX_ENTRIES_PER_TX, self.max_entries_per_txn as u64);
        metadata.put_uint(META_KEY_MAX_FILE_SIZE, self.max_segment_size);
        metadata.put_uint(META_KEY_MAX_VALUE_CACHE_SIZE, self.max_value_cache_size);

        metadata
    }

    /// Convert Metadata to Options.
    pub fn from_metadata(metadata: Metadata, dir: PathBuf) -> Result<Self> {
        let isolation_level =
            IsolationLevel::from_u64(metadata.get_uint(META_KEY_ISOLATION_LEVEL)?)
                .ok_or(Error::CorruptedMetadata)?;

        Ok(Options {
            dir,
            isolation_level,
            max_key_size: metadata.get_uint(META_KEY_MAX_KEY_SIZE)?,
            max_value_size: metadata.get_uint(META_KEY_MAX_VALUE_SIZE)?,
            max_value_threshold: metadata.get_uint(META_KEY_MAX_VALUE_THRESHOLD)? as usize,
            max_entries_per_txn: metadata.get_uint(META_KEY_MAX_ENTRIES_PER_TX)? as u32,
            max_segment_size: metadata.get_uint(META_KEY_MAX_FILE_SIZE)?,
            max_value_cache_size: metadata.get_uint(META_KEY_MAX_VALUE_CACHE_SIZE)?,
            disk_persistence: true,
        })
    }

    /// Returns true if the data should be persisted on disk.
    pub fn should_persist_data(&self) -> bool {
        self.disk_persistence
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn default_options() {
        let options = Options::default();

        assert_eq!(options.dir, PathBuf::from(""));
        assert_eq!(options.max_key_size, 1024);
        assert_eq!(options.max_value_size, 1024 * 1024);
        assert_eq!(options.max_entries_per_txn, 1 << 12);
        assert_eq!(options.max_value_threshold, 64);
        assert_eq!(options.isolation_level, IsolationLevel::SnapshotIsolation);
        assert_eq!(options.max_segment_size, 1 << 29);
        assert_eq!(options.max_value_cache_size, 100000);
        assert!(options.disk_persistence);
    }

    #[test]
    fn options_to_metadata() {
        let options = Options {
            dir: PathBuf::from("/test/dir"),
            max_key_size: 2048,
            max_value_size: 4096,
            max_entries_per_txn: 500,
            max_value_threshold: 128,
            isolation_level: IsolationLevel::SerializableSnapshotIsolation,
            max_segment_size: 1 << 25, // 32 MB
            max_value_cache_size: 200000,
            disk_persistence: true,
        };

        let metadata = options.to_metadata();

        assert_eq!(
            metadata.get_uint(META_KEY_ISOLATION_LEVEL).unwrap(),
            IsolationLevel::SerializableSnapshotIsolation as u64
        );
        assert_eq!(metadata.get_uint(META_KEY_MAX_KEY_SIZE).unwrap(), 2048);
        assert_eq!(metadata.get_uint(META_KEY_MAX_VALUE_SIZE).unwrap(), 4096);
        assert_eq!(
            metadata.get_uint(META_KEY_MAX_VALUE_THRESHOLD).unwrap(),
            128
        );
        assert_eq!(metadata.get_uint(META_KEY_MAX_ENTRIES_PER_TX).unwrap(), 500);
        assert_eq!(metadata.get_uint(META_KEY_MAX_FILE_SIZE).unwrap(), 1 << 25);
        assert_eq!(
            metadata.get_uint(META_KEY_MAX_VALUE_CACHE_SIZE).unwrap(),
            200000
        );
    }

    #[test]
    fn options_from_metadata() {
        let mut metadata = Metadata::new(None);
        metadata.put_uint(
            META_KEY_ISOLATION_LEVEL,
            IsolationLevel::SerializableSnapshotIsolation as u64,
        );
        metadata.put_uint(META_KEY_MAX_KEY_SIZE, 2048);
        metadata.put_uint(META_KEY_MAX_VALUE_SIZE, 4096);
        metadata.put_uint(META_KEY_MAX_VALUE_THRESHOLD, 128);
        metadata.put_uint(META_KEY_MAX_ENTRIES_PER_TX, 500);
        metadata.put_uint(META_KEY_MAX_FILE_SIZE, 1 << 25);
        metadata.put_uint(META_KEY_MAX_VALUE_CACHE_SIZE, 200000);

        let dir = PathBuf::from("/test/dir");
        let options_result = Options::from_metadata(metadata, dir.clone());

        assert!(options_result.is_ok());

        let options = options_result.unwrap();

        assert_eq!(options.dir, dir);
        assert_eq!(options.max_key_size, 2048);
        assert_eq!(options.max_value_size, 4096);
        assert_eq!(options.max_value_threshold, 128);
        assert_eq!(options.max_entries_per_txn, 500);
        assert_eq!(
            options.isolation_level,
            IsolationLevel::SerializableSnapshotIsolation
        );
        assert_eq!(options.max_segment_size, 1 << 25);
        assert_eq!(options.max_value_cache_size, 200000);
        assert!(options.disk_persistence);
    }
}
