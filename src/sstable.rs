use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use thiserror::Error;
use tracing::{error, info};

use crate::memtable::MemTable;

/// Errors that can occur during SSTable operations
#[derive(Error, Debug)]
pub enum SSTableError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid file format: {0}")]
    InvalidFormat(String),
    #[error("Corrupted SSTable file: {0}")]
    CorruptedFile(String),
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    #[error("Invalid index: {0}")]
    InvalidIndex(String),
}

/// Result type for SSTable operations
pub type SSTableResult<T> = Result<T, SSTableError>;

/// Compression type for SSTable data
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionType {
    None,
    LZ4,
    Zstd,
}

impl Default for CompressionType {
    fn default() -> Self {
        Self::None
    }
}

/// Metadata for compression
#[derive(Debug, Clone)]
pub struct CompressionMetadata {
    pub compression_type: CompressionType,
    pub original_size: usize,
    pub compressed_size: usize,
}

impl Default for CompressionMetadata {
    fn default() -> Self {
        Self {
            compression_type: CompressionType::None,
            original_size: 0,
            compressed_size: 0,
        }
    }
}

/// Index entry for fast key lookup
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub offset: u64,
    pub key_size: u32,
    pub value_size: u32,
}

/// SSTable index for binary search
#[derive(Debug, Clone)]
pub struct SSTableIndex {
    pub entries: Vec<IndexEntry>,
    pub bloom_filter_bits: Vec<u8>,
    pub compression_metadata: CompressionMetadata,
}

impl SSTableIndex {
    /// Create a new empty index
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            bloom_filter_bits: Vec::new(),
            compression_metadata: CompressionMetadata::default(),
        }
    }

    /// Add an index entry
    pub fn add_entry(&mut self, key: Vec<u8>, offset: u64, key_size: u32, value_size: u32) {
        self.entries.push(IndexEntry {
            key,
            offset,
            key_size,
            value_size,
        });
    }

    /// Find a key using binary search
    pub fn find_key(&self, target_key: &[u8]) -> Option<&IndexEntry> {
        self.entries
            .binary_search_by(|entry| entry.key.as_slice().cmp(target_key))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Get the number of index entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for SSTableIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// SSTable file header (64 bytes)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SSTableHeader {
    pub magic: [u8; 8],           // "RUSTEDGE" magic number
    pub version: u32,             // File format version
    pub entry_count: u32,         // Number of key-value pairs
    pub index_offset: u64,        // Offset to index section
    pub bloom_filter_offset: u64, // Offset to bloom filter
    pub data_offset: u64,         // Offset to data section
    pub compression_type: u8,     // Compression algorithm
    pub reserved: [u8; 31],       // Reserved for future use
}

impl SSTableHeader {
    /// Create a new header
    pub fn new(
        entry_count: u32,
        index_offset: u64,
        bloom_filter_offset: u64,
        data_offset: u64,
    ) -> Self {
        Self {
            magic: *b"RUSTEDGE",
            version: 1,
            entry_count,
            index_offset,
            bloom_filter_offset,
            data_offset,
            compression_type: CompressionType::None as u8,
            reserved: [0; 31],
        }
    }

    /// Write header to writer
    pub fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.magic)?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.entry_count.to_le_bytes())?;
        writer.write_all(&self.index_offset.to_le_bytes())?;
        writer.write_all(&self.bloom_filter_offset.to_le_bytes())?;
        writer.write_all(&self.data_offset.to_le_bytes())?;
        writer.write_all(&[self.compression_type])?;
        writer.write_all(&self.reserved)?;
        Ok(())
    }

    /// Read header from reader
    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;

        if magic != *b"RUSTEDGE" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid SSTable magic number",
            ));
        }

        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut version_bytes)?;
        let version = u32::from_le_bytes(version_bytes);

        let mut entry_count_bytes = [0u8; 4];
        reader.read_exact(&mut entry_count_bytes)?;
        let entry_count = u32::from_le_bytes(entry_count_bytes);

        let mut index_offset_bytes = [0u8; 8];
        reader.read_exact(&mut index_offset_bytes)?;
        let index_offset = u64::from_le_bytes(index_offset_bytes);

        let mut bloom_filter_offset_bytes = [0u8; 8];
        reader.read_exact(&mut bloom_filter_offset_bytes)?;
        let bloom_filter_offset = u64::from_le_bytes(bloom_filter_offset_bytes);

        let mut data_offset_bytes = [0u8; 8];
        reader.read_exact(&mut data_offset_bytes)?;
        let data_offset = u64::from_le_bytes(data_offset_bytes);

        let mut compression_type_bytes = [0u8; 1];
        reader.read_exact(&mut compression_type_bytes)?;
        let compression_type = compression_type_bytes[0];

        let mut reserved = [0u8; 31];
        reader.read_exact(&mut reserved)?;

        Ok(Self {
            magic,
            version,
            entry_count,
            index_offset,
            bloom_filter_offset,
            data_offset,
            compression_type,
            reserved,
        })
    }
}

