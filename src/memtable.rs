use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::{debug, trace};

/// Errors that can occur during MemTable operations
#[derive(Error, Debug)]
pub enum MemTableError {
    #[error("MemTable is full and cannot accept more data")]
    TableFull,
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("Invalid value: {0}")]
    InvalidValue(String),
}

/// Result type for MemTable operations
pub type MemTableResult<T> = Result<T, MemTableError>;

/// Represents a single entry in the MemTable
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>, // None for deletions (tombstones)
    pub timestamp: u64,
    pub sequence_number: u64,
}

impl Entry {
    /// Create a new entry
    pub fn new(key: Vec<u8>, value: Option<Vec<u8>>, timestamp: u64, sequence_number: u64) -> Self {
        Self {
            key,
            value,
            timestamp,
            sequence_number,
        }
    }

    /// Check if this entry is a deletion (tombstone)
    pub fn is_deletion(&self) -> bool {
        self.value.is_none()
    }

    /// Get the size of this entry in bytes
    pub fn size_bytes(&self) -> usize {
        self.key.len() + self.value.as_ref().map_or(0, |v| v.len()) + 16 // timestamp + sequence
    }
}

/// Thread-safe MemTable implementation using a sorted vector for simplicity and performance
pub struct MemTable {
    data: Arc<RwLock<Vec<Entry>>>,
    size_bytes: Arc<RwLock<usize>>,
    max_size_bytes: usize,
    sequence_number: Arc<RwLock<u64>>,
}

impl MemTable {
    /// Create a new MemTable with the specified maximum size
    pub fn new(max_size_bytes: usize) -> Self {
        Self {
            data: Arc::new(RwLock::new(Vec::new())),
            size_bytes: Arc::new(RwLock::new(0)),
            max_size_bytes,
            sequence_number: Arc::new(RwLock::new(0)),
        }
    }

    /// Create a new MemTable with default size (64MB)
    pub fn new_default() -> Self {
        Self::new(64 * 1024 * 1024)
    }

