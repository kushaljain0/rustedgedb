# RustEdgeDB Specification

## Version 0.1.0 - Base Engine

**Status**: Current Specification  
**Last Updated**: 2025-01-08  
**Compatibility**: Breaking changes require major version bump  

---

## Table of Contents
1. [Motivation](#motivation)
2. [Data Structures](#data-structures)
3. [API Surface](#api-surface)
4. [Invariants](#invariants)
5. [Testing Strategy](#testing-strategy)
6. [Implementation Notes](#implementation-notes)

---

## Motivation

### Design Goals
RustEdgeDB v0.1 implements a **simple LSM-based key-value store** designed for:

- **Edge Computing**: Minimal resource footprint (< 10MB memory, < 50MB disk)
- **Embedded Use**: Single-file storage, no external dependencies
- **Deterministic Behavior**: Consistent performance across platforms
- **Crash Recovery**: WAL-based durability with fast startup

### LSM Tree Benefits
- **Write-Optimized**: Sequential writes for high throughput
- **Read-Optimized**: Bloom filters and sparse indexes for fast lookups
- **Space Efficient**: Compression and compaction reduce storage overhead
- **Crash Safe**: WAL ensures durability, SSTables provide persistence

### Target Use Cases
- **Configuration Storage**: Application settings and user preferences
- **Session Data**: Temporary state with persistence
- **Local Caching**: Fast access to frequently used data
- **Edge Analytics**: Small datasets with real-time requirements

---

## Data Structures

### 1. MemTable
**Purpose**: In-memory write buffer for recent operations

#### Structure
```rust
pub struct MemTable {
    data: Arc<RwLock<Vec<Entry>>>,
    size_bytes: Arc<RwLock<usize>>,
    max_size_bytes: usize,
    sequence_number: Arc<RwLock<u64>>,
}

pub struct Entry {
    key: Vec<u8>,
    value: Option<Vec<u8>>, // None for deletions
    timestamp: u64,
    sequence_number: u64,
}
```

#### Properties
- **Ordered**: Sorted vector maintains sorted key order using binary search
- **Bounded**: Configurable maximum size (default: 64MB)
- **Mutable**: Supports in-place updates and deletions
- **Fast**: O(log n) operations for all operations
- **Thread-Safe**: Uses Arc + RwLock for concurrent access

#### Implementation Details
- **Data Structure**: Sorted vector with binary search for O(log n) operations
- **Thread Safety**: Arc<RwLock<Vec<Entry>>> for shared mutable state
- **Size Tracking**: Accurate byte-level size monitoring with configurable limits
- **Sequence Numbers**: Monotonically increasing sequence numbers for all operations
- **Tombstone Support**: Deletions create entries with None values
- **Error Handling**: Comprehensive error types using thiserror crate
- **Logging**: Structured logging with tracing crate for observability

#### Lifecycle
1. **Creation**: Empty MemTable with sequence number 0
2. **Population**: Writes accumulate until size threshold
3. **Flush**: Converted to SSTable when full
4. **Replacement**: New MemTable created for continued writes

### 2. Write-Ahead Log (WAL)
**Purpose**: Durability guarantee for crash recovery

#### Structure
```rust
pub struct WAL {
    file: BufWriter<File>,
    path: PathBuf,
    sequence_number: u64,
}

pub struct WALRecord {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>, // None for deletions (tombstones)
    pub timestamp: u64,
    pub sequence_number: u64,
}
```

#### Properties
- **Append-Only**: Sequential writes for maximum performance
- **Buffered**: Uses BufWriter for efficient I/O operations
- **Recoverable**: Automatic sequence number recovery on startup
- **Truncatable**: Safe truncation after successful flush to SSTable
- **Corruption Resilient**: Handles partial writes and seeks to next valid record

#### Record Format
Each WAL record follows this binary structure:
```
[Key Length: 4 bytes (u32, little-endian)]
[Value Length: 4 bytes (u32, little-endian)] 
[Timestamp: 8 bytes (u64, little-endian)]
[Sequence Number: 8 bytes (u64, little-endian)]
[Key Data: variable length]
[Value Data: variable length (if value_len > 0)]
```

#### Durability Guarantees
- **Write Ordering**: Records written in sequence number order
- **Crash Recovery**: All committed writes recoverable via `recover()` method
- **Partial Writes**: Corrupted records detected and skipped during recovery
- **Truncation Safety**: Only truncate after confirmed flush to SSTable
- **Sequence Continuity**: Monotonically increasing sequence numbers for all operations

### 3. SSTable (Sorted String Table)
**Purpose**: Immutable, persistent storage for flushed data

#### Structure
```rust
pub struct SSTable {
    file: File,
    index: SSTableIndex,
    bloom_filter: BloomFilter,
    compression: CompressionType,
}

pub struct SSTableIndex {
    key_offsets: Vec<(Vec<u8>, u64)>, // (key, offset)
    bloom_filter_bits: Vec<u8>,
    compression_metadata: CompressionMetadata,
}
```

#### Properties
- **Immutable**: Once written, never modified
- **Sorted**: Keys maintained in sorted order
- **Compressed**: Configurable compression (LZ4, Zstd)
- **Indexed**: Sparse index for fast key location
- **Bloom Filtered**: Fast negative lookups

#### File Format
```
[Header: 64 bytes]
[Bloom Filter: variable]
[Sparse Index: variable]
[Data Blocks: variable]
[Footer: 32 bytes]
```

### 4. Compaction
**Purpose**: Merge multiple SSTables into fewer, larger files

#### Strategy
- **Leveled Compaction**: SSTables organized in levels
- **Size Tiers**: Each level has size constraints
- **Merge Policy**: Combine overlapping key ranges
- **Space Reclamation**: Remove deleted keys and duplicates

#### Levels
- **Level 0**: MemTable flushes, may overlap
- **Level 1-6**: Non-overlapping, exponentially larger
- **Level 7**: Final level, no further compaction

#### Compaction Trigger
- **Level 0**: > 4 SSTables
- **Level N**: > 10^N MB total size
- **Manual**: Explicit compaction request

---

## API Surface

### Core Operations

#### Put Operation
```rust
pub async fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError>
```

**Semantics**: Store key-value pair with current timestamp
**Durability**: WAL write + MemTable update
**Performance**: O(log n) in MemTable, O(1) amortized

#### Get Operation
```rust
pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError>
```

**Semantics**: Retrieve value for key, None if not found
**Search Order**: MemTable → Level 0 → Level 1... → Level 7
**Performance**: O(log n) with bloom filter optimization

#### Delete Operation
```rust
pub async fn delete(&mut self, key: &[u8]) -> Result<(), DatabaseError>
```

**Semantics**: Mark key as deleted (tombstone)
**Durability**: WAL write + MemTable update
**Cleanup**: Removed during compaction

### Batch Operations

#### Batch Write
```rust
pub async fn batch_write(&mut self, operations: Vec<BatchOp>) -> Result<(), DatabaseError>

pub enum BatchOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}
```

**Semantics**: Atomic batch of multiple operations
**Durability**: Single WAL entry for entire batch
**Performance**: O(n log n) for n operations

### Configuration

#### Database Options
```rust
pub struct DatabaseOptions {
    pub data_dir: PathBuf,
    pub memtable_size: usize,
    pub sstable_size: usize,
    pub compression: CompressionType,
    pub bloom_filter_bits: usize,
    pub max_levels: usize,
}
```

---

## Invariants

### 1. WAL Durability
**Requirement**: All committed writes survive crashes

**Implementation**:
- WAL write before MemTable update
- fsync after WAL write completion
- Truncate WAL only after successful flush
- CRC32 validation on recovery

**Verification**:
- Crash recovery tests with random power loss
- WAL corruption detection and handling
- Partial write recovery

### 2. Compaction Order
**Requirement**: SSTables maintain sorted order within levels

**Implementation**:
- Level 0: May overlap (MemTable flushes)
- Level 1+: Non-overlapping key ranges
- Compaction merges overlapping SSTables
- Sorted output maintains invariants

**Verification**:
- Property-based tests for sort order
- Compaction correctness validation
- Level boundary verification

### 3. Get Correctness
**Requirement**: Get returns most recent value for key

**Implementation**:
- Search order: MemTable → Level 0 → Level 1... → Level 7
- Sequence number determines recency
- Deletion tombstones override older values
- Compaction preserves value history

**Verification**:
- Property-based tests for consistency
- Concurrent write/read scenarios
- Deletion and re-insertion patterns

### 4. Space Efficiency
**Requirement**: Storage overhead < 20% of data size

**Implementation**:
- Configurable compression (LZ4, Zstd)
- Bloom filters reduce unnecessary I/O
- Sparse indexing minimizes index size
- Compaction removes duplicates and tombstones

**Verification**:
- Storage overhead measurement
- Compression ratio validation
- Index size optimization

---

## Testing Strategy

### 1. Unit Tests

#### MemTable Tests
```rust
#[cfg(test)]
mod memtable_tests {
    #[test]
    fn test_memtable_creation() {
        // Test MemTable initialization and default values
    }
    
    #[test]
    fn test_memtable_put_and_get() {
        // Test basic put/get operations and overwriting
    }
    
    #[test]
    fn test_memtable_delete() {
        // Test delete and tombstone behavior
    }
    
    #[test]
    fn test_memtable_size_tracking() {
        // Test size boundary enforcement and limits
    }
    
    #[test]
    fn test_memtable_sequence_numbers() {
        // Test sequence number incrementing
    }
    
    #[test]
    fn test_memtable_entries() {
        // Test entry retrieval and sorting
    }
    
    #[test]
    fn test_memtable_clear() {
        // Test clearing and reset functionality
    }
    
    #[test]
    fn test_memtable_invalid_inputs() {
        // Test error handling for invalid inputs
    }
    
    #[test]
    fn test_memtable_thread_safety() {
        // Test concurrent access with multiple threads
    }
    
    #[test]
    fn test_memtable_ordering() {
        // Test that entries maintain sorted order
    }
}
```

#### WAL Tests
```rust
#[cfg(test)]
mod wal_tests {
    #[test]
    fn test_wal_creation() {
        // Test WAL initialization and file creation
    }
    
    #[test]
    fn test_wal_put_and_get() {
        // Test basic put operations and sequence numbering
    }
    
    #[test]
    fn test_wal_delete() {
        // Test delete operations and tombstone creation
    }
    
    #[test]
    fn test_wal_recovery() {
        // Test complete recovery workflow: write entries, restart, replay into MemTable
    }
    
    #[test]
    fn test_wal_sequence_numbers() {
        // Test sequence number incrementing and continuity
    }
    
    #[test]
    fn test_wal_truncation() {
        // Test safe truncation after successful operations
    }
    
    #[test]
    fn test_wal_corruption_handling() {
        // Test corruption detection and recovery mechanisms
    }
    
    #[test]
    fn test_wal_record_structure() {
        // Test WALRecord creation and validation
    }
    
    #[test]
    fn test_wal_to_entry_conversion() {
        // Test conversion from WALRecord to MemTable Entry
    }
}
```

#### SSTable Tests
```rust
#[cfg(test)]
mod sstable_tests {
    #[test]
    fn test_sstable_creation_and_reading() {
        // Test file format
    }
    
    #[test]
    fn test_bloom_filter_accuracy() {
        // Test false positive rates
    }
    
    #[test]
    fn test_compression_and_decompression() {
        // Test compression algorithms
    }
}
```

### 2. Integration Tests

#### End-to-End Operations
```rust
#[test]
fn test_put_get_delete_workflow() {
    // Test complete operation sequence
}

#[test]
fn test_batch_operations() {
    // Test atomic batch behavior
}

#[test]
fn test_concurrent_access() {
    // Test thread safety
}
```

#### Crash Recovery Tests
```rust
#[test]
fn test_crash_recovery_with_wal() {
    // Simulate crashes during operations
}

#[test]
fn test_partial_write_recovery() {
    // Test corrupted WAL handling
}

#[test]
fn test_compaction_crash_recovery() {
    // Test compaction interruption
}
```

### 3. Property-Based Tests

#### Consistency Properties
```rust
proptest! {
    #[test]
    fn test_get_after_put_returns_value(keys: Vec<Vec<u8>>, values: Vec<Vec<u8>>) {
        // Property: put(key, value) followed by get(key) returns value
    }
    
    #[test]
    fn test_delete_removes_value(keys: Vec<Vec<u8>>, values: Vec<Vec<u8>>) {
        // Property: delete(key) followed by get(key) returns None
    }
    
    #[test]
    fn test_compaction_preserves_values(data: Vec<(Vec<u8>, Vec<u8>)>) {
        // Property: compaction doesn't lose valid data
    }
}
```

#### Performance Properties
```rust
proptest! {
    #[test]
    fn test_memtable_size_bounds(operations: Vec<BatchOp>) {
        // Property: MemTable never exceeds size limit
    }
    
    #[test]
    fn test_compaction_reduces_file_count(sstables: Vec<SSTable>) {
        // Property: Compaction reduces total file count
    }
}
```

### 4. Performance Tests

#### Benchmark Suite
```rust
#[bench]
fn bench_put_operations(b: &mut Bencher) {
    // Measure put throughput
}

#[bench]
fn bench_get_operations(b: &mut Bencher) {
    // Measure get latency
}

#[bench]
fn bench_compaction_speed(b: &mut Bencher) {
    // Measure compaction performance
}
```

---

## Implementation Notes

### MemTable Implementation
- **Data Structure Choice**: Sorted vector with binary search instead of BTreeMap
  - **Rationale**: Simpler to implement, easier to make thread-safe, still O(log n) performance
  - **Trade-offs**: Insertions are O(n) due to shifting, but reads remain O(log n)
  - **Thread Safety**: Arc<RwLock<Vec<Entry>>> provides safe concurrent access
- **Size Tracking**: Accurate byte-level monitoring with configurable limits
- **Sequence Numbers**: Monotonically increasing for all operations (put, delete)
- **Tombstone Support**: Deletions create entries with None values for proper LSM semantics

### WAL Implementation
- **File Management**: Uses `BufWriter<File>` for efficient buffered I/O operations
  - **Rationale**: BufWriter provides automatic buffering for better performance
  - **Trade-offs**: Requires explicit flush calls for durability guarantees
  - **Recovery**: Automatic sequence number recovery from existing WAL files
- **Record Format**: Binary format with fixed-size headers and variable-length data
  - **Header**: 24 bytes total (key_len, value_len, timestamp, sequence_number)
  - **Data**: Key and value bytes as specified in header
  - **Validation**: Size limits (1MB key, 100MB value) prevent abuse
- **Corruption Handling**: Robust recovery mechanisms for partial writes
  - **Seek Recovery**: Attempts to find next valid record after corruption
  - **Graceful Degradation**: Continues recovery from corruption points
  - **Logging**: Comprehensive error logging for troubleshooting

### Error Handling
- **Custom error types** using `thiserror`
- **Recoverable vs. fatal errors** clearly distinguished
- **Context information** included in error messages
- **Error codes** for programmatic handling

### Logging and Observability
- **Structured logging** with `tracing` crate
- **Performance metrics** for all operations
- **Debug information** for troubleshooting
- **Audit trail** for compliance requirements

### Configuration Management
- **Environment variables** for deployment settings
- **Configuration files** for complex setups
- **Runtime configuration** for dynamic adjustments
- **Validation** of all configuration values

### Future Extensions
- **Multi-version concurrency control** (MVCC)
- **Distributed coordination** for clustering
- **Advanced query language** support
- **Plugin system** for custom storage engines

---

## Version Compatibility

### Breaking Changes
- **API signature changes** require major version bump
- **File format changes** require migration tools
- **Configuration changes** require documentation updates
- **Performance characteristics** may change in minor versions

### Migration Path
- **v0.1.0 → v0.2.0**: Automatic migration for file formats
- **v0.x.0 → v1.0.0**: Breaking changes with migration guide
- **Deprecation warnings** provided in advance
- **Backward compatibility** maintained where possible

---

**This specification defines the foundation for RustEdgeDB. All implementations must adhere to these requirements to ensure compatibility and correctness.**