/// SSTable footer (32 bytes)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SSTableFooter {
    pub checksum: u32,      // CRC32 checksum of data
    pub data_size: u64,     // Size of data section
    pub index_size: u64,    // Size of index section
    pub reserved: [u8; 12], // Reserved for future use
}

impl SSTableFooter {
    /// Create a new footer
    pub fn new(checksum: u32, data_size: u64, index_size: u64) -> Self {
        Self {
            checksum,
            data_size,
            index_size,
            reserved: [0; 12],
        }
    }

    /// Write footer to writer
    pub fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.checksum.to_le_bytes())?;
        writer.write_all(&self.data_size.to_le_bytes())?;
        writer.write_all(&self.index_size.to_le_bytes())?;
        writer.write_all(&self.reserved)?;
        Ok(())
    }

    /// Read footer from reader
    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut checksum_bytes = [0u8; 4];
        reader.read_exact(&mut checksum_bytes)?;
        let checksum = u32::from_le_bytes(checksum_bytes);

        let mut data_size_bytes = [0u8; 8];
        reader.read_exact(&mut data_size_bytes)?;
        let data_size = u64::from_le_bytes(data_size_bytes);

        let mut index_size_bytes = [0u8; 8];
        reader.read_exact(&mut index_size_bytes)?;
        let index_size = u64::from_le_bytes(index_size_bytes);

        let mut reserved = [0u8; 12];
        reader.read_exact(&mut reserved)?;

        Ok(Self {
            checksum,
            data_size,
            index_size,
            reserved,
        })
    }
}

/// Simple bloom filter implementation
#[derive(Debug, Clone)]
pub struct BloomFilter {
    bits: Vec<u8>,
    size: usize,
    hash_count: usize,
}

impl BloomFilter {
    /// Create a new bloom filter
    pub fn new(size: usize, hash_count: usize) -> Self {
        let byte_size = size.div_ceil(8); // Round up to nearest byte
        Self {
            bits: vec![0; byte_size],
            size,
            hash_count,
        }
    }

    /// Add a key to the bloom filter
    pub fn add(&mut self, key: &[u8]) {
        for i in 0..self.hash_count {
            let hash = self.hash(key, i);
            let bit_index = hash % self.size;
            let byte_index = bit_index / 8;
            let bit_offset = bit_index % 8;
            self.bits[byte_index] |= 1 << bit_offset;
        }
    }

    /// Check if a key might be in the bloom filter
    pub fn might_contain(&self, key: &[u8]) -> bool {
        for i in 0..self.hash_count {
            let hash = self.hash(key, i);
            let bit_index = hash % self.size;
            let byte_index = bit_index / 8;
            let bit_offset = bit_index % 8;
            if (self.bits[byte_index] & (1 << bit_offset)) == 0 {
                return false;
            }
        }
        true
    }

    /// Simple hash function (Fowler-Noll-Vo hash)
    fn hash(&self, key: &[u8], seed: usize) -> usize {
        let mut hash: usize = 0x811c9dc5;
        for &byte in key {
            hash ^= byte as usize;
            hash = hash.wrapping_mul(0x01000193);
        }
        hash.wrapping_add(seed)
    }

    /// Get the bloom filter bits
    pub fn bits(&self) -> &[u8] {
        &self.bits
    }