    /// Put a key-value pair into the MemTable
    pub fn put(&self, key: &[u8], value: &[u8]) -> MemTableResult<()> {
        if key.is_empty() {
            return Err(MemTableError::InvalidKey("Key cannot be empty".to_string()));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let sequence_number = {
            let mut seq = self.sequence_number.write().unwrap();
            *seq += 1;
            *seq
        };

        let entry = Entry::new(
            key.to_vec(),
            Some(value.to_vec()),
            timestamp,
            sequence_number,
        );

        // Check if adding this entry would exceed the size limit
        let current_size = *self.size_bytes.read().unwrap();
        let entry_size = entry.size_bytes();
        if current_size + entry_size > self.max_size_bytes {
            return Err(MemTableError::TableFull);
        }

        // Update the data
        let mut data = self.data.write().unwrap();
        let old_entry = self.insert_or_update(&mut data, entry.clone());

        // Update size tracking - recalculate total size
        if let Some(old_entry) = old_entry {
            // If updating, we need to account for the size difference
            let old_size = old_entry.size_bytes();
            let size_diff = entry_size.saturating_sub(old_size);
            *self.size_bytes.write().unwrap() += size_diff;
        } else {
            // If inserting new, add the full entry size
            *self.size_bytes.write().unwrap() += entry_size;
        }

        debug!(
            "Put key={:?}, value_len={}, sequence={}, size_bytes={}",
            String::from_utf8_lossy(key),
            value.len(),
            sequence_number,
            *self.size_bytes.read().unwrap()
        );

        Ok(())
    }

    /// Get a value from the MemTable
    pub fn get(&self, key: &[u8]) -> MemTableResult<Option<Vec<u8>>> {
        if key.is_empty() {
            return Err(MemTableError::InvalidKey("Key cannot be empty".to_string()));
        }

        let data = self.data.read().unwrap();
        let result = self
            .find_entry(&data, key)
            .and_then(|entry| entry.value.clone());

        trace!(
            "Get key={:?}, found={}",
            String::from_utf8_lossy(key),
            result.is_some()
        );

        Ok(result)
    }

    /// Delete a key from the MemTable (creates a tombstone)
    pub fn delete(&self, key: &[u8]) -> MemTableResult<()> {
        if key.is_empty() {
            return Err(MemTableError::InvalidKey("Key cannot be empty".to_string()));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let sequence_number = {
            let mut seq = self.sequence_number.write().unwrap();
            *seq += 1;
            *seq
        };

        let entry = Entry::new(key.to_vec(), None, timestamp, sequence_number);

        // Check if adding this entry would exceed the size limit
        let current_size = *self.size_bytes.read().unwrap();
        let entry_size = entry.size_bytes();
        if current_size + entry_size > self.max_size_bytes {
            return Err(MemTableError::TableFull);
        }

        // Update the data
        let mut data = self.data.write().unwrap();
        let old_entry = self.insert_or_update(&mut data, entry.clone());

        // Update size tracking - recalculate total size
        if let Some(old_entry) = old_entry {
            // If updating, we need to account for the size difference
            let old_size = old_entry.size_bytes();
            let size_diff = entry_size.saturating_sub(old_size);
            *self.size_bytes.write().unwrap() += size_diff;
        } else {
            // If inserting new, add the full entry size
            *self.size_bytes.write().unwrap() += entry_size;
        }

        debug!(
            "Delete key={:?}, sequence={}, size_bytes={}",
            String::from_utf8_lossy(key),
            sequence_number,
            *self.size_bytes.read().unwrap()
        );

        Ok(())
    }

    /// Get the current size of the MemTable in bytes
    pub fn size_bytes(&self) -> usize {
        *self.size_bytes.read().unwrap()
    }

    /// Check if the MemTable is empty
    pub fn is_empty(&self) -> bool {
        self.data.read().unwrap().is_empty()
    }

    /// Get the number of entries in the MemTable
    pub fn len(&self) -> usize {
        self.data.read().unwrap().len()
    }

    /// Get the current sequence number
    pub fn sequence_number(&self) -> u64 {
        *self.sequence_number.read().unwrap()
    }

    /// Check if the MemTable is full
    pub fn is_full(&self) -> bool {
        self.size_bytes() >= self.max_size_bytes
    }

    /// Get all entries as a vector (for flushing to SSTable)
    pub fn entries(&self) -> Vec<Entry> {
        self.data.read().unwrap().clone()
    }

    /// Clear the MemTable and reset sequence number
    pub fn clear(&self) {
        let mut data = self.data.write().unwrap();
        data.clear();
        *self.size_bytes.write().unwrap() = 0;
        *self.sequence_number.write().unwrap() = 0;
        debug!("MemTable cleared");
    }

    /// Insert or update an entry in the sorted vector
    fn insert_or_update(&self, data: &mut Vec<Entry>, entry: Entry) -> Option<Entry> {
        match data.binary_search_by(|e| e.key.as_slice().cmp(entry.key.as_slice())) {
            Ok(index) => {
                // Key exists, update it
                let old_entry = std::mem::replace(&mut data[index], entry);
                Some(old_entry)
            }
            Err(index) => {
                // Key doesn't exist, insert it
                data.insert(index, entry);
                None
            }
        }
    }

    /// Find an entry by key using binary search
    fn find_entry<'a>(&self, data: &'a [Entry], key: &[u8]) -> Option<&'a Entry> {
        data.binary_search_by(|e| e.key.as_slice().cmp(key))
            .ok()
            .map(|index| &data[index])
    }
}

