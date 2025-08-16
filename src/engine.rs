use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::{error, info, trace, warn};

use crate::memtable::{MemTable, MemTableError};
use crate::sstable::{CompressionType, SSTable, SSTableError};
use crate::wal::{WAL, WALError};

/// Errors that can occur during Engine operations
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("WAL error: {0}")]
    WAL(#[from] WALError),
    #[error("MemTable error: {0}")]
    MemTable(#[from] MemTableError),
    #[error("SSTable error: {0}")]
    SSTable(#[from] SSTableError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),
}

/// Result type for Engine operations
pub type EngineResult<T> = Result<T, EngineError>;

/// Configuration options for the Engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Data directory for storing WAL and SSTable files
    pub data_dir: PathBuf,
    /// Maximum size of MemTable in bytes before flushing to SSTable
    pub memtable_size: usize,
    /// Compression type for SSTable files
    pub compression: CompressionType,
    /// Maximum number of SSTable levels
    pub max_levels: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data"),
            memtable_size: 64 * 1024 * 1024, // 64MB
            compression: CompressionType::None,
            max_levels: 7,
        }
    }
}

/// Main database engine that orchestrates WAL, MemTable, and SSTable operations
pub struct Engine {
    /// Write-Ahead Log for durability
    wal: WAL,
    /// In-memory table for fast writes
    memtable: MemTable,
    /// Configuration options
    config: EngineConfig,
    /// List of SSTable files, ordered by level (newest first)
    sstables: Arc<RwLock<Vec<SSTable>>>,
    /// Current sequence number across all operations
    sequence_number: Arc<RwLock<u64>>,
}

impl Engine {
    /// Create a new Engine instance
    pub async fn new<P: AsRef<Path>>(data_dir: P) -> EngineResult<Self> {
        let config = EngineConfig {
            data_dir: data_dir.as_ref().to_path_buf(),
            ..Default::default()
        };
        Self::with_config(config).await
    }

    /// Create a new Engine instance with custom configuration
    pub async fn with_config(config: EngineConfig) -> EngineResult<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(&config.data_dir).map_err(EngineError::Io)?;

        // Initialize WAL
        let wal_path = config.data_dir.join("wal.log");
        let wal = WAL::new(wal_path)?;

        // Initialize MemTable
        let memtable = MemTable::new(config.memtable_size);

        // Initialize SSTable list
        let sstables = Arc::new(RwLock::new(Vec::new()));

        // Initialize sequence number
        let sequence_number = Arc::new(RwLock::new(0));

        let mut engine = Self {
            wal,
            memtable,
            config,
            sstables,
            sequence_number,
        };

        // Attempt recovery from existing WAL
        engine.recover_from_wal()?;

        // Load existing SSTables from the data directory
        engine.load_existing_sstables()?;

