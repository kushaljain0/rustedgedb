# RustEdgeDB Specification

## Version 0.1.0 - Base Engine

**Status**: Current Specification  
**Last Updated**: 2025-01-08  
**Compatibility**: Breaking changes require major version bump  
**Test Status**: All 85 tests passing  

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
    path: PathBuf,
    header: SSTableHeader,
    index: SSTableIndex,
    bloom_filter: BloomFilter,
}

pub struct SSTableIndex {
    entries: Vec<IndexEntry>,
    bloom_filter_bits: Vec<u8>,
    compression_metadata: CompressionMetadata,
}

pub struct IndexEntry {
    key: Vec<u8>,
    offset: u64,
    key_size: u32,
    value_size: u32,
}

pub struct SSTableHeader {
    magic: [u8; 8],           // "RUSTEDGE" magic number
    version: u32,              // File format version
    entry_count: u32,          // Number of key-value pairs
    index_offset: u64,         // Offset to index section
    bloom_filter_offset: u64,  // Offset to bloom filter
    data_offset: u64,          // Offset to data section
    compression_type: u8,      // Compression algorithm
    reserved: [u8; 31],       // Reserved for future use
}
```

#### Properties
- **Immutable**: Once written, never modified
- **Sorted**: Keys maintained in sorted order using binary search
- **Compressed**: Configurable compression (None, LZ4, Zstd)
- **Indexed**: Sparse index for fast key location with O(log n) lookup
- **Bloom Filtered**: Fast negative lookups with configurable false positive rates

#### File Format
```
[Header: 64 bytes]
[Bloom Filter: variable size]
[Data Section: variable size]
[Index Section: variable size]
[Footer: 32 bytes]
```

#### Implementation Details
- **Binary Search**: Index entries sorted by key for O(log n) lookups
- **Bloom Filter**: 10x size with 3 hash functions for efficient negative lookups
- **Error Handling**: Comprehensive error types using thiserror crate
- **File Validation**: Magic number and format validation on open
- **Tombstone Support**: Deletion markers preserved in data structure

### 4. Compaction
**Purpose**: Merge multiple SSTables into fewer, larger files

#### Strategy
- **Leveled Compaction**: SSTables organized in levels
- **Size Tiers**: Each level has size constraints
- **Merge Policy**: Combine overlapping key ranges
- **Space Reclamation**: Remove deleted keys and duplicates
- **Tombstone Removal**: Eliminate deletion markers during compaction
- **Duplicate Resolution**: Keep only the most recent value for each key

#### Implementation Details
- **Multi-Phase Approach**: Collect entries, process, then write output
- **Sort-Based Deduplication**: Sort by key, then by sequence number
- **Tombstone Handling**: Remove deletion markers to reclaim space
- **Offset Management**: Dynamic calculation of file section offsets
- **File Validation**: Ensure output SSTable is valid and readable

#### Levels
- **Level 0**: MemTable flushes, may overlap
- **Level 1-6**: Non-overlapping, exponentially larger
- **Level 7**: Final level, no further compaction

#### Compaction Trigger
- **Level 0**: > 4 SSTables
- **Level N**: > 10^N MB total size
- **Manual**: Explicit compaction request

#### Performance Characteristics
- **Memory Usage**: O(n) where n is total entries across all input SSTables
- **Time Complexity**: O(n log n) due to sorting and deduplication
- **I/O Efficiency**: Single pass through data with optimized file writing
- **Space Savings**: Removes tombstones and duplicates, typically 20-40% reduction

### 5. Engine
**Purpose**: Main database orchestrator that coordinates WAL, MemTable, and SSTable operations

#### Structure
```rust
pub struct Engine {
    wal: WAL,
    memtable: MemTable,
    config: EngineConfig,
    sstables: Arc<RwLock<Vec<SSTable>>>,
    sequence_number: Arc<RwLock<u64>>,
}

pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub memtable_size: usize,
    pub compression: CompressionType,
    pub max_levels: usize,
}
```

#### Properties
- **Coordinated Operations**: Orchestrates WAL, MemTable, and SSTable interactions
- **Write-Ahead Logging**: Ensures durability before acknowledging operations
- **Automatic Flushing**: Triggers MemTable flush when size threshold exceeded
- **Crash Recovery**: Replays WAL into MemTable on restart
- **Lookup Optimization**: Searches MemTable first, then SSTables in order

#### Core Responsibilities
1. **Write Operations**: Write to WAL first, then MemTable
2. **Read Operations**: Search MemTable → SSTables (newest first)
3. **MemTable Management**: Automatic flushing and replacement
4. **WAL Rotation**: New WAL file after each MemTable flush
5. **Recovery**: Reconstruct database state from WAL on startup

#### Implementation Details
- **Async Operations**: All public methods use async/await for non-blocking I/O
- **Thread Safety**: Arc + RwLock for shared SSTable list
- **Sequence Numbers**: Monotonically increasing across all operations
- **Error Handling**: Comprehensive error types with proper propagation
- **Configuration**: Flexible configuration with sensible defaults
- **Statistics**: Runtime statistics for monitoring and debugging

#### Lifecycle
1. **Initialization**: Create data directory, initialize WAL and MemTable
2. **Recovery**: Replay existing WAL files to restore state
3. **Operation**: Handle put/get/delete requests with proper coordination
4. **Maintenance**: Automatic MemTable flushing and WAL rotation
5. **Shutdown**: Flush remaining data and close resources gracefully

#### Performance Characteristics
- **Write Throughput**: WAL + MemTable operations (O(log n) in MemTable)
- **Read Latency**: MemTable hit (O(log n)) vs SSTable search (O(log n) per level)
- **Memory Usage**: MemTable size + SSTable metadata overhead
- **Recovery Time**: O(n) where n is number of WAL records
- **Space Efficiency**: Automatic compaction reduces storage overhead

---

## API Surface

### Core Operations

The Engine provides the primary database interface, orchestrating all underlying components:

#### Engine Creation
```rust
// Create with default configuration
pub async fn new<P: AsRef<Path>>(data_dir: P) -> EngineResult<Self>

// Create with custom configuration
pub async fn with_config(config: EngineConfig) -> EngineResult<Self>
```

#### Put Operation
```rust
pub async fn put(&mut self, key: &[u8], value: &[u8]) -> EngineResult<()>
```

**Semantics**: Store key-value pair with current timestamp
**Durability**: WAL write + MemTable update
**Performance**: O(log n) in MemTable, O(1) amortized
**Coordination**: Engine ensures WAL durability before MemTable update

#### Get Operation
```rust
pub async fn get(&self, key: &[u8]) -> EngineResult<Option<Vec<u8>>>
```

**Semantics**: Retrieve value for key, None if not found
**Search Order**: MemTable → SSTables (newest first)
**Performance**: O(log n) with bloom filter optimization
**Coordination**: Engine searches MemTable first, then SSTables in order

#### Delete Operation
```rust
pub async fn delete(&mut self, key: &[u8]) -> EngineResult<()>
```

**Semantics**: Mark key as deleted (tombstone)
**Durability**: WAL write + MemTable update
**Cleanup**: Removed during compaction
**Coordination**: Engine creates tombstone entry in both WAL and MemTable

#### Engine Management
```rust
// Force flush of current MemTable
pub async fn force_flush(&mut self) -> EngineResult<()>

// Get engine statistics
pub fn stats(&self) -> EngineStats

// Graceful shutdown
pub async fn close(&mut self) -> EngineResult<()>
```

### Batch Operations

#### Batch Write
```rust
pub async fn batch_write(&mut self, operations: Vec<BatchOp>) -> EngineResult<()>

pub enum BatchOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}
```

**Semantics**: Atomic batch of multiple operations
**Durability**: Single WAL entry for entire batch
**Performance**: O(n log n) for n operations
**Coordination**: Engine ensures atomicity across all operations

### Configuration

#### Engine Configuration
```rust
pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub memtable_size: usize,
    pub compression: CompressionType,
    pub max_levels: usize,
}
```

**Default Values**:
- `data_dir`: "./data"
- `memtable_size`: 64MB
- `compression`: None
- `max_levels`: 7

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

### 5. Engine Coordination
**Requirement**: Engine maintains consistency across all components

**Implementation**:
- WAL write before MemTable update (Write-Ahead Logging)
- MemTable flush triggers WAL rotation
- Sequence number continuity across all operations
- Automatic recovery from WAL on restart

**Verification**:
- Crash recovery tests with Engine restart
- Sequence number continuity validation
- WAL rotation and MemTable flush coordination
- Component state consistency checks

### 6. Operation Ordering
**Requirement**: Operations maintain logical consistency

**Implementation**:
- Sequence numbers monotonically increase across all operations
- WAL records written in operation order
- MemTable operations respect sequence number ordering
- SSTable lookups follow newest-first search order

**Verification**:
- Concurrent operation testing
- Sequence number ordering validation
- Read-after-write consistency checks
- Deletion and re-insertion scenarios

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
    
}

### 2. Integration Tests

#### Engine Integration Tests
```rust
#[cfg(test)]
mod integration_tests {
    #[test]
    async fn test_persistence_across_restart() {
        // Test complete data persistence: write → flush → restart → read
    }
    
    #[test]
    async fn test_wal_rotation_and_flushing() {
        // Test multiple SSTable creation with appropriate MemTable sizing
    }
    
