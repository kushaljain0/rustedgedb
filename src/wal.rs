use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use thiserror::Error;
use tracing::{debug, error, info, trace, warn};

use crate::memtable::{Entry, MemTable, MemTableError};

/// Errors that can occur during WAL operations
#[derive(Error, Debug)]
pub enum WALError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid record format: {0}")]
    InvalidRecord(String),
    #[error("Corrupted WAL file: {0}")]
    CorruptedFile(String),
    #[error("MemTable error: {0}")]
    MemTable(#[from] MemTableError),
    #[error("WAL file not found: {0}")]
    FileNotFound(String),
}

/// Result type for WAL operations
pub type WALResult<T> = Result<T, WALError>;

/// Represents a single WAL record
#[derive(Debug, Clone, PartialEq)]
pub struct WALRecord {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>, // None for deletions (tombstones)
    pub timestamp: u64,
    pub sequence_number: u64,
}

impl WALRecord {
    /// Create a new WAL record
    pub fn new(key: Vec<u8>, value: Option<Vec<u8>>, timestamp: u64, sequence_number: u64) -> Self {
        Self {
            key,
            value,
            timestamp,
            sequence_number,
        }
    }

    /// Check if this record is a deletion (tombstone)
    pub fn is_deletion(&self) -> bool {
        self.value.is_none()
    }

    /// Convert to a MemTable Entry
    pub fn to_entry(&self) -> Entry {
        Entry::new(
            self.key.clone(),
            self.value.clone(),
            self.timestamp,
            self.sequence_number,
        )
    }
}

/// Write-Ahead Log implementation for durability
pub struct WAL {
    file: BufWriter<File>,
    path: std::path::PathBuf,
    sequence_number: u64,
}

impl WAL {
    /// Create a new WAL file or open existing one
    pub fn new<P: AsRef<Path>>(path: P) -> WALResult<Self> {
        let path = path.as_ref().to_path_buf();

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)
            .map_err(WALError::Io)?;

        let mut wal = Self {
            file: BufWriter::new(file),
            path,
            sequence_number: 0,
        };

        // Recover sequence number from existing file
        wal.recover_sequence_number()?;