        info!("Engine initialized successfully");
        Ok(engine)
    }

    /// Put a key-value pair into the database
    pub async fn put(&mut self, key: &[u8], value: &[u8]) -> EngineResult<()> {
        if key.is_empty() {
            return Err(EngineError::InvalidConfig(
                "Key cannot be empty".to_string(),
            ));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write to WAL first (Write-Ahead Logging) - WAL manages sequence numbers
        self.wal.put(key, value, timestamp)?;

        // Then write to MemTable
        self.memtable.put(key, value)?;

        // Check if MemTable needs to be flushed
        if self.memtable.is_full() {
            self.flush_memtable().await?;
        }

        trace!(
            "Put operation completed: key={:?}, seq={}",
            key, self.wal.sequence_number()
        );
        Ok(())
    }

    /// Get a value by key from the database
    pub async fn get(&self, key: &[u8]) -> EngineResult<Option<Vec<u8>>> {
        if key.is_empty() {
            return Err(EngineError::InvalidConfig(
                "Key cannot be empty".to_string(),
            ));
        }

        // First, check MemTable (most recent data)
        if let Some(value) = self.memtable.get(key)? {
            return Ok(Some(value));
        }

        // Then check SSTables in order (newest first)
        let mut sstables = self.sstables.write().unwrap();
        for sstable in sstables.iter_mut() {
            if let Ok(Some(value)) = sstable.get(key) {
                return Ok(Some(value));
            }
        }

        // Key not found
        Ok(None)
    }

    /// Delete a key from the database
    pub async fn delete(&mut self, key: &[u8]) -> EngineResult<()> {
        if key.is_empty() {
            return Err(EngineError::InvalidConfig(
                "Key cannot be empty".to_string(),
            ));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write deletion to WAL first
        self.wal.delete(key, timestamp)?;

        // Then mark as deleted in MemTable
        self.memtable.delete(key)?;

        // Check if MemTable needs to be flushed
        if self.memtable.is_full() {
            self.flush_memtable().await?;
        }

        trace!(
            "Delete operation completed: key={:?}, seq={}",
            key, self.wal.sequence_number()
        );
        Ok(())
    }

    /// Flush the current MemTable to an SSTable
    async fn flush_memtable(&mut self) -> EngineResult<()> {
        info!("Flushing MemTable to SSTable");

        // Create SSTable filename with timestamp
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let sstable_path = self
            .config
            .data_dir
            .join(format!("sstable_{}.sst", timestamp));

        // Debug: Check MemTable contents before flush
        let entries = self.memtable.entries();
        println!("DEBUG: MemTable has {} entries before flush", entries.len());
        if !entries.is_empty() {
            println!("DEBUG: First entry: key={:?}, value_len={}", 
                  String::from_utf8_lossy(&entries[0].key),
                  entries[0].value.as_ref().map_or(0, |v| v.len()));
        }

        // Flush MemTable to SSTable
        let sstable =
            SSTable::from_memtable(&sstable_path, &self.memtable, self.config.compression)?;

        println!("DEBUG: SSTable created at {:?} with {} entries", sstable_path, sstable.entry_count());

        // Add to SSTable list
        {
            let mut sstables = self.sstables.write().unwrap();
            sstables.insert(0, sstable); // Insert at beginning (newest first)
        }

        // Create new MemTable
        self.memtable = MemTable::new(self.config.memtable_size);

        // Rotate WAL file
        self.rotate_wal()?;

        info!("MemTable flushed successfully to {:?}", sstable_path);
        Ok(())
    }

    /// Rotate the WAL file after MemTable flush
    fn rotate_wal(&mut self) -> EngineResult<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let new_wal_path = self.config.data_dir.join(format!("wal_{}.log", timestamp));

        // Create new WAL
        let new_wal = WAL::new(&new_wal_path)?;

        // Replace old WAL
        self.wal = new_wal;

        info!("WAL rotated to {:?}", new_wal_path);
        Ok(())
    }

    /// Recover from existing WAL files
    fn recover_from_wal(&mut self) -> EngineResult<()> {
        info!("Attempting WAL recovery...");
        
        // Find all WAL files and sort them chronologically
        let mut wal_files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.config.data_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if let Some(file_name) = path.file_name() {
                        if let Some(name) = file_name.to_str() {
                            if name == "wal.log" || (name.starts_with("wal_") && name.ends_with(".log")) {
                                wal_files.push(path);
                            }
                        }
                    }
                }
            }
        }
        
        // Sort WAL files by timestamp (oldest first for recovery)
        wal_files.sort_by(|a, b| {
            let a_name = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let b_name = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
            
            // Handle the initial wal.log file (treat as timestamp 0)
            let a_time = if a_name == "wal.log" {
                0
            } else {
                a_name.strip_prefix("wal_")
                    .and_then(|s| s.strip_suffix(".log"))
                    .and_then(|s| s.parse::<u128>().ok())
                    .unwrap_or(0)
            };
            
            let b_time = if b_name == "wal.log" {
                0
            } else {
                b_name.strip_prefix("wal_")
                    .and_then(|s| s.strip_suffix(".log"))
                    .and_then(|s| s.parse::<u128>().ok())
                    .unwrap_or(0)
            };
            
            a_time.cmp(&b_time) // Oldest first for recovery
        });
        
        // Recover from each WAL file in order
        for wal_path in &wal_files {
            info!("Recovering from WAL: {:?}", wal_path);
            let wal = WAL::new(wal_path)?;
            wal.recover(&self.memtable)?;
        }
        
        // Also recover from the current WAL if it exists
        if !wal_files.is_empty() {
            // Sync Engine sequence number with the last WAL's sequence number
            if let Ok(last_wal) = WAL::new(&wal_files.last().unwrap()) {
                let mut seq = self.sequence_number.write().unwrap();
                *seq = last_wal.sequence_number();
            }
        }
        
        info!("WAL recovery completed from {} files", wal_files.len());
        Ok(())
    }

    /// Load existing SSTables from the data directory
    fn load_existing_sstables(&mut self) -> EngineResult<()> {
        info!("Loading existing SSTables from {:?}", self.config.data_dir);
        
        let mut sstable_files = Vec::new();
        
        // Scan directory for SSTable files
        if let Ok(entries) = std::fs::read_dir(&self.config.data_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if let Some(extension) = path.extension() {
                        if extension == "sst" {
                            sstable_files.push(path);
                        }
                    }
                }
            }
        }
        
        // Sort by timestamp (newest first)
        sstable_files.sort_by(|a, b| {
            let a_time = a.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("sstable_"))
                .and_then(|s| s.parse::<u128>().ok())
                .unwrap_or(0);
            let b_time = b.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("sstable_"))
                .and_then(|s| s.parse::<u128>().ok())
                .unwrap_or(0);
            b_time.cmp(&a_time) // Newest first
        });
        
        // Load each SSTable
        for sstable_path in sstable_files {
            match SSTable::open(&sstable_path) {
                Ok(sstable) => {
                    info!("Loaded SSTable: {:?}", sstable_path);
                    let mut sstables = self.sstables.write().unwrap();
                    sstables.push(sstable);
                }
                Err(e) => {
                    warn!("Failed to load SSTable {:?}: {}", sstable_path, e);
                }
            }
        }
        
        let sstable_count = self.sstables.read().unwrap().len();
        info!("Loaded {} existing SSTables", sstable_count);
        Ok(())
    }

    /// Get database statistics
    pub fn stats(&self) -> EngineStats {
        let memtable_size = self.memtable.size_bytes();
        let sstable_count = self.sstables.read().unwrap().len();

        EngineStats {
            memtable_size,
            sstable_count,
            data_dir: self.config.data_dir.clone(),
        }
    }

    /// Force flush of MemTable (useful for testing and shutdown)
    pub async fn force_flush(&mut self) -> EngineResult<()> {
        if !self.memtable.is_empty() {
            self.flush_memtable().await?;
        }
        Ok(())
    }

    /// Close the engine and flush any remaining data
    pub async fn close(&mut self) -> EngineResult<()> {
        info!("Closing Engine");

        // Force flush any remaining data
        self.force_flush().await?;

        info!("Engine closed successfully");
        Ok(())
    }
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct EngineStats {
    pub memtable_size: usize,
    pub sstable_count: usize,
    pub data_dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_engine() -> (Engine, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let engine = Engine::new(temp_dir.path()).await.unwrap();
        (engine, temp_dir)
    }

    #[tokio::test]
    async fn test_basic_operations() {
        let (mut engine, _temp_dir) = create_test_engine().await;

        // Test put and get
        engine.put(b"key1", b"value1").await.unwrap();
        let value = engine.get(b"key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Test delete
        engine.delete(b"key1").await.unwrap();
        let value = engine.get(b"key1").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_persistence() {
        let temp_dir = tempdir().unwrap();
        let engine_path = temp_dir.path();

        // Create engine and add data
        {
            let mut engine = Engine::new(engine_path).await.unwrap();
            engine
                .put(b"persistent_key", b"persistent_value")
                .await
                .unwrap();
            engine.put(b"another_key", b"another_value").await.unwrap();
            engine.close().await.unwrap();
        }

        // Reopen engine and verify data persists
        let engine = Engine::new(engine_path).await.unwrap();
        let value1 = engine.get(b"persistent_key").await.unwrap();
        let value2 = engine.get(b"another_key").await.unwrap();

        assert_eq!(value1, Some(b"persistent_value".to_vec()));
        assert_eq!(value2, Some(b"another_value".to_vec()));
    }

    #[tokio::test]
    async fn test_crash_recovery() {
        let temp_dir = tempdir().unwrap();
        let engine_path = temp_dir.path();

        // Create engine and add data
        let mut engine = Engine::new(engine_path).await.unwrap();
        engine
            .put(b"recovery_key", b"recovery_value")
            .await
            .unwrap();
        engine.put(b"test_key", b"test_value").await.unwrap();

        // Simulate crash by dropping engine without proper close
        drop(engine);

        // Reopen engine and verify recovery
        let engine = Engine::new(engine_path).await.unwrap();
        let value1 = engine.get(b"recovery_key").await.unwrap();
        let value2 = engine.get(b"test_key").await.unwrap();

        assert_eq!(value1, Some(b"recovery_value".to_vec()));
        assert_eq!(value2, Some(b"test_value".to_vec()));
    }

    #[tokio::test]
    async fn test_memtable_flush() {
        let temp_dir = tempdir().unwrap();
        let engine_path = temp_dir.path();

        // Create engine with small MemTable size
        let config = EngineConfig {
            data_dir: engine_path.to_path_buf(),
            memtable_size: 100, // Very small to trigger flush
            compression: CompressionType::None,
            max_levels: 7,
        };

        let mut engine = Engine::with_config(config).await.unwrap();

        // Add data that exceeds MemTable size
        engine.put(b"key1", b"value1").await.unwrap();
        engine.put(b"key2", b"value2").await.unwrap();
        engine.put(b"key3", b"value3").await.unwrap();

        // Force flush
        engine.force_flush().await.unwrap();

        // Verify data is still accessible
        let value1 = engine.get(b"key1").await.unwrap();
        let value2 = engine.get(b"key2").await.unwrap();
        let value3 = engine.get(b"key3").await.unwrap();

        assert_eq!(value1, Some(b"value1".to_vec()));
        assert_eq!(value2, Some(b"value2".to_vec()));
        assert_eq!(value3, Some(b"value3".to_vec()));

        // Check stats
        let stats = engine.stats();
        assert_eq!(stats.sstable_count, 1);
    }

    #[tokio::test]
    async fn test_correctness_with_deletions() {
        let (mut engine, _temp_dir) = create_test_engine().await;

        // Add data
        engine.put(b"user:1", b"John").await.unwrap();
        engine.put(b"user:2", b"Jane").await.unwrap();
        engine.put(b"config:theme", b"dark").await.unwrap();

        // Verify initial state
        assert_eq!(engine.get(b"user:1").await.unwrap(), Some(b"John".to_vec()));
        assert_eq!(engine.get(b"user:2").await.unwrap(), Some(b"Jane".to_vec()));
        assert_eq!(
            engine.get(b"config:theme").await.unwrap(),
            Some(b"dark".to_vec())
        );

        // Delete a key
        engine.delete(b"user:1").await.unwrap();
        assert_eq!(engine.get(b"user:1").await.unwrap(), None);

        // Update a key
        engine.put(b"user:2", b"Jane Smith").await.unwrap();
        assert_eq!(
            engine.get(b"user:2").await.unwrap(),
            Some(b"Jane Smith".to_vec())
        );

        // Verify other keys unchanged
        assert_eq!(
            engine.get(b"config:theme").await.unwrap(),
            Some(b"dark".to_vec())
        );
    }

    #[tokio::test]
    async fn test_empty_key_handling() {
        let (mut engine, _temp_dir) = create_test_engine().await;

        // Test empty key in put
        let result = engine.put(b"", b"value").await;
        assert!(result.is_err());

        // Test empty key in get
        let result = engine.get(b"").await;
        assert!(result.is_err());

        // Test empty key in delete
        let result = engine.delete(b"").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sequence_number_continuity() {
        let (mut engine, _temp_dir) = create_test_engine().await;

        // Add multiple operations
        engine.put(b"key1", b"value1").await.unwrap();
        engine.put(b"key2", b"value2").await.unwrap();
        engine.delete(b"key1").await.unwrap();
        engine.put(b"key3", b"value3").await.unwrap();

        // Verify all operations are accessible
        assert_eq!(engine.get(b"key1").await.unwrap(), None); // Deleted
        assert_eq!(engine.get(b"key2").await.unwrap(), Some(b"value2".to_vec()));
        assert_eq!(engine.get(b"key3").await.unwrap(), Some(b"value3".to_vec()));
    }
}