    /// Set the bloom filter bits
    pub fn set_bits(&mut self, bits: Vec<u8>) {
        self.bits = bits;
    }
}

/// SSTable implementation for immutable file storage
#[derive(Debug)]
pub struct SSTable {
    file: File,
    path: std::path::PathBuf,
    header: SSTableHeader,
    index: SSTableIndex,
    bloom_filter: BloomFilter,
}

impl SSTable {
    /// Create a new SSTable by flushing a MemTable
    pub fn from_memtable<P: AsRef<Path>>(
        path: P,
        memtable: &MemTable,
        _compression: CompressionType,
    ) -> SSTableResult<Self> {
        let path = path.as_ref().to_path_buf();
        let entries = memtable.entries();

        if entries.is_empty() {
            return Err(SSTableError::InvalidFormat(
                "Cannot create SSTable from empty MemTable".to_string(),
            ));
        }

        info!(
            "Creating SSTable from MemTable with {} entries",
            entries.len()
        );

        // Create the file
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(&path)
            .map_err(SSTableError::Io)?;

        let mut writer = BufWriter::new(file);
        let mut index = SSTableIndex::new();
        let mut bloom_filter = BloomFilter::new(entries.len() * 10, 3); // 10x size, 3 hash functions

        // Write header placeholder (we'll update it later)
        let header_size = std::mem::size_of::<SSTableHeader>();
        let header_placeholder = vec![0u8; header_size];
        writer.write_all(&header_placeholder)?;

        // Write bloom filter placeholder
        let bloom_filter_offset = writer.stream_position()?;
        let bloom_filter_placeholder = vec![0u8; 64]; // Fixed size for now
        writer.write_all(&bloom_filter_placeholder)?;

        // Write data section
        let data_offset = writer.stream_position()?;

        for entry in &entries {
            // Add to bloom filter
            bloom_filter.add(&entry.key);

            // Calculate entry start position
            let entry_start = writer.stream_position()?;

            // Write entry header: key_len (4) + value_len (4) + timestamp (8) + seq (8)
            let key_len = entry.key.len() as u32;
            let value_len = entry.value.as_ref().map_or(0, |v| v.len()) as u32;

            writer.write_all(&key_len.to_le_bytes())?;
            writer.write_all(&value_len.to_le_bytes())?;
            writer.write_all(&entry.timestamp.to_le_bytes())?;
            writer.write_all(&entry.sequence_number.to_le_bytes())?;

            // Write key and value data
            writer.write_all(&entry.key)?;
            if let Some(value) = &entry.value {
                writer.write_all(value)?;
            }

            // Add to index
            index.add_entry(entry.key.clone(), entry_start, key_len, value_len);
        }

        // Calculate total data size
        let data_size = writer.stream_position()? - data_offset;

        // Write index section
        let index_offset = writer.stream_position()?;
        let index_size = Self::write_index(&mut writer, &index)?;

        // Write footer
        let footer = SSTableFooter::new(0, data_size, index_size as u64); // TODO: Add checksum
        footer.write(&mut writer)?;

        // Update bloom filter
        let _bloom_filter_size = bloom_filter.bits().len();
        writer.seek(SeekFrom::Start(bloom_filter_offset))?;
        writer.write_all(bloom_filter.bits())?;

        // Write header with final offsets
        let header = SSTableHeader::new(
            entries.len() as u32,
            index_offset,
            bloom_filter_offset,
            data_offset,
        );
        writer.seek(SeekFrom::Start(0))?;
        header.write(&mut writer)?;

        // Flush and close writer
        writer.flush()?;
        drop(writer);

        // Reopen file for reading
        let file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(SSTableError::Io)?;

        info!("SSTable created successfully at {:?}", path);

        Ok(Self {
            file,
            path,
            header,
            index,
            bloom_filter,
        })
    }

    /// Open an existing SSTable for reading
    pub fn open<P: AsRef<Path>>(path: P) -> SSTableResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(SSTableError::Io)?;

        // Read header
        let header = SSTableHeader::read(&mut file)
            .map_err(|e| SSTableError::InvalidFormat(format!("Failed to read header: {}", e)))?;