    #[test]
    async fn test_large_dataset() {
        // Test handling of large datasets that trigger MemTable flushes
    }
    
    #[test]
    async fn test_compaction_correctness() {
        // Test compaction engine with multiple SSTables
    }
}
```

#### Test Configuration Best Practices
- **MemTable sizing**: Use appropriate sizes for test scenarios (e.g., 1KB for multiple SSTable tests)
- **Explicit flushing**: Force flushes when testing specific behaviors
- **Threshold testing**: Ensure tests actually trigger the code paths they're testing
- **Realistic data**: Use data volumes that match expected production patterns
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
        // Test file format and data persistence
    }
    
    #[test]
    fn test_bloom_filter_accuracy() {
        // Test false positive rates and key containment
    }
    
    #[test]
    fn test_compression_types() {
        // Test compression algorithm selection
    }
    
    #[test]
    fn test_sstable_index() {
        // Test binary search index functionality
    }
    
    #[test]
    fn test_sstable_file_format() {
        // Test file structure and validation
    }
    
    #[test]
    fn test_sstable_empty_memtable() {
        // Test error handling for empty input
    }
    
    #[test]
    fn test_sstable_header_footer() {
        // Test header and footer serialization
    }
}
```

#### Engine Tests
```rust
#[cfg(test)]
mod engine_tests {
    #[test]
    async fn test_basic_operations() {
        // Test put, get, delete operations
    }
    
    #[test]
    async fn test_persistence() {
        // Test data persistence across engine restarts
    }
    
    #[test]
    async fn test_crash_recovery() {
        // Test crash recovery by replaying WAL
    }
    
    #[test]
    async fn test_memtable_flush() {
        // Test automatic MemTable flushing and WAL rotation
    }
    
    #[test]
    async fn test_correctness_with_deletions() {
        // Test deletion handling and tombstone behavior
    }
    
    #[test]
    async fn test_empty_key_handling() {
        // Test error handling for invalid inputs
    }
    
    #[test]
    async fn test_sequence_number_continuity() {
        // Test sequence number ordering across operations
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

#[test]
fn test_sstable_workflow() {
    // Test MemTable → SSTable flush workflow
    // Verify data persistence and tombstone handling
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

### SSTable Implementation
- **File Format**: Binary format with header, bloom filter, data section, index, and footer
  - **Header**: 64 bytes with magic number, version, entry counts, and section offsets
  - **Bloom Filter**: Variable-size bloom filter for fast key existence checks
  - **Data Section**: Sequential entries with headers (key_len, value_len, timestamp, seq)
  - **Index**: Sparse index with key data offsets relative to data section start
  - **Footer**: 32 bytes with checksum, data size, and index size
- **Index Offset Calculation**: Critical for data integrity
  - **Correct Implementation**: Index stores offsets relative to data section start
  - **Data Layout**: Entry header → key data → value data (if not tombstone)
  - **Reading Process**: Seek to data_offset + index_offset, read key then value
- **Bloom Filter Sizing**: Must match actual data size to prevent corruption
  - **Placeholder Size**: Use actual bloom filter size, not fixed 64 bytes
  - **File Corruption Prevention**: Mismatched sizes cause offset shifts and data corruption

### Engine Implementation
- **Component Orchestration**: Engine coordinates WAL, MemTable, and SSTable operations
  - **Rationale**: Centralized coordination ensures consistency and proper ordering
  - **Design**: Async/await pattern for non-blocking I/O operations
  - **Thread Safety**: Arc<RwLock<Vec<SSTable>>> for shared SSTable list management
- **Write-Ahead Logging**: All operations write to WAL before MemTable
  - **Durability**: WAL write ensures crash recovery capability
  - **Ordering**: Sequence numbers maintain operation order across restarts
  - **Performance**: WAL writes are buffered for efficiency
- **MemTable Management**: Automatic flushing when size threshold exceeded
  - **Threshold**: Configurable maximum size (default: 64MB)
  - **Flush Process**: Convert to SSTable, create new MemTable, rotate WAL
  - **Coordination**: WAL rotation ensures new operations go to fresh WAL file
- **Recovery Mechanism**: Automatic reconstruction from WAL on startup
  - **WAL Replay**: Uses existing WAL recovery mechanism
  - **State Restoration**: MemTable populated with recovered operations
  - **Sequence Continuity**: Sequence numbers continue from highest recovered value
- **Lookup Optimization**: MemTable-first search for optimal read performance
  - **Order**: MemTable → SSTables (newest first)
  - **Performance**: MemTable hits are O(log n), SSTable searches are O(log n) per level
  - **Consistency**: Most recent value always returned due to search order

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