impl Default for MemTable {
    fn default() -> Self {
        Self::new_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memtable_creation() {
        let memtable = MemTable::new(1024);
        assert_eq!(memtable.size_bytes(), 0);
        assert!(memtable.is_empty());
        assert_eq!(memtable.len(), 0);
        assert!(!memtable.is_full());
    }

    #[test]
    fn test_memtable_put_and_get() {
        let memtable = MemTable::new(1024);

        // Test basic put and get
        memtable.put(b"key1", b"value1").unwrap();
        assert_eq!(memtable.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(memtable.len(), 1);
        assert!(!memtable.is_empty());

        // Test overwriting
        memtable.put(b"key1", b"new_value").unwrap();
        assert_eq!(memtable.get(b"key1").unwrap(), Some(b"new_value".to_vec()));
        assert_eq!(memtable.len(), 1); // Still only one key
    }

    #[test]
    fn test_memtable_delete() {
        let memtable = MemTable::new(1024);

        // Put a value
        memtable.put(b"key1", b"value1").unwrap();
        assert_eq!(memtable.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Delete it
        memtable.delete(b"key1").unwrap();
        assert_eq!(memtable.get(b"key1").unwrap(), None);

        // Entry still exists but as tombstone
        assert_eq!(memtable.len(), 1);
    }

    #[test]
    fn test_memtable_size_tracking() {
        let memtable = MemTable::new(50); // Size limit that allows one entry but not two

        // Should fit
        memtable.put(b"key1", b"value1").unwrap();
        assert!(memtable.size_bytes() > 0);
        assert!(!memtable.is_full());

        // Should reject the second entry due to size limit
        let result = memtable.put(b"key2", b"value2");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemTableError::TableFull));

        // Should still be able to get the first entry
        assert_eq!(memtable.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(memtable.len(), 1);
    }

    #[test]
    fn test_memtable_sequence_numbers() {
        let memtable = MemTable::new(1024);

        let initial_seq = memtable.sequence_number();

        memtable.put(b"key1", b"value1").unwrap();
        assert_eq!(memtable.sequence_number(), initial_seq + 1);

        memtable.put(b"key2", b"value2").unwrap();
        assert_eq!(memtable.sequence_number(), initial_seq + 2);

        memtable.delete(b"key1").unwrap();
        assert_eq!(memtable.sequence_number(), initial_seq + 3);
    }

    #[test]
    fn test_memtable_entries() {
        let memtable = MemTable::new(1024);

        memtable.put(b"key1", b"value1").unwrap();
        memtable.put(b"key2", b"value2").unwrap();
        memtable.delete(b"key1").unwrap();

        let entries = memtable.entries();
        assert_eq!(entries.len(), 2);

        // Check that entries are sorted by key
        assert!(entries[0].key <= entries[1].key);

        // Check that deletion is preserved
        let deleted_entry = entries.iter().find(|e| e.key == b"key1").unwrap();
        assert!(deleted_entry.is_deletion());
    }

    #[test]
    fn test_memtable_clear() {
        let memtable = MemTable::new(1024);

        memtable.put(b"key1", b"value1").unwrap();
        memtable.put(b"key2", b"value2").unwrap();

        assert_eq!(memtable.len(), 2);
        assert!(!memtable.is_empty());

        memtable.clear();

        assert_eq!(memtable.len(), 0);
        assert!(memtable.is_empty());
        assert_eq!(memtable.size_bytes(), 0);
        assert_eq!(memtable.sequence_number(), 0);
    }

    #[test]
    fn test_memtable_invalid_inputs() {
        let memtable = MemTable::new(1024);

        // Empty key
        let result = memtable.put(b"", b"value");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemTableError::InvalidKey(_)));

        let result = memtable.get(b"");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemTableError::InvalidKey(_)));

        let result = memtable.delete(b"");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MemTableError::InvalidKey(_)));
    }

    #[test]
    fn test_memtable_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let memtable = Arc::new(MemTable::new(1024 * 1024));
        let mut handles = vec![];

        // Spawn multiple threads doing puts
        for i in 0..10 {
            let memtable = Arc::clone(&memtable);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let key = format!("key_{}_{}", i, j);
                    let value = format!("value_{}_{}", i, j);
                    memtable.put(key.as_bytes(), value.as_bytes()).unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all data was written
        assert_eq!(memtable.len(), 1000);

        // Verify we can read the data
        for i in 0..10 {
            for j in 0..100 {
                let key = format!("key_{}_{}", i, j);
                let expected_value = format!("value_{}_{}", i, j);
                let actual_value = memtable.get(key.as_bytes()).unwrap();
                assert_eq!(actual_value, Some(expected_value.into_bytes()));
            }
        }
    }

    #[test]
    fn test_memtable_ordering() {
        let memtable = MemTable::new(1024);

        // Insert keys in random order
        memtable.put(b"zebra", b"zebra_value").unwrap();
        memtable.put(b"apple", b"apple_value").unwrap();
        memtable.put(b"banana", b"banana_value").unwrap();

        let entries = memtable.entries();
        assert_eq!(entries.len(), 3);

        // Verify they're sorted
        assert_eq!(entries[0].key, b"apple");
        assert_eq!(entries[1].key, b"banana");
        assert_eq!(entries[2].key, b"zebra");
    }
}