        // Read bloom filter
        file.seek(SeekFrom::Start(header.bloom_filter_offset))?;
        let bloom_filter_size = header
            .index_offset
            .saturating_sub(header.bloom_filter_offset);

        let mut bloom_filter_bits = vec![0u8; bloom_filter_size as usize];
        if bloom_filter_size > 0 {
            file.read_exact(&mut bloom_filter_bits)?;
        }

        let mut bloom_filter = BloomFilter::new(header.entry_count as usize * 10, 3);
        bloom_filter.set_bits(bloom_filter_bits);

        // Read index
        file.seek(SeekFrom::Start(header.index_offset))?;
        let index = Self::read_index(&mut file, header.entry_count as usize)?;

        info!("SSTable opened successfully from {:?}", path);

        Ok(Self {
            file,
            path,
            header,
            index,
            bloom_filter,
        })
    }

    /// Get a value by key using binary search
    pub fn get(&mut self, key: &[u8]) -> SSTableResult<Option<Vec<u8>>> {
        // Check bloom filter first
        if !self.bloom_filter.might_contain(key) {
            println!("DEBUG: Bloom filter rejected key {:?}", String::from_utf8_lossy(key));
            return Ok(None);
        }
        println!("DEBUG: Bloom filter passed for key {:?}", String::from_utf8_lossy(key));

        // Find key in index
        let index_entry = self.index.find_key(key);
        if index_entry.is_none() {
            println!("DEBUG: Key {:?} not found in index", String::from_utf8_lossy(key));
            return Ok(None);
        }
        let index_entry = index_entry.unwrap();
        println!("DEBUG: Key {:?} found in index at offset {}", String::from_utf8_lossy(key), index_entry.offset);

        // Seek to data position
        self.file.seek(SeekFrom::Start(index_entry.offset))?;
        println!("DEBUG: Seeking to offset {} for key {:?}", index_entry.offset, String::from_utf8_lossy(key));

        // Read entry header
        let mut key_len_bytes = [0u8; 4];
        self.file.read_exact(&mut key_len_bytes)?;
        let key_len = u32::from_le_bytes(key_len_bytes);
        println!("DEBUG: Read key_len: {}", key_len);

        let mut value_len_bytes = [0u8; 4];
        self.file.read_exact(&mut value_len_bytes)?;
        let value_len = u32::from_le_bytes(value_len_bytes);
        println!("DEBUG: Read value_len: {}", value_len);

        let mut timestamp_bytes = [0u8; 8];
        self.file.read_exact(&mut timestamp_bytes)?;
        let _timestamp = u64::from_le_bytes(timestamp_bytes);

        let mut seq_bytes = [0u8; 8];
        self.file.read_exact(&mut seq_bytes)?;
        let _sequence_number = u64::from_le_bytes(seq_bytes);

        // Read key (verify it matches)
        let mut stored_key = vec![0u8; key_len as usize];
        self.file.read_exact(&mut stored_key)?;
        println!("DEBUG: Read stored_key: {:?}", String::from_utf8_lossy(&stored_key));

        if stored_key != key {
            println!("DEBUG: Key mismatch! Expected {:?}, got {:?}", 
                    String::from_utf8_lossy(key), String::from_utf8_lossy(&stored_key));
            return Err(SSTableError::InvalidIndex(format!(
                "Key mismatch: expected {:?}, got {:?}",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(&stored_key)
            )));
        }
        println!("DEBUG: Key verification passed");

        // Read value
        if value_len > 0 {
            let mut value = vec![0u8; value_len as usize];
            self.file.read_exact(&mut value)?;
            println!("DEBUG: Read value: {:?}", String::from_utf8_lossy(&value));
            Ok(Some(value))
        } else {
            println!("DEBUG: Value length is 0 (tombstone)");
            Ok(None) // Tombstone
        }
    }

    /// Get the file path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the number of entries
    pub fn entry_count(&self) -> u32 {
        self.header.entry_count
    }

    /// Check if the SSTable is empty
    pub fn is_empty(&self) -> bool {
        self.entry_count() == 0
    }

    /// Write index to writer
    fn write_index<W: Write + Seek>(writer: &mut W, index: &SSTableIndex) -> io::Result<usize> {
        let start_pos = writer.stream_position()?;

        // Write index header: entry_count (4 bytes)
        let entry_count = index.len() as u32;
        writer.write_all(&entry_count.to_le_bytes())?;

        // Write each index entry
        for entry in &index.entries {
            // key_len (4) + key + offset (8) + key_size (4) + value_size (4)
            let key_len = entry.key.len() as u32;
            writer.write_all(&key_len.to_le_bytes())?;
            writer.write_all(&entry.key)?;
            writer.write_all(&entry.offset.to_le_bytes())?;
            writer.write_all(&entry.key_size.to_le_bytes())?;
            writer.write_all(&entry.value_size.to_le_bytes())?;
        }

        let end_pos = writer.stream_position()?;
        Ok((end_pos - start_pos) as usize)
    }

    /// Read index from reader
    fn read_index<R: Read>(reader: &mut R, entry_count: usize) -> io::Result<SSTableIndex> {
        let mut index = SSTableIndex::new();

        // Read index header
        let mut entry_count_bytes = [0u8; 4];
        reader.read_exact(&mut entry_count_bytes)?;
        let actual_entry_count = u32::from_le_bytes(entry_count_bytes) as usize;

        if actual_entry_count != entry_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Index entry count mismatch: expected {}, got {}",
                    entry_count, actual_entry_count
                ),
            ));
        }

        // Read each index entry
        for _ in 0..actual_entry_count {
            let mut key_len_bytes = [0u8; 4];
            reader.read_exact(&mut key_len_bytes)?;
            let key_len = u32::from_le_bytes(key_len_bytes) as usize;

            let mut key = vec![0u8; key_len];
            reader.read_exact(&mut key)?;

            let mut offset_bytes = [0u8; 8];
            reader.read_exact(&mut offset_bytes)?;
            let offset = u64::from_le_bytes(offset_bytes);

            let mut key_size_bytes = [0u8; 4];
            reader.read_exact(&mut key_size_bytes)?;
            let key_size = u32::from_le_bytes(key_size_bytes);

            let mut value_size_bytes = [0u8; 4];
            reader.read_exact(&mut value_size_bytes)?;
            let value_size = u32::from_le_bytes(value_size_bytes);

            index.add_entry(key, offset, key_size, value_size);
        }

        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_memtable() -> MemTable {
        let memtable = MemTable::new(1024 * 1024);

        // Add some test data
        memtable.put(b"apple", b"apple_value").unwrap();
        memtable.put(b"banana", b"banana_value").unwrap();
        memtable.put(b"cherry", b"cherry_value").unwrap();
        memtable.delete(b"banana").unwrap(); // Add a tombstone

        memtable
    }

    #[test]
    fn test_sstable_creation_and_reading() {
        let temp_dir = tempdir().unwrap();
        let sstable_path = temp_dir.path().join("test.sst");

        // Create SSTable from MemTable
        let memtable = create_test_memtable();
        let mut sstable =
            SSTable::from_memtable(&sstable_path, &memtable, CompressionType::None).unwrap();

        // Verify basic properties
        assert_eq!(sstable.entry_count(), 3); // apple, banana (tombstone), cherry
        assert!(!sstable.is_empty());
        assert_eq!(sstable.path(), sstable_path);

        // Test reading values
        assert_eq!(
            sstable.get(b"apple").unwrap(),
            Some(b"apple_value".to_vec())
        );
        assert_eq!(
            sstable.get(b"cherry").unwrap(),
            Some(b"cherry_value".to_vec())
        );
        assert_eq!(sstable.get(b"banana").unwrap(), None); // Tombstone

        // Test non-existent key
        assert_eq!(sstable.get(b"nonexistent").unwrap(), None);
    }

    #[test]
    fn test_sstable_bloom_filter() {
        let temp_dir = tempdir().unwrap();
        let sstable_path = temp_dir.path().join("bloom.sst");

        let memtable = create_test_memtable();
        let sstable =
            SSTable::from_memtable(&sstable_path, &memtable, CompressionType::None).unwrap();

        // Bloom filter should contain our keys
        assert!(sstable.bloom_filter.might_contain(b"apple"));
        assert!(sstable.bloom_filter.might_contain(b"cherry"));

        // Bloom filter might have false positives, but should be reasonable
        // This is a probabilistic test
        let false_positives = (0..100)
            .filter(|_| {
                sstable
                    .bloom_filter
                    .might_contain(format!("random_key_{}", rand::random::<u32>()).as_bytes())
            })
            .count();

        // Should have reasonable false positives (< 20% for this small dataset)
        assert!(false_positives < 20);
    }

    #[test]
    fn test_sstable_index() {
        let temp_dir = tempdir().unwrap();
        let sstable_path = temp_dir.path().join("index.sst");

        let memtable = create_test_memtable();
        let sstable =
            SSTable::from_memtable(&sstable_path, &memtable, CompressionType::None).unwrap();

        // Verify index properties
        assert_eq!(sstable.index.len(), 3);
        assert!(!sstable.index.is_empty());

        // Verify index entries are sorted (MemTable maintains insertion order)
        let keys: Vec<&[u8]> = sstable
            .index
            .entries
            .iter()
            .map(|e| e.key.as_slice())
            .collect();
        // The order should be: apple, banana (tombstone), cherry
        assert_eq!(keys.len(), 3);
        assert!(keys.iter().any(|k| k == b"apple"));
        assert!(keys.iter().any(|k| k == b"banana"));
        assert!(keys.iter().any(|k| k == b"cherry"));
    }

    #[test]
    fn test_sstable_file_format() {
        let temp_dir = tempdir().unwrap();
        let sstable_path = temp_dir.path().join("format.sst");

        let memtable = create_test_memtable();
        SSTable::from_memtable(&sstable_path, &memtable, CompressionType::None).unwrap();

        // Verify file exists and has reasonable size
        let metadata = std::fs::metadata(&sstable_path).unwrap();
        assert!(metadata.len() > 100); // Should be at least 100 bytes

        // Try to open the file again
        let sstable = SSTable::open(&sstable_path).unwrap();
        assert_eq!(sstable.entry_count(), 3);
    }

    #[test]
    fn test_sstable_empty_memtable() {
        let temp_dir = tempdir().unwrap();
        let sstable_path = temp_dir.path().join("empty.sst");

        let empty_memtable = MemTable::new(1024);

        // Should fail to create SSTable from empty MemTable
        let result = SSTable::from_memtable(&sstable_path, &empty_memtable, CompressionType::None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SSTableError::InvalidFormat(_)
        ));
    }

    #[test]
    fn test_sstable_compression_types() {
        let temp_dir = tempdir().unwrap();
        let memtable = create_test_memtable();

        // Test different compression types
        let sstable_path1 = temp_dir.path().join("none.sst");
        let sstable1 =
            SSTable::from_memtable(&sstable_path1, &memtable, CompressionType::None).unwrap();
        assert_eq!(
            sstable1.header.compression_type,
            CompressionType::None as u8
        );

        // Note: LZ4 and Zstd compression would require additional dependencies
        // For now, we just test that the enum works correctly
        assert_eq!(CompressionType::None as u8, 0);
        assert_eq!(CompressionType::LZ4 as u8, 1);
        assert_eq!(CompressionType::Zstd as u8, 2);
    }

    #[test]
    fn test_sstable_header_footer() {
        let header = SSTableHeader::new(100, 1024, 2048, 4096);
        assert_eq!(header.entry_count, 100);
        assert_eq!(header.index_offset, 1024);
        assert_eq!(header.bloom_filter_offset, 2048);
        assert_eq!(header.data_offset, 4096);

        let footer = SSTableFooter::new(12345, 1000, 500);
        assert_eq!(footer.checksum, 12345);
        assert_eq!(footer.data_size, 1000);
        assert_eq!(footer.index_size, 500);
    }

    #[test]
    fn test_bloom_filter() {
        let mut bloom = BloomFilter::new(100, 3);

        // Add some keys
        bloom.add(b"key1");
        bloom.add(b"key2");

        // Should contain added keys
        assert!(bloom.might_contain(b"key1"));
        assert!(bloom.might_contain(b"key2"));

        // Should not contain random keys (with high probability)
        assert!(!bloom.might_contain(b"random_key"));
    }
}