        info!(
            "WAL initialized at {:?} with sequence number {}",
            wal.path, wal.sequence_number
        );
        Ok(wal)
    }

    /// Write a record to the WAL
    pub fn write_record(&mut self, record: &WALRecord) -> WALResult<()> {
        // Ensure sequence number is correct
        if record.sequence_number != self.sequence_number + 1 {
            return Err(WALError::InvalidRecord(format!(
                "Expected sequence number {}, got {}",
                self.sequence_number + 1,
                record.sequence_number
            )));
        }

        // Write record header: key_len (4 bytes) + value_len (4 bytes) + timestamp (8 bytes) + seq (8 bytes)
        let key_len = record.key.len() as u32;
        let value_len = record.value.as_ref().map_or(0, |v| v.len()) as u32;

        self.file.write_all(&key_len.to_le_bytes())?;
        self.file.write_all(&value_len.to_le_bytes())?;
        self.file.write_all(&record.timestamp.to_le_bytes())?;
        self.file.write_all(&record.sequence_number.to_le_bytes())?;

        // Write key and value data
        self.file.write_all(&record.key)?;
        if let Some(value) = &record.value {
            self.file.write_all(value)?;
        }

        // Flush to ensure durability
        self.file.flush()?;

        // Update sequence number
        self.sequence_number = record.sequence_number;

        trace!(
            "WAL write: key={:?}, value_len={}, seq={}",
            String::from_utf8_lossy(&record.key),
            record.value.as_ref().map_or(0, |v| v.len()),
            record.sequence_number
        );

        Ok(())
    }

    /// Write a put operation to the WAL
    pub fn put(&mut self, key: &[u8], value: &[u8], timestamp: u64) -> WALResult<()> {
        let record = WALRecord::new(
            key.to_vec(),
            Some(value.to_vec()),
            timestamp,
            self.sequence_number + 1,
        );
        self.write_record(&record)
    }

    /// Write a delete operation to the WAL
    pub fn delete(&mut self, key: &[u8], timestamp: u64) -> WALResult<()> {
        let record = WALRecord::new(
            key.to_vec(),
            None, // None indicates deletion
            timestamp,
            self.sequence_number + 1,
        );
        self.write_record(&record)
    }

    /// Get the current sequence number
    pub fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    /// Recover all records from the WAL into a MemTable
    pub fn recover(&self, memtable: &MemTable) -> WALResult<()> {
        info!("Starting WAL recovery for {:?}", self.path);

        let file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(WALError::Io)?;

        let mut reader = BufReader::new(file);
        let mut recovered_count = 0;
        let mut corrupted_count = 0;

        loop {
            match self.read_record(&mut reader) {
                Ok(Some(record)) => {
                    // Apply record to MemTable
                    if record.is_deletion() {
                        memtable.delete(&record.key).map_err(WALError::MemTable)?;
                    } else {
                        let value = record.value.as_ref().unwrap();
                        memtable
                            .put(&record.key, value)
                            .map_err(WALError::MemTable)?;
                    }
                    recovered_count += 1;

                    trace!(
                        "Recovered record: key={:?}, seq={}, is_deletion={}",
                        String::from_utf8_lossy(&record.key),
                        record.sequence_number,
                        record.is_deletion()
                    );
                }
                Ok(None) => {
                    // End of file
                    break;
                }
                Err(e) => {
                    warn!("Corrupted record during recovery: {}", e);
                    corrupted_count += 1;

                    // Try to find the next valid record by seeking forward
                    if let Err(seek_err) = self.seek_to_next_record(&mut reader) {
                        error!("Failed to seek to next record: {}", seek_err);
                        break;
                    }
                }
            }
        }

        info!(
            "WAL recovery completed: {} records recovered, {} corrupted",
            recovered_count, corrupted_count
        );

        Ok(())
    }

    /// Read a single record from the reader
    fn read_record<R: Read + Seek>(&self, reader: &mut R) -> WALResult<Option<WALRecord>> {
        // Read header (24 bytes total)
        let mut header = [0u8; 24];
        match reader.read_exact(&mut header) {
            Ok(_) => {}
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Ok(None); // End of file
            }
            Err(e) => return Err(WALError::Io(e)),
        }

        // Parse header
        let key_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let value_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let timestamp = u64::from_le_bytes([
            header[8], header[9], header[10], header[11], header[12], header[13], header[14],
            header[15],
        ]);
        let sequence_number = u64::from_le_bytes([
            header[16], header[17], header[18], header[19], header[20], header[21], header[22],
            header[23],
        ]);

        // Validate record sizes
        if key_len > 1024 * 1024 || value_len > 100 * 1024 * 1024 {
            // 1MB key, 100MB value limit
            return Err(WALError::InvalidRecord(format!(
                "Invalid record size: key_len={}, value_len={}",
                key_len, value_len
            )));
        }

        // Read key
        let mut key = vec![0u8; key_len];
        reader.read_exact(&mut key)?;

        // Read value (if any)
        let value = if value_len > 0 {
            let mut value_data = vec![0u8; value_len];
            reader.read_exact(&mut value_data)?;
            Some(value_data)
        } else {
            None
        };

        Ok(Some(WALRecord::new(key, value, timestamp, sequence_number)))
    }

    /// Try to seek to the next valid record after corruption
    fn seek_to_next_record<R: Read + Seek>(&self, reader: &mut R) -> WALResult<()> {
        // Try to find the next record by looking for a valid header pattern
        let mut buffer = [0u8; 1024];
        let mut offset: i64 = 0;

        loop {
            match reader.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    // Look for potential record headers (reasonable key/value lengths)
                    for i in 0..n.saturating_sub(23) {
                        let key_len = u32::from_le_bytes([
                            buffer[i],
                            buffer[i + 1],
                            buffer[i + 2],
                            buffer[i + 3],
                        ]) as usize;
                        let value_len = u32::from_le_bytes([
                            buffer[i + 4],
                            buffer[i + 5],
                            buffer[i + 6],
                            buffer[i + 7],
                        ]) as usize;

                        // Check if these look like reasonable lengths
                        if key_len <= 1024 * 1024 && value_len <= 100 * 1024 * 1024 {
                            // Seek to this potential record start
                            reader.seek(SeekFrom::Current(offset + i as i64))?;
                            return Ok(());
                        }
                    }
                    offset += n as i64;
                }
                Ok(_) => break, // End of file
                Err(e) => return Err(WALError::Io(e)),
            }
        }

        // If we get here, we couldn't find a valid record
        Err(WALError::CorruptedFile(
            "Could not find next valid record".to_string(),
        ))
    }

    /// Recover the sequence number from the existing WAL file
    fn recover_sequence_number(&mut self) -> WALResult<()> {
        let file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(WALError::Io)?;

        let mut reader = BufReader::new(file);
        let mut max_seq = 0u64;

        loop {
            match self.read_record(&mut reader) {
                Ok(Some(record)) => {
                    max_seq = max_seq.max(record.sequence_number);
                }
                Ok(None) => break,
                Err(_) => {
                    // Stop at first corruption, we have the highest sequence number
                    break;
                }
            }
        }

        self.sequence_number = max_seq;
        debug!("Recovered sequence number: {}", self.sequence_number);
        Ok(())
    }

    /// Truncate the WAL file (call after successful flush to SSTable)
    pub fn truncate(&mut self) -> WALResult<()> {
        // Flush any pending writes
        self.file.flush()?;

        // Close the current writer and truncate the file
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)
            .map_err(WALError::Io)?;

        self.file = BufWriter::new(file);
        self.sequence_number = 0;

        info!("WAL truncated at {:?}", self.path);
        Ok(())
    }

    /// Get the current file size
    pub fn file_size(&self) -> WALResult<u64> {
        let metadata = std::fs::metadata(&self.path).map_err(WALError::Io)?;
        Ok(metadata.len())
    }
}

impl Drop for WAL {
    fn drop(&mut self) {
        if let Err(e) = self.file.flush() {
            error!("Failed to flush WAL on drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_wal() -> (WAL, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let wal_path = temp_dir.path().join("test.wal");
        let wal = WAL::new(&wal_path).unwrap();
        (wal, temp_dir)
    }

    #[test]
    fn test_wal_creation() {
        let (wal, _temp_dir) = create_test_wal();
        assert_eq!(wal.sequence_number(), 0);
        assert!(wal.file_size().unwrap() == 0);
    }

    #[test]
    fn test_wal_put_and_get() {
        let (mut wal, _temp_dir) = create_test_wal();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write some records
        wal.put(b"key1", b"value1", timestamp).unwrap();
        wal.put(b"key2", b"value2", timestamp + 1).unwrap();

        assert_eq!(wal.sequence_number(), 2);
    }

    #[test]
    fn test_wal_delete() {
        let (mut wal, _temp_dir) = create_test_wal();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write a put and then delete
        wal.put(b"key1", b"value1", timestamp).unwrap();
        wal.delete(b"key1", timestamp + 1).unwrap();

        assert_eq!(wal.sequence_number(), 2);
    }

    #[test]
    fn test_wal_recovery() {
        let temp_dir = tempdir().unwrap();
        let wal_path = temp_dir.path().join("recovery.wal");

        // Create WAL and write some records
        {
            let mut wal = WAL::new(&wal_path).unwrap();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            wal.put(b"key1", b"value1", timestamp).unwrap();
            wal.put(b"key2", b"value2", timestamp + 1).unwrap();
            wal.delete(b"key1", timestamp + 2).unwrap();
        }

        // Create new WAL instance and recover
        let wal = WAL::new(&wal_path).unwrap();
        let memtable = MemTable::new(1024 * 1024);

        wal.recover(&memtable).unwrap();

        // Verify recovery
        assert_eq!(memtable.get(b"key1").unwrap(), None); // Deleted
        assert_eq!(memtable.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(memtable.len(), 2); // Both records exist (one as tombstone)
    }

    #[test]
    fn test_wal_sequence_numbers() {
        let (mut wal, _temp_dir) = create_test_wal();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Sequence numbers should increment
        wal.put(b"key1", b"value1", timestamp).unwrap();
        assert_eq!(wal.sequence_number(), 1);

        wal.put(b"key2", b"value2", timestamp + 1).unwrap();
        assert_eq!(wal.sequence_number(), 2);

        wal.delete(b"key1", timestamp + 2).unwrap();
        assert_eq!(wal.sequence_number(), 3);
    }

    #[test]
    fn test_wal_truncation() {
        let (mut wal, _temp_dir) = create_test_wal();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Write some records
        wal.put(b"key1", b"value1", timestamp).unwrap();
        wal.put(b"key2", b"value2", timestamp + 1).unwrap();

        let size_before = wal.file_size().unwrap();
        assert!(size_before > 0);

        // Truncate
        wal.truncate().unwrap();

        let size_after = wal.file_size().unwrap();
        assert_eq!(size_after, 0);
        assert_eq!(wal.sequence_number(), 0);
    }

    #[test]
    fn test_wal_corruption_handling() {
        let temp_dir = tempdir().unwrap();
        let wal_path = temp_dir.path().join("corruption.wal");

        // Create WAL and write some records
        {
            let mut wal = WAL::new(&wal_path).unwrap();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            wal.put(b"key1", b"value1", timestamp).unwrap();
            wal.put(b"key2", b"value2", timestamp + 1).unwrap();
        }

        // Corrupt the file by appending garbage
        {
            let mut file = OpenOptions::new().append(true).open(&wal_path).unwrap();
            file.write_all(b"corrupted data here").unwrap();
        }

        // Try to recover
        let wal = WAL::new(&wal_path).unwrap();
        let memtable = MemTable::new(1024 * 1024);

        // Recovery should succeed despite corruption
        wal.recover(&memtable).unwrap();

        // Should have recovered the valid records
        assert_eq!(memtable.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(memtable.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_wal_record_structure() {
        let record = WALRecord::new(
            b"test_key".to_vec(),
            Some(b"test_value".to_vec()),
            1234567890,
            42,
        );

        assert!(!record.is_deletion());
        assert_eq!(record.key, b"test_key");
        assert_eq!(record.value, Some(b"test_value".to_vec()));
        assert_eq!(record.timestamp, 1234567890);
        assert_eq!(record.sequence_number, 42);

        // Test deletion record
        let delete_record = WALRecord::new(b"delete_key".to_vec(), None, 1234567890, 43);

        assert!(delete_record.is_deletion());
        assert_eq!(delete_record.value, None);
    }

    #[test]
    fn test_wal_to_entry_conversion() {
        let record = WALRecord::new(
            b"test_key".to_vec(),
            Some(b"test_value".to_vec()),
            1234567890,
            42,
        );

        let entry = record.to_entry();
        assert_eq!(entry.key, b"test_key");
        assert_eq!(entry.value, Some(b"test_value".to_vec()));
        assert_eq!(entry.timestamp, 1234567890);
        assert_eq!(entry.sequence_number, 42);
    }
}
